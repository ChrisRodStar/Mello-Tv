//
//  benchmark_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Performance benchmarks for the recommendation engine: ranking scoring formulas,
//  franchise sequel spacing, taste input fingerprints, and viewing signal decay.
//

package recommend_test

import (
	"fmt"
	"testing"
	"time"

	"stv-catalog/internal/model"
	"stv-catalog/internal/recommend"
)

func generateBenchmarkTitles(count int) []model.Title {
	titles := make([]model.Title, count)
	for i := 0; i < count; i++ {
		typ := "movie"
		if i%3 == 0 {
			typ = "tv"
		}
		titles[i] = model.Title{
			Identity: model.Identity{Type: typ, ID: 1000 + i},
			Title:    fmt.Sprintf("Benchmark Franchise: Episode %d", i%10),
			Poster:   fmt.Sprintf("/poster_%d.jpg", i),
			Rating:   6.0 + float64(i%40)/10.0,
		}
	}
	return titles
}

func generateBenchmarkInputs() recommend.Inputs {
	signals := make([]recommend.ViewingSignal, 50)
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	for i := 0; i < 50; i++ {
		typ := "movie"
		if i%2 == 0 {
			typ = "tv"
		}
		signals[i] = recommend.ViewingSignal{
			Identity:           model.Identity{Type: typ, ID: 500 + i},
			Title:              fmt.Sprintf("Watched Title %d", i),
			Watched:            float64(600 + i*120),
			Duration:           3600,
			Completed:          i%3 == 0,
			MeaningfulEpisodes: i % 10,
			UpdatedAt:          now.Add(-time.Duration(i) * 24 * time.Hour),
		}
	}

	favorites := make([]recommend.Favorite, 20)
	for i := 0; i < 20; i++ {
		favorites[i] = recommend.Favorite{
			Identity: model.Identity{Type: "movie", ID: 100 + i},
			Title:    fmt.Sprintf("Favorite Title %d", i),
		}
	}

	return recommend.Inputs{
		Completed:    true,
		Format:       "both",
		Genres:       []string{"action", "comedy", "drama", "scifi", "animation"},
		Favorites:    favorites,
		Signals:      signals,
		ActivityDays: []string{"2026-10-14", "2026-10-15", "2026-10-16"},
	}
}

func BenchmarkInputs_Fingerprint(b *testing.B) {
	inputs := generateBenchmarkInputs()
	b.ReportAllocs()
	for b.Loop() {
		_ = inputs.Fingerprint()
	}
}

func BenchmarkInputs_Seeds(b *testing.B) {
	inputs := generateBenchmarkInputs()
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	b.ReportAllocs()
	for b.Loop() {
		_ = inputs.Seeds(now)
	}
}

func BenchmarkRank_100Candidates(b *testing.B) {
	titlesA := generateBenchmarkTitles(50)
	titlesB := generateBenchmarkTitles(50)
	lists := []recommend.Contribution{
		{Titles: titlesA, Weight: 5.0},
		{Titles: titlesB, Weight: 3.0},
	}
	excluded := map[string]bool{"movie:1001": true, "movie:1002": true}

	b.ReportAllocs()
	for b.Loop() {
		_ = recommend.Rank(lists, excluded)
	}
}

func BenchmarkRank_1000Candidates(b *testing.B) {
	titlesA := generateBenchmarkTitles(500)
	titlesB := generateBenchmarkTitles(500)
	lists := []recommend.Contribution{
		{Titles: titlesA, Weight: 5.0},
		{Titles: titlesB, Weight: 3.5},
	}
	excluded := map[string]bool{"movie:1001": true, "movie:1005": true}

	b.ReportAllocs()
	for b.Loop() {
		_ = recommend.Rank(lists, excluded)
	}
}

func BenchmarkDiversify_100Titles(b *testing.B) {
	titles := generateBenchmarkTitles(100)
	b.ReportAllocs()
	for b.Loop() {
		_ = recommend.Diversify(titles)
	}
}

func BenchmarkRecommendations_View(b *testing.B) {
	pool := generateBenchmarkTitles(100)
	rec := recommend.Recommendations{
		Fingerprint: "test-fingerprint",
		Pool:        pool,
		Sections: []model.Shelf{
			{ID: "for-you", Heading: "Picked for you", Titles: pool[:24]},
			{ID: "taste-movie-action", Heading: "Action Movies", Titles: pool[:20]},
		},
	}

	b.ReportAllocs()
	for b.Loop() {
		_ = rec.View("all", "movie")
	}
}

func BenchmarkSerendipityMix(b *testing.B) {
	taste := generateBenchmarkTitles(70)
	discovery := generateBenchmarkTitles(30)
	b.ReportAllocs()
	for b.Loop() {
		_ = recommend.SerendipityMix(taste, discovery, 24)
	}
}
