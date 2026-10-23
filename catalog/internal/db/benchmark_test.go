//
//  benchmark_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Performance benchmarks for SQLite storage engine: point-lookups, batch writes,
//  cross-ID enrichments, full-text substring queries, and snapshot exports.
//

package db_test

import (
	"context"
	"fmt"
	"path/filepath"
	"testing"
	"time"

	"stv-catalog/internal/db"
	"stv-catalog/internal/model"
)

func setupSeededBenchmarkStore(b *testing.B, titleCount int) (*db.Store, context.Context) {
	b.Helper()
	dir := b.TempDir()
	store, err := db.Open(filepath.Join(dir, "bench.sqlite"))
	if err != nil {
		b.Fatalf("failed to open benchmark store: %v", err)
	}
	b.Cleanup(func() { _ = store.Close() })

	ctx := context.Background()
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

	writes := make([]db.Write, titleCount)
	for i := 0; i < titleCount; i++ {
		t := model.Title{
			Identity: model.Identity{Type: "movie", ID: 1000 + i},
			Title:    fmt.Sprintf("Seeded Benchmark Movie %d", i),
			Rating:   7.5,
			Poster:   fmt.Sprintf("/poster_%d.jpg", i),
			Details:  true,
		}
		writes[i] = db.Write{
			Kind: "title",
			Key:  t.Key(),
			Data: t,
			TTL:  24 * time.Hour,
		}
	}

	if err := store.PutBatch(ctx, writes, now); err != nil {
		b.Fatalf("failed to seed store: %v", err)
	}

	// Also add cross-IDs for half of them
	crossWrites := make([]db.Write, titleCount/2)
	for i := 0; i < titleCount/2; i++ {
		crossWrites[i] = db.Write{
			Kind: "cross_ids",
			Key:  fmt.Sprintf("movie:%d", 1000+i),
			Data: db.CrossIDs{IMDb: fmt.Sprintf("tt%07d", i+1), MAL: 2000 + i},
			TTL:  365 * 24 * time.Hour,
		}
	}
	if err := store.PutBatch(ctx, crossWrites, now); err != nil {
		b.Fatalf("failed to seed cross-IDs: %v", err)
	}

	return store, ctx
}

func BenchmarkStore_Get(b *testing.B) {
	store, ctx := setupSeededBenchmarkStore(b, 500)
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

	i := 0
	for b.Loop() {
		key := fmt.Sprintf("movie:%d", 1000+(i%500))
		_, _ = store.Get(ctx, "title", key, now)
		i++
	}
}

func BenchmarkStore_TitleEnriched(b *testing.B) {
	store, ctx := setupSeededBenchmarkStore(b, 500)
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

	i := 0
	for b.Loop() {
		key := fmt.Sprintf("movie:%d", 1000+(i%250))
		_, _ = store.Title(ctx, key, now)
		i++
	}
}

func BenchmarkStore_Search(b *testing.B) {
	store, ctx := setupSeededBenchmarkStore(b, 1000)
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

	i := 0
	for b.Loop() {
		query := fmt.Sprintf("Movie %d", i%100)
		_, _ = store.Search(ctx, query, 0, now)
		i++
	}
}

func BenchmarkStore_PutSingle(b *testing.B) {
	dir := b.TempDir()
	store, err := db.Open(filepath.Join(dir, "put_bench.sqlite"))
	if err != nil {
		b.Fatalf("failed to open store: %v", err)
	}
	b.Cleanup(func() { _ = store.Close() })

	ctx := context.Background()
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	t := model.Title{
		Identity: model.Identity{Type: "movie", ID: 9999},
		Title:    "Benchmark Single Write",
		Rating:   8.0,
	}

	for b.Loop() {
		_ = store.Put(ctx, "title", t.Key(), t, now, 24*time.Hour)
	}
}

func BenchmarkStore_PutBatch50(b *testing.B) {
	dir := b.TempDir()
	store, err := db.Open(filepath.Join(dir, "batch_bench.sqlite"))
	if err != nil {
		b.Fatalf("failed to open store: %v", err)
	}
	b.Cleanup(func() { _ = store.Close() })

	ctx := context.Background()
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

	writes := make([]db.Write, 50)
	for i := 0; i < 50; i++ {
		writes[i] = db.Write{
			Kind: "shelf",
			Key:  fmt.Sprintf("bench-shelf-%d", i),
			Data: map[string]string{"heading": "Benchmark Shelf"},
			TTL:  6 * time.Hour,
		}
	}

	for b.Loop() {
		_ = store.PutBatch(ctx, writes, now)
	}
}

func BenchmarkStore_ExportSnapshot(b *testing.B) {
	store, ctx := setupSeededBenchmarkStore(b, 200)
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

	for b.Loop() {
		_, _ = store.Export(ctx, now)
	}
}
