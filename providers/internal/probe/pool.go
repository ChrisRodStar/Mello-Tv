package probe

import (
	"context"
	"errors"
	"stv-providers/internal/cache"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"sync"
	"time"
)

// Limits enforces concurrency, memory, byte count, and timeout limits on probing workers.
type Limits struct {
	Concurrent        int
	SamplesPerVariant int
	SampleBytes       int64
	TotalSampleBytes  int64
	SourceTimeout     time.Duration
	MaxVariants       int
}

// DefaultLimits provides production defaults calibrated for low-latency title resolution.
func DefaultLimits() Limits {
	return Limits{
		Concurrent:        4,
		SamplesPerVariant: 3,
		SampleBytes:       128 * 1024,
		TotalSampleBytes:  4 * 1024 * 1024,
		SourceTimeout:     6 * time.Second,
		MaxVariants:       6,
	}
}

// mediaClient allows offline probe fixtures while New still requires the guarded network client.
type mediaClient interface {
	Fetch(context.Context, network.Request) (network.Response, error)
}

// Pool coordinates concurrent media probing across candidates using bounded worker routines.
type Pool struct {
	client   mediaClient
	slots    chan struct{}
	limits   Limits
	history  *cache.Observations
	evidence cache.Values[[]byte]
	prefixes cache.Values[mp4Proof]
}

// New constructs a new media probing Pool.
func New(client *network.Client, limits Limits) (*Pool, error) {
	if client == nil || limits.Concurrent <= 0 || limits.SamplesPerVariant <= 0 || limits.SampleBytes <= 0 || limits.TotalSampleBytes <= 0 || limits.SourceTimeout <= 0 || limits.MaxVariants <= 0 {
		return nil, errors.New("client and positive probe limits required")
	}

	return &Pool{
		client:  client,
		slots:   make(chan struct{}, limits.Concurrent),
		limits:  limits,
		history: cache.NewObservations(),
	}, nil
}

// byteBudget safely tracks and bounds cumulative byte downloads across concurrent probes.
type byteBudget struct {
	mu        sync.Mutex
	remaining int64
	pending   int64
	changed   chan struct{}
}

// reserve attempts to allocate up to n bytes from the remaining budget.
func (b *byteBudget) reserve(n int64) int64 {
	b.mu.Lock()
	defer b.mu.Unlock()

	if n > b.remaining {
		n = b.remaining
	}
	b.remaining -= n
	return n
}

// reservePlaylist waits when in-flight reads could return enough unused bytes
// for the full per-playlist limit. Only permanently spent bytes may reduce that
// limit; pending reservations must not make an otherwise valid playlist fail.
func (b *byteBudget) reservePlaylist(ctx context.Context, limit int64) (int64, error) {
	for {
		if err := ctx.Err(); err != nil {
			return 0, err
		}
		b.mu.Lock()
		if b.remaining >= limit || b.pending == 0 {
			allocated := min(limit, b.remaining)
			b.remaining -= allocated
			b.pending += allocated
			b.mu.Unlock()
			return allocated, nil
		}
		if b.changed == nil {
			b.changed = make(chan struct{})
		}
		changed := b.changed
		b.mu.Unlock()

		select {
		case <-ctx.Done():
			return 0, ctx.Err()
		case <-changed:
		}
	}
}

// finishPlaylist settles an in-flight reservation and wakes waiting requests.
// Failed reads charge the entire reservation because transferred bytes are unknown.
func (b *byteBudget) finishPlaylist(reserved, spent int64) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.remaining += max(int64(0), reserved-spent)
	b.pending -= reserved
	if b.changed != nil {
		close(b.changed)
		b.changed = nil
	}
}

// ProbeAll adapts a finite candidate batch to the same lookup-scoped streaming
// round used by the coordinator. The feeder is cancelled and joined on return.
func (p *Pool) ProbeAll(ctx context.Context, candidates []model.Candidate) []model.Probed {
	feedContext, cancelFeed := context.WithCancel(ctx)
	defer cancelFeed()
	jobs := make(chan model.Candidate)
	feedDone := make(chan struct{})
	go func() {
		defer close(feedDone)
		defer close(jobs)
		for _, candidate := range candidates {
			select {
			case jobs <- candidate:
			case <-feedContext.Done():
				return
			}
		}
	}()

	results := p.ProbeStream(feedContext, jobs)
	cancelFeed()
	<-feedDone
	return results
}

// ProbeStream starts work as candidates arrive. One lookup shares the existing
// byte budgets, worker limit and single reliability round across all providers.
// The caller closes candidates after collection, or cancels ctx on abandonment.
func (p *Pool) ProbeStream(ctx context.Context, candidates <-chan model.Candidate) []model.Probed {
	return p.ProbeProgress(ctx, candidates, nil)
}

// ProbeProgress reports completed verified renditions without detaching workers.
// The callback runs serially; all workers still join before this method returns.
func (p *Pool) ProbeProgress(ctx context.Context, candidates <-chan model.Candidate, progress func([]model.Probed)) []model.Probed {
	sampleBudget := &byteBudget{remaining: p.limits.TotalSampleBytes}
	playlistBudget := &byteBudget{remaining: p.limits.TotalSampleBytes}
	results := make(chan []model.Probed, 1)

	var workers sync.WaitGroup
	for workerIndex := 0; workerIndex < p.limits.Concurrent; workerIndex++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			for {
				var candidate model.Candidate
				select {
				case <-ctx.Done():
					return
				case next, open := <-candidates:
					if !open {
						return
					}
					candidate = next
				}

				select {
				case p.slots <- struct{}{}:
				case <-ctx.Done():
					return
				}
				items, reused := p.reuse(ctx, candidate, sampleBudget, playlistBudget)
				if !reused {
					candidateCtx := context.WithValue(ctx, progressKey{}, func(item model.Probed) {
						if progress == nil {
							return
						}
						select {
						case results <- []model.Probed{item}:
						case <-ctx.Done():
						}
					})
					items = p.probe(candidateCtx, candidate, sampleBudget, playlistBudget)
					p.remember(ctx, candidate, items)
				}
				<-p.slots
				select {
				case results <- items:
				case <-ctx.Done():
					return
				}
			}
		}()
	}

	joined := make(chan struct{})
	go func() {
		defer close(joined)
		workers.Wait()
		close(results)
	}()

	var finalProbed []model.Probed
	positions := make(map[string]int)
	for items := range results {
		for _, item := range items {
			if progress == nil {
				finalProbed = append(finalProbed, item)
				continue
			}
			if position, exists := positions[item.Source.ID]; exists {
				finalProbed[position] = item
			} else {
				positions[item.Source.ID] = len(finalProbed)
				finalProbed = append(finalProbed, item)
			}
		}
		if progress != nil {
			progress(finalProbed)
		}
	}
	<-joined

	// Record only after the complete collection/probe round. Finishing an early
	// provider must not update history used by a later provider in this lookup.
	if ctx.Err() == nil {
		p.history.RecordRound(finalProbed)
	}
	return finalProbed
}

type progressKey struct{}

func reportRendition(ctx context.Context, item model.Probed) {
	if report, ok := ctx.Value(progressKey{}).(func(model.Probed)); ok {
		report(item)
	}
}

// WithHeightLimit avoids spending the probe round on renditions that exceed the
// caller's device/preference ceiling. Unknown metadata still follows eligibility.
type heightLimitKey struct{}

func WithHeightLimit(ctx context.Context, deviceHeight, preferenceHeight int) context.Context {
	ceiling := deviceHeight
	if preferenceHeight > 0 && (ceiling <= 0 || preferenceHeight < ceiling) {
		ceiling = preferenceHeight
	}
	return context.WithValue(ctx, heightLimitKey{}, ceiling)
}
