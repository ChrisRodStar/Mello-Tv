//
//  upstream_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Unit tests for upstream TMDB HTTP client, artwork key parsing, rate limit backoff,
//  and URL path construction.
//

package upstream_test

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
	"time"

	"stv-catalog/internal/model"
	"stv-catalog/internal/upstream"
)

func TestArtworkKeyAndURL(t *testing.T) {
	cases := []struct {
		size     string
		filename string
		wantKey  string
		wantURL  string
	}{
		{size: "w500", filename: "/poster.jpg", wantKey: "w500/poster.jpg", wantURL: "/internal/v1/artwork/w500/poster.jpg"},
		{size: "w300", filename: "/still.png", wantKey: "w300/still.png", wantURL: "/internal/v1/artwork/w300/still.png"},
		{size: "w1280", filename: "/backdrop.webp", wantKey: "w1280/backdrop.webp", wantURL: "/internal/v1/artwork/w1280/backdrop.webp"},
		{size: "original", filename: "/bad.jpg", wantKey: "", wantURL: ""},
		{size: "w500", filename: "bad-no-slash.jpg", wantKey: "", wantURL: ""},
		{size: "w500", filename: "/bad.exe", wantKey: "", wantURL: ""},
	}

	for _, tc := range cases {
		gotKey := upstream.ArtworkKey(tc.size, tc.filename)
		if gotKey != tc.wantKey {
			t.Errorf("ArtworkKey(%q, %q) = %q, want %q", tc.size, tc.filename, gotKey, tc.wantKey)
		}

		gotURL := upstream.ArtworkURL(tc.size, tc.filename)
		if gotURL != tc.wantURL {
			t.Errorf("ArtworkURL(%q, %q) = %q, want %q", tc.size, tc.filename, gotURL, tc.wantURL)
		}
	}
}

func TestArtKeyFromURL(t *testing.T) {
	cases := []struct {
		urlPath string
		wantKey string
	}{
		{urlPath: "/internal/v1/artwork/w500/poster.jpg", wantKey: "w500/poster.jpg"},
		{urlPath: "/internal/v1/artwork/w300/still.png", wantKey: "w300/still.png"},
		{urlPath: "/other/path/w500/poster.jpg", wantKey: ""},
		{urlPath: "/internal/v1/artwork/invalid", wantKey: ""},
	}

	for _, tc := range cases {
		if got := upstream.ArtKeyFromURL(tc.urlPath); got != tc.wantKey {
			t.Errorf("ArtKeyFromURL(%q) = %q, want %q", tc.urlPath, got, tc.wantKey)
		}
	}
}

func TestUpstream_FetchRateLimit(t *testing.T) {
	calls := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if calls == 1 {
			w.Header().Set("Retry-After", "1")
			w.WriteHeader(http.StatusTooManyRequests)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprintln(w, `{"results": []}`)
	}))
	defer srv.Close()

	u := upstream.NewUpstream("dummy-token")
	u.Base = srv.URL + "/"
	u.Interval = 10 * time.Millisecond

	ctx := context.Background()

	// First call should trigger rate limit backoff
	_, _, err := u.Fetch(ctx, "test", false)
	if err == nil {
		t.Fatal("expected status 429 error on first call")
	}

	// Immediate next call should fail with upstream cooldown
	_, _, err = u.Fetch(ctx, "test", false)
	if err == nil || err.Error() != "upstream cooldown" {
		t.Fatalf("expected 'upstream cooldown' error, got: %v", err)
	}
}

func TestGenreSpecs_MatchCanonicalGenres(t *testing.T) {
	for genre := range model.CanonicalGenres {
		spec, ok := upstream.GenreSpecs[genre]
		if !ok {
			t.Errorf("missing GenreSpec for canonical genre %q", genre)
		}
		if spec.Name == "" {
			t.Errorf("empty display name in GenreSpec for %q", genre)
		}
	}
}

func TestOnboardingGenreDiscovery(t *testing.T) {
	testNow := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	specs := upstream.OnboardingBrowse(testNow)
	if len(specs) != 8 {
		t.Fatalf("expected 8 onboarding categories, got %d", len(specs))
	}
	for _, spec := range specs {
		parsed, err := url.Parse(spec.Path)
		if err != nil {
			t.Fatal(err)
		}
		genre := strings.TrimPrefix(spec.ID, "onboarding-")
		query := parsed.Query()
		if upstream.OnboardingArtworkTitles[spec.ID] <= 0 {
			t.Fatalf("missing curated title for %s", genre)
		}
		language := "en"
		if genre == "animation" {
			language = "ja"
		}
		if query.Get("with_original_language") != language ||
			query.Get("with_genres") != upstream.GenreSpecs[genre].Movie ||
			query.Get("include_adult") != "false" ||
			query.Get("vote_count.gte") != "100" ||
			query.Get("primary_release_date.lte") != testNow.Format("2006-01-02") {
			t.Fatalf("unqualified category %s", genre)
		}
	}
}

func BenchmarkArtworkKey(b *testing.B) {
	b.ReportAllocs()
	for b.Loop() {
		_ = upstream.ArtworkKey("w500", "/poster_12345.jpg")
	}
}

func BenchmarkUpstream_JSON(b *testing.B) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprintln(w, `{"id":550,"title":"Fight Club","vote_average":8.4,"release_date":"1999-10-15"}`)
	}))
	defer srv.Close()

	u := upstream.NewUpstream("dummy-token")
	u.Base = srv.URL + "/"
	u.Interval = 0 // zero throttle for raw throughput benchmark

	ctx := context.Background()
	var out struct {
		ID     int     `json:"id"`
		Title  string  `json:"title"`
		Rating float64 `json:"vote_average"`
	}

	b.ReportAllocs()
	for b.Loop() {
		_ = u.JSON(ctx, "movie/550", &out)
	}
}

func TestRecommendationDiscoveryIncludesOlderAndInternationalTitles(t *testing.T) {
	u := upstream.NewUpstream("")
	path, e := u.DiscoverPath(context.Background(), "crime", "movie", time.Now())
	if e != nil {
		t.Fatal(e)
	}
	parsed, e := url.Parse(path)
	if e != nil {
		t.Fatal(e)
	}
	if parsed.Query().Get("primary_release_date.gte") != "" || parsed.Query().Get("with_original_language") != "" {
		t.Fatal("recommendations still exclude older or international films")
	}
	anime, e := u.DiscoverPath(context.Background(), "animation", "tv", time.Now())
	if e != nil {
		t.Fatal(e)
	}
	parsed, _ = url.Parse(anime)
	if parsed.Query().Get("with_original_language") != "ja" {
		t.Fatal("Anime lost its original-language rule")
	}
}

func TestTopicsUseVerifiedKeywordsAndRuntimeBounds(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Query().Get("query") {
		case "heist":
			fmt.Fprint(w, `{"results":[{"id":10,"name":"heist"}]}`)
		case "time travel":
			fmt.Fprint(w, `{"results":[{"id":20,"name":"time travel"}]}`)
		default:
			fmt.Fprint(w, `{"results":[{"id":30,"name":"unrelated"}]}`)
		}
	}))
	defer server.Close()
	u := upstream.NewUpstream("fixture")
	u.Base = server.URL + "/"
	u.Interval = 0
	specs := u.TopicBrowse(context.Background(), time.Now())
	found := map[string]bool{}
	for _, spec := range specs {
		found[spec.ID] = true
		parsed, e := url.Parse(spec.Path)
		if e != nil {
			t.Fatal(e)
		}
		q := parsed.Query()
		if q.Get("with_original_language") != "" {
			t.Fatal("topics exclude international titles")
		}
		if spec.ID == "topic-short-movies" && (q.Get("with_runtime.gte") != "1" || q.Get("with_runtime.lte") != "99") {
			t.Fatal("short-film promise is not enforced")
		}
		if spec.ID == "topic-heists" && q.Get("with_keywords") != "10" {
			t.Fatal("keyword lookup was not used")
		}
	}
	if !found["topic-heists"] || !found["topic-time-travel"] || !found["topic-classics"] || found["topic-detectives"] {
		t.Fatal("missing or guessed topic membership")
	}
}

func TestListGenreIDsBecomeUsableGenreNames(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, `{"results":[{"id":1,"title":"A mystery","genre_ids":[9648,80],"vote_average":7}]}`)
	}))
	defer server.Close()
	u := upstream.NewUpstream("fixture")
	u.Base = server.URL + "/"
	u.Interval = 0
	titles, e := u.List(context.Background(), "discover/movie", "movie")
	if e != nil || len(titles) != 1 || len(titles[0].Genres) != 2 || titles[0].Genres[0] != "Mystery" {
		t.Fatalf("list lost classification: %+v %v", titles, e)
	}
}
