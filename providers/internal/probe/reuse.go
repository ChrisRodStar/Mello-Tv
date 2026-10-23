package probe

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"fmt"
	"net/http"
	"reflect"
	"sort"
	"strings"
	"time"
)

type mp4Proof struct {
	Digest [32]byte
	Total  int64
	Length int64
}

type remembered struct {
	Results []model.Probed
	MP4     mp4Proof
}

type reusePolicyKey struct{}
type reusePolicy struct {
	scope string
	fresh bool
}

// WithReuseScope keeps probe evidence private to the complete lookup context.
// A source refresh explicitly invalidates and bypasses that evidence.
func WithReuseScope(ctx context.Context, scope string, fresh bool) context.Context {
	return context.WithValue(ctx, reusePolicyKey{}, reusePolicy{scope: scope, fresh: fresh})
}

func evidenceKey(ctx context.Context, candidate model.Candidate) (string, bool) {
	policy, ok := ctx.Value(reusePolicyKey{}).(reusePolicy)
	if !ok || policy.scope == "" {
		return "", false
	}
	encoded, err := json.Marshal(candidate)
	if err != nil {
		return "", false
	}
	return fmt.Sprintf("%s:%x", policy.scope, sha256.Sum256(encoded)), true
}

func (p *Pool) remember(ctx context.Context, candidate model.Candidate, results []model.Probed) {
	key, ok := evidenceKey(ctx, candidate)
	if !ok {
		return
	}
	var verified []model.Probed
	lifetime := 15 * time.Second
	for _, result := range results {
		observation := result.Observation
		if !observation.Accessible || observation.Failure != "" || observation.SamplesCompleted < p.limits.SamplesPerVariant || observation.DeliveryBPS == nil {
			continue
		}
		lifetime = min(lifetime, time.Until(observation.At.Add(15*time.Second)))
		if result.Source.ExpiresAt != nil {
			lifetime = min(lifetime, time.Until(result.Source.ExpiresAt.Add(-5*time.Second)))
		}
		verified = append(verified, result)
	}
	if len(verified) == 0 || lifetime <= 0 {
		return
	}
	sort.SliceStable(verified, func(first, second int) bool {
		firstHeight, secondHeight := 0, 0
		if verified[first].Observation.Variant != nil && verified[first].Observation.Variant.Height != nil {
			firstHeight = *verified[first].Observation.Variant.Height
		}
		if verified[second].Observation.Variant != nil && verified[second].Observation.Variant.Height != nil {
			secondHeight = *verified[second].Observation.Variant.Height
		}
		return firstHeight > secondHeight
	})
	// Retain a small verified ladder. Each retained rendition is freshly
	// access-checked on reuse, with original measurement time and throughput.
	verified = verified[:min(len(verified), 2)]
	entry := remembered{Results: verified}
	if candidate.Format == model.MP4 {
		var found bool
		entry.MP4, found = p.prefixes.Get(key)
		if !found {
			return
		}
	}
	encoded, err := json.Marshal(entry)
	// At most 64 entries of 64 KiB: bounded to 4 MiB of retained JSON bodies.
	if err != nil || len(encoded) > 64*1024 {
		return
	}
	p.evidence.Put(key, encoded, lifetime, 64)
}

func (p *Pool) reuse(ctx context.Context, candidate model.Candidate, sampleBudget, playlistBudget *byteBudget) ([]model.Probed, bool) {
	key, ok := evidenceKey(ctx, candidate)
	if !ok {
		return nil, false
	}
	policy, _ := ctx.Value(reusePolicyKey{}).(reusePolicy)
	if policy.fresh {
		p.evidence.Delete(key)
		return nil, false
	}
	encoded, found := p.evidence.Get(key)
	if !found {
		return nil, false
	}
	var entry remembered
	if json.Unmarshal(encoded, &entry) != nil || len(entry.Results) == 0 {
		p.evidence.Delete(key)
		return nil, false
	}
	checkCtx, cancel := context.WithTimeout(ctx, 1500*time.Millisecond)
	defer cancel()
	if p.checkEvidence(checkCtx, candidate, entry, sampleBudget, playlistBudget) != nil {
		p.evidence.Delete(key)
		return nil, false
	}
	return entry.Results, true
}

func (p *Pool) checkEvidence(ctx context.Context, candidate model.Candidate, entry remembered, sampleBudget, playlistBudget *byteBudget) error {
	if candidate.ExpiresAt != nil && !candidate.ExpiresAt.After(time.Now().Add(5*time.Second)) {
		return fmt.Errorf("source near expiry")
	}
	if candidate.Format == model.MP4 {
		if entry.MP4.Length <= 0 || sampleBudget.reserve(entry.MP4.Length) != entry.MP4.Length {
			return fmt.Errorf("access check budget exhausted")
		}
		response, _, err := p.mp4Range(ctx, candidate, 0, entry.MP4.Length, entry.MP4.Total)
		if err != nil {
			return err
		}
		if sha256.Sum256(response.Body) != entry.MP4.Digest {
			return fmt.Errorf("MP4 prefix changed")
		}
		return nil
	}
	master, err := p.fetchPlaylist(ctx, candidate, candidate.URL, playlistBudget)
	if err != nil {
		return err
	}
	for _, result := range entry.Results {
		variant := result.Observation.Variant
		media := master
		if variant != nil {
			matched := false
			for _, current := range master.Metadata.Variants {
				if reflect.DeepEqual(current, *variant) {
					matched = true
					break
				}
			}
			if !matched {
				return fmt.Errorf("rendition changed")
			}
			media, err = p.fetchPlaylist(ctx, candidate, variant.URL, playlistBudget)
			if err != nil {
				return err
			}
		}
		if len(media.Metadata.Variants) != 0 || media.UnsupportedProtection || master.UnsupportedProtection || (media.Encrypted || master.Encrypted) != result.Observation.Encrypted || len(media.Samples) == 0 {
			return fmt.Errorf("playlist protection or shape changed")
		}
		// Track/rendition changes invalidate old language and codec evidence.
		tracks := append(append([]model.Track(nil), media.Metadata.Tracks...), master.Metadata.Tracks...)
		if variant == nil {
			tracks = media.Metadata.Tracks
		}
		if !reflect.DeepEqual(tracks, result.Observation.Metadata.Tracks) {
			return fmt.Errorf("tracks changed")
		}
		segment := media.Samples[0]
		length := int64(4096)
		if segment.RangeLength > 0 {
			length = min(length, segment.RangeLength)
		}
		if sampleBudget.reserve(length) != length {
			return fmt.Errorf("access check budget exhausted")
		}
		response, err := p.client.Fetch(ctx, network.Request{
			URL:     segment.URL,
			Context: candidate.Context,
			Headers: map[string]string{"Range": fmt.Sprintf("bytes=%d-%d", segment.RangeStart, segment.RangeStart+length-1)},
			Limit:   length,
			Sample:  true,
		})
		if err != nil {
			return err
		}
		if segment.RangeStart > 0 && response.Status != http.StatusPartialContent {
			return fmt.Errorf("media range ignored")
		}
		if response.Status == http.StatusPartialContent {
			var actualStart, actualEnd int64
			if _, err := fmt.Sscanf(response.Header.Get("Content-Range"), "bytes %d-%d/", &actualStart, &actualEnd); err != nil || actualStart != segment.RangeStart || actualEnd < actualStart || actualEnd-actualStart+1 < int64(len(response.Body)) {
				return fmt.Errorf("media range changed")
			}
		}
		contentType := strings.ToLower(response.Header.Get("Content-Type"))
		if (response.Status != http.StatusOK && response.Status != http.StatusPartialContent) || len(response.Body) == 0 || strings.Contains(contentType, "text/") || strings.Contains(contentType, "json") || strings.Contains(contentType, "mpegurl") || strings.HasPrefix(strings.TrimSpace(string(response.Body[:min(len(response.Body), 64)])), "#EXTM3U") {
			return fmt.Errorf("media access changed")
		}
	}
	return nil
}
