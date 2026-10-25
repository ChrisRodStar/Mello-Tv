package probe

import (
	"context"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"fmt"
	"sort"
	"strings"
	"time"
)

// fetchPlaylist downloads and parses an HLS manifest, deducting bytes from the caller's budget.
func (p *Pool) fetchPlaylist(
	ctx context.Context,
	candidate model.Candidate,
	manifestURL string,
	budget *byteBudget,
) (Playlist, error) {
	bytesReserved, err := budget.reservePlaylist(ctx, 1024*1024)
	if err != nil {
		return Playlist{}, err
	}
	if bytesReserved <= 0 {
		return Playlist{}, model.Fail(model.Unavailable, "playlist", "probe byte budget exhausted")
	}

	resp, err := p.client.Fetch(ctx, network.Request{
		URL:     manifestURL,
		Context: candidate.Context,
		Limit:   bytesReserved,
	})
	if err != nil {
		budget.finishPlaylist(bytesReserved, bytesReserved)
		return Playlist{}, err
	}
	budget.finishPlaylist(bytesReserved, int64(len(resp.Body)))

	if resp.Status < 200 || resp.Status >= 300 {
		return Playlist{}, model.Fail(model.UpstreamFailure, "playlist", "playlist HTTP rejected")
	}

	parsed, err := ParsePlaylist(resp.Body, resp.URL)
	if err != nil {
		return Playlist{}, model.Fail(model.ProtocolChange, "playlist", "malformed or unsupported HLS response")
	}

	return parsed, nil
}

// sampleSegment requests a bounded chunk of media to verify transport viability, latency, and throughput.
func (p *Pool) sampleSegment(
	ctx context.Context,
	candidate model.Candidate,
	playlist Playlist,
	variant *model.Variant,
	budget *byteBudget,
) model.Probed {
	observation := model.Observation{
		At:              time.Now(),
		Format:          model.HLS,
		Metadata:        playlist.Metadata,
		Variant:         variant,
		Encrypted:       playlist.Encrypted,
		ProtectionKnown: !playlist.UnsupportedProtection,
	}

	result := model.Probed{
		Source:      candidate,
		Observation: observation,
	}

	if playlist.UnsupportedProtection {
		result.Observation.Failure = model.Unsupported
		return result
	}

	sampleSize := p.limits.SampleBytes
	if playlist.RangeLength > 0 && playlist.RangeLength < sampleSize {
		sampleSize = playlist.RangeLength
	}

	bytesAllocated := budget.reserve(sampleSize)
	if bytesAllocated <= 0 {
		result.Observation.Failure = model.Unavailable
		return result
	}

	rangeEnd := playlist.RangeStart + bytesAllocated - 1
	if rangeEnd < playlist.RangeStart {
		result.Observation.Failure = model.ProtocolChange
		return result
	}

	resp, err := p.client.Fetch(ctx, network.Request{
		URL:     playlist.SampleURL,
		Context: candidate.Context,
		Headers: map[string]string{
			"Range": fmt.Sprintf("bytes=%d-%d", playlist.RangeStart, rangeEnd),
		},
		Limit:  bytesAllocated,
		Sample: true,
	})
	if err != nil {
		result.Observation.Failure = model.Category(err)
		return result
	}

	if resp.Status != 200 && resp.Status != 206 {
		result.Observation.Failure = model.UpstreamFailure
		return result
	}

	if playlist.RangeStart > 0 && resp.Status != 206 {
		result.Observation.Failure = model.Unsupported
		return result
	}

	if resp.Status == 206 {
		var actualStart, actualStop int64
		if _, err := fmt.Sscanf(resp.Header.Get("Content-Range"), "bytes %d-%d/", &actualStart, &actualStop); err != nil || actualStart != playlist.RangeStart || actualStop < actualStart {
			result.Observation.Failure = model.ProtocolChange
			return result
		}
	}

	contentType := strings.ToLower(resp.Header.Get("Content-Type"))
	headerSnippet := strings.TrimSpace(string(resp.Body[:min(len(resp.Body), 64)]))

	// Reject HTML error pages or redirected playlists masquerading as media segments.
	textualType := strings.Contains(contentType, "text/html") ||
		strings.Contains(contentType, "json") || strings.Contains(contentType, "mpegurl")
	if len(resp.Body) == 0 ||
		(textualType && !IsTransportStream(resp.Body)) ||
		strings.HasPrefix(headerSnippet, "#EXTM3U") {
		result.Observation.Failure = model.ProtocolChange
		return result
	}

	result.Observation.Accessible = true
	result.Observation.FirstByte = &resp.FirstByte
	result.Observation.SampleBytes = int64(len(resp.Body))

	// Calculate throughput for samples with measurable transfer duration.
	if len(resp.Body) >= 16*1024 && resp.Transfer >= time.Millisecond {
		bps := float64(len(resp.Body)-1) * 8.0 / resp.Transfer.Seconds()
		result.Observation.DeliveryBPS = &bps
	}

	return result
}

// sample checks evenly spaced segments from the available playlist, including later seek positions.
// The slowest measured transfer and longest first-byte delay are conservative estimates, not playback proof.
func (p *Pool) sample(
	ctx context.Context,
	candidate model.Candidate,
	playlist Playlist,
	variant *model.Variant,
	budget *byteBudget,
) model.Probed {
	sampleCount := min(p.limits.SamplesPerVariant, len(playlist.Samples))
	if sampleCount == 0 {
		return model.Probed{
			Source: candidate,
			Observation: model.Observation{
				At:      time.Now(),
				Failure: model.ProtocolChange,
			},
		}
	}

	var combined model.Probed
	allSpeedsMeasured := true

	for sampleIndex := 0; sampleIndex < sampleCount; sampleIndex++ {
		segmentIndex := 0
		if sampleCount > 1 {
			segmentIndex = sampleIndex * (len(playlist.Samples) - 1) / (sampleCount - 1)
		}

		segment := playlist.Samples[segmentIndex]
		samplePlaylist := playlist
		samplePlaylist.SampleURL = segment.URL
		samplePlaylist.RangeStart = segment.RangeStart
		samplePlaylist.RangeLength = segment.RangeLength

		probed := p.sampleSegment(ctx, candidate, samplePlaylist, variant, budget)

		if sampleIndex == 0 {
			combined = probed
		} else {
			combined.Observation.SampleBytes += probed.Observation.SampleBytes

			if probed.Observation.FirstByte != nil &&
				(combined.Observation.FirstByte == nil || *probed.Observation.FirstByte > *combined.Observation.FirstByte) {
				combined.Observation.FirstByte = probed.Observation.FirstByte
			}

			if probed.Observation.DeliveryBPS != nil &&
				(combined.Observation.DeliveryBPS == nil || *probed.Observation.DeliveryBPS < *combined.Observation.DeliveryBPS) {
				combined.Observation.DeliveryBPS = probed.Observation.DeliveryBPS
			}
		}

		if !probed.Observation.Accessible || probed.Observation.Failure != "" {
			combined.Observation.Accessible = false
			combined.Observation.Failure = probed.Observation.Failure
			combined.Observation.DeliveryBPS = nil
			return combined
		}

		combined.Observation.SamplesCompleted++
		if probed.Observation.DeliveryBPS == nil && probed.Observation.SampleBytes >= 16*1024 {
			allSpeedsMeasured = false
		}
	}

	if !allSpeedsMeasured || combined.Observation.DeliveryBPS == nil {
		combined.Observation.DeliveryBPS = nil
	}

	return combined
}

// probe resolves master playlists, discovers child variants, and samples renditions within configured limits.
func (p *Pool) probe(
	ctx context.Context,
	candidate model.Candidate,
	sampleBudget *byteBudget,
	playlistBudget *byteBudget,
) []model.Probed {
	probeCtx, cancel := context.WithTimeout(ctx, p.limits.SourceTimeout)
	defer cancel()

	makeFailureResult := func(err error) []model.Probed {
		return []model.Probed{
			{
				Source: candidate,
				Observation: model.Observation{
					At:      time.Now(),
					Failure: model.Category(err),
				},
			},
		}
	}

	// Reject candidates expiring in under 5 seconds.
	if candidate.ExpiresAt != nil && !candidate.ExpiresAt.After(time.Now().Add(5*time.Second)) {
		return makeFailureResult(model.Fail(model.Unavailable, "expiry", "source expired or near expiry"))
	}

	if candidate.Format == model.MP4 {
		return []model.Probed{p.probeMP4(probeCtx, candidate, sampleBudget, playlistBudget)}
	}

	if candidate.Format != model.HLS && candidate.Format != model.UnknownFormat {
		return makeFailureResult(model.Fail(model.Unsupported, "probe", "source format probing not implemented"))
	}

	playlist, err := p.fetchPlaylist(probeCtx, candidate, candidate.URL, playlistBudget)
	if err != nil {
		return makeFailureResult(err)
	}

	// Direct media playlist (single rendition).
	if len(playlist.Metadata.Variants) == 0 {
		return []model.Probed{
			p.sample(probeCtx, candidate, playlist, nil, sampleBudget),
		}
	}

	// Probe higher resolutions first so variant/time limits do not favor manifest ordering.
	variants := append([]model.Variant(nil), playlist.Metadata.Variants...)
	if ceiling, ok := ctx.Value(heightLimitKey{}).(int); ok && ceiling > 0 {
		filtered := variants[:0]
		for _, variant := range variants {
			if variant.Height == nil || *variant.Height <= ceiling {
				filtered = append(filtered, variant)
			}
		}
		variants = filtered
	}
	sort.SliceStable(variants, func(first, second int) bool {
		firstHeight := 0
		secondHeight := 0
		if variants[first].Height != nil {
			firstHeight = *variants[first].Height
		}
		if variants[second].Height != nil {
			secondHeight = *variants[second].Height
		}
		return firstHeight > secondHeight
	})

	var results []model.Probed
	for i, variant := range variants {
		if i >= p.limits.MaxVariants || probeCtx.Err() != nil {
			break
		}

		variantCandidate := candidate
		variantCandidate.ID = fmt.Sprintf("%s:variant:%d", candidate.ID, i)
		// Keep the playable master: the child video playlist alone can omit audio/subtitles.
		variantCandidate.SelectedVariantURL = variant.URL
		variantCandidate.Format = model.HLS

		mediaPlaylist, err := p.fetchPlaylist(probeCtx, candidate, variant.URL, playlistBudget)
		if err != nil {
			failedResult := makeFailureResult(err)[0]
			failedResult.Source = variantCandidate
			failedResult.Observation.Variant = &variant
			results = append(results, failedResult)
			continue
		}

		if len(mediaPlaylist.Metadata.Variants) > 0 {
			nestedError := makeFailureResult(model.Fail(model.Unsupported, "playlist", "nested master playlist not supported"))[0]
			nestedError.Source = variantCandidate
			results = append(results, nestedError)
			continue
		}

		mediaPlaylist.Metadata.Tracks = append(mediaPlaylist.Metadata.Tracks, playlist.Metadata.Tracks...)
		mediaPlaylist.Metadata.TracksKnown = playlist.Metadata.TracksKnown
		mediaPlaylist.Encrypted = mediaPlaylist.Encrypted || playlist.Encrypted
		mediaPlaylist.UnsupportedProtection = mediaPlaylist.UnsupportedProtection || playlist.UnsupportedProtection

		completed := p.sample(probeCtx, variantCandidate, mediaPlaylist, &variant, sampleBudget)
		completed.Observation.Metadata.Variants = playlist.Metadata.Variants
		results = append(results, completed)
		reportRendition(probeCtx, completed)
	}

	if len(results) == 0 {
		if probeCtx.Err() != nil {
			return makeFailureResult(probeCtx.Err())
		}
		return makeFailureResult(model.Fail(model.Unavailable, "probe", "no variants sampled"))
	}

	return results
}
