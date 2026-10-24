package selection

import (
	"testing"

	"stv-providers/internal/model"
)

func TestCodecEligibilityKeepsHVC1DistinctFromHEV1(t *testing.T) {
	lookup := selectionLookup()
	lookup.Device.Formats = []model.Format{model.HLS, model.MP4}
	lookup.Device.Codecs = []string{"avc1", "hvc1", "mp4a"}

	hevcSource := selectionCandidate("unsupported-hev1", 1080, 16_000_000)
	hevcSource.Source.Provider = model.VidLink
	hevcSource.Source.Format = model.MP4
	hevcSource.Observation.Format = model.MP4
	hevcSource.Observation.Variant.Codecs = []string{"hev1", "mp4a"}

	compatibleSource := selectionCandidate("compatible-avc1", 720, 16_000_000)
	compatibleSource.Observation.Variant.Codecs = []string{"avc1", "mp4a"}

	result := Select(lookup, []model.Probed{hevcSource, compatibleSource})
	if result.Selected == nil || result.Selected.Candidate.Source.ID != "compatible-avc1" ||
		result.Rejected["unsupported-hev1"] != "codec unsupported" || len(result.Backups) != 0 {
		t.Fatal("an unsupported HEVC tag displaced the compatible source")
	}

	// A different player may explicitly support hev1. Preserve that capability
	// rather than banning this codec/provider globally based on one Mac decoder.
	lookup.Device.Codecs = append(lookup.Device.Codecs, "hev1")
	result = Select(lookup, []model.Probed{hevcSource, compatibleSource})
	if result.Selected == nil || result.Selected.Candidate.Source.ID != "unsupported-hev1" || len(result.Backups) != 1 {
		t.Fatal("explicit codec support or quality-first cross-provider ranking changed")
	}
}
