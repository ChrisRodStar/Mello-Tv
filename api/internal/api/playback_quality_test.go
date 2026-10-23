package api

import (
	"encoding/json"
	"testing"
)

func TestPlaybackReportsOnlyBoundedRelayAvailableHeights(t *testing.T) {
	fixture := newFixture(t)
	fixture.mu.Lock()
	fixture.ready.AvailableHeights = []int{720, 1080}
	fixture.mu.Unlock()
	profile := fixture.profile("")
	response := fixture.want(200, "POST", "/api/v1/profiles/"+profile.ID+"/playback", "", map[string]any{"type": "movie", "id": 7}, "quality-startup")
	var reply struct {
		Heights []int `json:"available_heights"`
	}
	if json.Unmarshal(response, &reply) != nil || len(reply.Heights) != 2 || reply.Heights[0] != 720 || reply.Heights[1] != 1080 {
		t.Fatal("verified relay quality metadata was not preserved")
	}
	fixture.mu.Lock()
	ready := fixture.ready
	fixture.mu.Unlock()
	for _, heights := range [][]int{{0}, {720, 720}, {16385}, make([]int, 33)} {
		ready.AvailableHeights = heights
		if validateReady(ready) == nil {
			t.Fatal("invalid quality metadata accepted")
		}
	}
	ready.AvailableHeights = []int{720}
	ready.Format = "mp4"
	if validateReady(ready) == nil {
		t.Fatal("adaptive heights accepted for progressive MP4")
	}
}
