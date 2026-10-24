package vidfast

import (
	"context"
	"errors"
	"net/http"
	"net/url"
	"os"
	"strings"
	"testing"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

type fixtureClient struct {
	fetch func(context.Context, network.Request) (network.Response, error)
}

func (c fixtureClient) Fetch(ctx context.Context, request network.Request) (network.Response, error) {
	return c.fetch(ctx, request)
}

func fixture(t *testing.T, name string) []byte {
	t.Helper()
	body, err := os.ReadFile("../../../testdata/providers/vidfast/" + name + ".html")
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func television() model.Title {
	season, episode := 0, 7
	animeID := int64(123)
	return model.Title{
		Type:          model.TV,
		TMDBID:        60625,
		IMDbID:        "tt2861424",
		MyAnimeListID: &animeID,
		Season:        &season,
		Episode:       &episode,
	}
}

func TestBootstrapCoordinatesAndCrossIDs(t *testing.T) {
	for _, title := range []model.Title{
		{
			Type:   model.Movie,
			TMDBID: 27205,
			IMDbID: "tt1375666",
		},
		television(),
	} {
		t.Run(string(title.Type), func(t *testing.T) {
			parsed, err := parseBootstrap(fixture(t, string(title.Type)), title)
			if err != nil {
				t.Fatal(err)
			}
			if parsed.Title != title || parsed.Token != "synthetic_bootstrap_token_only" {
				t.Fatalf("bootstrap changed identifiers or token")
			}
		})
	}
}

func TestBootstrapRejectsChangedProtocol(t *testing.T) {
	original := string(fixture(t, "tv"))
	for name, body := range map[string]string{
		"missing":       "<script>runPlayer()</script>",
		"duplicate":     original + original,
		"host":          strings.ReplaceAll(original, "vidfast.vc", "vidfast.pro"),
		"id":            strings.ReplaceAll(original, "60625", "60626"),
		"season":        strings.ReplaceAll(original, `season\":\"0`, `season\":\"1`),
		"episode":       strings.ReplaceAll(original, `episode\":\"7`, `episode\":\"8`),
		"numeric-id":    strings.ReplaceAll(original, `id\":\"60625\"`, `id\":60625`),
		"bad-token":     strings.ReplaceAll(original, "synthetic_bootstrap_token_only", "https://source.invalid/master.m3u8"),
		"empty-token":   strings.ReplaceAll(original, "synthetic_bootstrap_token_only", ""),
		"embed-budget":  strings.Repeat("x", maxEmbedSize+1),
		"flight-budget": `<script>self.__next_f.push([1,"` + strings.Repeat("x", maxFlightSize+1) + `"])</script>`,
	} {
		t.Run(name, func(t *testing.T) {
			_, err := parseBootstrap([]byte(body), television())
			if model.Category(err) != model.ProtocolChange {
				t.Fatalf("expected protocol change, got %v", err)
			}
		})
	}
}

func TestAdapterRejectsUnrecognizedPlayer(t *testing.T) {
	for _, title := range []model.Title{
		{
			Type:   model.Movie,
			TMDBID: 27205,
		},
		television(),
	} {
		t.Run(string(title.Type), func(t *testing.T) {
			calls := 0
			adapter := &Adapter{client: fixtureClient{fetch: func(ctx context.Context, request network.Request) (network.Response, error) {
				calls++
				if calls == 1 && (request.URL != origin+embedPath(title) || request.Limit != maxEmbedSize || request.Method != "") {
					t.Fatalf("unexpected bootstrap request")
				}
				if _, hasDeadline := ctx.Deadline(); !hasDeadline {
					t.Fatal("missing adapter deadline")
				}
				targetURL, _ := url.Parse(request.URL)
				return network.Response{
					URL:    targetURL,
					Status: http.StatusOK,
					Body:   fixture(t, string(title.Type)),
				}, nil
			}}}
			result, err := adapter.Resolve(context.Background(), model.Lookup{Title: title})
			var failure *model.Failure
			if !errors.As(err, &failure) || failure.Stage != "bundle" || len(result.Candidates) != 0 || calls != 2 || result.Duration <= 0 {
				t.Fatalf("unverified bootstrap must not become a candidate: %v", err)
			}
		})
	}
}

func TestAdapterInputAndCancellation(t *testing.T) {
	adapter := New(nil)
	var nilCtx context.Context
	if _, err := adapter.Resolve(nilCtx, model.Lookup{}); err == nil {
		t.Fatal("nil context accepted")
	}
	_, err := adapter.Resolve(context.Background(), model.Lookup{Title: television()})
	if model.Category(err) != model.Unavailable {
		t.Fatal(err)
	}
	_, err = adapter.Resolve(context.Background(), model.Lookup{})
	if model.Category(err) != model.Unsupported {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err = adapter.Resolve(ctx, model.Lookup{Title: television()})
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	adapter.client = fixtureClient{fetch: func(context.Context, network.Request) (network.Response, error) {
		return network.Response{}, errors.Join(context.DeadlineExceeded, errors.New("https://private.invalid/?secret=private"))
	}}
	_, err = adapter.Resolve(context.Background(), model.Lookup{Title: television()})
	if !errors.Is(err, context.DeadlineExceeded) || strings.Contains(err.Error(), "secret") {
		t.Fatalf("lost cancellation or exposed private request: %v", err)
	}
}

func TestAdapterHTTPAndRedirects(t *testing.T) {
	for _, status := range []int{
		http.StatusNotFound,
		http.StatusGone,
		http.StatusForbidden,
		http.StatusTooManyRequests,
		http.StatusOK,
	} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			adapter := &Adapter{client: fixtureClient{fetch: func(context.Context, network.Request) (network.Response, error) {
				// Controlled transport may follow a valid redirect; changed endpoints
				// must still not be interpreted as the mapped player protocol.
				targetURL, _ := url.Parse(origin + "/unexpected")
				if status != http.StatusOK {
					targetURL, _ = url.Parse(origin + embedPath(television()))
				}
				return network.Response{
					URL:    targetURL,
					Status: status,
				}, nil
			}}}
			_, err := adapter.Resolve(context.Background(), model.Lookup{Title: television()})
			var expected model.ErrorCategory
			switch status {
			case http.StatusNotFound, http.StatusGone:
				expected = model.Unavailable
			case http.StatusOK:
				expected = model.ProtocolChange
			default:
				expected = model.UpstreamFailure
			}
			if model.Category(err) != expected {
				t.Fatal(err)
			}
		})
	}
}
