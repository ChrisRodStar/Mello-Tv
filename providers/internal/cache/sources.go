package cache

import (
	"slices"
	"sync"
	"time"

	"stv-providers/internal/model"
)

// cacheEntry holds a cached adapter result and its expiration timestamp.
type cacheEntry struct {
	value     model.AdapterResult
	expiresAt time.Time
}

// Sources is a thread-safe in-memory cache for resolved stream candidates.
type Sources struct {
	mu      sync.Mutex
	entries map[string]cacheEntry
	max     int
	ttl     time.Duration
}

// NewSources constructs a new Sources cache with a maximum capacity and default TTL.
func NewSources(max int, ttl time.Duration) *Sources {
	if max <= 0 {
		max = 128
	}
	if ttl <= 0 {
		ttl = 20 * time.Second
	}

	return &Sources{
		entries: make(map[string]cacheEntry),
		max:     max,
		ttl:     ttl,
	}
}

// CloneResult isolates nested mutable model data without JSON encoding/parsing.
func CloneResult(result model.AdapterResult) model.AdapterResult {
	cloned := result
	cloned.Issues = slices.Clone(result.Issues)
	if result.Candidates != nil {
		cloned.Candidates = make([]model.Candidate, len(result.Candidates))
		for i, candidate := range result.Candidates {
			cloned.Candidates[i] = candidate.Clone()
		}
	}
	return cloned
}

// Get retrieves an unexpired AdapterResult from the cache, if present.
func (c *Sources) Get(key string) (model.AdapterResult, bool) {
	if c == nil {
		return model.AdapterResult{}, false
	}

	c.mu.Lock()
	entry, found := c.entries[key]
	if !found || !time.Now().Before(entry.expiresAt) {
		delete(c.entries, key)
		c.mu.Unlock()
		return model.AdapterResult{}, false
	}
	c.mu.Unlock()

	return CloneResult(entry.value), true
}

// Put caches an AdapterResult, clamping its expiration to the earliest candidate expiry.
func (c *Sources) Put(key string, result model.AdapterResult) {
	if c == nil || len(result.Candidates) == 0 {
		return
	}

	now := time.Now()
	effectiveExpiry := now.Add(c.ttl)

	// Clamp cache expiration if any source candidate expires sooner (with a 5-second buffer).
	for _, candidate := range result.Candidates {
		if candidate.ExpiresAt != nil && !candidate.ExpiresAt.IsZero() {
			bufferedExpiry := candidate.ExpiresAt.Add(-5 * time.Second)
			if bufferedExpiry.Before(effectiveExpiry) {
				effectiveExpiry = bufferedExpiry
			}
		}
	}

	// Do not cache already expired results.
	if !effectiveExpiry.After(now) {
		return
	}

	cloned := CloneResult(result)
	c.mu.Lock()
	defer c.mu.Unlock()

	// Evict expired entries to reclaim space.
	for entryKey, entry := range c.entries {
		if !now.Before(entry.expiresAt) {
			delete(c.entries, entryKey)
		}
	}

	// Evict an arbitrary entry if maximum capacity is reached.
	if len(c.entries) >= c.max {
		for entryKey := range c.entries {
			delete(c.entries, entryKey)
			break
		}
	}

	c.entries[key] = cacheEntry{
		value:     cloned,
		expiresAt: effectiveExpiry,
	}
}

// Delete drops a provider result whose entire returned source set failed access.
func (c *Sources) Delete(key string) {
	if c == nil {
		return
	}
	c.mu.Lock()
	delete(c.entries, key)
	c.mu.Unlock()
}

// Len returns the current number of cached candidate entries.
func (c *Sources) Len() int {
	if c == nil {
		return 0
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	return len(c.entries)
}

