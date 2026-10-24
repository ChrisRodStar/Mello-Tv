package cache

import (
	"net/url"
	"sync"
	"time"

	"stv-providers/internal/model"
)

// reliabilityStats tracks recent probe success metrics with decay over time.
type reliabilityStats struct {
	updatedAt time.Time
	successes int
	samples   int
}

// Observations maintains an in-memory history of recent probe outcomes per server hostname.
type Observations struct {
	mu      sync.Mutex
	entries map[string]reliabilityStats
}

// NewObservations initializes an empty probe observations cache.
func NewObservations() *Observations {
	return &Observations{
		entries: make(map[string]reliabilityStats),
	}
}

// observationKey identifies the provider/server/CDN whose recent availability is measured.
func observationKey(candidate model.Candidate) string {
	observedURL := candidate.URL
	if candidate.SelectedVariantURL != "" {
		observedURL = candidate.SelectedVariantURL
	}
	parsedURL, err := url.Parse(observedURL)
	if err != nil || parsedURL.Hostname() == "" {
		return ""
	}
	return string(candidate.Provider) + ":" + candidate.Server + ":" + parsedURL.Hostname()
}

// RecordRound records at most one availability outcome per provider/server/CDN per lookup.
// All renditions receive the same history snapshot; three variants are not three independent rounds.
// A mixed success/failure round is conservatively recorded as a failure for that endpoint.
func (o *Observations) RecordRound(candidates []model.Probed) {
	if o == nil || len(candidates) == 0 {
		return
	}

	roundOutcomes := make(map[string]bool)
	for _, candidate := range candidates {

		key := observationKey(candidate.Source)
		if key == "" {
			continue
		}

		observation := candidate.Observation
		success := observation.Accessible && observation.Failure == ""
		if !success && observation.Failure != model.UpstreamFailure && observation.Failure != model.ProtocolChange {
			continue
		}

		previousSuccess, recorded := roundOutcomes[key]
		roundOutcomes[key] = success && (!recorded || previousSuccess)
	}

	o.mu.Lock()
	defer o.mu.Unlock()

	now := time.Now()
	roundReliability := make(map[string]*float64)
	for key, success := range roundOutcomes {
		roundReliability[key] = o.record(key, success, now)
	}

	for index := range candidates {
		candidates[index].Observation.Reliability = roundReliability[observationKey(candidates[index].Source)]
	}
}

// record updates one endpoint's bounded history while the caller holds the cache lock.
func (o *Observations) record(key string, success bool, now time.Time) *float64 {
	stats := o.entries[key]

	// Reset stale history after 10 minutes of inactivity.
	if now.Sub(stats.updatedAt) > 10*time.Minute {
		stats = reliabilityStats{}
	}

	// Apply half-life decay to balance recent observations against older history.
	if stats.samples >= 20 {
		stats.samples /= 2
		stats.successes /= 2
	}

	stats.samples++
	if success {
		stats.successes++
	}
	stats.updatedAt = now

	// Enforce maximum capacity with lazy eviction of expired entries.
	if _, exists := o.entries[key]; !exists && len(o.entries) >= 1024 {
		for entryKey, entryStats := range o.entries {
			if now.Sub(entryStats.updatedAt) > 10*time.Minute {
				delete(o.entries, entryKey)
			}
		}

		if len(o.entries) >= 1024 {
			for entryKey := range o.entries {
				delete(o.entries, entryKey)
				break
			}
		}
	}

	o.entries[key] = stats

	// Require at least 3 samples before publishing an estimated reliability ratio.
	if stats.samples < 3 {
		return nil
	}

	rate := float64(stats.successes) / float64(stats.samples)
	return &rate
}

// Len returns the current count of tracked endpoint records.
func (o *Observations) Len() int {
	if o == nil {
		return 0
	}
	o.mu.Lock()
	defer o.mu.Unlock()
	return len(o.entries)
}

