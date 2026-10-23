package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"sync"
	"testing"
	"time"
)

func TestBlockedPreloadDoesNotBlockAnotherPlayback(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	entered := make(chan struct{})
	unblock := make(chan struct{})
	original := f.provider.Config.Handler
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Idempotency-Key") != "" {
			t.Error("provider received client key")
		}
		// The first lookup is speculative and deliberately remains in progress.
		select {
		case <-entered:
		default:
			close(entered)
			select {
			case <-unblock:
			case <-r.Context().Done():
			}
		}
		original.ServeHTTP(w, r)
	}))
	defer provider.Close()
	var unblockOnce sync.Once
	defer unblockOnce.Do(func() { close(unblock) })
	f.s.cfg.ProviderURL = provider.URL
	path := "/api/v1/profiles/" + p.ID + "/playback"
	done := make(chan int, 1)
	go func() {
		status, _ := f.request("POST", path, "", map[string]any{"type": "movie", "id": 7}, "preload")
		done <- status
	}()
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("preload did not reach provider")
	}
	f.want(200, "POST", path, "", map[string]any{"type": "movie", "id": 8}, "foreground")
	// Same-key retry still serializes rather than issuing a duplicate create.
	f.want(429, "POST", path, "", map[string]any{"type": "movie", "id": 7}, "preload")
	unblockOnce.Do(func() { close(unblock) })
	if status := <-done; status != 200 {
		t.Fatalf("preload status %d", status)
	}
}

func TestConcurrentSameKeyCreatesExactlyOneSession(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	path := "/api/v1/profiles/" + p.ID + "/playback"
	results := make(chan string, 2)
	for range 2 {
		go func() {
			_, data := f.request("POST", path, "", map[string]any{"type": "movie", "id": 7}, "same-key")
			var reply struct{ ID string }
			_ = json.Unmarshal(data, &reply)
			results <- reply.ID
		}()
	}
	first, second := <-results, <-results
	if first == "" || first != second {
		t.Fatalf("create identities differ: %q %q", first, second)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.providerCalls != 1 || f.relayCreates != 1 {
		t.Fatalf("duplicate work: providers=%d relay=%d", f.providerCalls, f.relayCreates)
	}
}

func TestPlaybackLockCancellationAndBound(t *testing.T) {
	var locks playbackLocks
	release, err := locks.acquire(context.Background(), "same", 0)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := locks.acquire(ctx, "same", time.Second); err != context.Canceled {
		t.Fatalf("expected cancellation, got %v", err)
	}
	other, err := locks.acquire(context.Background(), "other", 0)
	if err != nil {
		t.Fatal(err)
	}
	other()
	release()
	if len(locks.entries) != 0 {
		t.Fatal("lock entries leaked")
	}
	var releases []func()
	for i := range 64 {
		release, err := locks.acquire(context.Background(), strconv.Itoa(i), 0)
		if err != nil {
			t.Fatal(err)
		}
		releases = append(releases, release)
	}
	if _, err := locks.acquire(context.Background(), "overflow", 0); err == nil {
		t.Fatal("unbounded playback admission")
	}
	for _, release := range releases {
		release()
	}
	if len(locks.entries) != 0 {
		t.Fatal("admission entries leaked")
	}
}
