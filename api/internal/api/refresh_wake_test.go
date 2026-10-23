package api

import (
	"context"
	"testing"
	"time"
)

func TestRefreshOutboxWakesOwnedWorkerBeforeMinuteTick(t *testing.T) {
	fixture := newFixture(t)
	passes := make(chan struct{}, 1)
	fixture.s.startWorker(func(ctx context.Context) {
		fixture.s.flushRefresh(ctx)
		select {
		case passes <- struct{}{}:
		default:
		}
	}, fixture.s.refreshWake)
	select {
	case <-passes:
	case <-time.After(3 * time.Second):
		t.Fatal("startup pass missing")
	}
	if err := fixture.s.queueRefresh(context.Background(), map[string]any{"kind": "season", "type": "tv", "id": 1622, "season": 1}); err != nil {
		t.Fatal(err)
	}
	select {
	case <-passes:
	case <-time.After(3 * time.Second):
		t.Fatal("refresh waited for periodic minute tick")
	}
	var count int
	if err := fixture.s.db.QueryRow("SELECT count(*) FROM refresh_jobs").Scan(&count); err != nil || count != 0 {
		t.Fatal("refresh outbox was not delivered", err)
	}
	select {
	case <-fixture.s.snapshotWake:
	default:
		t.Fatal("successful refresh did not wake snapshot owner")
	}
}
