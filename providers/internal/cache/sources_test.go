package cache

import (
	"bytes"
	"encoding/json"
	"family-stream-providers/internal/model"
	"testing"
	"time"
)

func TestExpiryAndIsolation(t *testing.T) {
	c := NewSources(2, time.Minute)
	expiry := time.Now().Add(-time.Second)
	r := model.AdapterResult{Candidates: []model.Candidate{{ID: "x", ExpiresAt: &expiry}}}
	c.Put("expired", r)
	if _, ok := c.Get("expired"); ok {
		t.Fatal("expired URL cached")
	}
	r.Candidates[0].ExpiresAt = nil
	c.Put("fresh", r)
	r.Candidates[0].ID = "changed"
	got, ok := c.Get("fresh")
	if !ok || got.Candidates[0].ID != "x" {
		t.Fatal("caller mutated cache")
	}
	got.Candidates[0].ID = "again"
	next, _ := c.Get("fresh")
	if next.Candidates[0].ID != "x" {
		t.Fatal("cache result not isolated")
	}
	c.Put("failure", model.AdapterResult{})
	if _, ok := c.Get("failure"); ok {
		t.Fatal("empty result cached")
	}
}

func TestClonePreservesAndIsolatesNestedMetadata(t *testing.T) {
	season, episode, width, height := 1, 2, 1920, 1080
	animeID, bitrate := int64(123), int64(5_000_000)
	expiry := time.Now().UTC()
	original := model.AdapterResult{
		Candidates: []model.Candidate{{
			ID: "fixture", ExpiresAt: &expiry,
			Title: model.Title{MyAnimeListID: &animeID, Season: &season, Episode: &episode},
			Context: model.FetchContext{
				Scopes:          []model.FetchScope{{Headers: map[string]string{"Authorization": "fixture"}}},
				PublicResources: &model.PublicResourceContext{Origin: "https://public.example"},
			},
			Metadata: model.Metadata{
				Tracks:       []model.Track{{Name: "English"}},
				Variants:     []model.Variant{{Width: &width, Height: &height, Bitrate: &bitrate, Codecs: []string{"avc1"}}},
				ProviderData: map[string]json.RawMessage{"field": json.RawMessage(`{"unknown":true}`)},
			},
		}},
		Issues: []model.Failure{{Detail: "fixture"}},
	}
	before, _ := json.Marshal(original)
	cloned := CloneResult(original)
	after, _ := json.Marshal(cloned)
	if !bytes.Equal(before, after) {
		t.Fatal("clone changed wire data")
	}
	candidate := &cloned.Candidates[0]
	*candidate.Title.MyAnimeListID = 0
	*candidate.Title.Season = 0
	*candidate.Title.Episode = 0
	*candidate.ExpiresAt = time.Time{}
	candidate.Context.Scopes[0].Headers["Authorization"] = "changed"
	candidate.Context.PublicResources.Origin = "changed"
	candidate.Metadata.Tracks[0].Name = "changed"
	*candidate.Metadata.Variants[0].Width = 0
	*candidate.Metadata.Variants[0].Height = 0
	*candidate.Metadata.Variants[0].Bitrate = 0
	candidate.Metadata.Variants[0].Codecs[0] = "changed"
	candidate.Metadata.ProviderData["field"][0] = 'x'
	cloned.Issues[0].Detail = "changed"
	after, _ = json.Marshal(original)
	if !bytes.Equal(before, after) {
		t.Fatal("clone aliases nested source data")
	}
}
