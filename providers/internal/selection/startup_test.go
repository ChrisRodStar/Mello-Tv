package selection

import (
	"family-stream-providers/internal/model"
	"testing"
	"time"
)

func TestStartupRequiresEligibleTargetAndMeasuredHeadroom(t *testing.T) {
	height := 1080
	bitrate := int64(2_000_000)
	speed := 4_000_000.0
	title := model.Title{Type: model.Movie, TMDBID: 1}
	lookup := model.Lookup{Title: title, StartupHeight: 1080, Device: model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1"}}}
	item := model.Probed{Source: model.Candidate{ID: "verified", Title: title}, Observation: model.Observation{At: time.Now(), Accessible: true, Format: model.HLS, ProtectionKnown: true, SamplesCompleted: 3, DeliveryBPS: &speed, Variant: &model.Variant{Height: &height, Bitrate: &bitrate, Codecs: []string{"avc1.640028"}}}}
	result := Select(lookup, []model.Probed{item})
	if !StartupReady(lookup, result) {
		t.Fatal("verified 1080p source did not qualify")
	}
	height = 720
	if StartupReady(lookup, result) {
		t.Fatal("720p started before requested 1080p was available")
	}
	height = 1080
	lookup.Preferences.AudioLanguages = []string{"en"}
	if StartupReady(lookup, result) {
		t.Fatal("unknown preferred language qualified early")
	}
	lookup.Preferences.AudioLanguages = nil
	lookup.Refresh = true
	if StartupReady(lookup, result) {
		t.Fatal("source refresh returned incomplete ranking")
	}
}

func TestAdaptiveStartupWaitsForCompatibleVerifiedLowerRendition(t *testing.T) {
	height, lowerHeight := 1080, 720
	bitrate := int64(2_000_000)
	speed := 8_000_000.0
	title := model.Title{Type: model.Movie, TMDBID: 1}
	lookup := model.Lookup{Title: title, StartupHeight: 1080, Device: model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1"}}}
	high := model.Variant{URL: "https://example.com/1080.m3u8", Height: &height, Bitrate: &bitrate, Codecs: []string{"avc1.640028"}}
	low := model.Variant{URL: "https://example.com/720.m3u8", Height: &lowerHeight, Bitrate: &bitrate, Codecs: []string{"avc1.64001f"}}
	top := model.Probed{Source: model.Candidate{ID: "high", URL: "https://example.com/master.m3u8", Provider: model.VidLink, Title: title}, Observation: model.Observation{At: time.Now(), Accessible: true, Format: model.HLS, ProtectionKnown: true, SamplesCompleted: 3, DeliveryBPS: &speed, Variant: &high, Metadata: model.Metadata{Variants: []model.Variant{high, low}}}}
	lower := top
	lower.Source.ID = "low"
	lower.Observation.Variant = &low
	if StartupReady(lookup, Select(lookup, []model.Probed{top})) {
		t.Fatal("unprobed lower rendition was published")
	}
	if !StartupReady(lookup, Select(lookup, []model.Probed{top, lower})) {
		t.Fatal("verified H264 levels did not form a ladder")
	}
	lower.Source.Provider = model.VidFast
	if StartupReady(lookup, Select(lookup, []model.Probed{top, lower})) {
		t.Fatal("different provider supplied adaptive segments")
	}
	lower.Source.Provider = model.VidLink
	lower.Source.URL = "https://other.example.com/master.m3u8"
	if StartupReady(lookup, Select(lookup, []model.Probed{top, lower})) {
		t.Fatal("different source supplied adaptive segments")
	}
}

func TestEqualQualityPrefersVerifiedLadderWithoutProviderPriority(t *testing.T) {
	height, width, lowHeight, lowWidth := 1080, 1920, 720, 1280
	bitrate := int64(2_000_000)
	fast, sustainable := 6_000_000.0, 4_000_000.0
	title := model.Title{Type: model.Movie, TMDBID: 1}
	lookup := model.Lookup{Title: title, StartupHeight: 1080, Device: model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1"}}}
	high := model.Probed{Source: model.Candidate{ID: "single", URL: "https://single.example.com/master.m3u8", Provider: model.VidLink, Title: title}, Observation: model.Observation{At: time.Now(), Accessible: true, Format: model.HLS, ProtectionKnown: true, SamplesCompleted: 3, DeliveryBPS: &fast, Variant: &model.Variant{Height: &height, Width: &width, Bitrate: &bitrate, Codecs: []string{"avc1"}}}}
	adaptive := high
	adaptive.Source.ID = "adaptive"
	adaptive.Source.Provider = model.VidSrc
	adaptive.Source.URL = "https://adaptive.example.com/master.m3u8"
	adaptive.Observation.DeliveryBPS = &sustainable
	low := adaptive
	low.Source.ID = "adaptive-low"
	lowVariant := *high.Observation.Variant
	lowVariant.Height = &lowHeight
	lowVariant.Width = &lowWidth
	low.Observation.Variant = &lowVariant
	selected := Select(lookup, []model.Probed{high, adaptive, low})
	if selected.Selected == nil || selected.Selected.Candidate.Source.ID != "adaptive" {
		t.Fatal("single rendition outranked an equally viable adaptive source")
	}
	selected = Select(lookup, []model.Probed{high, adaptive})
	if selected.Selected == nil || selected.Selected.Candidate.Source.ID != "single" {
		t.Fatal("unverified adaptive source received a preference")
	}
}
