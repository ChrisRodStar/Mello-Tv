package vidfast

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"net/http"
	"net/url"
	"os"
	"strings"
	"testing"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

const fixturePlayerSHA256 = "ec8e48186134e2d088456999c8931601825a1d9e05f3913c3e7b76e94553fad1"

func playerFixture(t *testing.T) []byte {
	t.Helper()
	body, err := os.ReadFile("../../../testdata/providers/vidfast/player.txt")
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func fixtureRecognizer(body []byte) (protocolMaterial, error) {
	return recognizePlayerVersion(body, fixturePlayerSHA256)
}

func TestRecognizedFixtureMaterial(t *testing.T) {
	body := playerFixture(t)
	material, err := fixtureRecognizer(body)
	if err != nil {
		t.Fatal(err)
	}
	if material.root != "/zirlaku/u/synthetic_protocol_root_0123456789abcdef" || material.csrf != "synthetic_csrf_token_0123456789" {
		t.Fatal("recognized literals were not extracted exactly")
	}

	// Synthetic material must never be accepted by the exported constructor's
	// production verifier; its digest is pinned independently of this fixture.
	adapter := New(nil)
	if _, err := adapter.recognize(body); model.Category(err) != model.ProtocolChange {
		t.Fatal("production constructor accepted synthetic player fingerprint")
	}
	if _, err := fixtureRecognizer(append(append([]byte(nil), body...), ' ')); model.Category(err) != model.ProtocolChange {
		t.Fatal("modified bundle accepted under the original fingerprint")
	}
}

func TestRecognizedVersionRejectsMissingOrAmbiguousMaterial(t *testing.T) {
	original := string(playerFixture(t))
	for name, body := range map[string]string{
		"missing-root":   strings.ReplaceAll(original, "/zirlaku/u/", "/unmapped/"),
		"missing-csrf":   strings.ReplaceAll(original, "X-Csrf-Token", "X-Unmapped-Token"),
		"ambiguous-root": strings.Replace(original, "synthetic_protocol_root_0123456789abcdef", "different_protocol_root_0123456789abcdef", 1),
		"ambiguous-csrf": strings.Replace(original, "synthetic_csrf_token_0123456789", "different_csrf_token_0123456789", 1),
	} {
		t.Run(name, func(t *testing.T) {
			// Authenticate each altered synthetic fixture so the test reaches
			// extraction validation rather than stopping at the checksum gate.
			digest := sha256.Sum256([]byte(body))
			_, err := recognizePlayerVersion([]byte(body), hex.EncodeToString(digest[:]))
			var failure *model.Failure
			if !errors.As(err, &failure) || failure.Category != model.ProtocolChange || failure.Detail == "unrecognized static player version" {
				t.Fatalf("invalid recognized material was not rejected by extraction: %v", err)
			}
		})
	}
}

func TestResolveWithRecognizedFixture(t *testing.T) {
	for _, title := range []model.Title{
		{
			Type:   model.Movie,
			TMDBID: 27205,
			IMDbID: "tt1375666",
		},
		television(),
	} {
		t.Run(string(title.Type), func(t *testing.T) {
			body := playerFixture(t)
			root := origin + "/zirlaku/u/synthetic_protocol_root_0123456789abcdef"
			calls := 0
			bundleCalls := 0
			round := 0
			adapter := &Adapter{
				recognize: fixtureRecognizer,
				client: fixtureClient{fetch: func(ctx context.Context, request network.Request) (network.Response, error) {
					calls++
					if _, bounded := ctx.Deadline(); !bounded {
						t.Fatal("adapter lost its bounded request context")
					}
					targetURL, err := url.Parse(request.URL)
					if err != nil {
						t.Fatal(err)
					}
					response := network.Response{
						URL:    targetURL,
						Status: http.StatusOK,
					}
					switch {
					case request.URL == origin+embedPath(title):
						if calls != 1 || request.Method != "" || request.Limit != maxEmbedSize {
							t.Fatal("unexpected bootstrap request")
						}
						response.Body = fixture(t, string(title.Type))
					case request.URL == origin+"/_next/static/chunks/365-fixture.js":
						bundleCalls++
						if calls != 2 || request.Method != http.MethodGet || request.Limit != 3*1024*1024 {
							t.Fatal("player discovery route, order or budget changed")
						}
						response.Body = body
					case strings.HasPrefix(request.URL, root+"/JgQXcWPNH7s/"):
						if calls != 3-round {
							t.Fatal("challenge ran before recognized player discovery")
						}
						response.Body = []byte("synthetic-challenge")
					case strings.HasPrefix(request.URL, root+"/dBWKuO8aW6U/"):
						if calls != 4-round {
							t.Fatal("unexpected probe order")
						}
						response.Body = sealedFixture(t, []byte(`[{"name":"fixture","data":"synthetic_server_token"}]`))
					case request.URL == root+"/M6SyMXJBpeg/synthetic_server_token":
						response.Body = sealedFixture(t, sourceFixture(title))
					default:
						t.Fatal("unexpected nested request")
					}
					if strings.HasPrefix(request.URL, root+"/") {
						if request.Method != http.MethodPost || request.Context.Scopes[1].Headers["X-Csrf-Token"] != "synthetic_csrf_token_0123456789" {
							t.Fatal("extracted protocol material did not reach POST requests")
						}
					}
					return response, nil
				}},
			}
			result, err := adapter.Resolve(context.Background(), model.Lookup{Title: title})
			if err != nil || len(result.Candidates) != 1 || calls != 5 || result.Duration <= 0 {
				t.Fatalf("complete native adapter fixture failed: %v", err)
			}
			calls = 0
			round = 1
			repeated, repeatErr := adapter.Resolve(context.Background(), model.Lookup{Title: title})
			if repeatErr != nil || len(repeated.Candidates) != 1 || calls != 4 || bundleCalls != 1 {
				t.Fatalf("warm resolution did not reuse only static material: %v", repeatErr)
			}
			candidate := result.Candidates[0]
			if candidate.Title != title || candidate.Server != "fixture" || candidate.Format != model.HLS {
				t.Fatal("full resolution lost title, server or media format")
			}
			if len(candidate.Context.Scopes) != 0 || candidate.Context.PublicResources.Referer != origin+"/" {
				t.Fatal("protocol credentials escaped into the media context")
			}
		})
	}
}
