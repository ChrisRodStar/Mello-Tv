//
//  recommend_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Unit tests for recommendation scoring, candidate ranking, franchise diversification,
//  viewing decay calculation, input fingerprint stability, and view filtering.
//

package recommend_test

import (
	"testing"
	"time"

	"stv-catalog/internal/model"
	"stv-catalog/internal/recommend"
)

var testNow = time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

func makeTestTitle(typ string, id int, name string) model.Title {
	return model.Title{
		Identity: model.Identity{Type: typ, ID: id},
		Title:    name,
		Poster:   "/poster.jpg",
		Rating:   8,
	}
}

func defaultInputs() recommend.Inputs {
	return recommend.Inputs{
		Completed:    true,
		Format:       "both",
		Genres:       []string{"action", "animation", "horror"},
		Favorites:    []recommend.Favorite{{Identity: model.Identity{Type: "movie", ID: 100}, Title: "Test Movie"}},
		ActivityDays: []string{"2026-10-14", "2026-10-15"},
	}
}

func TestRank_ScoringFormulaAndTypeCollision(t *testing.T) {
	seed := makeTestTitle("movie", 1, "Seed")
	a := makeTestTitle("movie", 2, "Galaxy 1")
	b := makeTestTitle("movie", 3, "Galaxy 2")
	c := makeTestTitle("tv", 2, "Other")
	d := makeTestTitle("movie", 4, "Different")
	noPoster := makeTestTitle("movie", 5, "No poster")
	noPoster.Poster = ""

	got := recommend.Rank([]recommend.Contribution{
		{Titles: []model.Title{seed, a, b, c, noPoster}, Weight: 5},
		{Titles: []model.Title{c, d, a}, Weight: 2},
	}, map[string]bool{seed.Key(): true})

	if len(got) != 4 || got[0].Key() != a.Key() {
		t.Fatalf("ranking order mismatch: %+v", got)
	}

	seen := map[string]bool{}
	for _, v := range got {
		seen[v.Key()] = true
	}
	if !seen["movie:2"] || !seen["tv:2"] {
		t.Fatal("expected no type collision between movie:2 and tv:2")
	}

	unique := recommend.Rank([]recommend.Contribution{{Titles: []model.Title{a, a, b}, Weight: 5}}, nil)
	if len(unique) != 2 {
		t.Fatal("duplicate titles not pruned from rank")
	}
}

func TestDiversify_SequelSpacing(t *testing.T) {
	a := makeTestTitle("movie", 2, "Galaxy 1")
	b := makeTestTitle("movie", 3, "Galaxy 2")
	c := makeTestTitle("tv", 2, "Other")
	d := makeTestTitle("movie", 4, "Different")

	diverse := recommend.Diversify([]model.Title{a, b, c, d})
	if len(diverse) != 4 {
		t.Fatalf("expected 4 titles, got %d", len(diverse))
	}
	if diverse[1].Key() == b.Key() {
		t.Fatal("sequel franchise was not spaced in diversification")
	}
}

func TestSeeds_SelectionAndDecay(t *testing.T) {
	p := defaultInputs()
	p.Favorites = nil
	p.Signals = []recommend.ViewingSignal{
		{Identity: model.Identity{Type: "movie", ID: 1}, Watched: 599, Duration: 1000, UpdatedAt: testNow},
		{Identity: model.Identity{Type: "movie", ID: 2}, Watched: 600, Duration: 3000, UpdatedAt: testNow.Add(-14 * 24 * time.Hour)},
		{Identity: model.Identity{Type: "movie", ID: 3}, Watched: 600, Duration: 4000, UpdatedAt: testNow},
		{Identity: model.Identity{Type: "movie", ID: 4}, Watched: 600, Duration: 4000, Completed: true, UpdatedAt: testNow},
		{Identity: model.Identity{Type: "tv", ID: 5}, Watched: 1200, MeaningfulEpisodes: 2, UpdatedAt: testNow},
		{Identity: model.Identity{Type: "tv", ID: 6}, Watched: 1200, MeaningfulEpisodes: 1, UpdatedAt: testNow},
		{Identity: model.Identity{Type: "tv", ID: 7}, Watched: 2700, MeaningfulEpisodes: 1, UpdatedAt: testNow},
		{Identity: model.Identity{Type: "tv", ID: 8}, Watched: 2700, MeaningfulEpisodes: 2, UpdatedAt: testNow.Add(-61 * 24 * time.Hour)},
	}

	seeds := p.Seeds(testNow)
	if len(seeds) != 4 {
		t.Fatalf("qualification got %d seeds, expected 4", len(seeds))
	}
	for _, s := range seeds {
		if s.ID == 2 && (s.Weight < 0.66 || s.Weight > 0.67) {
			t.Fatalf("half life weight mismatch for 14-day old seed: %v", s.Weight)
		}
	}
}

func TestSeeds_MaxHistoryBound(t *testing.T) {
	p := defaultInputs()
	p.Favorites = nil
	p.Signals = nil
	for i := 1; i <= 30; i++ {
		p.Signals = append(p.Signals, recommend.ViewingSignal{
			Identity:  model.Identity{Type: "tv", ID: i},
			Watched:   2700,
			UpdatedAt: testNow,
		})
	}

	seeds := p.Seeds(testNow)
	if len(seeds) != 20 {
		t.Fatalf("history seeds exceeded maximum bound of 20, got %d", len(seeds))
	}
}

func TestRecommendations_View(t *testing.T) {
	a := makeTestTitle("movie", 2, "Galaxy 1")
	b := makeTestTitle("movie", 3, "Galaxy 2")
	c := makeTestTitle("tv", 2, "Other")

	view := (recommend.Recommendations{
		Pool:     []model.Title{c, a, b},
		Sections: []model.Shelf{{ID: "for-you"}},
	}).View("all", "movie")

	if view.Sections[0].Titles[0].Type != "movie" {
		t.Fatal("preferred format not placed first in 'all' view")
	}

	tvView := (recommend.Recommendations{
		Pool:     []model.Title{c, a, b},
		Sections: []model.Shelf{{ID: "for-you"}},
	}).View("tv", "movie")

	if len(tvView.Sections[0].Titles) != 1 || tvView.Sections[0].Titles[0].Type != "tv" {
		t.Fatal("category filter failed to prune non-tv titles")
	}
}

func TestInputs_Cadence(t *testing.T) {
	p := defaultInputs()
	p.ActivityDays = []string{"2026-10-15", "2026-10-16"}
	if p.Cadence(testNow) != 24*time.Hour {
		t.Fatal("expected active profile cadence of 24h")
	}

	p.ActivityDays = []string{"2026-10-15"}
	if p.Cadence(testNow) != 7*24*time.Hour {
		t.Fatal("expected inactive profile cadence of 7 days")
	}
}

func TestInputs_FingerprintStability(t *testing.T) {
	p := defaultInputs()
	q := p

	q.ActivityDays = []string{"2026-10-14", "2026-10-15"}
	if p.Fingerprint() != q.Fingerprint() {
		t.Fatal("activity days must not change taste fingerprint")
	}

	q.Genres = []string{"horror", "animation", "action"}
	if p.Fingerprint() != q.Fingerprint() {
		t.Fatal("genre ordering must be canonicalized and not change fingerprint")
	}
}

func TestInputs_Validate(t *testing.T) {
	p := defaultInputs()
	if err := p.Validate(testNow); err != nil {
		t.Fatalf("valid inputs rejected: %v", err)
	}

	invalidFormat := p
	invalidFormat.Format = "podcast"
	if err := invalidFormat.Validate(testNow); err == nil {
		t.Fatal("expected error for invalid format")
	}

	invalidGenre := p
	invalidGenre.Genres = []string{"space-western"}
	if err := invalidGenre.Validate(testNow); err == nil {
		t.Fatal("expected error for invalid genre")
	}
}

func TestSerendipityMix(t *testing.T) {
	taste := []model.Title{
		makeTestTitle("movie", 1, "Taste Movie 1"),
		makeTestTitle("movie", 2, "Taste Movie 2"),
		makeTestTitle("movie", 3, "Taste Movie 3"),
		makeTestTitle("movie", 4, "Taste Movie 4"),
		makeTestTitle("movie", 5, "Taste Movie 5"),
		makeTestTitle("movie", 6, "Taste Movie 6"),
		makeTestTitle("movie", 7, "Taste Movie 7"),
	}
	discovery := []model.Title{
		makeTestTitle("movie", 101, "Discovery Movie 1"),
		makeTestTitle("movie", 102, "Discovery Movie 2"),
		makeTestTitle("movie", 103, "Discovery Movie 3"),
	}

	blended := recommend.SerendipityMix(taste, discovery, 10)
	if len(blended) != 10 {
		t.Fatalf("expected 10 blended titles, got %d", len(blended))
	}

	// Verify both taste and discovery items are present
	hasTaste := false
	hasDiscovery := false
	for _, item := range blended {
		if item.ID < 100 {
			hasTaste = true
		} else {
			hasDiscovery = true
		}
	}
	if !hasTaste || !hasDiscovery {
		t.Fatalf("expected mixture of taste and discovery titles, got %+v", blended)
	}
}

func TestContextualShelf(t *testing.T) {
	titles := []model.Title{
		makeTestTitle("movie", 550, "Fight Club"),
		makeTestTitle("movie", 680, "Pulp Fiction"),
	}

	shelf := recommend.ContextualShelf("Whiplash", titles)
	if shelf.ID != "because-whiplash" {
		t.Errorf("shelf.ID = %q, want because-whiplash", shelf.ID)
	}
	if shelf.Heading != "Because you watched Whiplash" {
		t.Errorf("shelf.Heading = %q, want 'Because you watched Whiplash'", shelf.Heading)
	}
	if len(shelf.Titles) != 2 {
		t.Errorf("expected 2 titles on contextual shelf, got %d", len(shelf.Titles))
	}
}
