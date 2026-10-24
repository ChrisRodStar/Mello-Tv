package probe

import (
	"context"
	"stv-providers/internal/cache"
	"stv-providers/internal/model"
	"testing"
	"time"
)

func reusePool(client mediaClient) *Pool {
	limits := DefaultLimits()
	limits.Concurrent = 1
	return &Pool{client: client, limits: limits, slots: make(chan struct{}, 1), history: cache.NewObservations()}
}

func TestRecentHLSEvidenceRequiresNewAccessAndIsolatesScope(t *testing.T) {
	client := &streamFixture{}
	pool := reusePool(client)
	candidate := model.Candidate{ID: "fixture", Provider: model.VidSrc, URL: "https://media.example/entry.m3u8", Format: model.HLS}
	ctx := WithReuseScope(context.Background(), "one-profile-device-title", false)
	first := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if len(first) != 1 || first[0].Observation.SamplesCompleted != 3 {
		t.Fatal("fresh probe missing")
	}
	at := first[0].Observation.At
	count := client.samples.Load()
	second := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if len(second) != 1 || !second[0].Observation.At.Equal(at) || client.samples.Load()-count != 1 {
		t.Fatal("warm evidence did not preserve original measurement and check access")
	}
	second[0].Source.URL = "changed"
	third := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if third[0].Source.URL != candidate.URL {
		t.Fatal("caller mutated immutable evidence")
	}
	count = client.samples.Load()
	pool.ProbeAll(WithReuseScope(context.Background(), "other-profile", false), []model.Candidate{candidate})
	if client.samples.Load()-count != 3 {
		t.Fatal("probe evidence crossed lookup scope")
	}
	count = client.samples.Load()
	pool.ProbeAll(WithReuseScope(context.Background(), "one-profile-device-title", true), []model.Candidate{candidate})
	if client.samples.Load()-count != 3 {
		t.Fatal("explicit refresh reused evidence")
	}
}

func TestMP4ReuseDetectsChangedObjectBeforeReturningEvidence(t *testing.T) {
	client := &mp4FixtureClient{body: fixtureMP4("avc1")}
	pool := reusePool(client)
	candidate := model.Candidate{ID: "mp4", Provider: model.VidLink, URL: "https://media.example/video.mp4", Format: model.MP4}
	ctx := WithReuseScope(context.Background(), "profile-device-title", false)
	first := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if len(first) != 1 || !first[0].Observation.Accessible {
		t.Fatal("MP4 fixture inaccessible")
	}
	calls := len(client.starts)
	warm := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if len(client.starts)-calls != 1 || !warm[0].Observation.At.Equal(first[0].Observation.At) {
		t.Fatal("MP4 range check did not reuse measurement")
	}
	client.body[8] ^= 1
	calls = len(client.starts)
	changed := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if len(client.starts)-calls < 4 || changed[0].Observation.At.Equal(first[0].Observation.At) {
		t.Fatal("changed MP4 prefix reused old evidence")
	}
	client.ignoreRange = true
	rejected := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if rejected[0].Observation.Accessible {
		t.Fatal("failed fresh access check returned cached success")
	}
}

func TestReuseDoesNotExtendEvidenceAge(t *testing.T) {
	client := &streamFixture{}
	pool := reusePool(client)
	candidate := model.Candidate{ID: "fixture", Provider: model.VidSrc, URL: "https://media.example/entry.m3u8", Format: model.HLS}
	ctx := WithReuseScope(context.Background(), "one", false)
	first := pool.ProbeAll(ctx, []model.Candidate{candidate})
	first[0].Observation.At = time.Now().Add(-time.Hour)
	// Caller mutation must not change the retained observation or refresh its TTL.
	second := pool.ProbeAll(ctx, []model.Candidate{candidate})
	if time.Since(second[0].Observation.At) > time.Minute {
		t.Fatal("caller altered evidence age")
	}
}
