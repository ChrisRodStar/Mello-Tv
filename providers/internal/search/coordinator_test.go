package search

import (
	"context"
	"errors"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/provider"
	"runtime"
	"sync/atomic"
	"testing"
	"time"
)

type waitingAdapter struct {
	started chan struct{}
	release chan struct{}
	calls   atomic.Int32
}

func (*waitingAdapter) ID() model.ProviderID                { return model.VidCore }
func (*waitingAdapter) Capabilities() provider.Capabilities { return provider.Capabilities{} }
func (a *waitingAdapter) Resolve(ctx context.Context, in model.Lookup) (model.AdapterResult, error) {
	if a.calls.Add(1) == 1 {
		close(a.started)
	}
	select {
	case <-ctx.Done():
		return model.AdapterResult{}, ctx.Err()
	case <-a.release:
		return model.AdapterResult{Candidates: []model.Candidate{{ID: "synthetic", Provider: model.VidCore, Title: in.Title, URL: "https://example.com/media.m3u8", Format: model.HLS}}}, nil
	}
}

type syntheticProber struct{}

func (syntheticProber) ProbeStream(_ context.Context, sources <-chan model.Candidate) []model.Probed {
	var out []model.Probed
	rate := float64(6_000_000)
	for s := range sources {
		out = append(out, model.Probed{Source: s, Observation: model.Observation{Accessible: true, Format: model.HLS, ProtectionKnown: true, DeliveryBPS: &rate}})
	}
	return out
}
func TestOneWaiterCancellationDoesNotCancelSharedLookup(t *testing.T) {
	adapter := &waitingAdapter{started: make(chan struct{}), release: make(chan struct{})}
	registry, e := provider.NewRegistry(adapter)
	if e != nil {
		t.Fatal(e)
	}
	c, e := New(registry, syntheticProber{}, DefaultLimits(), nil)
	if e != nil {
		t.Fatal(e)
	}
	defer c.Close()
	ctx, stop := context.WithTimeout(context.Background(), 2*time.Second)
	defer stop()
	first, cancelFirst := context.WithCancel(ctx)
	defer cancelFirst()
	in := model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 1}, Device: model.Device{Formats: []model.Format{model.HLS}, AllowUnknownCodecs: true, AllowUnknownResolution: true}}
	one := make(chan error, 1)
	two := make(chan error, 1)
	go func() { _, err := c.Resolve(first, in); one <- err }()
	select {
	case <-adapter.started:
	case <-ctx.Done():
		t.Fatal("adapter did not start")
	}
	go func() { _, err := c.Resolve(ctx, in); two <- err }()
	key, _ := requestKey(in)
	for {
		c.mu.Lock()
		f := c.flights[key]
		ready := f != nil && f.waiters == 2
		c.mu.Unlock()
		if ready {
			break
		}
		if ctx.Err() != nil {
			t.Fatal("second caller did not join")
		}
		runtime.Gosched()
	}
	cancelFirst()
	select {
	case err := <-one:
		if !errors.Is(err, context.Canceled) {
			t.Fatal("cancelled caller did not detach")
		}
	case <-ctx.Done():
		t.Fatal("caller cancellation stalled")
	}
	close(adapter.release)
	select {
	case err := <-two:
		if err != nil {
			t.Fatalf("remaining caller failed: %v", err)
		}
	case <-ctx.Done():
		t.Fatal("remaining caller stalled")
	}
	if adapter.calls.Load() != 1 {
		t.Fatal("identical callers duplicated resolver work")
	}
}

type joiningAdapter struct {
	started, cancelled, release chan struct{}
	calls                       atomic.Int32
}

func (*joiningAdapter) ID() model.ProviderID {
	return model.VidCore
}
func (*joiningAdapter) Capabilities() provider.Capabilities {
	return provider.Capabilities{}
}
func (a *joiningAdapter) Resolve(ctx context.Context, _ model.Lookup) (model.AdapterResult, error) {
	first := a.calls.Add(1) == 1
	if first {
		close(a.started)
	}
	<-ctx.Done()
	if first {
		close(a.cancelled)
	}
	<-a.release
	return model.AdapterResult{}, ctx.Err()
}
func TestCloseJoinsCancelledAdapter(t *testing.T) {
	adapter := &joiningAdapter{
		started:   make(chan struct{}),
		cancelled: make(chan struct{}),
		release:   make(chan struct{}),
	}
	registry, _ := provider.NewRegistry(adapter)
	coordinator, _ := New(registry, syntheticProber{}, DefaultLimits(), nil)
	resolved := make(chan struct{})
	go func() {
		defer close(resolved)
		_, _ = coordinator.Resolve(context.Background(), model.Lookup{Title: model.Title{
			Type:   model.Movie,
			TMDBID: 1,
		}})
	}()
	<-adapter.started
	closed := make(chan struct{})
	go func() {
		coordinator.Close()
		close(closed)
	}()
	<-adapter.cancelled
	select {
	case <-closed:
		t.Fatal("close returned before adapter joined")
	default:
	}
	close(adapter.release)
	select {
	case <-closed:
	case <-time.After(time.Second):
		t.Fatal("close stalled")
	}
	<-resolved
}
func TestAdapterCrossIDConflictRejected(t *testing.T) {
	lookup := model.Lookup{Title: model.Title{
		Type:   model.Movie,
		TMDBID: 550,
		IMDbID: "tt0137523",
	}}
	result := model.AdapterResult{Candidates: []model.Candidate{{
		ID:       "synthetic",
		Provider: model.VidCore,
		Title: model.Title{
			Type:   model.Movie,
			TMDBID: 550,
			IMDbID: "tt0000001",
		},
		URL: "https://media.example.com/master.m3u8",
	}}}
	if validResult(result, model.VidCore, lookup, 32) {
		t.Fatal("conflicting cross-ID admitted")
	}
}

func TestExplicitRefreshBypassesAdapterCacheWithoutChangingRanking(t *testing.T) {
	adapter := &waitingAdapter{
		started: make(chan struct{}),
		release: make(chan struct{}),
	}
	close(adapter.release)
	registry, _ := provider.NewRegistry(adapter)
	coordinator, _ := New(registry, syntheticProber{}, DefaultLimits(), nil)
	defer coordinator.Close()
	lookup := model.Lookup{
		Title: model.Title{
			Type:   model.Movie,
			TMDBID: 550,
		},
		Device: model.Device{
			Formats:                []model.Format{model.HLS},
			AllowUnknownCodecs:     true,
			AllowUnknownResolution: true,
		},
	}
	for _, refresh := range []bool{
		false,
		false,
		true,
	} {
		lookup.Refresh = refresh
		outcome, err := coordinator.Resolve(context.Background(), lookup)
		if err != nil || outcome.Selected == nil || len(outcome.Providers) != 4 {
			t.Fatal("shared selection unavailable")
		}
	}
	if adapter.calls.Load() != 2 {
		t.Fatal("cache reused for an explicit source refresh")
	}
}

func TestCancelledFlightRetainsAdmissionUntilAdapterJoins(t *testing.T) {
	adapter := &joiningAdapter{
		started:   make(chan struct{}),
		cancelled: make(chan struct{}),
		release:   make(chan struct{}),
	}
	registry, _ := provider.NewRegistry(adapter)
	limits := DefaultLimits()
	limits.MaxFlights = 1
	coordinator, _ := New(registry, syntheticProber{}, limits, nil)
	defer coordinator.Close()
	defer close(adapter.release)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	ended := make(chan struct{})
	go func() {
		defer close(ended)
		_, _ = coordinator.Resolve(ctx, model.Lookup{Title: model.Title{
			Type:   model.Movie,
			TMDBID: 1,
		}})
	}()
	<-adapter.started
	cancel()
	<-ended
	<-adapter.cancelled
	secondCtx, stop := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer stop()
	_, err := coordinator.Resolve(secondCtx, model.Lookup{Title: model.Title{
		Type:   model.Movie,
		TMDBID: 2,
	}})
	if model.Category(err) != model.Unavailable || adapter.calls.Load() != 1 {
		t.Fatal("cancelled but unjoined work escaped flight admission")
	}
}
