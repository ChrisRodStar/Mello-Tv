package selection

import (
	"family-stream-providers/internal/model"
	"testing"
)

func TestSustainableSourceBeatsUnsustainableResolution(t *testing.T) {
	title := model.Title{Type: model.Movie, TMDBID: 1}
	h := 720
	bitrate := int64(2_000_000)
	good := float64(6_000_000)
	bad := float64(1_000_000)
	base := model.Probed{Source: model.Candidate{ID: "good", Provider: model.VidLink, Title: title, Format: model.HLS}, Observation: model.Observation{Accessible: true, Format: model.HLS, ProtectionKnown: true, DeliveryBPS: &good, Variant: &model.Variant{Height: &h, Bitrate: &bitrate, Codecs: []string{"avc1.4d401f"}}}}
	other := base
	other.Source.ID = "bad"
	other.Source.Provider = model.VidCore
	other.Observation.DeliveryBPS = &bad
	in := model.Lookup{Title: title, Device: model.Device{Formats: []model.Format{model.HLS}, Codecs: []string{"avc1"}}}
	selected := Select(in, []model.Probed{other, base})
	if selected.Selected == nil || selected.Selected.Candidate.Source.ID != "good" || selected.Rejected["bad"] == "" {
		t.Fatal("unsustainable source won")
	}
	in.Preferences.RequireAudio = true
	in.Preferences.AudioLanguages = []string{"ja"}
	if Select(in, []model.Probed{base}).Selected != nil {
		t.Fatal("unknown required language accepted")
	}
}
