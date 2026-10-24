package selection

import (
	"math"
	"slices"
	"strings"
	"time"

	"stv-providers/internal/model"
)

// containsFormat checks if the format slice contains the specified format.
func containsFormat(items []model.Format, target model.Format) bool {
	return slices.Contains(items, target)
}

// languageRank searches available tracks for the best match against the user's ordered language preferences.
func languageRank(preferredLanguages []string, availableTracks []model.Track, kind string) (int, bool) {
	for rankIdx, lang := range preferredLanguages {
		for _, track := range availableTracks {
			if track.Kind == kind && track.Language != "" && strings.EqualFold(track.Language, lang) {
				return rankIdx, true
			}
		}
	}
	return len(preferredLanguages), false
}

// collectTracks aggregates tracks from both the provider metadata and HLS manifest, filtering by variant group.
// Pre-allocates slice capacity to avoid intermediate allocations and reduce garbage collector pressure.
func collectTracks(probed model.Probed) []model.Track {
	srcTracks := probed.Source.Metadata.Tracks
	obsTracks := probed.Observation.Metadata.Tracks
	total := len(srcTracks) + len(obsTracks)
	if total == 0 {
		return nil
	}

	combined := make([]model.Track, 0, total)
	variant := probed.Observation.Variant

	filterTrack := func(track model.Track) {
		if variant != nil && track.Group != "" {
			if track.Kind == "audio" && track.Group != variant.AudioGroup {
				return
			}
			if track.Kind == "subtitles" && track.Group != variant.SubtitleGroup {
				return
			}
		}
		combined = append(combined, track)
	}

	for _, track := range srcTracks {
		filterTrack(track)
	}
	for _, track := range obsTracks {
		filterTrack(track)
	}

	return combined
}

// Eligible validates whether a probed stream satisfies device constraints, user preferences, and playback safety rules.
func Eligible(lookup model.Lookup, probed model.Probed) (bool, string) {
	// Rule 1: Title and episode coordinates must match lookup.
	if !lookup.Title.SameContent(probed.Source.Title) {
		return false, "title or episode mismatch"
	}

	// Rule 2: Expiration safety margin (must have >5s remaining).
	if probed.Source.ExpiresAt != nil && !probed.Source.ExpiresAt.After(time.Now().Add(5*time.Second)) {
		return false, "source expired or near expiry"
	}

	// Rule 3: Sample accessibility.
	obs := probed.Observation
	if !obs.Accessible || obs.Failure != "" {
		return false, "media sample unavailable"
	}

	// Rule 4: Device container format compatibility.
	if !containsFormat(lookup.Device.Formats, obs.Format) {
		return false, "device format unsupported"
	}

	// Rule 5: DRM and encryption compatibility.
	if !obs.ProtectionKnown || (obs.Encrypted && !lookup.Device.SupportsEncryptedHLS) {
		return false, "encryption unsupported or unknown"
	}

	// Rule 6: Audio and subtitle language requirements.
	tracks := collectTracks(probed)
	if _, ok := languageRank(lookup.Preferences.AudioLanguages, tracks, "audio"); lookup.Preferences.RequireAudio && !ok {
		return false, "required audio language not established"
	}
	if _, ok := languageRank(lookup.Preferences.SubtitleLanguages, tracks, "subtitles"); lookup.Preferences.RequireSubtitles && !ok {
		return false, "required subtitles not established"
	}

	// Rule 7: Video resolution boundaries.
	variant := obs.Variant
	if variant == nil || variant.Height == nil {
		if !lookup.Device.AllowUnknownResolution {
			return false, "resolution unknown"
		}
	} else {
		if *variant.Height <= 0 {
			return false, "invalid resolution height"
		}
		if lookup.Device.MaxHeight > 0 && *variant.Height > lookup.Device.MaxHeight {
			return false, "device resolution limit"
		}
		if lookup.Preferences.MaxHeight > 0 && *variant.Height > lookup.Preferences.MaxHeight {
			return false, "preferred resolution limit"
		}
	}

	// Rule 8: Video codec compatibility.
	if variant == nil || len(variant.Codecs) == 0 {
		if !lookup.Device.AllowUnknownCodecs {
			return false, "codecs unknown"
		}
	} else {
		for _, codec := range variant.Codecs {
			isSupported := false
			codecLower := strings.ToLower(codec)
			for _, allowedCodec := range lookup.Device.Codecs {
				allowedLower := strings.ToLower(allowedCodec)
				if codecLower == allowedLower || strings.HasPrefix(codecLower, allowedLower+".") {
					isSupported = true
					break
				}
			}
			if !isSupported {
				return false, "codec unsupported"
			}
		}
	}

	// Rule 9: Delivery throughput and headroom (must be at least 1.5x declared bitrate).
	if obs.DeliveryBPS == nil || *obs.DeliveryBPS <= 0 || math.IsNaN(*obs.DeliveryBPS) || math.IsInf(*obs.DeliveryBPS, 0) {
		return false, "delivery speed not measured"
	}
	if variant != nil && variant.Bitrate != nil && *variant.Bitrate <= 0 {
		return false, "declared bitrate invalid"
	}
	if variant != nil && variant.Bitrate != nil && *obs.DeliveryBPS < float64(*variant.Bitrate)*1.5 {
		return false, "insufficient estimated delivery headroom"
	}

	return true, ""
}
