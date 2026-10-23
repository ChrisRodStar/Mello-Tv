//
//  model_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Unit tests for pure media domain models, identity validation, canonical keys,
//  and genre mappings.
//

package model_test

import (
	"encoding/json"
	"testing"

	"stv-catalog/internal/model"
)

func TestIdentity_Key(t *testing.T) {
	cases := []struct {
		id   model.Identity
		want string
	}{
		{id: model.Identity{Type: "movie", ID: 27205}, want: "movie:27205"},
		{id: model.Identity{Type: "tv", ID: 60625}, want: "tv:60625"},
	}

	for _, tc := range cases {
		if got := tc.id.Key(); got != tc.want {
			t.Errorf("Identity.Key() = %q, want %q", got, tc.want)
		}
	}
}

func TestIdentity_Valid(t *testing.T) {
	cases := []struct {
		id   model.Identity
		want bool
	}{
		{id: model.Identity{Type: "movie", ID: 100}, want: true},
		{id: model.Identity{Type: "tv", ID: 200}, want: true},
		{id: model.Identity{Type: "music", ID: 100}, want: false},
		{id: model.Identity{Type: "movie", ID: 0}, want: false},
		{id: model.Identity{Type: "movie", ID: -5}, want: false},
		{id: model.Identity{Type: "", ID: 100}, want: false},
	}

	for _, tc := range cases {
		if got := tc.id.Valid(); got != tc.want {
			t.Errorf("Identity.Valid() for %+v = %v, want %v", tc.id, got, tc.want)
		}
	}
}

func TestSeasonKey(t *testing.T) {
	got := model.SeasonKey(1399, 1)
	want := "tv:1399:1"
	if got != want {
		t.Errorf("SeasonKey(1399, 1) = %q, want %q", got, want)
	}
}

func TestValidGenre(t *testing.T) {
	valid := []string{"action", "comedy", "drama", "crime", "scifi", "animation", "documentary", "horror", "family"}
	for _, g := range valid {
		if !model.ValidGenre(g) {
			t.Errorf("expected genre %q to be valid", g)
		}
	}

	invalid := []string{"western", "thriller", "news", "reality", "", "123"}
	for _, g := range invalid {
		if model.ValidGenre(g) {
			t.Errorf("expected genre %q to be invalid", g)
		}
	}
}

func TestTitle_JSONSerialization(t *testing.T) {
	title := model.Title{
		Identity: model.Identity{Type: "movie", ID: 550},
		Title:    "Fight Club",
		Overview: "An insomniac office worker...",
		Rating:   8.4,
		Genres:   []string{"Drama"},
		Details:  true,
	}

	data, err := json.Marshal(title)
	if err != nil {
		t.Fatalf("json.Marshal failed: %v", err)
	}

	var decoded model.Title
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("json.Unmarshal failed: %v", err)
	}

	if decoded.Key() != "movie:550" || decoded.Title != "Fight Club" || decoded.Rating != 8.4 {
		t.Errorf("decoded title mismatch: %+v", decoded)
	}
}

func BenchmarkIdentity_Key(b *testing.B) {
	id := model.Identity{Type: "movie", ID: 27205}
	b.ReportAllocs()

	for b.Loop() {
		_ = id.Key()
	}
}

func BenchmarkSeasonKey(b *testing.B) {
	b.ReportAllocs()

	for b.Loop() {
		_ = model.SeasonKey(1399, 5)
	}
}
