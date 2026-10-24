package vidfast

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/url"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

func sealedFixture(t *testing.T, plaintext []byte) []byte {
	t.Helper()
	salt := make([]byte, 16)
	nonce := make([]byte, 12)
	key := sha256.Sum256(joined(responseKey(), salt))
	block, err := aes.NewCipher(key[:])
	if err != nil {
		t.Fatal(err)
	}
	mode, err := cipher.NewGCM(block)
	if err != nil {
		t.Fatal(err)
	}
	payload := mode.Seal(nil, nonce, joined([]byte("fixture!"), plaintext), nil)
	return []byte(base64.StdEncoding.EncodeToString(joined(salt, nonce, payload)))
}

func sourceFixture(title model.Title) []byte {
	encoded, _ := json.Marshal(map[string]any{
		"url":               "https://media.example.com/master.m3u8",
		"tmdbId":            title.TMDBID,
		"mp4":               false,
		"noReferrer":        false,
		"englishTrackIndex": 0,
		"tracks": []map[string]string{{
			"file":  "https://media.example.com/sub.srt",
			"label": "English",
		}},
		"expires":    "unknown",
		"protection": nil,
	})
	return encoded
}

func TestNativeProbeAndUnlock(t *testing.T) {
	material := protocolMaterial{
		root: "/zirlaku/u/fixture",
		csrf: "synthetic-csrf-token",
	}
	calls := 0
	title := television()
	adapter := &Adapter{client: fixtureClient{fetch: func(ctx context.Context, request network.Request) (network.Response, error) {
		calls++
		if request.Method != http.MethodPost || !strings.HasPrefix(request.URL, origin+material.root+"/") {
			t.Fatal("protocol request escaped recognized root or method")
		}
		if len(request.Context.Scopes) != 2 || request.Context.Scopes[1].PathPrefix != material.root || request.Context.Scopes[1].Headers["X-Csrf-Token"] != material.csrf {
			t.Fatal("missing scoped protocol header")
		}
		targetURL, _ := url.Parse(request.URL)
		response := network.Response{
			URL:    targetURL,
			Status: http.StatusOK,
		}
		switch calls {
		case 1:
			if !strings.Contains(targetURL.Path, "/JgQXcWPNH7s/") || request.Limit != 4096 {
				t.Fatal("wrong challenge stage")
			}
			response.Body = []byte("synthetic-challenge")
		case 2:
			if !strings.Contains(targetURL.Path, "/dBWKuO8aW6U/") {
				t.Fatal("wrong probe stage")
			}
			response.Body = sealedFixture(t, []byte(`[{"name":"fixture","data":"synthetic_server_token"}]`))
		case 3:
			if targetURL.Path != material.root+"/M6SyMXJBpeg/synthetic_server_token" {
				t.Fatal("wrong unlock stage")
			}
			response.Body = sealedFixture(t, sourceFixture(title))
		default:
			t.Fatal("unbounded protocol requests")
		}
		return response, nil
	}}}
	servers, err := adapter.probeServers(context.Background(), bootstrap{
		Title: title,
		Token: "synthetic_bootstrap_token_only",
	}, material)
	if err != nil || len(servers) != 1 {
		t.Fatalf("native probe rejected: %v", err)
	}
	result, err := adapter.sources(context.Background(), title, servers, material)
	if err != nil || len(result.Candidates) != 1 || calls != 3 {
		t.Fatalf("native source flow rejected: %v", err)
	}
	candidate := result.Candidates[0]
	if candidate.Title != title || candidate.Provider != model.VidFast || candidate.URL != "https://media.example.com/master.m3u8" || candidate.SelectedVariantURL != "" {
		t.Fatal("source identity or master linkage changed")
	}
	if candidate.ContentVerified || candidate.ExpiresAt != nil || candidate.Metadata.TracksKnown || len(candidate.Metadata.Tracks) != 0 {
		t.Fatal("unknown source metadata became verified")
	}
	for _, key := range []string{
		"tracks",
		"englishTrackIndex",
		"expires",
		"protection",
	} {
		if candidate.Metadata.ProviderData[key] == nil {
			t.Fatalf("lost private metadata: %s", key)
		}
	}
}

func TestSourceValidation(t *testing.T) {
	title := model.Title{
		Type:   model.Movie,
		TMDBID: 27205,
		IMDbID: "tt1375666",
	}
	original := string(sourceFixture(title))
	for name, body := range map[string]string{
		"private-url":             strings.ReplaceAll(original, "https://media.example.com/master.m3u8", "https://127.0.0.1/master.m3u8"),
		"wrong-id":                strings.ReplaceAll(original, "27205", "27206"),
		"missing-referrer-policy": strings.ReplaceAll(original, `"noReferrer":false,`, ""),
		"format-conflict":         strings.ReplaceAll(original, `"mp4":false`, `"mp4":true`),
		"null":                    "null",
		"malformed":               "{",
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := normalize([]byte(body), title, "fixture"); model.Category(err) != model.ProtocolChange {
				t.Fatalf("invalid source accepted: %v", err)
			}
		})
	}
	progressive := strings.ReplaceAll(strings.ReplaceAll(original, "master.m3u8", "video.mp4"), `"mp4":false`, `"mp4":true`)
	candidate, err := normalize([]byte(progressive), title, "fixture")
	if err != nil || candidate.Format != model.MP4 {
		t.Fatalf("progressive source rejected: %v", err)
	}
	candidate, err = normalize([]byte(strings.ReplaceAll(original, `"noReferrer":false`, `"noReferrer":true`)), title, "fixture")
	if err != nil || candidate.Context.PublicResources != nil {
		t.Fatal("explicit no-referrer policy lost")
	}
}

func TestStalledAlternativesPreserveCompletedCandidate(t *testing.T) {
	title := television()
	var active atomic.Int32
	var maximum atomic.Int32
	adapter := &Adapter{client: fixtureClient{fetch: func(ctx context.Context, request network.Request) (network.Response, error) {
		current := active.Add(1)
		defer active.Add(-1)
		for old := maximum.Load(); current > old; old = maximum.Load() {
			if maximum.CompareAndSwap(old, current) {
				break
			}
		}
		if strings.HasSuffix(request.URL, "/ready") {
			targetURL, _ := url.Parse(request.URL)
			return network.Response{
				URL:    targetURL,
				Status: 200,
				Body:   sealedFixture(t, sourceFixture(title)),
			}, nil
		}
		<-ctx.Done()
		return network.Response{}, ctx.Err()
	}}}
	ctx, cancel := context.WithTimeout(context.Background(), 800*time.Millisecond)
	defer cancel()
	result, err := adapter.sources(ctx, title, []server{
		{
			Name: "ready",
			Data: "ready",
		},
		{
			Name: "slow",
			Data: "slow",
		},
		{
			Name: "later",
			Data: "later",
		},
		{
			Name: "last",
			Data: "last",
		},
	}, protocolMaterial{root: "/zirlaku/u/fixture"})
	if err != nil || len(result.Candidates) != 1 || len(result.Issues) != 3 {
		t.Fatalf("completed source lost at deadline: %v", err)
	}
	if active.Load() != 0 || maximum.Load() > 3 {
		t.Fatal("unlock worker lifecycle or concurrency bound violated")
	}
	if ctx.Err() != nil {
		t.Fatal("return margin exhausted parent deadline")
	}
}

func TestUnrecognizedBundleAndChallengeBounds(t *testing.T) {
	if _, err := recognizePlayer([]byte("unrecognized player")); model.Category(err) != model.ProtocolChange {
		t.Fatal("unknown executable accepted")
	}
	for _, body := range [][]byte{
		nil,
		[]byte("\n"),
		[]byte("é"),
		[]byte(strings.Repeat("x", 4097)),
	} {
		if _, err := challengeToken(body); err == nil {
			t.Fatal("unrecognized challenge accepted")
		}
	}
	for _, salt := range [][]byte{
		nil,
		make([]byte, 15),
		make([]byte, 17),
	} {
		if _, err := packRequest("fixture", salt, 0); err == nil {
			t.Fatal("invalid request framing accepted")
		}
	}
}

func TestExplicitCancellationDiscardsLookup(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	title := television()
	adapter := &Adapter{client: fixtureClient{fetch: func(context.Context, network.Request) (network.Response, error) {
		cancel()
		return network.Response{}, context.Canceled
	}}}
	_, err := adapter.sources(ctx, title, []server{{
		Name: "fixture",
		Data: "fixture",
	}}, protocolMaterial{root: "/zirlaku/u/fixture"})
	if model.Category(err) != model.Cancelled {
		t.Fatalf("explicit cancellation lost: %v", err)
	}
}

func TestSourceReportedEpisodeMismatch(t *testing.T) {
	title := television()
	var record map[string]any
	if err := json.Unmarshal(sourceFixture(title), &record); err != nil {
		t.Fatal(err)
	}
	record["season"] = 0
	record["episode"] = 8
	body, _ := json.Marshal(record)
	if _, err := normalize(body, title, "fixture"); model.Category(err) != model.ProtocolChange {
		t.Fatal("reported wrong episode accepted")
	}
	record["episode"] = 7
	body, _ = json.Marshal(record)
	candidate, err := normalize(body, title, "fixture")
	if err != nil || candidate.Title != title {
		t.Fatalf("matching special episode or cross-IDs changed: %v", err)
	}
}
