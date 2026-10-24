package cache

import (
	"testing"

	"stv-providers/internal/model"
)

func observationCandidate(identifier string, hostname string, accessible bool) model.Probed {
	return model.Probed{
		Source: model.Candidate{
			ID:       identifier,
			Provider: model.VidCore,
			Server:   "supreme",
			URL:      "https://" + hostname + "/" + identifier + ".m3u8",
		},
		Observation: model.Observation{Accessible: accessible},
	}
}

func TestVariantsCountAsOneReliabilityRound(t *testing.T) {
	history := NewObservations()
	candidates := []model.Probed{
		observationCandidate("1080", "example.com", true),
		observationCandidate("720", "example.com", true),
		observationCandidate("480", "example.com", true),
	}

	for round := 1; round <= 3; round++ {
		history.RecordRound(candidates)
		for _, candidate := range candidates {
			reliability := candidate.Observation.Reliability
			if round < 3 && reliability != nil {
				t.Fatalf("history published after only %d independent rounds", round)
			}
			if round == 3 && (reliability == nil || *reliability != 1) {
				t.Fatal("variants did not receive the same three-round history")
			}
		}
		if stats := history.entries[observationKey(candidates[0].Source)]; stats.samples != round {
			t.Fatalf("samples = %d, want %d", stats.samples, round)
		}
	}
}

func TestMixedRoundAndIndependentEndpoints(t *testing.T) {
	history := NewObservations()
	candidates := []model.Probed{
		observationCandidate("good", "example.com", true),
		observationCandidate("bad", "example.com", false),
		observationCandidate("other", "other.example.com", true),
	}
	candidates[1].Observation.Failure = model.UpstreamFailure
	for range 3 {
		history.RecordRound(candidates)
	}
	for index, want := range []float64{0, 0, 1} {
		if got := candidates[index].Observation.Reliability; got == nil || *got != want {
			t.Fatalf("candidate %d reliability = %v, want %v", index, got, want)
		}
	}
}

func TestLocalProbeFailuresDoNotPoisonHistory(t *testing.T) {
	history := NewObservations()
	for _, failure := range []model.ErrorCategory{model.Cancelled, model.Unavailable, model.Timeout, model.Unsupported} {
		candidate := observationCandidate("local", "example.com", false)
		candidate.Observation.Failure = failure
		history.RecordRound([]model.Probed{candidate})
	}
	if len(history.entries) != 0 {
		t.Fatal("local or ambiguous probe failure counted as endpoint failure")
	}
}

func TestMasterHandoffKeepsRenditionEndpointHistorySeparate(t *testing.T) {
	history := NewObservations()
	first := observationCandidate("first", "master.example.com", true)
	first.Source.SelectedVariantURL = "https://first.example.com/video.m3u8"
	second := observationCandidate("second", "master.example.com", false)
	second.Source.SelectedVariantURL = "https://second.example.com/video.m3u8"
	second.Observation.Failure = model.UpstreamFailure
	candidates := []model.Probed{first, second}
	for range 3 {
		history.RecordRound(candidates)
	}
	if candidates[0].Observation.Reliability == nil || *candidates[0].Observation.Reliability != 1 || candidates[1].Observation.Reliability == nil || *candidates[1].Observation.Reliability != 0 {
		t.Fatal("playable master URL merged distinct rendition endpoint histories")
	}

	if history.Len() != 2 {
		t.Fatalf("expected 2 distinct endpoint histories, got %d", history.Len())
	}

	// Nil Observations safety
	var nilObs *Observations
	nilObs.RecordRound(candidates)
	if nilObs.Len() != 0 {
		t.Fatalf("expected 0 from nil Observations.Len(), got %d", nilObs.Len())
	}
}
