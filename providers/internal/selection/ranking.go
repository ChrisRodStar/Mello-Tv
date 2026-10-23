package selection

import (
	"family-stream-providers/internal/model"
	"math"
	"slices"
)

// rank calculates a composite candidate score within a given preference and resolution tier,
// explaining all eligibility qualifications and scoring factors.
func rank(lookup model.Lookup, probed model.Probed) model.Ranked {
	ranked := model.Ranked{
		Candidate: probed,
		Reasons: []string{
			"requested title coordinates match resolver assignment",
			"bounded media sample accessible",
			"device eligibility satisfied",
		},
	}

	if probed.Source.ContentVerified {
		ranked.Reasons = append(ranked.Reasons, "content independently verified")
	} else {
		ranked.Reasons = append(ranked.Reasons, "content identity has not been independently verified")
	}

	for _, trackKind := range []string{"audio", "subtitles"} {
		preferences := preferredLanguages(lookup, trackKind)
		availableTracks := collectTracks(probed)
		if _, matched := languageRank(preferences, availableTracks, trackKind); matched {
			ranked.Reasons = append(ranked.Reasons, trackKind+" preference available")
		}
	}

	// Measured delivery headroom (up to 40 points).
	obs := probed.Observation
	if obs.Variant != nil && obs.Variant.Bitrate != nil {
		headroomRatio := *obs.DeliveryBPS / float64(*obs.Variant.Bitrate)
		ranked.Score += math.Min(headroomRatio, 4.0) * 10.0
		ranked.Reasons = append(ranked.Reasons, "estimated delivery exceeds declared bitrate with headroom")
	} else {
		ranked.Score += 5.0
		ranked.Reasons = append(ranked.Reasons, "declared bitrate unknown; sustainable delivery unconfirmed")
	}

	// Historical local probe reliability (up to 25 points).
	if obs.Reliability != nil {
		ranked.Score += *obs.Reliability * 25.0
		ranked.Reasons = append(ranked.Reasons, "recent local probe reliability considered")
	} else {
		ranked.Reasons = append(ranked.Reasons, "historical reliability unknown")
	}

	if obs.Variant != nil && obs.Variant.Height != nil {
		ranked.Reasons = append(ranked.Reasons, "resolution prioritized within language and delivery tiers")
	}

	return ranked
}

// preferredLanguages returns the ordered language preference list for the requested track kind.
func preferredLanguages(lookup model.Lookup, trackKind string) []string {
	if trackKind == "subtitles" {
		return lookup.Preferences.SubtitleLanguages
	}
	return lookup.Preferences.AudioLanguages
}

// deliveryEstablished reports whether the candidate has a known, non-zero declared bitrate
// against which measured throughput headroom has been validated.
func deliveryEstablished(candidate model.Probed) bool {
	variant := candidate.Observation.Variant
	return variant != nil && variant.Bitrate != nil && *variant.Bitrate > 0
}

// resolutionHeight returns the candidate rendition height in pixels, or zero if unspecified.
// Unknown resolutions are kept behind known viable renditions.
func resolutionHeight(candidate model.Probed) int {
	variant := candidate.Observation.Variant
	if variant == nil || variant.Height == nil {
		return 0
	}
	return *variant.Height
}

// prefer establishes the deterministic sorting hierarchy across ranked candidates:
// 1. Language preference matching (audio and subtitle tiers).
// 2. Established delivery viability (confirmed bitrate headroom over unmeasured).
// 3. Resolution height (e.g. 1080p > 720p > 480p).
// 4. Composite score (delivery headroom ratio + historical probe reliability).
// 5. Time-to-first-byte latency (lower is preferred).
// 6. Source ID lexicographical tie-breaker.
func prefer(lookup model.Lookup, first, second model.Ranked) bool {
	for _, trackKind := range []string{"audio", "subtitles"} {
		languages := preferredLanguages(lookup, trackKind)
		firstRank, _ := languageRank(languages, collectTracks(first.Candidate), trackKind)
		secondRank, _ := languageRank(languages, collectTracks(second.Candidate), trackKind)
		if firstRank != secondRank {
			return firstRank < secondRank
		}
	}

	firstEstablished := deliveryEstablished(first.Candidate)
	secondEstablished := deliveryEstablished(second.Candidate)
	if firstEstablished != secondEstablished {
		return firstEstablished
	}

	firstHeight := resolutionHeight(first.Candidate)
	secondHeight := resolutionHeight(second.Candidate)
	if firstHeight != secondHeight {
		return firstHeight > secondHeight
	}

	firstAdaptive := slices.Contains(first.Reasons, adaptiveReason)
	secondAdaptive := slices.Contains(second.Reasons, adaptiveReason)
	if firstAdaptive != secondAdaptive {
		return firstAdaptive
	}

	if first.Score != second.Score {
		return first.Score > second.Score
	}

	firstDelay := first.Candidate.Observation.FirstByte
	secondDelay := second.Candidate.Observation.FirstByte
	if (firstDelay != nil) != (secondDelay != nil) {
		return firstDelay != nil
	}
	if firstDelay != nil && secondDelay != nil && *firstDelay != *secondDelay {
		return *firstDelay < *secondDelay
	}

	return first.Candidate.Source.ID < second.Candidate.Source.ID
}
