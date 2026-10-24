package search

import (
	"context"
	"encoding/json"
	"stv-providers/internal/model"
	"stv-providers/internal/provider"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// The slow adapter cannot finish until the test observes the fast candidate in
// probing. This proves overlap without using a wall-clock speed threshold.
type pipelineAdapter struct {
	providerID model.ProviderID
	release    <-chan struct{}
	calls      atomic.Int32
}

func (adapter *pipelineAdapter) ID() model.ProviderID {
	return adapter.providerID
}

func (*pipelineAdapter) Capabilities() provider.Capabilities {
	return provider.Capabilities{}
}

func (adapter *pipelineAdapter) Resolve(ctx context.Context, lookup model.Lookup) (model.AdapterResult, error) {
	adapter.calls.Add(1)
	if adapter.release != nil {
		select {
		case <-adapter.release:
		case <-ctx.Done():
			return model.AdapterResult{}, ctx.Err()
		}
	}
	return model.AdapterResult{
		Candidates: []model.Candidate{{
			ID:       "same-server-id",
			Provider: adapter.providerID,
			Title:    lookup.Title,
			URL:      "https://media.example.com/master.m3u8",
			Format:   model.HLS,
		}},
	}, nil
}

type pipelineProber struct {
	firstProbed chan struct{}
	firstOnce   sync.Once
}

func (prober *pipelineProber) ProbeStream(ctx context.Context, candidates <-chan model.Candidate) []model.Probed {
	var results []model.Probed
	for {
		select {
		case <-ctx.Done():
			return results
		case candidate, open := <-candidates:
			if !open {
				return results
			}
			height := 720
			if candidate.Provider == model.VidLink {
				height = 1080
			}
			bitrate := int64(2_000_000)
			delivery := 12_000_000.0
			results = append(results, model.Probed{
				Source: candidate,
				Observation: model.Observation{
					Accessible:      true,
					Format:          model.HLS,
					ProtectionKnown: true,
					DeliveryBPS:     &delivery,
					Variant: &model.Variant{
						Height:  &height,
						Bitrate: &bitrate,
					},
				},
			})
			prober.firstOnce.Do(func() { close(prober.firstProbed) })
		}
	}
}

func TestProbingOverlapsCollectionWithoutSelectingEarly(t *testing.T) {
	for _, cached := range []bool{false, true} {
		name := "fresh"
		if cached {
			name = "cached"
		}
		t.Run(name, func(t *testing.T) {
			timeout, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			release := make(chan struct{})
			fast := &pipelineAdapter{providerID: model.VidCore}
			slow := &pipelineAdapter{providerID: model.VidLink, release: release}
			registry, err := provider.NewRegistry(fast, slow)
			if err != nil {
				t.Fatal(err)
			}
			prober := &pipelineProber{firstProbed: make(chan struct{})}
			coordinator, err := New(registry, prober, DefaultLimits(), nil)
			if err != nil {
				t.Fatal(err)
			}
			defer coordinator.Close()
			lookup := model.Lookup{
				Title: model.Title{Type: model.Movie, TMDBID: 7, IMDbID: "tt1234567"},
				Device: model.Device{
					Formats:            []model.Format{model.HLS},
					AllowUnknownCodecs: true,
				},
			}
			if cached {
				key, keyErr := requestKey(lookup)
				if keyErr != nil {
					t.Fatal(keyErr)
				}
				result, resolveErr := fast.Resolve(timeout, lookup)
				if resolveErr != nil {
					t.Fatal(resolveErr)
				}
				fast.calls.Store(0)
				coordinator.sources.Put(key+":"+string(model.VidCore), result)
			}
			type outcome struct {
				selection model.Selection
				err       error
			}
			finished := make(chan outcome, 1)
			go func() {
				selected, resolveErr := coordinator.Resolve(timeout, lookup)
				finished <- outcome{selection: selected, err: resolveErr}
			}()

			select {
			case <-prober.firstProbed:
			case <-timeout.Done():
				t.Fatal("fast candidate waited for slow provider before probing")
			}
			select {
			case <-finished:
				t.Fatal("selected 720p before collection finished")
			default:
			}
			close(release)
			select {
			case result := <-finished:
				if result.err != nil {
					t.Fatal(result.err)
				}
				if result.selection.Selected == nil || result.selection.Selected.Candidate.Source.Provider != model.VidLink {
					t.Fatal("late 1080p source was not selected")
				}
				if len(result.selection.Backups) != 1 || result.selection.Backups[0].Candidate.Source.Provider != model.VidCore {
					t.Fatal("fast 720p source was not retained as backup")
				}
			case <-timeout.Done():
				t.Fatal("pipelined lookup did not settle")
			}
			if cached && fast.calls.Load() != 0 {
				t.Fatal("cached adapter was queried again")
			}
		})
	}
}

type progressiveFixture struct{}

func (progressiveFixture) ProbeStream(ctx context.Context, candidates <-chan model.Candidate) []model.Probed {
	return progressiveFixture{}.ProbeProgress(ctx, candidates, nil)
}

func (progressiveFixture) ProbeProgress(ctx context.Context, candidates <-chan model.Candidate, progress func([]model.Probed)) []model.Probed {
	var results []model.Probed
	for candidate := range candidates {
		height := 1080
		bitrate := int64(2_000_000)
		speed := 5_000_000.0
		results = append(results, model.Probed{Source: candidate, Observation: model.Observation{
			At: time.Now(), Accessible: true, Format: model.HLS, ProtectionKnown: true, SamplesCompleted: 3, DeliveryBPS: &speed,
			Variant: &model.Variant{Height: &height, Bitrate: &bitrate, Codecs: []string{"avc1"}},
		}})
		if progress != nil {
			progress(results)
		}
	}
	return results
}

func TestVerifiedStartupReturnsBeforeSlowProviderAndCompleteWaiterGetsBackups(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	release := make(chan struct{})
	registry, err := provider.NewRegistry(&pipelineAdapter{providerID: model.VidCore}, &pipelineAdapter{providerID: model.VidLink, release: release})
	if err != nil {
		t.Fatal(err)
	}
	coordinator, err := New(registry, progressiveFixture{}, DefaultLimits(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer coordinator.Close()
	lookup := model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 7}, StartupHeight: 1080, Device: model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1"}}}
	initial, err := coordinator.Resolve(ctx, lookup)
	if err != nil || initial.Selected == nil || !initial.Pending {
		t.Fatalf("verified startup waited for unrelated provider: %v", err)
	}
	lookup.WaitForComplete = true
	done := make(chan model.Selection, 1)
	go func() { full, _ := coordinator.Resolve(ctx, lookup); done <- full }()
	select {
	case <-done:
		t.Fatal("complete waiter received provisional result")
	default:
	}
	close(release)
	full := <-done
	if full.Pending || full.Selected == nil || len(full.Backups) != 1 {
		t.Fatal("background lookup lost verified backups")
	}
	again, err := coordinator.Resolve(ctx, lookup)
	if err != nil || again.Pending || len(again.Backups) != 1 {
		t.Fatal("completed snapshot not retained for late enrichment")
	}
}

func TestCancelledCompletionJoinsCommittedStartupBackground(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	blocked := make(chan struct{})
	registry, _ := provider.NewRegistry(&pipelineAdapter{providerID: model.VidCore}, &pipelineAdapter{providerID: model.VidLink, release: blocked})
	coordinator, err := New(registry, progressiveFixture{}, DefaultLimits(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer coordinator.Close()
	lookup := model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 7}, StartupHeight: 1080, Device: model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1"}}}
	if _, err := coordinator.Resolve(ctx, lookup); err != nil {
		t.Fatal(err)
	}
	completion, stop := context.WithCancel(ctx)
	lookup.WaitForComplete = true
	finished := make(chan struct{})
	go func() { coordinator.Resolve(completion, lookup); close(finished) }()
	key, _ := requestKey(lookup)
	for {
		coordinator.mu.Lock()
		flight := coordinator.flights[key]
		attached := flight != nil && flight.completionRequested
		coordinator.mu.Unlock()
		if attached {
			break
		}
		if ctx.Err() != nil {
			t.Fatal("completion waiter did not attach")
		}
		time.Sleep(time.Millisecond)
	}
	stop()
	<-finished
	for {
		coordinator.mu.Lock()
		remaining := len(coordinator.flights)
		coordinator.mu.Unlock()
		if remaining == 0 {
			break
		}
		if ctx.Err() != nil {
			t.Fatal("cancelled startup background did not join")
		}
		time.Sleep(time.Millisecond)
	}
}

func TestCompletionPrefersActiveRoundOverPreviousSnapshot(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	release := make(chan struct{})
	registry, _ := provider.NewRegistry(&pipelineAdapter{providerID: model.VidCore}, &pipelineAdapter{providerID: model.VidLink, release: release})
	coordinator, err := New(registry, progressiveFixture{}, DefaultLimits(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer coordinator.Close()
	lookup := model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 7}, StartupHeight: 1080, Device: model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1"}}}
	initial, err := coordinator.Resolve(ctx, lookup)
	if err != nil || !initial.Pending {
		t.Fatalf("startup failed: %v", err)
	}
	old := cloneSelection(initial)
	old.Pending = false
	old.Selected.Candidate.Source.ID = "previous-round"
	key, _ := requestKey(lookup)
	encoded, _ := json.Marshal(old)
	coordinator.completed.Put(key, encoded, 10*time.Second, 16)
	lookup.WaitForComplete = true
	done := make(chan model.Selection, 1)
	go func() { full, _ := coordinator.Resolve(ctx, lookup); done <- full }()
	for {
		coordinator.mu.Lock()
		active := coordinator.flights[key]
		attached := active != nil && active.completionRequested
		coordinator.mu.Unlock()
		if attached {
			break
		}
		select {
		case <-done:
			t.Fatal("previous snapshot bypassed active completion owner")
		case <-ctx.Done():
			t.Fatal("completion did not attach")
		default:
			time.Sleep(time.Millisecond)
		}
	}
	close(release)
	select {
	case full := <-done:
		if full.Pending || full.Selected == nil || full.Selected.Candidate.Source.ID == "previous-round" || len(full.Backups) != 1 {
			t.Fatal("completion did not return the current full round")
		}
	case <-ctx.Done():
		t.Fatal("completion did not finish")
	}
}
