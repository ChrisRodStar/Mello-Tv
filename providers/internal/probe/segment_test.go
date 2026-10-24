package probe

import (
	"context"
	"net/http"
	"net/url"
	"strings"
	"testing"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

type mislabeledFixture struct {
	body []byte
}

func (client *mislabeledFixture) Fetch(_ context.Context, request network.Request) (network.Response, error) {
	parsedURL, _ := url.Parse(request.URL)
	return network.Response{
		URL:    parsedURL,
		Status: http.StatusOK,
		Header: http.Header{"Content-Type": []string{"text/html; charset=UTF-8"}},
		Body:   client.body,
	}, nil
}

func transportFixture() []byte {
	body := make([]byte, 188*5)
	for packet := 0; packet < 5; packet++ {
		body[packet*188] = 0x47
		body[packet*188+3] = 0x10
	}
	return body
}

func TestMislabeledSegmentsRequireTransportPacketEvidence(t *testing.T) {
	valid := transportFixture()
	corrupt := append([]byte(nil), valid...)
	corrupt[188] = 0
	badAdaptation := append([]byte(nil), valid...)
	badAdaptation[3] = 0x30
	badAdaptation[4] = 184
	for _, testCase := range []struct {
		name       string
		body       []byte
		accessible bool
	}{
		{
			name:       "aligned TS",
			body:       valid,
			accessible: true,
		},
		{
			name: "HTML error",
			body: []byte("<!doctype html><title>error</title>"),
		},
		{
			name: "JSON error",
			body: []byte(`{"error":"unavailable"}`),
		},
		{
			name: "playlist",
			body: []byte("#EXTM3U\n#EXTINF:4,\nsegment.ts\n"),
		},
		{
			name: "too few packets",
			body: valid[:188*4],
		},
		{
			name: "broken alignment",
			body: corrupt,
		},
		{
			name: "bad adaptation",
			body: badAdaptation,
		},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			pool := &Pool{
				client: &mislabeledFixture{body: testCase.body},
				limits: DefaultLimits(),
			}
			playlist := Playlist{SampleURL: "https://cdn.example/segment.html"}
			result := pool.sampleSegment(context.Background(), model.Candidate{}, playlist, nil, &byteBudget{remaining: 128 * 1024})
			if result.Observation.Accessible != testCase.accessible {
				t.Fatal("MIME label overrode container evidence")
			}
		})
	}
}

func TestLargePlaylistReservationReturnsUnusedBytes(t *testing.T) {
	// Long signed URLs can make an ordinary movie playlist exceed 512 KiB.
	manifest := "#EXTM3U\n" + strings.Repeat("#EXTINF:4,\n"+strings.Repeat("a", 300)+".ts\n", 1800)
	client := &fixtureMediaClient{playlists: map[string]string{"/movie.m3u8": manifest}}
	pool := &Pool{
		client: client,
		limits: DefaultLimits(),
	}
	budget := &byteBudget{remaining: 2 * 1024 * 1024}
	for index := 0; index < 2; index++ {
		playlist, err := pool.fetchPlaylist(context.Background(), model.Candidate{}, "https://cdn.example/movie.m3u8", budget)
		if err != nil || len(playlist.Samples) != 1800 {
			t.Fatal("bounded large media playlist rejected")
		}
	}
	if budget.remaining != 2*1024*1024-2*int64(len(manifest)) {
		t.Fatal("unused reservation not returned to shared budget")
	}
	if client.requests[0].Limit != 1024*1024 {
		t.Fatal("per-playlist bound changed")
	}
}
