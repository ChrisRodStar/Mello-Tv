package api

import (
	"context"
	"fmt"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func recommendationTestTitle(typ string, id int64) Title {
	return Title{Identity: Identity{typ, id}, Title: fmt.Sprintf("Choice %d", id), Date: "1998-01-01", Poster: "/internal/v1/artwork/w500/poster.jpg", Genres: []string{"Mystery"}}
}

func (f *fixture) recommendationSnapshot(rows []Shelf, personal map[string]Recommendations) {
	f.t.Helper()
	now := time.Now().UTC().Add(-2 * time.Hour)
	records := []Record{}
	add := func(kind, key string, data any) {
		records = append(records, Record{Kind: kind, Key: key, Data: marshal(data), Updated: now, Expires: now.Add(time.Hour), Revision: 1, State: "stale"})
	}
	for _, row := range rows {
		add("shelf", row.ID, row)
	}
	for pid, rec := range personal {
		add("recommendations", pid, rec)
	}
	f.mu.Lock()
	f.snapshot = Snapshot{Version: "1", Revision: 1, Exported: now, Records: records}
	f.mu.Unlock()
	if e := f.s.SyncSnapshot(context.Background()); e != nil {
		f.t.Fatal(e)
	}
}

func TestRecommendationPageUsesRetainedDataAndNoRepeats(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	pool := []Title{}
	for i := int64(1); i <= 24; i++ {
		pool = append(pool, recommendationTestTitle("movie", i))
	}
	topic := []Title{}
	for i := int64(10); i <= 30; i++ {
		topic = append(topic, recommendationTestTitle("movie", i))
	}
	rec := Recommendations{Fingerprint: "saved", Pool: pool, Sections: []Shelf{{ID: "for-you", Heading: "Picked for you"}, {ID: "taste-movie-crime", Heading: "Crime movies", Titles: pool}}}
	f.recommendationSnapshot([]Shelf{{ID: "topic-mysteries", Heading: "Mystery stories", Kind: "topic", Titles: topic}}, map[string]Recommendations{p.ID: rec})
	f.mu.Lock()
	f.catalogMode = "outage"
	f.mu.Unlock()
	request := httptest.NewRequest("GET", "/api/v1/profiles/"+p.ID+"/home", nil)
	page, e := f.s.recommendationPage(request, p)
	if e != nil {
		t.Fatal(e)
	}
	seen := map[string]bool{}
	for _, title := range page.Featured {
		seen[title.Key()] = true
	}
	for _, row := range page.Sections {
		if strings.Contains(row.Heading, "Top 10") || strings.Contains(row.Heading, "Premiere") {
			t.Fatal("invented chart or premiere")
		}
		for _, title := range row.Titles {
			if seen[title.Key()] {
				t.Fatalf("repeated title %s", title.Key())
			}
			seen[title.Key()] = true
		}
	}
	if len(page.Featured) != 1 || len(page.Sections) < 2 {
		t.Fatalf("page lost useful choices: %+v", page)
	}
	again, e := f.s.recommendationPage(request, p)
	if e != nil || page.Version != again.Version {
		t.Fatal("unchanged page is unstable")
	}
	if f.providerCalls != 0 {
		t.Fatal("browsing contacted a provider")
	}
}

func TestRecommendationFeedbackIsImmediateReversibleAndProfileScoped(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	other := f.profile("")
	titles := []Title{}
	for i := int64(1); i <= 8; i++ {
		titles = append(titles, recommendationTestTitle("movie", i))
	}
	f.recommendationSnapshot([]Shelf{{ID: "topic-classics", Heading: "Before 2000", Kind: "topic", Titles: titles}}, nil)
	path := "/api/v1/profiles/" + p.ID
	f.want(204, "PUT", path+"/recommendation-hidden/movie/1", "", nil, "")
	page, e := f.s.recommendationPage(httptest.NewRequest("GET", path+"/home", nil), p)
	if e != nil {
		t.Fatal(e)
	}
	for _, row := range page.Sections {
		for _, title := range row.Titles {
			if title.ID == 1 {
				t.Fatal("hidden title survived saved snapshot")
			}
		}
	}
	otherPage, e := f.s.recommendationPage(httptest.NewRequest("GET", "/home", nil), other)
	if e != nil || len(otherPage.Featured) != 1 || otherPage.Featured[0].ID != 1 {
		t.Fatal("feedback leaked to another profile")
	}
	f.want(204, "PUT", path+"/recommendation-topics/topic-classics", "", nil, "")
	page, e = f.s.recommendationPage(httptest.NewRequest("GET", path+"/home", nil), p)
	if e != nil || len(page.Sections) != 0 {
		t.Fatal("hidden category survived")
	}
	f.restart()
	f.want(200, "GET", path+"/recommendation-feedback", "", nil, "")
	f.want(204, "DELETE", path+"/recommendation-hidden/movie/1", "", nil, "")
	f.want(204, "DELETE", path+"/recommendation-topics/topic-classics", "", nil, "")
	page, e = f.s.recommendationPage(httptest.NewRequest("GET", path+"/home", nil), p)
	if e != nil || len(page.Featured) != 1 || page.Featured[0].ID != 1 {
		t.Fatal("undo did not restore suggestions")
	}
	var count int
	if e = f.s.db.QueryRow("SELECT count(*) FROM progress WHERE profile=?", p.ID).Scan(&count); e != nil || count != 0 {
		t.Fatal("feedback fabricated viewing progress")
	}
}

func TestRecommendationFiltersRejectBadInputAndKeepFreshGeneralRows(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	old := []Title{recommendationTestTitle("movie", 1), recommendationTestTitle("movie", 2), recommendationTestTitle("movie", 3)}
	fresh := []Title{recommendationTestTitle("movie", 10), recommendationTestTitle("movie", 11), recommendationTestTitle("movie", 12)}
	short := []Title{recommendationTestTitle("movie", 20), recommendationTestTitle("movie", 21)}
	short[0].Runtime = 99
	short[1].Runtime = 120
	f.recommendationSnapshot([]Shelf{{ID: "topic-classics", Heading: "Before 2000", Titles: fresh}, {ID: "topic-short-movies", Heading: "Under 100 minutes", Titles: short}}, map[string]Recommendations{p.ID: {Fingerprint: "saved", Pool: old, Sections: []Shelf{{ID: "topic-classics", Heading: "Before 2000", Titles: old}}}})
	for _, query := range []string{"?type=invalid", "?topic=invalid"} {
		if _, e := f.s.recommendationPage(httptest.NewRequest("GET", "/home"+query, nil), p); e == nil {
			t.Fatal("invalid filter accepted")
		}
	}
	page, e := f.s.recommendationPage(httptest.NewRequest("GET", "/home?topic=short", nil), p)
	if e != nil || len(page.Sections) != 1 || len(page.Sections[0].Titles) != 1 || page.Sections[0].Titles[0].ID != 20 {
		t.Fatal("short filter ignored known runtime")
	}
	page, e = f.s.recommendationPage(httptest.NewRequest("GET", "/home?topic=classics", nil), p)
	if e != nil || len(page.Sections) != 1 || page.Sections[0].Titles[0].ID != 10 {
		t.Fatal("daily snapshot shadowed current general shelf")
	}
}

func TestRecommendationEligibilityDoesNotHideTVAfterOneEpisode(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	for _, typ := range []string{"movie", "tv"} {
		progress := Progress{Target: Target{Identity: Identity{typ, 7}}, Completed: true, Updated: time.Now()}
		if typ == "tv" {
			season, episode := 1, 1
			progress.Season = &season
			progress.Episode = &episode
		}
		if _, e := f.s.db.Exec("INSERT INTO progress VALUES(?,?,?,?,?)", p.ID, progress.Target.Key(), typ, 7, marshal(progress)); e != nil {
			t.Fatal(e)
		}
	}
	excluded, _, e := f.s.recommendationExclusions(httptest.NewRequest("GET", "/home", nil), p.ID)
	if e != nil || !excluded["movie:7"] || excluded["tv:7"] {
		t.Fatal("movie completion and episode completion were confused")
	}
	future := recommendationTestTitle("movie", 99)
	future.Date = "2999-01-01"
	movie := recommendationTestTitle("movie", 8)
	tv := recommendationTestTitle("tv", 8)
	got := eligibleTitles([]Title{future, movie, tv, movie}, "all", excluded, time.Now())
	if len(got) != 2 {
		t.Fatal("future release, duplicate, or media-type collision mishandled")
	}
}

func TestIgnoredTasteRetainsFavoritesButStopsSeeding(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	path := "/api/v1/profiles/" + p.ID
	f.want(204, "PUT", path+"/favorites/movie/7", "", nil, "")
	f.want(204, "PUT", path+"/recommendation-ignored/movie/7", "", nil, "")
	input, _, e := f.s.derived(context.Background(), p.ID)
	if e != nil || len(input.Favorites) != 0 {
		t.Fatal("ignored favorite still seeded taste")
	}
	var count int
	if e = f.s.db.QueryRow("SELECT count(*) FROM lists WHERE profile=? AND kind='favorites'", p.ID).Scan(&count); e != nil || count != 1 {
		t.Fatal("ignore deleted user favorite")
	}
	f.want(204, "DELETE", path+"/recommendation-ignored/movie/7", "", nil, "")
	input, _, e = f.s.derived(context.Background(), p.ID)
	if e != nil || len(input.Favorites) != 1 {
		t.Fatal("undo failed to restore taste seed")
	}
}

func TestCategoryExpansionIncludesMoreThanHomePreview(t *testing.T) {
	f := newFixture(t)
	p := f.profile("")
	titles := []Title{}
	for i := int64(1); i <= 20; i++ {
		titles = append(titles, recommendationTestTitle("movie", i))
	}
	f.recommendationSnapshot([]Shelf{{ID: "topic-classics", Heading: "Before 2000", Titles: titles}}, nil)
	page, e := f.s.recommendationPage(httptest.NewRequest("GET", "/home?shelf=topic-classics", nil), p)
	if e != nil || len(page.Sections) != 1 || len(page.Sections[0].Titles) != 20 || len(page.Featured) != 0 {
		t.Fatal("See all lost titles or reserved a featured tile")
	}
	hidden, e := f.s.recommendationPage(httptest.NewRequest("GET", "/home?shelf=topic-unknown", nil), p)
	if e != nil || len(hidden.Sections) != 0 {
		t.Fatal("unknown category fell back to unrelated suggestions")
	}
}
