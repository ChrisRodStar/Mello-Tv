package probe

import (
	"context"
	"stv-providers/internal/cache"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"net/http"
	"net/url"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

type streamFixture struct {
	samples   atomic.Int32
	third     chan struct{}
	thirdOnce sync.Once
	block     bool
	started   chan struct{}
	startOnce sync.Once
	active    atomic.Int32
}

func (fixture *streamFixture) Fetch(ctx context.Context, request network.Request) (network.Response, error) {
	fixture.active.Add(1)
	defer fixture.active.Add(-1)
	if fixture.started != nil {
		fixture.startOnce.Do(func() { close(fixture.started) })
	}
	if fixture.block {
		<-ctx.Done()
		return network.Response{}, ctx.Err()
	}
	parsed, err := url.Parse(request.URL)
	if err != nil {
		return network.Response{}, err
	}
	body := []byte("#EXTM3U\n#EXTINF:4,\none.ts\n#EXTINF:4,\ntwo.ts\n#EXTINF:4,\nthree.ts\n#EXT-X-ENDLIST\n")
	contentType := "application/vnd.apple.mpegurl"
	if request.Sample {
		body = make([]byte, request.Limit)
		contentType = "video/mp2t"
		if fixture.samples.Add(1) == 3 && fixture.third != nil {
			fixture.thirdOnce.Do(func() { close(fixture.third) })
		}
	}
	return network.Response{
		URL:      parsed,
		Status:   http.StatusOK,
		Header:   http.Header{"Content-Type": []string{contentType}},
		Body:     body,
		Transfer: time.Millisecond,
	}, nil
}

func TestStreamingCandidatesShareOneSampleBudget(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	fixture := &streamFixture{third: make(chan struct{})}
	limits := DefaultLimits()
	limits.Concurrent = 1
	limits.SampleBytes = 16 * 1024
	limits.TotalSampleBytes = 3 * limits.SampleBytes
	pool := &Pool{
		client:  fixture,
		limits:  limits,
		slots:   make(chan struct{}, limits.Concurrent),
		history: cache.NewObservations(),
	}
	jobs := make(chan model.Candidate, 1)
	jobs <- model.Candidate{ID: "fast", Provider: model.VidCore, URL: "https://media.example.com/one.m3u8", Format: model.HLS}
	finished := make(chan []model.Probed, 1)
	go func() { finished <- pool.ProbeStream(ctx, jobs) }()
	select {
	case <-fixture.third:
	case <-ctx.Done():
		t.Fatal("probing waited for candidate collection to close")
	}
	jobs <- model.Candidate{ID: "later", Provider: model.VidLink, URL: "https://media.example.com/two.m3u8", Format: model.HLS}
	close(jobs)
	select {
	case results := <-finished:
		if len(results) != 2 || results[0].Observation.SamplesCompleted != 3 || results[1].Observation.Failure != model.Unavailable {
			t.Fatalf("unexpected shared-budget results: %+v", results)
		}
		if fixture.samples.Load() != 3 {
			t.Fatal("later provider received a new sample budget")
		}
	case <-ctx.Done():
		t.Fatal("streaming probe did not settle")
	}
}

func TestStreamingProbeCancellationJoinsActiveFetches(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	fixture := &streamFixture{block: true, started: make(chan struct{})}
	limits := DefaultLimits()
	pool := &Pool{
		client: fixture, limits: limits,
		slots: make(chan struct{}, limits.Concurrent), history: cache.NewObservations(),
	}
	jobs := make(chan model.Candidate, 1)
	jobs <- model.Candidate{ID: "blocked", URL: "https://media.example.com/master.m3u8", Format: model.HLS}
	finished := make(chan struct{})
	go func() {
		pool.ProbeStream(ctx, jobs)
		close(finished)
	}()
	select {
	case <-fixture.started:
	case <-time.After(2 * time.Second):
		t.Fatal("probe did not start")
	}
	cancel()
	select {
	case <-finished:
		if fixture.active.Load() != 0 || len(pool.slots) != 0 {
			t.Fatal("probe returned before active work released its resources")
		}
	case <-time.After(2 * time.Second):
		t.Fatal("cancelled streaming probe did not join")
	}
}
