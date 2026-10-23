package selection

import (
	"family-stream-providers/internal/model"
	"math"
	"reflect"
	"testing"
	"time"
)

func selectionLookup() model.Lookup {
	season := 1
	episode := 1
	return model.Lookup{
		Title: model.Title{Type: model.TV, TMDBID: 60625, Season: &season, Episode: &episode},
		Device: model.Device{
			Formats: []model.Format{model.HLS},
			Codecs:  []string{"avc1"},
		},
	}
}

func selectionCandidate(identifier string, height int, delivery float64) model.Probed {
	bitrate := int64(6_000_000)
	return model.Probed{
		Source: model.Candidate{
			ID:       identifier,
			Provider: model.VidCore,
			Title:    selectionLookup().Title,
			Format:   model.HLS,
		},
		Observation: model.Observation{
			Accessible:      true,
			Format:          model.HLS,
			ProtectionKnown: true,
			DeliveryBPS:     &delivery,
			Variant: &model.Variant{
				Height:  &height,
				Bitrate: &bitrate,
				Codecs:  []string{"avc1.4d401f"},
			},
		},
	}
}

func selectedIdentifiers(result model.Selection) []string {
	var identifiers []string
	if result.Selected != nil {
		identifiers = append(identifiers, result.Selected.Candidate.Source.ID)
	}
	for _, backup := range result.Backups {
		identifiers = append(identifiers, backup.Candidate.Source.ID)
	}
	return identifiers
}

func TestQualityFirstSelectionAndBackups(t *testing.T) {
	lookup := selectionLookup()
	primary := selectionCandidate("1080-primary", 1080, 16_400_000)
	backup := selectionCandidate("1080-backup", 1080, 10_000_000)
	medium := selectionCandidate("720", 720, 30_000_000)
	low := selectionCandidate("480", 480, 60_000_000)
	perfectReliability := 1.0
	low.Observation.Reliability = &perfectReliability

	candidates := []model.Probed{low, backup, medium, primary}
	want := []string{"1080-primary", "1080-backup", "720", "480"}
	for range len(candidates) {
		result := Select(lookup, candidates)
		if got := selectedIdentifiers(result); !reflect.DeepEqual(got, want) {
			t.Fatalf("selection order = %v, want %v", got, want)
		}
		candidates = append(candidates[1:], candidates[0])
	}
}

func TestQualityFallsBackWhenDeliveryOrDeviceDisallows1080(t *testing.T) {
	high := selectionCandidate("1080", 1080, 8_000_000)
	medium := selectionCandidate("720", 720, 12_000_000)
	lookup := selectionLookup()
	result := Select(lookup, []model.Probed{high, medium})
	if result.Selected == nil || result.Selected.Candidate.Source.ID != "720" || result.Rejected["1080"] == "" {
		t.Fatal("insufficient 1080p headroom did not fall back to 720p")
	}

	lookup.Device.MaxHeight = 720
	high.Observation.DeliveryBPS = medium.Observation.DeliveryBPS
	result = Select(lookup, []model.Probed{high, medium})
	if result.Rejected["1080"] != "device resolution limit" {
		t.Fatalf("device rejection = %q", result.Rejected["1080"])
	}
}

func TestLanguagePreferencesPrecedeResolution(t *testing.T) {
	lookup := selectionLookup()
	lookup.Preferences.AudioLanguages = []string{"en", "ja"}
	high := selectionCandidate("1080-ja", 1080, 12_000_000)
	high.Observation.Metadata.Tracks = []model.Track{{Kind: "audio", Language: "ja"}}
	medium := selectionCandidate("720-en", 720, 12_000_000)
	medium.Observation.Metadata.Tracks = []model.Track{{Kind: "audio", Language: "en"}}

	result := Select(lookup, []model.Probed{high, medium})
	if result.Selected == nil || result.Selected.Candidate.Source.ID != "720-en" {
		t.Fatal("preferred audio was displaced by resolution")
	}
}

func TestUnknownBitrateRemainsBehindEstablishedDelivery(t *testing.T) {
	known := selectionCandidate("known", 720, 12_000_000)
	unknown := selectionCandidate("unknown", 1080, 30_000_000)
	unknown.Observation.Variant.Bitrate = nil

	result := Select(selectionLookup(), []model.Probed{unknown, known})
	if result.Selected == nil || result.Selected.Candidate.Source.ID != "known" || len(result.Backups) != 1 {
		t.Fatal("unknown sustainable delivery displaced an established source")
	}
}

func TestInvalidDeliveryIsRejected(t *testing.T) {
	for _, delivery := range []float64{0, -1, math.NaN(), math.Inf(1)} {
		candidate := selectionCandidate("invalid", 1080, delivery)
		if result := Select(selectionLookup(), []model.Probed{candidate}); result.Selected != nil {
			t.Fatalf("invalid delivery %v accepted", delivery)
		}
	}
}

func TestResponseDelayAndIdentifierBreakWithinTierTies(t *testing.T) {
	first := selectionCandidate("a", 1080, 12_000_000)
	second := selectionCandidate("b", 1080, 12_000_000)
	firstDelay := 500 * time.Millisecond
	secondDelay := 100 * time.Millisecond
	first.Observation.FirstByte = &firstDelay
	second.Observation.FirstByte = &secondDelay

	result := Select(selectionLookup(), []model.Probed{first, second})
	if result.Selected == nil || result.Selected.Candidate.Source.ID != "b" {
		t.Fatal("lower measured response delay did not break tie")
	}
	first.Observation.FirstByte = nil
	second.Observation.FirstByte = nil
	result = Select(selectionLookup(), []model.Probed{second, first})
	if result.Selected == nil || result.Selected.Candidate.Source.ID != "a" {
		t.Fatal("identifier tie-break was not deterministic")
	}
}

func TestMixedKnownAndUnknownDelayOrdering(t *testing.T) {
	unknown := selectionCandidate("a-unknown", 1080, 12_000_000)
	slow := selectionCandidate("b-slow", 1080, 12_000_000)
	fast := selectionCandidate("c-fast", 1080, 12_000_000)
	slowDelay := 500 * time.Millisecond
	fastDelay := 100 * time.Millisecond
	slow.Observation.FirstByte = &slowDelay
	fast.Observation.FirstByte = &fastDelay
	candidates := []model.Probed{unknown, slow, fast}
	for range len(candidates) {
		result := Select(selectionLookup(), candidates)
		want := []string{"c-fast", "b-slow", "a-unknown"}
		if got := selectedIdentifiers(result); !reflect.DeepEqual(got, want) {
			t.Fatalf("mixed delay order = %v, want %v", got, want)
		}
		candidates = append(candidates[1:], candidates[0])
	}
}

func BenchmarkSelect(b *testing.B) {
	lookup := selectionLookup()
	candidates := make([]model.Probed, 20)
	for i := 0; i < 20; i++ {
		candidates[i] = selectionCandidate(string(rune('a'+i)), 1080-(i%3)*360, float64(10_000_000+i*500_000))
	}
	b.ResetTimer()
	b.ReportAllocs()
	for i := 0; i < b.N; i++ {
		_ = Select(lookup, candidates)
	}
}

