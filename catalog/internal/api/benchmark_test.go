//
//  benchmark_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Performance benchmarks for HTTP API layer: route matching, bearer token authentication,
//  health checks, and end-to-end title request handling.
//

package api_test

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"stv-catalog/internal/api"
	"stv-catalog/internal/db"
	"stv-catalog/internal/model"
	"stv-catalog/internal/service"
	"stv-catalog/internal/upstream"
)

func setupBenchmarkAPI(b *testing.B, token string) http.Handler {
	b.Helper()
	dir := b.TempDir()
	store, err := db.Open(filepath.Join(dir, "api_bench.sqlite"))
	if err != nil {
		b.Fatalf("failed to open store: %v", err)
	}
	b.Cleanup(func() { _ = store.Close() })

	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	ctx := context.Background()

	// Seed 100 titles
	for i := 1; i <= 100; i++ {
		t := model.Title{
			Identity: model.Identity{Type: "movie", ID: i},
			Title:    fmt.Sprintf("API Benchmark Movie %d", i),
			Rating:   8.2,
			Poster:   fmt.Sprintf("/poster_%d.jpg", i),
			Details:  true,
		}
		_ = store.Put(ctx, "title", t.Key(), t, now, 24*time.Hour)
	}

	up := upstream.NewUpstream("")
	svc := service.NewService(store, up)
	svc.Now = func() time.Time { return now }

	return api.NewHandler(svc, token)
}

func BenchmarkHandler_HealthLive(b *testing.B) {
	h := setupBenchmarkAPI(b, "")
	req := httptest.NewRequest(http.MethodGet, "/internal/v1/health/live", nil)
	rec := httptest.NewRecorder()

	for b.Loop() {
		rec.Body.Reset()
		clear(rec.Header())
		h.ServeHTTP(rec, req)
	}
}

func BenchmarkHandler_HealthLive_WithAuth(b *testing.B) {
	const token = "01234567890123456789012345678901"
	h := setupBenchmarkAPI(b, token)
	req := httptest.NewRequest(http.MethodGet, "/internal/v1/health/live", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	rec := httptest.NewRecorder()

	for b.Loop() {
		rec.Body.Reset()
		clear(rec.Header())
		h.ServeHTTP(rec, req)
	}
}

func BenchmarkHandler_GetTitle(b *testing.B) {
	h := setupBenchmarkAPI(b, "")
	rec := httptest.NewRecorder()

	i := 0
	for b.Loop() {
		rec.Body.Reset()
		clear(rec.Header())
		target := fmt.Sprintf("/internal/v1/titles/movie/%d", (i%100)+1)
		req := httptest.NewRequest(http.MethodGet, target, nil)
		h.ServeHTTP(rec, req)
		i++
	}
}

func BenchmarkHandler_ParallelGetTitle(b *testing.B) {
	h := setupBenchmarkAPI(b, "")

	b.RunParallel(func(pb *testing.PB) {
		rec := httptest.NewRecorder()
		i := 0
		for pb.Next() {
			rec.Body.Reset()
			clear(rec.Header())
			target := fmt.Sprintf("/internal/v1/titles/movie/%d", (i%100)+1)
			req := httptest.NewRequest(http.MethodGet, target, nil)
			h.ServeHTTP(rec, req)
			i++
		}
	})
}
