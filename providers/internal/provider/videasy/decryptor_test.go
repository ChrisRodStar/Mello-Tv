package videasy

import (
	"context"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/probe"
)

func TestKeystreamVector(t *testing.T) {
	st := initStream("test-seed", 1622)
	expectedWords := []uint32{
		1565862539, 4109863583,
		230583110, 1106669467,
		4046983225, 3597725526,
		55526469, 3807938347,
	}

	for i, expected := range expectedWords {
		actual := st.nextByte(uint32(i))
		if actual != expected {
			t.Fatalf("word %d: expected %d, got %d", i, expected, actual)
		}
	}
}

func TestAdapterCapabilities(t *testing.T) {
	adapter := New(nil)
	if adapter.ID() != model.Videasy {
		t.Fatalf("expected ID %s, got %s", model.Videasy, adapter.ID())
	}
	caps := adapter.Capabilities()
	if len(caps.MediaTypes) != 2 {
		t.Fatalf("expected 2 media types, got %d", len(caps.MediaTypes))
	}
}

func TestResolveDirect(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping live network integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()

	opts, err := network.LocalMullvadOptions(ctx, "1.1.1.1:53")
	if err != nil {
		t.Fatalf("opts: %v", err)
	}
	client, err := network.New(opts)
	if err != nil {
		t.Fatalf("client: %v", err)
	}
	defer client.Close()

	adapter := New(client)
	lookup := model.Lookup{
		Title: model.NewTV(1622, 1, 10),
	}

	res, err := adapter.Resolve(ctx, lookup)
	if err != nil {
		t.Fatalf("resolve failed: %v", err)
	}

	t.Logf("Found %d candidates in %v", len(res.Candidates), res.Duration)
	for _, c := range res.Candidates {
		t.Logf("Candidate: %s -> %s", c.ID, c.URL)
	}
}

func TestDirectStreamProbe(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping live network integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()

	opts, err := network.LocalMullvadOptions(ctx, "1.1.1.1:53")
	if err != nil {
		t.Fatalf("opts: %v", err)
	}
	client, err := network.New(opts)
	if err != nil {
		t.Fatalf("client: %v", err)
	}
	defer client.Close()

	adapter := New(client)
	// Test The Matrix (movie 603)
	lookup := model.Lookup{
		Title: model.Title{
			Type:   model.Movie,
			TMDBID: 603,
			IMDbID: "tt0133093",
		},
	}

	res, err := adapter.Resolve(ctx, lookup)
	if err != nil {
		t.Fatalf("resolve: %v", err)
	}

	for _, c := range res.Candidates {
		if c.ID != "videasy-miami-master" && c.ID != "videasy-boise-master" {
			continue
		}
		resp, err := client.Fetch(ctx, network.Request{
			URL:     c.URL,
			Context: c.Context,
		})
		if err != nil {
			t.Fatalf("[%s] GET %s error: %v", c.ID, c.URL, err)
		}
		t.Logf("[%s] master playlist:\n%s", c.ID, string(resp.Body))

		// Parse playlist
		parsed, err := probe.ParsePlaylist(resp.Body, resp.URL)
		if err != nil {
			t.Fatalf("[%s] parse master: %v", c.ID, err)
		}
		t.Logf("[%s] variants: %d", c.ID, len(parsed.Metadata.Variants))
		for _, v := range parsed.Metadata.Variants {
			var h int
			if v.Height != nil {
				h = *v.Height
			}
			t.Logf("  variant %dp -> %s", h, v.URL)
			if h == 2160 {
				vResp, err := client.Fetch(ctx, network.Request{
					URL:     v.URL,
					Context: c.Context,
				})
				if err != nil {
					t.Fatalf("fetch 2160p playlist: %v", err)
				}
				t.Logf("  2160p playlist (first 500 chars):\n%s", string(vResp.Body[:min(len(vResp.Body), 500)]))

				vParsed, err := probe.ParsePlaylist(vResp.Body, vResp.URL)
				if err != nil {
					t.Fatalf("parse 2160p playlist: %v", err)
				}
				t.Logf("  2160p samples: %d, sampleURL: %s", len(vParsed.Samples), vParsed.SampleURL)

				// Fetch sample segment
				sResp, err := client.Fetch(ctx, network.Request{
					URL:     vParsed.SampleURL,
					Context: c.Context,
					Headers: map[string]string{
						"Range": "bytes=0-131071",
					},
					Limit:  131072,
					Sample: true,
				})
				if err != nil {
					t.Fatalf("fetch sample segment: %v", err)
				}
				if sResp.Status != 200 && sResp.Status != 206 {
					t.Fatalf("unexpected sample segment status: %d", sResp.Status)
				}
			}
		}
	}
}
