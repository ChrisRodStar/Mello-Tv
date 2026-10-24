package vidlink

import (
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"os"
	"slices"
	"strings"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/provider"
	"golang.org/x/crypto/nacl/secretbox"
)

type fixtureFetcher func(context.Context, network.Request) (network.Response, error)

func (fetch fixtureFetcher) Fetch(ctx context.Context, request network.Request) (network.Response, error) {
	return fetch(ctx, request)
}

func TestTokenMessageAndPublicAlgorithm(t *testing.T) {
	now := time.Unix(1700000000, 0)
	token, err := generateToken(60625, now)
	if err != nil {
		t.Fatal(err)
	}
	envelope, err := base64.RawURLEncoding.DecodeString(token)
	if err != nil || len(envelope) != 24+16+5+8 {
		t.Fatal("unexpected token envelope")
	}
	var nonce [24]byte
	if string(envelope[:24]) != string(nonce[:]) {
		t.Fatal("player nonce mismatch")
	}
	keyBytes, err := hex.DecodeString(publicTokenKeyHex)
	if err != nil {
		t.Fatal(err)
	}
	var key [32]byte
	copy(key[:], keyBytes)
	message, authenticated := secretbox.Open(nil, envelope[24:], &nonce, &key)
	if !authenticated || string(message[:5]) != "60625" || binary.BigEndian.Uint64(message[5:]) != 1700000120 {
		t.Fatal("token did not preserve the traced ID and big-endian two-minute expiry")
	}
}

func TestContextAndInputFailures(t *testing.T) {
	var nilCtx context.Context
	if _, err := New(nil).Resolve(nilCtx, model.Lookup{}); err == nil {
		t.Fatal("nil context accepted")
	}

	cancelledContext, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := New(nil).Resolve(cancelledContext, model.Lookup{})
	if !errors.Is(err, context.Canceled) {
		t.Fatal("cancellation cause lost")
	}
	expiredContext, stop := context.WithDeadline(context.Background(), time.Unix(0, 0))
	defer stop()
	_, err = New(nil).Resolve(expiredContext, model.Lookup{})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("deadline cause lost")
	}
	_, err = New(nil).Resolve(context.Background(), model.Lookup{})
	var failure *model.Failure
	if !errors.As(err, &failure) || failure.Stage != "input" || failure.Category != model.Unsupported {
		t.Fatal("invalid input accepted")
	}
	_, err = New(nil).Resolve(context.Background(), model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 27205}})
	if model.Category(err) != model.Unavailable {
		t.Fatal("nil controlled client accepted")
	}
}

func TestUnknownPlayerStopsBeforeSourceRequest(t *testing.T) {
	calls := 0
	adapter := &Adapter{client: fixtureFetcher(func(_ context.Context, request network.Request) (network.Response, error) {
		calls++
		parsedURL, err := url.Parse(request.URL)
		if err != nil {
			t.Fatal(err)
		}
		return network.Response{URL: parsedURL, Status: 200, Body: []byte("synthetic changed player")}, nil
	})}
	result, err := adapter.Resolve(context.Background(), model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 27205}})
	if model.Category(err) != model.ProtocolChange || calls != 2 || len(result.Candidates) != 0 {
		t.Fatal("unknown player allowed token/source requests")
	}
}

func TestTransportCauseIsPreservedWithoutSignedDiagnostics(t *testing.T) {
	privateCause := errors.New("https://example.com/?token=private-fixture")
	adapter := &Adapter{client: fixtureFetcher(func(context.Context, network.Request) (network.Response, error) {
		return network.Response{}, privateCause
	})}
	_, err := adapter.Resolve(context.Background(), model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 27205}})
	if !errors.Is(err, privateCause) || strings.Contains(err.Error(), "private-fixture") || model.Category(err) != model.UpstreamFailure {
		t.Fatal("transport cause lost or sensitive text exposed")
	}
}

func TestNormalizeCapturedShapeAndUnknowns(t *testing.T) {
	body, err := os.ReadFile("../../../testdata/providers/vidlink/sources.json")
	if err != nil {
		t.Fatal(err)
	}
	season, episode := 1, 1
	title := model.Title{Type: model.TV, TMDBID: 60625, Season: &season, Episode: &episode}
	result, err := normalize(body, title)
	if err != nil || len(result.Candidates) != 2 {
		t.Fatalf("captured-shape normalization: %v", err)
	}
	for _, candidate := range result.Candidates {
		if candidate.Provider != model.VidLink || candidate.Format != model.MP4 || !candidate.Title.SameContent(title) {
			t.Fatal("candidate identity changed")
		}
		if candidate.ExpiresAt != nil || candidate.ContentVerified || candidate.Metadata.TracksKnown || len(candidate.Metadata.Tracks) > 0 || len(candidate.Metadata.Variants[0].Codecs) > 0 {
			t.Fatal("unknown expiration, language or sample-entry codec was invented")
		}
		if len(candidate.Metadata.ProviderData["captions"]) == 0 {
			t.Fatal("SRT caption metadata discarded")
		}
	}
	wrongEpisode := 2
	title.Episode = &wrongEpisode
	if _, err := normalize(body, title); model.Category(err) != model.ProtocolChange {
		t.Fatal("wrong episode accepted")
	}
	for _, malformed := range []string{`{`, `{}`, `{"sourceId":"fixture","stream":{"qualities":{"720":{"type":"mp4","url":"http://127.0.0.1/video.mp4"}}}}`} {
		if _, err := normalize([]byte(malformed), title); model.Category(err) != model.ProtocolChange {
			t.Fatal("malformed/unsafe source accepted")
		}
	}
	if _, err := normalize([]byte(`{"sourceId":"fixture","stream":null}`), title); model.Category(err) != model.Unavailable {
		t.Fatal("missing source miscategorized")
	}
}

func TestSourceCredentialsStayOnResourcePath(t *testing.T) {
	resourceURL, err := url.Parse("https://media.example.com/session/master.m3u8")
	if err != nil {
		t.Fatal(err)
	}
	fetchContext, err := sourceContext(resourceURL, map[string]string{"Cookie": "fixture=value", "Authorization": "fixture"})
	if err != nil {
		t.Fatal(err)
	}
	if fetchContext.PublicResources != nil || len(fetchContext.Scopes) != 1 || fetchContext.Scopes[0].Origin != network.Origin(resourceURL) || fetchContext.Scopes[0].PathPrefix != "/session/master.m3u8" || fetchContext.Scopes[0].CookiePath != "/session/master.m3u8" {
		t.Fatal("credential scope broadened")
	}
}

func TestRegistryAndTracedCapabilities(t *testing.T) {
	adapter := New(nil)
	capabilities := adapter.Capabilities()
	if len(capabilities.MediaTypes) != 2 || len(capabilities.Identifiers) != 1 || capabilities.Identifiers[0] != "tmdb" {
		t.Fatal("incorrect traced capabilities")
	}
	registry, err := provider.NewRegistry(adapter)
	if err != nil {
		t.Fatal(err)
	}
	expectedProviders := []model.ProviderID{
		model.VidCore,
		model.VidLink,
		model.VidSrc,
		model.VidFast,
	}
	var registeredProviders []model.ProviderID
	for _, registered := range registry.Adapters() {
		registeredProviders = append(registeredProviders, registered.ID())
	}
	if !slices.Equal(registeredProviders, expectedProviders) {
		t.Fatal("registry does not contain exactly the four planned providers")
	}
}

func TestCapturedPlayerOfflineReplay(t *testing.T) {
	fixturePath := os.Getenv("VIDLINK_PLAYER_FIXTURE")
	if fixturePath == "" {
		t.Skip("set VIDLINK_PLAYER_FIXTURE to a private captured fu.wasm; bytes are inspected, never executed")
	}
	player, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Fatal("read private player fixture")
	}
	payload, err := os.ReadFile("../../../testdata/providers/vidlink/sources.json")
	if err != nil {
		t.Fatal(err)
	}
	requests := 0
	adapter := &Adapter{client: fixtureFetcher(func(_ context.Context, request network.Request) (network.Response, error) {
		requests++
		parsedURL, err := url.Parse(request.URL)
		if err != nil {
			t.Fatal("invalid protocol request URL")
		}
		body := []byte("<html>synthetic embed</html>")
		switch requests {
		case 1:
			if parsedURL.Path != "/tv/60625/1/1" {
				t.Fatal("episode mapping changed")
			}
		case 2:
			if parsedURL.Path != "/fu.wasm" {
				t.Fatal("static player discovery changed")
			}
			body = player
		case 3:
			if !strings.HasPrefix(parsedURL.Path, "/api/b/tv/") || !strings.HasSuffix(parsedURL.Path, "/1/1") || parsedURL.Query().Get("multiLang") != "1" || request.Jar == nil {
				t.Fatal("native source request mapping changed")
			}
			body = payload
		default:
			t.Fatal("unexpected protocol request")
		}
		return network.Response{URL: parsedURL, Status: 200, Body: body}, nil
	})}
	season, episode := 1, 1
	result, err := adapter.Resolve(context.Background(), model.Lookup{
		Title: model.Title{Type: model.TV, TMDBID: 60625, Season: &season, Episode: &episode},
	})
	if err != nil || len(result.Candidates) != 2 || requests != 3 {
		t.Fatalf("offline protocol replay did not resolve: %v", err)
	}
}

func TestRequestedIdentifierFixtures(t *testing.T) {
	body, err := os.ReadFile("../../../testdata/providers/vidlink/lookups.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixtures []struct {
		Name   string       `json:"name"`
		Lookup model.Lookup `json:"lookup"`
	}
	if err := json.Unmarshal(body, &fixtures); err != nil {
		t.Fatal(err)
	}
	if len(fixtures) != 4 {
		t.Fatal("missing movie/TV/anime preference fixtures")
	}
	for _, fixture := range fixtures {
		t.Run(fixture.Name, func(t *testing.T) {
			if err := fixture.Lookup.Title.Validate(); err != nil {
				t.Fatal(err)
			}
			path := embedPath(fixture.Lookup.Title)
			if !strings.HasPrefix(path, "/movie/") && !strings.HasSuffix(path, "/1/1") {
				t.Fatal("unexpected title or episode mapping")
			}
		})
	}
}

func TestCancellationDuringFetchExitsWithCause(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	started := make(chan struct{})
	finished := make(chan struct{})
	adapter := &Adapter{client: fixtureFetcher(func(fetchContext context.Context, _ network.Request) (network.Response, error) {
		close(started)
		<-fetchContext.Done()
		return network.Response{}, fetchContext.Err()
	})}
	var resolveErr error
	go func() {
		defer close(finished)
		_, resolveErr = adapter.Resolve(ctx, model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 27205}})
	}()
	defer func() {
		cancel()
		<-finished
	}()
	select {
	case <-started:
	case <-ctx.Done():
		t.Fatal("fetch did not start")
	}
	cancel()
	<-finished
	if !errors.Is(resolveErr, context.Canceled) || model.Category(resolveErr) != model.Cancelled {
		t.Fatal("mid-request cancellation cause lost")
	}
}

func TestHTTPFailuresAreCategorized(t *testing.T) {
	for _, status := range []int{404, 410, 403, 429, 503} {
		adapter := &Adapter{client: fixtureFetcher(func(_ context.Context, request network.Request) (network.Response, error) {
			parsedURL, err := url.Parse(request.URL)
			if err != nil {
				t.Fatal(err)
			}
			return network.Response{URL: parsedURL, Status: status}, nil
		})}
		_, err := adapter.Resolve(context.Background(), model.Lookup{Title: model.Title{Type: model.Movie, TMDBID: 27205}})
		expected := model.UpstreamFailure
		if status == 404 || status == 410 {
			expected = model.Unavailable
		}
		if model.Category(err) != expected {
			t.Fatal("HTTP rejection miscategorized")
		}
	}
}

func TestExplicitPublicIdentityDoesNotBroadenCredentials(t *testing.T) {
	resourceURL, err := url.Parse("https://media.example.com/private/master.m3u8")
	if err != nil {
		t.Fatal(err)
	}
	fetchContext, err := sourceContext(resourceURL, map[string]string{
		"Referer":       "https://player.example.com/",
		"Origin":        "https://player.example.com",
		"Authorization": "fixture",
	})
	if err != nil || fetchContext.PublicResources == nil || len(fetchContext.Scopes) != 1 || fetchContext.Scopes[0].Headers["Authorization"] != "fixture" {
		t.Fatal("public identification and scoped credentials were not separated")
	}
	if _, err := sourceContext(resourceURL, map[string]string{"Cookie": "bad\r\nheader"}); err == nil {
		t.Fatal("header injection accepted")
	}
}

func TestValidatedPlayerReuseKeepsFreshSessionsAndSources(t *testing.T) {
	payload, err := os.ReadFile("../../../testdata/providers/vidlink/sources.json")
	if err != nil {
		t.Fatal(err)
	}
	var jars []http.CookieJar
	sourceCalls := 0
	adapter := &Adapter{client: fixtureFetcher(func(_ context.Context, request network.Request) (network.Response, error) {
		parsedURL, _ := url.Parse(request.URL)
		response := network.Response{URL: parsedURL, Status: 200}
		if strings.HasPrefix(parsedURL.Path, "/tv/") {
			jars = append(jars, request.Jar)
			response.Body = []byte("fresh embed")
		} else if strings.HasPrefix(parsedURL.Path, "/api/") {
			sourceCalls++
			response.Body = payload
		} else {
			t.Fatal("validated player downloaded again")
		}
		return response, nil
	})}
	adapter.players.Put(origin+"/fu.wasm", true, time.Minute, 1)
	season, episode := 1, 1
	lookup := model.Lookup{Title: model.Title{Type: model.TV, TMDBID: 60625, Season: &season, Episode: &episode}}
	for range 2 {
		result, err := adapter.Resolve(context.Background(), lookup)
		if err != nil || len(result.Candidates) != 2 {
			t.Fatalf("cached algorithm changed sources: %v", err)
		}
	}
	if sourceCalls != 2 || len(jars) != 2 || jars[0] == nil || jars[0] == jars[1] {
		t.Fatal("static reuse retained session or source response")
	}
}
