package api

import (
	"context"
	"encoding/json"
	"time"
)

// scheduleBackups owns the remainder of an opted-in startup lookup. It adds only
// verified fallback metadata; the relay keeps the playing generation and grants.
func (s *Service) scheduleBackups(playback Playback, profile string, initial json.RawMessage) {
	var selection struct {
		Pending bool `json:"pending"`
	}
	if json.Unmarshal(initial, &selection) != nil || !selection.Pending {
		return
	}
	s.lifecycleMu.Lock()
	if s.stopping.Load() {
		s.lifecycleMu.Unlock()
		return
	}
	// Two completion jobs leave two of the four provider HTTP connections for
	// foreground requests. Saturation retains the already imported backups.
	select {
	case s.enrichmentSlots <- struct{}{}:
	default:
		s.lifecycleMu.Unlock()
		return
	}
	ctx, cancel := context.WithTimeout(s.ctx, 35*time.Second)
	if s.enrichmentCancels == nil {
		s.enrichmentCancels = make(map[string]context.CancelFunc)
	}
	s.enrichmentCancels[playback.ID] = cancel
	s.wg.Add(1)
	s.lifecycleMu.Unlock()
	go func() {
		defer s.wg.Done()
		defer func() { <-s.enrichmentSlots }()
		defer cancel()
		defer func() {
			s.lifecycleMu.Lock()
			delete(s.enrichmentCancels, playback.ID)
			s.lifecycleMu.Unlock()
		}()
		full, err := s.resolveMode(ctx, profile, playback.Request, false, true)
		if err != nil {
			return
		}
		release, err := s.playLocks.acquire(ctx, "session/"+playback.ID, time.Second)
		if err != nil {
			return
		}
		current, err := s.loadPlayback(ctx, playback.ID, profile)
		release()
		if err != nil || current.Revoked || current.Ready.SID != playback.Ready.SID || current.Ready.Generation != playback.Ready.Generation {
			return
		}
		relayCtx, cancelRelay := context.WithTimeout(ctx, time.Second)
		defer cancelRelay()
		_, _ = s.private(relayCtx, "relay", "POST", "/internal/v1/sessions/"+current.Ready.SID+"/backups", map[string]any{
			"owner_id":            profile,
			"expected_generation": current.Ready.Generation,
			"selection":           json.RawMessage(full),
		}, "", 32768)
	}()
}

func (s *Service) cancelBackups(playbackID string) {
	s.lifecycleMu.Lock()
	cancel := s.enrichmentCancels[playbackID]
	s.lifecycleMu.Unlock()
	if cancel != nil {
		cancel()
	}
}
