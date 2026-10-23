package api

import (
	"context"
	"sync"
	"time"
)

// Only identical creates or operations on the same playback serialize. Network
// work for another title must never hold up an explicit Play request.
type playbackLocks struct {
	mu      sync.Mutex
	entries map[string]*playbackLock
}

type playbackLock struct {
	token chan struct{}
	refs  int
}

func (locks *playbackLocks) acquire(ctx context.Context, key string, wait time.Duration) (func(), error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	locks.mu.Lock()
	entry := locks.entries[key]
	if entry == nil {
		if len(locks.entries) >= 64 {
			locks.mu.Unlock()
			return nil, &fault{429, "playback_busy"}
		}
		if locks.entries == nil {
			locks.entries = make(map[string]*playbackLock)
		}
		entry = &playbackLock{token: make(chan struct{}, 1)}
		locks.entries[key] = entry
	}
	entry.refs++
	locks.mu.Unlock()

	detach := func() {
		locks.mu.Lock()
		defer locks.mu.Unlock()
		entry.refs--
		if entry.refs == 0 {
			delete(locks.entries, key)
		}
	}
	release := func() {
		<-entry.token
		detach()
	}
	select {
	case entry.token <- struct{}{}:
		return release, nil
	default:
	}
	if wait > 0 {
		timer := time.NewTimer(wait)
		defer timer.Stop()
		select {
		case entry.token <- struct{}{}:
			return release, nil
		case <-ctx.Done():
			detach()
			return nil, ctx.Err()
		case <-timer.C:
		}
	}
	detach()
	return nil, &fault{429, "playback_busy"}
}
