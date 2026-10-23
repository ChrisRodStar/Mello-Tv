//
//  service_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Unit tests for background worker queues, job deduplication, sweep scheduling,
//  and recommendation pipelines.
//

package service_test

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"stv-catalog/internal/db"
	"stv-catalog/internal/model"
	"stv-catalog/internal/service"
	"stv-catalog/internal/upstream"
)

var testNow = time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

func openTestStore(t *testing.T) *db.Store {
	t.Helper()
	dir := t.TempDir()
	store, err := db.Open(filepath.Join(dir, "service_test.sqlite"))
	if err != nil {
		t.Fatalf("failed to open test store: %v", err)
	}
	t.Cleanup(func() { _ = store.Close() })
	return store
}

func TestJob_ValidationAndKey(t *testing.T) {
	cases := []struct {
		job     service.Job
		wantKey string
		valid   bool
	}{
		{
			job: service.Job{
				Kind:     "title",
				Identity: model.Identity{Type: "movie", ID: 100},
			},
			wantKey: "title/movie:100",
			valid:   true,
		},
		{
			job: service.Job{
				Kind:     "season",
				Identity: model.Identity{Type: "tv", ID: 50},
				Season:   2,
			},
			wantKey: "season/tv:50:2",
			valid:   true,
		},
		{
			job: service.Job{
				Kind:   "recommendations",
				Handle: "alice",
			},
			wantKey: "recommendations/alice",
			valid:   true,
		},
		{
			job: service.Job{
				Kind:  "search",
				Query: "Batman",
			},
			wantKey: "search/Batman",
			valid:   true,
		},
		{
			job: service.Job{
				Kind: "browse",
			},
			wantKey: "browse",
			valid:   true,
		},
		{
			job: service.Job{
				Kind:    "artwork",
				Artwork: "w500/image.jpg",
			},
			wantKey: "artwork/w500/image.jpg",
			valid:   false, // Artwork storage is disabled/rejected
		},
	}

	for _, tc := range cases {
		if got := tc.job.Key(); got != tc.wantKey {
			t.Errorf("job.Key() = %q, want %q", got, tc.wantKey)
		}
		err := tc.job.Validate()
		if tc.valid && err != nil {
			t.Errorf("job.Validate() failed unexpectedly for %+v: %v", tc.job, err)
		}
		if !tc.valid && err == nil {
			t.Errorf("job.Validate() expected error for %+v", tc.job)
		}
	}
}

func TestService_RequestDeduplication(t *testing.T) {
	store := openTestStore(t)
	up := upstream.NewUpstream("")
	svc := service.NewService(store, up)
	svc.Now = func() time.Time { return testNow }

	ctx := context.Background()
	job := service.Job{
		Kind:     "title",
		Identity: model.Identity{Type: "movie", ID: 42},
	}

	status1, err := svc.Request(ctx, job)
	if err != nil {
		t.Fatalf("first request failed: %v", err)
	}
	if status1 != "queued" {
		t.Errorf("first request status = %q, want 'queued'", status1)
	}

	// Second request while first is still pending should be deduplicated
	status2, err := svc.Request(ctx, job)
	if err != nil {
		t.Fatalf("second request failed: %v", err)
	}
	if status2 != "deduplicated" {
		t.Errorf("second request status = %q, want 'deduplicated'", status2)
	}
}

func BenchmarkJob_Key(b *testing.B) {
	job := service.Job{
		Kind:     "title",
		Identity: model.Identity{Type: "movie", ID: 550},
	}
	b.ReportAllocs()
	for b.Loop() {
		_ = job.Key()
	}
}

func BenchmarkJob_Validate(b *testing.B) {
	job := service.Job{
		Kind:     "title",
		Identity: model.Identity{Type: "movie", ID: 550},
	}
	b.ReportAllocs()
	for b.Loop() {
		_ = job.Validate()
	}
}

func BenchmarkService_RequestDeduplicated(b *testing.B) {
	dir := b.TempDir()
	store, err := db.Open(filepath.Join(dir, "bench_service.sqlite"))
	if err != nil {
		b.Fatalf("failed to open store: %v", err)
	}
	b.Cleanup(func() { _ = store.Close() })

	up := upstream.NewUpstream("")
	svc := service.NewService(store, up)
	svc.Now = func() time.Time { return testNow }

	ctx := context.Background()
	job := service.Job{
		Kind:     "title",
		Identity: model.Identity{Type: "movie", ID: 100},
	}
	// Pre-queue the job once so subsequent requests hit deduplication
	_, _ = svc.Request(ctx, job)

	b.ReportAllocs()
	for b.Loop() {
		_, _ = svc.Request(ctx, job)
	}
}
