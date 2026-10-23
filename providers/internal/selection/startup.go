package selection

import (
	"family-stream-providers/internal/model"
	"reflect"
	"sort"
	"strings"
	"time"
)

// StartupReady never promotes labels or unknown delivery into startup evidence.
// The caller explicitly opts into this resolution target; complete ranking still
// runs to populate verified backups without switching the playing source.
func StartupReady(lookup model.Lookup, result model.Selection) bool {
	if lookup.StartupHeight <= 0 || lookup.Refresh || result.Selected == nil {
		return false
	}
	candidate := result.Selected.Candidate
	if time.Since(candidate.Observation.At) > 15*time.Second {
		return false
	}
	if eligible, _ := Eligible(lookup, candidate); !eligible {
		return false
	}
	target := lookup.StartupHeight
	if lookup.Device.MaxHeight > 0 {
		target = min(target, lookup.Device.MaxHeight)
	}
	if lookup.Preferences.MaxHeight > 0 {
		target = min(target, lookup.Preferences.MaxHeight)
	}
	if resolutionHeight(candidate) < target || !deliveryEstablished(candidate) || candidate.Observation.SamplesCompleted < 3 {
		return false
	}
	for _, kind := range []string{"audio", "subtitles"} {
		preferred := preferredLanguages(lookup, kind)
		if len(preferred) > 0 {
			rank, matched := languageRank(preferred, collectTracks(candidate), kind)
			if !matched || rank != 0 {
				return false
			}
		}
	}
	// A multi-rendition source needs one verified lower compatible rung before
	// publishing its initial master. This lets the player adapt within one
	// source without exposing unprobed advertised alternatives.
	if candidate.Observation.Format == model.HLS && candidate.Observation.Variant != nil {
		needsLower := false
		selectedVariant := candidate.Observation.Variant
		for _, variant := range candidate.Observation.Metadata.Variants {
			if variant.Height != nil && *variant.Height < resolutionHeight(candidate) &&
				compatibleStartupCodecs(variant.Codecs, selectedVariant.Codecs) &&
				variant.AudioGroup == selectedVariant.AudioGroup && variant.SubtitleGroup == selectedVariant.SubtitleGroup {
				needsLower = true
			}
		}
		if needsLower {
			for _, ranked := range result.Backups {
				lower := ranked.Candidate
				if lower.Source.URL != candidate.Source.URL || lower.Source.Provider != candidate.Source.Provider ||
					!reflect.DeepEqual(lower.Source.Context, candidate.Source.Context) ||
					resolutionHeight(lower) >= resolutionHeight(candidate) || lower.Observation.Variant == nil ||
					!compatibleStartupCodecs(lower.Observation.Variant.Codecs, selectedVariant.Codecs) ||
					lower.Observation.Variant.AudioGroup != selectedVariant.AudioGroup ||
					lower.Observation.Variant.SubtitleGroup != selectedVariant.SubtitleGroup ||
					time.Since(lower.Observation.At) > 15*time.Second || lower.Observation.SamplesCompleted < 3 {
					continue
				}
				if eligible, _ := Eligible(lookup, lower); eligible {
					return true
				}
			}
			return false
		}
	}
	return true
}

func compatibleStartupCodecs(first, second []string) bool {
	if len(first) == 0 || len(first) != len(second) {
		return false
	}
	families := func(codecs []string) []string {
		result := make([]string, 0, len(codecs))
		for _, codec := range codecs {
			result = append(result, strings.ToLower(strings.Split(codec, ".")[0]))
		}
		sort.Strings(result)
		return result
	}
	return reflect.DeepEqual(families(first), families(second))
}

const adaptiveReason = "verified same-source adaptive rendition available"

func hasVerifiedAdaptiveLower(candidate model.Probed, alternatives []model.Ranked) bool {
	selected := candidate.Observation.Variant
	if candidate.Observation.Format != model.HLS || selected == nil || selected.Width == nil || selected.Height == nil || candidate.Observation.SamplesCompleted < 3 {
		return false
	}
	for _, ranked := range alternatives {
		lower := ranked.Candidate
		variant := lower.Observation.Variant
		if lower.Observation.Format == model.HLS && variant != nil && variant.Width != nil && variant.Height != nil &&
			*variant.Height < *selected.Height && lower.Observation.SamplesCompleted >= 3 &&
			lower.Source.URL == candidate.Source.URL && lower.Source.Provider == candidate.Source.Provider &&
			reflect.DeepEqual(lower.Source.Context, candidate.Source.Context) &&
			compatibleStartupCodecs(variant.Codecs, selected.Codecs) &&
			variant.AudioGroup == selected.AudioGroup && variant.SubtitleGroup == selected.SubtitleGroup &&
			time.Since(lower.Observation.At) <= 15*time.Second {
			return true
		}
	}
	return false
}
