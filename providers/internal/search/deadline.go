package search

import "time"

// Limits defines timeouts, flight concurrency, and candidate collection caps for searches.
type Limits struct {
	Window                   time.Duration
	ProviderTimeout          time.Duration
	MaxFlights               int
	ConcurrentLookups        int
	MaxCandidatesPerProvider int
}

// DefaultLimits returns production-tuned limits ensuring lookups complete well within UI timeouts.
func DefaultLimits() Limits {
	return Limits{
		Window:                   25 * time.Second,
		ProviderTimeout:          18 * time.Second,
		MaxFlights:               64,
		ConcurrentLookups:        10,
		MaxCandidatesPerProvider: 32,
	}
}
