package probe

import (
	"context"
	"errors"
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

type pendingPlaylistClient struct {
	requests chan int64
	manifest []byte
}

func (client *pendingPlaylistClient) Fetch(_ context.Context, request network.Request) (network.Response, error) {
	client.requests <- request.Limit
	if int64(len(client.manifest)) > request.Limit {
		return network.Response{}, errors.New("response limit exceeded")
	}
	parsedURL, _ := url.Parse(request.URL)
	return network.Response{
		URL:    parsedURL,
		Status: http.StatusOK,
		Body:   client.manifest,
	}, nil
}

func waitForPendingBudget(t *testing.T, budget *byteBudget) {
	t.Helper()
	timeout := time.NewTimer(time.Second)
	defer timeout.Stop()
	ticker := time.NewTicker(time.Millisecond)
	defer ticker.Stop()
	for {
		budget.mu.Lock()
		waiting := budget.changed != nil
		budget.mu.Unlock()
		if waiting {
			return
		}
		select {
		case <-ticker.C:
		case <-timeout.C:
			t.Fatal("playlist request did not wait for pending reservations")
		}
	}
}

func TestPendingReservationsDoNotTruncateChildPlaylist(t *testing.T) {
	const requestLimit = int64(1024 * 1024)
	const completedBytes = int64(600 * 1024)
	budget := &byteBudget{remaining: 4 * requestLimit}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	for index := 0; index < 4; index++ {
		if reserved, err := budget.reservePlaylist(ctx, requestLimit); err != nil || reserved != requestLimit {
			t.Fatal("initial parallel reservations rejected")
		}
	}
	budget.finishPlaylist(requestLimit, completedBytes)
	manifest := []byte("#EXTM3U\n#" + strings.Repeat("x\n#", int(completedBytes)/3) + "\n#EXTINF:4,\nsegment.ts\n")
	client := &pendingPlaylistClient{
		requests: make(chan int64, 1),
		manifest: manifest,
	}
	pool := &Pool{
		client: client,
		limits: DefaultLimits(),
	}
	finished := make(chan error, 1)
	go func() {
		_, err := pool.fetchPlaylist(ctx, model.Candidate{}, "https://cdn.example/child.m3u8", budget)
		finished <- err
	}()
	waitForPendingBudget(t, budget)
	select {
	case <-client.requests:
		t.Fatal("child request started with a temporary reduced read limit")
	default:
	}
	budget.finishPlaylist(requestLimit, 0)
	select {
	case err := <-finished:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal("child did not resume after pending read completed")
	}
	if limit := <-client.requests; limit != requestLimit {
		t.Fatal("child playlist lost its full bounded read allowance")
	}
	budget.finishPlaylist(requestLimit, 0)
	budget.finishPlaylist(requestLimit, 0)
	if budget.remaining != 4*requestLimit-completedBytes-int64(len(manifest)) || budget.pending != 0 {
		t.Fatal("settled metadata budget does not equal actual completed reads")
	}
}

func TestPendingBudgetWaitHonorsCancellation(t *testing.T) {
	budget := &byteBudget{remaining: 1024}
	if _, err := budget.reservePlaylist(context.Background(), 1024); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	finished := make(chan error, 1)
	go func() {
		_, err := budget.reservePlaylist(ctx, 1024)
		finished <- err
	}()
	waitForPendingBudget(t, budget)
	cancel()
	select {
	case err := <-finished:
		if !errors.Is(err, context.Canceled) {
			t.Fatal("budget wait lost cancellation")
		}
	case <-time.After(time.Second):
		t.Fatal("canceled budget waiter leaked")
	}
	budget.finishPlaylist(1024, 1024)
	if reserved, err := budget.reservePlaylist(context.Background(), 1024); err != nil || reserved != 0 {
		t.Fatal("failed read was not charged against total budget")
	}
}
