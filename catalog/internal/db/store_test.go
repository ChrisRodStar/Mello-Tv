//
//  store_test.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Unit and transactional safety tests for SQLite persistence, WAL checkpointing,
//  schema migrations, storage limits, and non-blocking backups via VACUUM INTO.
//

package db_test

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"stv-catalog/internal/db"
	"stv-catalog/internal/model"
)

var testNow = time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)

func openTestStore(t *testing.T) (*db.Store, string) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "test.sqlite")
	store, err := db.Open(path)
	if err != nil {
		t.Fatalf("db.Open failed: %v", err)
	}
	t.Cleanup(func() { _ = store.Close() })
	return store, path
}

func TestStore_PutAndGet(t *testing.T) {
	store, _ := openTestStore(t)
	ctx := context.Background()

	title := model.Title{
		Identity: model.Identity{Type: "movie", ID: 101},
		Title:    "Inception",
		Rating:   8.8,
		Details:  true,
	}

	err := store.Put(ctx, "title", title.Key(), title, testNow, 24*time.Hour)
	if err != nil {
		t.Fatalf("store.Put failed: %v", err)
	}

	record, err := store.Get(ctx, "title", title.Key(), testNow)
	if err != nil {
		t.Fatalf("store.Get failed: %v", err)
	}

	if record.State != "fresh" {
		t.Errorf("record state = %q, want 'fresh'", record.State)
	}

	// Test expired record state
	expiredRecord, err := store.Get(ctx, "title", title.Key(), testNow.Add(48*time.Hour))
	if err != nil {
		t.Fatalf("store.Get expired record failed: %v", err)
	}
	if expiredRecord.State != "stale" {
		t.Errorf("expired record state = %q, want 'stale'", expiredRecord.State)
	}
}

func TestStore_CapacityLimits(t *testing.T) {
	store, _ := openTestStore(t)
	ctx := context.Background()

	store.MaxBytes = 1
	err := store.Put(ctx, "shelf", "test-shelf", map[string]string{"foo": "bar"}, testNow, time.Hour)
	if !errors.Is(err, db.ErrCapacity) {
		t.Fatalf("expected ErrCapacity, got: %v", err)
	}
}

func TestStore_SnapshotAtomicity(t *testing.T) {
	store, _ := openTestStore(t)
	ctx := context.Background()

	put := func(v int) error {
		return store.PutBatch(ctx, []db.Write{
			{Kind: "shelf", Key: "a", Data: map[string]int{"gen": v}, TTL: time.Hour},
			{Kind: "shelf", Key: "b", Data: map[string]int{"gen": v}, TTL: time.Hour},
		}, testNow)
	}

	if err := put(0); err != nil {
		t.Fatal(err)
	}

	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 1; i < 30; i++ {
			if err := put(i); err != nil {
				return
			}
		}
	}()

	for range 30 {
		snap, err := store.Export(ctx, testNow)
		if err != nil {
			t.Fatal(err)
		}
		if len(snap.Records) == 2 {
			if snap.Records[0].Revision != snap.Records[1].Revision || !bytes.Equal(snap.Records[0].Data, snap.Records[1].Data) {
				t.Fatal("torn snapshot detected during concurrent writes")
			}
		}
	}
	wg.Wait()
}

func TestStore_BackupAndPermissions(t *testing.T) {
	store, _ := openTestStore(t)
	ctx := context.Background()

	path := filepath.Join(t.TempDir(), "backup.sqlite")
	if err := store.Backup(ctx, path); err != nil {
		t.Fatalf("store.Backup failed: %v", err)
	}

	// Backup cannot overwrite existing file
	if err := store.Backup(ctx, path); err == nil {
		t.Fatal("backup should fail when target file already exists")
	}

	info, err := os.Stat(path)
	if err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("backup file permissions must be 0600, got: %v", info.Mode().Perm())
	}

	restored, err := db.Open(path)
	if err != nil {
		t.Fatalf("restored db.Open failed: %v", err)
	}
	defer restored.Close()

	if err := restored.Health(ctx); err != nil {
		t.Fatalf("restored db health failed: %v", err)
	}
}
