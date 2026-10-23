package vidsrc

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"reflect"
	"strings"
	"testing"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
)

type fixtureClient struct {
	testing  *testing.T
	title    model.Title
	requests []network.Request
	mutate   func(network.Request, *network.Response) error
	inline   bool
}

func (c *fixtureClient) Fetch(ctx context.Context, request network.Request) (network.Response, error) {
	c.testing.Helper()
	if err := ctx.Err(); err != nil {
		return network.Response{}, err
	}
	c.requests = append(c.requests, request)
	parsedURL, err := url.Parse(request.URL)
	if err != nil {
		c.testing.Fatal(err)
	}
	if request.Limit <= 0 || request.Limit > 256*1024 || request.Jar != nil {
		c.testing.Fatal("unbounded request or unexpected cookies")
	}
	if len(request.Context.Scopes) != 1 || request.Context.Scopes[0].Headers["Referer"] == "" {
		c.testing.Fatal("missing scoped request identity")
	}
	response := network.Response{
		URL:    parsedURL,
		Status: http.StatusOK,
		Header: make(http.Header),
	}
	kind := string(c.title.Type)
	switch parsedURL.Path {
	case "/embed" + embedPath(c.title):
		if sameOrigin(parsedURL, origin) {
			response.Body = fixture(c.testing, kind+"-embed.html")
		} else if sameOrigin(parsedURL, playerOrigin) {
			response.Body = fixture(c.testing, kind+"-landing.html")
		} else {
			c.testing.Fatal("unrecognized embed origin")
		}
	case "/vs_src.php":
		if !matchesCoordinates(parsedURL.Query(), c.title, "id") {
			c.testing.Fatal("gate coordinates not preserved")
		}
		response.Body, _ = json.Marshal(map[string]string{"src": playerOrigin + "/embed" + embedPath(c.title) + "?vs=fixture-gate"})
	case "/embed/player" + embedPath(c.title):
		response.Body = fixture(c.testing, kind+"-player.html")
	case "/api.php":
		query := parsedURL.Query()
		if !matchesCoordinates(query, c.title, "tmdb") || query.Get("api_token") != "fixture-api-token" || len(query["stream_urls"]) != 1 {
			c.testing.Fatal("stream coordinates or token not preserved")
		}
		var payload map[string]json.RawMessage
		if err := json.Unmarshal(fixture(c.testing, "encrypted.json"), &payload); err != nil {
			c.testing.Fatal(err)
		}
		var data map[string]json.RawMessage
		if err := json.Unmarshal(payload["data"], &data); err != nil {
			c.testing.Fatal(err)
		}
		if c.title.Type == model.TV {
			data["imdb_id"] = json.RawMessage(`"tt2861424"`)
			data["season"] = json.RawMessage(`"1"`)
			data["episode"] = json.RawMessage(`"1"`)
		}
		payload["data"], _ = json.Marshal(data)
		if c.inline {
			payload["vs"], _ = json.Marshal(map[string]any{
				"w":    42,
				"wasm": base64.StdEncoding.EncodeToString(fixture(c.testing, "rotating-layout.wasm")),
			})
		}
		response.Body, _ = json.Marshal(payload)
	case "/wasm.php":
		response.Body = fixture(c.testing, "rotating-layout.wasm")
	case "/generate.php":
		response.Body = []byte(`{"token":"fixture-host-token"}`)
	default:
		c.testing.Fatal("unexpected fixture request path")
	}
	if c.mutate != nil {
		if err := c.mutate(request, &response); err != nil {
			return network.Response{}, err
		}
	}
	return response, nil
}

func testTitle(kind model.MediaType) model.Title {
	title := model.Title{
		Type:   kind,
		TMDBID: 27205,
		IMDbID: "tt1375666",
	}
	if kind == model.TV {
		season, episode := 1, 1
		malID := 21
		crossID := int64(malID)
		title = model.Title{
			Type:          kind,
			TMDBID:        60625,
			IMDbID:        "tt2861424",
			MyAnimeListID: &crossID,
			Season:        &season,
			Episode:       &episode,
		}
	}
	return title
}

func TestCompleteMovieTVProtocol(t *testing.T) {
	for _, kind := range []model.MediaType{model.Movie, model.TV} {
		for _, inline := range []bool{false, true} {
			t.Run(string(kind)+map[bool]string{
				false: "/module-url",
				true:  "/inline",
			}[inline], func(t *testing.T) {
				title := testTitle(kind)
				client := &fixtureClient{
					testing: t,
					title:   title,
					inline:  inline,
				}
				adapter := &Adapter{client: client}
				result, err := adapter.Resolve(context.Background(), model.Lookup{Title: title})
				if err != nil {
					t.Fatal(err)
				}
				if len(result.Candidates) != 2 || len(result.Issues) != 0 || result.Duration <= 0 {
					t.Fatal("complete fixture did not return alternatives")
				}
				for index, candidate := range result.Candidates {
					if candidate.Provider != model.VidSrc || !reflect.DeepEqual(candidate.Title, title) || candidate.Format != model.HLS || candidate.ExpiresAt != nil || candidate.ContentVerified || candidate.Metadata.TracksKnown || len(candidate.Metadata.Tracks) != 0 || candidate.SelectedVariantURL != "" {
						t.Fatal("title/unknown metadata or playable master changed")
					}
					if !strings.HasSuffix(candidate.URL, "?token=fixture-host-token") || candidate.Context.PublicResources.Referer != playerOrigin+"/" || len(candidate.Context.Scopes) != 0 {
						t.Fatal("public media context/token not preserved")
					}
					if index > 0 && candidate.ID == result.Candidates[0].ID {
						t.Fatal("alternatives lost independent identities")
					}
				}
				tokenRequests := 0
				for _, request := range client.requests {
					if strings.Contains(request.URL, "/generate.php") {
						tokenRequests++
					}
				}
				if tokenRequests != 1 {
					t.Fatal("host token not reused within lookup")
				}
			})
		}
	}
}

func TestProtocolFailuresStopAtBoundary(t *testing.T) {
	cases := []struct {
		name     string
		path     string
		body     string
		status   int
		redirect bool
		category model.ErrorCategory
	}{
		{
			name:     "not found",
			path:     "/vs_src.php",
			status:   404,
			category: model.Unavailable,
		},
		{
			name:     "challenge",
			path:     "/api.php",
			status:   403,
			category: model.UpstreamFailure,
		},
		{
			name:     "changed gate host",
			path:     "/vs_src.php",
			body:     `{"src":"https://unrelated.example/embed/movie/27205?vs=fixture"}`,
			category: model.ProtocolChange,
		},
		{
			name:     "private gate",
			path:     "/vs_src.php",
			body:     `{"src":"https://127.0.0.1/embed/movie/27205?vs=fixture"}`,
			category: model.ProtocolChange,
		},
		{
			name:     "wrong episode iframe",
			path:     "/vs_src.php",
			body:     `{"src":"https://stellarconductornexus.com/embed/movie/42?vs=fixture"}`,
			category: model.ProtocolChange,
		},
		{
			name:     "redirect",
			path:     "/vs_src.php",
			redirect: true,
			category: model.ProtocolChange,
		},
		{
			name:     "changed cipher",
			path:     "/wasm.php",
			body:     "unknown module",
			category: model.ProtocolChange,
		},
		{
			name:     "API verification rejection",
			path:     "/api.php",
			body:     `{"status_code":"403","message":"verification required"}`,
			category: model.UpstreamFailure,
		},
		{
			name:     "missing API status",
			path:     "/api.php",
			body:     `{"data":{"stream_urls":[]}}`,
			category: model.ProtocolChange,
		},
		{
			name:     "changed API",
			path:     "/api.php",
			body:     `{"status_code":"200","data":{"stream_urls":{}}}`,
			category: model.ProtocolChange,
		},
		{
			name:     "no streams",
			path:     "/api.php",
			body:     `{"status_code":"200","data":{"stream_urls":[]}}`,
			category: model.Unavailable,
		},
		{
			name:     "wrong cross-ID",
			path:     "/api.php",
			body:     `{"status_code":"200","data":{"imdb_id":"tt0000001","stream_urls":[]}}`,
			category: model.ProtocolChange,
		},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			title := testTitle(model.Movie)
			client := &fixtureClient{
				testing: t,
				title:   title,
			}
			client.mutate = func(request network.Request, response *network.Response) error {
				if response.URL.Path == testCase.path {
					if testCase.body != "" {
						response.Body = []byte(testCase.body)
					}
					if testCase.status != 0 {
						response.Status = testCase.status
					}
					if testCase.redirect {
						response.URL, _ = url.Parse("https://other.example/")
					}
				}
				return nil
			}
			result, err := (&Adapter{client: client}).Resolve(context.Background(), model.Lookup{Title: title})
			if err == nil || model.Category(err) != testCase.category || len(result.Candidates) != 0 {
				t.Fatalf("incorrect categorized failure: %v", err)
			}
			if strings.Contains(err.Error(), "fixture-api-token") || strings.Contains(err.Error(), "https://") {
				t.Fatal("private request leaked")
			}
		})
	}
}

func TestCancellationAndSanitizedCauses(t *testing.T) {
	title := testTitle(model.Movie)
	ctx, cancel := context.WithCancel(context.Background())
	client := &fixtureClient{
		testing: t,
		title:   title,
	}
	client.mutate = func(request network.Request, response *network.Response) error { cancel(); return context.Canceled }
	_, err := (&Adapter{client: client}).Resolve(ctx, model.Lookup{Title: title})
	if !errors.Is(err, context.Canceled) || model.Category(err) != model.Cancelled {
		t.Fatal("cancellation cause lost")
	}
	secretCause := errors.New("https://private.example/?token=must-not-leak")
	client.mutate = func(request network.Request, response *network.Response) error { return secretCause }
	_, err = (&Adapter{client: client}).Resolve(context.Background(), model.Lookup{Title: title})
	if !errors.Is(err, secretCause) || strings.Contains(err.Error(), "must-not-leak") {
		t.Fatal("transport cause leaked or lost")
	}
}

func TestInputValidationBeforeNetwork(t *testing.T) {
	adapter := New(nil)
	if adapter.ID() != model.VidSrc || !reflect.DeepEqual(adapter.Capabilities().Identifiers, []string{"tmdb"}) {
		t.Fatal("incorrect adapter identity")
	}
	_, err := adapter.Resolve(context.Background(), model.Lookup{Title: testTitle(model.Movie)})
	if model.Category(err) != model.Unavailable {
		t.Fatal("unguarded client accepted")
	}
	_, err = adapter.Resolve(context.Background(), model.Lookup{})
	if model.Category(err) != model.Unsupported {
		t.Fatal("invalid lookup accepted")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err = adapter.Resolve(ctx, model.Lookup{Title: testTitle(model.Movie)})
	if !errors.Is(err, context.Canceled) {
		t.Fatal("pre-cancellation ignored")
	}
}
