package search

import (
	"context"
	"encoding/json"
	"errors"
	"family-stream-providers/internal/cache"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/probe"
	"family-stream-providers/internal/provider"
	"family-stream-providers/internal/selection"
	"net/url"
	"strconv"
	"sync"
	"time"
)

// Prober defines the contract for probing media stream candidates.
type Prober interface {
	ProbeStream(ctx context.Context, candidates <-chan model.Candidate) []model.Probed
}

// Coordinator orchestrates multi-provider search, deduplication, candidate caching, probing, and ranking.
type Coordinator struct {
	registry   *provider.Registry
	prober     Prober
	limits     Limits
	sources    *cache.Sources
	slots      chan struct{}
	mu         sync.Mutex
	flights    map[string]*flight
	events     chan model.ProviderStatus
	completed  cache.Values[[]byte]
	closed     chan struct{}
	closeOnce  sync.Once
	shutdownWg sync.WaitGroup
}

// New constructs and initializes a new search Coordinator.
func New(registry *provider.Registry, prober Prober, limits Limits, diagnostic func(model.ProviderStatus)) (*Coordinator, error) {
	if registry == nil || prober == nil {
		return nil, errors.New("registry and prober required")
	}

	if limits.Window <= 0 || limits.ProviderTimeout <= 0 || limits.MaxFlights <= 0 || limits.ConcurrentLookups <= 0 || limits.MaxCandidatesPerProvider <= 0 {
		return nil, errors.New("positive lookup limits required")
	}

	coord := &Coordinator{
		registry: registry,
		prober:   prober,
		limits:   limits,
		sources:  cache.NewSources(256, 30*time.Minute),
		slots:    make(chan struct{}, limits.ConcurrentLookups),
		flights:  make(map[string]*flight),
		closed:   make(chan struct{}),
	}

	if diagnostic != nil {
		coord.events = make(chan model.ProviderStatus, 64)
		coord.shutdownWg.Add(1)
		go func() {
			defer coord.shutdownWg.Done()
			for {
				select {
				case <-coord.closed:
					return
				case status := <-coord.events:
					diagnostic(status)
				}
			}
		}()
	}

	return coord, nil
}

// Resolve executes a coordinated lookup across providers with in-flight request coalescing.
func (c *Coordinator) Resolve(ctx context.Context, lookup model.Lookup) (model.Selection, error) {
	if err := ctx.Err(); err != nil {
		return model.Selection{}, err
	}

	if err := lookup.Title.Validate(); err != nil {
		return model.Selection{}, model.Fail(model.Unsupported, "input", "invalid title")
	}

	lookup = cloneLookup(lookup)
	key, err := requestKey(lookup)
	if err != nil {
		return model.Selection{}, err
	}

	select {
	case <-c.closed:
		return model.Selection{}, model.Fail(model.Unavailable, "lookup", "coordinator closed")
	default:
	}
	cacheKey, err := sourceKey(lookup)
	if err != nil {
		return model.Selection{}, err
	}
	c.mu.Lock()
	select {
	case <-c.closed:
		c.mu.Unlock()
		return model.Selection{}, model.Fail(model.Unavailable, "lookup", "coordinator closed")
	default:
	}

	activeFlight := c.flights[key]
	if activeFlight != nil && activeFlight.abandoned {
		c.mu.Unlock()
		return model.Selection{}, model.Fail(model.Unavailable, "lookup", "lookup cancellation in progress")
	}
	if activeFlight == nil {
		// A completion caller belongs to an active newer round before any older
		// retained snapshot. Only late callers without a flight use the cache.
		if lookup.WaitForComplete && !lookup.Refresh {
			if encoded, found := c.completed.Get(key); found {
				c.mu.Unlock()
				var completed model.Selection
				if json.Unmarshal(encoded, &completed) != nil {
					return model.Selection{}, model.Fail(model.Unavailable, "lookup", "invalid completed snapshot")
				}
				return completed, nil
			}
		}
		if len(c.flights) >= c.limits.MaxFlights {
			c.mu.Unlock()
			return model.Selection{}, model.Fail(model.Unavailable, "lookup", "lookup queue full")
		}

		jobCtx, cancelJob := context.WithTimeout(context.Background(), c.limits.Window)
		activeFlight = &flight{
			done:   make(chan struct{}),
			ready:  make(chan struct{}),
			cancel: cancelJob,
		}
		c.flights[key] = activeFlight

		c.shutdownWg.Add(1)
		go func() {
			defer c.shutdownWg.Done()
			selectionOutcome, runErr := c.run(jobCtx, cacheKey, lookup, func(initial model.Selection) {
				if !selection.StartupReady(lookup, initial) {
					return
				}
				initial.Pending = true
				snapshot := cloneSelection(initial)
				c.mu.Lock()
				if !activeFlight.published {
					activeFlight.initial = snapshot
					activeFlight.published = true
					close(activeFlight.ready)
				}
				c.mu.Unlock()
			})
			if runErr == nil {
				if encoded, encodeErr := json.Marshal(selectionOutcome); encodeErr == nil && len(encoded) <= 2*1024*1024 {
					c.completed.Put(key, encoded, 10*time.Second, 16)
				}
			}

			c.mu.Lock()
			activeFlight.result = selectionOutcome
			activeFlight.err = runErr
			if c.flights[key] == activeFlight {
				delete(c.flights, key)
			}
			close(activeFlight.done)
			c.mu.Unlock()

			cancelJob()
		}()
	}

	if lookup.WaitForComplete {
		activeFlight.completionRequested = true
	}
	activeFlight.waiters++
	c.mu.Unlock()

	defer func() {
		c.mu.Lock()
		activeFlight.waiters--
		if activeFlight.waiters == 0 && (!activeFlight.published || activeFlight.completionRequested) {
			activeFlight.abandoned = true
			activeFlight.cancel()
			// Retain admission until the cancelled owner and its adapters join.
			// Rapid caller cancellation must not create uncounted live flights.
		}
		c.mu.Unlock()
	}()

	var ready <-chan struct{}
	if !lookup.WaitForComplete {
		ready = activeFlight.ready
	}
	select {
	case <-ctx.Done():
		return model.Selection{}, ctx.Err()
	case <-ready:
		if selection.StartupReady(lookup, activeFlight.initial) {
			return cloneSelection(activeFlight.initial), nil
		}
		select {
		case <-ctx.Done():
			return model.Selection{}, ctx.Err()
		case <-activeFlight.done:
			return cloneSelection(activeFlight.result), activeFlight.err
		}
	case <-activeFlight.done:
		return cloneSelection(activeFlight.result), activeFlight.err
	}
}

// validResult verifies that candidates returned by an adapter adhere to strict security and ownership bounds.
func validResult(result model.AdapterResult, providerID model.ProviderID, lookup model.Lookup, maxCandidates int) bool {
	if len(result.Candidates) > maxCandidates {
		return false
	}

	seenIDs := make(map[string]bool)
	for _, candidate := range result.Candidates {
		if seenIDs[candidate.ID] {
			return false
		}
		seenIDs[candidate.ID] = true

		parsedURL, err := url.Parse(candidate.URL)
		if err != nil ||
			network.ValidateURL(parsedURL) != nil ||
			candidate.Provider != providerID ||
			candidate.ID == "" ||
			!candidate.Title.SameContent(lookup.Title) ||
			(lookup.Title.IMDbID != "" && candidate.Title.IMDbID != "" && lookup.Title.IMDbID != candidate.Title.IMDbID) ||
			(lookup.Title.MyAnimeListID != nil && candidate.Title.MyAnimeListID != nil && *lookup.Title.MyAnimeListID != *candidate.Title.MyAnimeListID) {
			return false
		}
	}

	return true
}

// run overlaps bounded provider collection with probing, then ranks the complete round.
func (c *Coordinator) run(ctx context.Context, cacheBaseKey string, lookup model.Lookup, publish func(model.Selection)) (model.Selection, error) {
	type adapterOutcome struct {
		result model.AdapterResult
		status model.ProviderStatus
	}

	ctx = network.WithManifestCache(ctx)
	ctx = probe.WithReuseScope(ctx, cacheBaseKey, lookup.Refresh)
	ctx = probe.WithHeightLimit(ctx, lookup.Device.MaxHeight, lookup.Preferences.MaxHeight)
	lookupCtx, stopLookups := context.WithCancel(ctx)
	probeContext, stopProbes := context.WithCancel(ctx)
	adapters := c.registry.Adapters()
	// The registry has four slots, each validated against its candidate cap.
	// Buffering at most that aggregate cap keeps probe backpressure from delaying
	// collection or causing an otherwise on-time provider result to be discarded.
	candidates := make(chan model.Candidate, len(adapters)*c.limits.MaxCandidatesPerProvider)
	// Each adapter publishes exactly once; the fixed four-slot buffer lets all
	// adapters settle even after collection has stopped, without detached sends.
	outcomes := make(chan adapterOutcome, len(adapters))
	probed := make(chan []model.Probed, 1)
	var adapterWg sync.WaitGroup
	var probeOwner sync.WaitGroup
	candidatesClosed := false
	closeCandidates := func() {
		if !candidatesClosed {
			close(candidates)
			candidatesClosed = true
		}
	}
	defer func() {
		stopLookups()
		closeCandidates()
		stopProbes()
		adapterWg.Wait()
		probeOwner.Wait()
	}()
	probeOwner.Add(1)
	go func() {
		defer probeOwner.Done()
		if progressive, ok := c.prober.(interface {
			ProbeProgress(context.Context, <-chan model.Candidate, func([]model.Probed)) []model.Probed
		}); ok && lookup.StartupHeight > 0 {
			probed <- progressive.ProbeProgress(probeContext, candidates, func(items []model.Probed) {
				publish(selection.Select(lookup, items))
			})
		} else {
			probed <- c.prober.ProbeStream(probeContext, candidates)
		}
	}()

	for _, registeredAdapter := range adapters {
		adapterWg.Add(1)
		go func(adapterItem provider.Adapter) {
			defer adapterWg.Done()
			startTime := time.Now()
			status := model.ProviderStatus{
				Provider: adapterItem.ID(),
			}

			var result model.AdapterResult
			var resolveErr error

			cacheKey := cacheBaseKey + ":" + string(adapterItem.ID())
			if cached, found := c.sources.Get(cacheKey); found && !lookup.Refresh {
				result = cached
			} else {
				providerCtx, cancelProvider := context.WithTimeout(lookupCtx, c.limits.ProviderTimeout)
				defer cancelProvider()

				select {
				case c.slots <- struct{}{}:
					result, resolveErr = adapterItem.Resolve(providerCtx, cloneLookup(lookup))
					<-c.slots
				case <-providerCtx.Done():
					resolveErr = providerCtx.Err()
				}

				if resolveErr == nil && !validResult(result, adapterItem.ID(), lookup, c.limits.MaxCandidatesPerProvider) {
					resolveErr = model.Fail(model.ProtocolChange, "candidates", "adapter returned invalid candidate data")
					result = model.AdapterResult{}
				}

				if resolveErr == nil && providerCtx.Err() == nil {
					c.sources.Put(cacheKey, result)
				}
			}

			status.Duration = time.Since(startTime)
			status.CandidateCount = len(result.Candidates)

			if resolveErr != nil {
				status.Failure = model.Category(resolveErr)
				result = model.AdapterResult{}
				status.CandidateCount = 0
			}

			outcomes <- adapterOutcome{
				result: result,
				status: status,
			}
		}(registeredAdapter)
	}

	var statuses []model.ProviderStatus
	seenIdentities := make(map[string]bool)
	candidateCount := 0

	// Keep the adapter collection deadline independent of ongoing media probes.
	collectionTimer := time.NewTimer(c.limits.ProviderTimeout)
	defer collectionTimer.Stop()

collectLoop:
	for range adapters {
		select {
		case outcome := <-outcomes:
			statuses = append(statuses, outcome.status)
			c.emit(outcome.status)
			for _, candidate := range outcome.result.Candidates {
				if candidate.Title.IMDbID == "" {
					candidate.Title.IMDbID = lookup.Title.IMDbID
				}
				if candidate.Title.MyAnimeListID == nil {
					candidate.Title.MyAnimeListID = lookup.Title.MyAnimeListID
				}
				prefixedID := string(candidate.Provider) + ":" + candidate.ID
				if seenIdentities[prefixedID] {
					continue
				}
				seenIdentities[prefixedID] = true
				candidate.ID = prefixedID
				candidates <- candidate
				candidateCount++
			}

		case <-collectionTimer.C:
			break collectLoop

		case <-ctx.Done():
			return model.Selection{Providers: statuses}, ctx.Err()
		}
	}

	stopLookups()
	closeCandidates()

	// Account for timed-out adapters.
	receivedProviders := make(map[model.ProviderID]bool)
	for _, status := range statuses {
		receivedProviders[status.Provider] = true
	}

	for _, registeredAdapter := range adapters {
		if !receivedProviders[registeredAdapter.ID()] {
			timedOutStatus := model.ProviderStatus{
				Provider: registeredAdapter.ID(),
				Duration: c.limits.ProviderTimeout,
				Failure:  model.Timeout,
			}
			statuses = append(statuses, timedOutStatus)
			c.emit(timedOutStatus)
		}
	}

	// Probing already ran during collection. Do not select an early low-quality
	// winner: retain the existing final ranking across the complete probe round.
	probedResults := <-probed
	// A set of dead links must not survive for the provider-result cache TTL.
	// Budget/eligibility failures do not establish that the upstream links died.
	for _, adapter := range adapters {
		failed, accessible := false, false
		for _, item := range probedResults {
			if item.Source.Provider != adapter.ID() {
				continue
			}
			accessible = accessible || item.Observation.Accessible
			category := item.Observation.Failure
			failed = failed || category == model.UpstreamFailure || category == model.ProtocolChange || category == model.Timeout
		}
		if failed && !accessible {
			c.sources.Delete(cacheBaseKey + ":" + string(adapter.ID()))
		}
	}
	selectionResult := selection.Select(lookup, probedResults)
	selectionResult.Providers = statuses

	if ctx.Err() != nil {
		return selectionResult, ctx.Err()
	}

	if selectionResult.Selected == nil {
		return selectionResult, model.Fail(
			model.Unavailable,
			"selection",
			"no eligible measured source among "+strconv.Itoa(candidateCount)+" candidates",
		)
	}

	return selectionResult, nil
}

// emit dispatches provider status events to the optional diagnostic channel if not closed.
func (c *Coordinator) emit(status model.ProviderStatus) {
	if c.events == nil {
		return
	}

	select {
	case <-c.closed:
		return
	case c.events <- status:
	default:
	}
}

// Close gracefully cancels outstanding background lookup tasks and releases coordinator resources.
func (c *Coordinator) Close() {
	c.closeOnce.Do(func() {
		c.mu.Lock()
		close(c.closed)
		for _, activeFlight := range c.flights {
			activeFlight.cancel()
		}
		c.mu.Unlock()

		c.shutdownWg.Wait()
	})
}
