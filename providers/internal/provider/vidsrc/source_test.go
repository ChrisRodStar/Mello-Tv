package vidsrc

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

func TestPlainAlternativesAndUnknownMetadata(t *testing.T) {
	title := testTitle(model.Movie)
	client := &fixtureClient{
		testing: t,
		title:   title,
	}
	client.mutate = func(request network.Request, response *network.Response) error {
		if response.URL.Path == "/generate.php" {
			response.Status = 404
		}
		return nil
	}
	body := []byte(`{"status_code":"200","data":{"stream_urls":["https://cdn.example/master.m3u8","https://cdn.example/master.m3u8","http://127.0.0.1/private","https://cdn.example/alternate.mp4"]},"default_subs":[{"language":"unknown","file":"https://subs.example/caption.srt"}],"expires":"uncertain","protection":"unknown"}`)
	result, err := (&Adapter{client: client}).sources(context.Background(), body, title)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Candidates) != 2 || len(result.Issues) != 2 {
		t.Fatal("usable alternatives, duplicate or per-record issues lost")
	}
	if len(client.requests) != 1 {
		t.Fatal("host token failures retried within a lookup")
	}
	for _, candidate := range result.Candidates {
		if candidate.ExpiresAt != nil || candidate.Metadata.TracksKnown || len(candidate.Metadata.Tracks) != 0 || candidate.ContentVerified || len(candidate.Metadata.ProviderData["default_subs"]) == 0 || len(candidate.Metadata.ProviderData["protection"]) == 0 {
			t.Fatal("unknown upstream metadata invented or discarded")
		}
	}
	if result.Candidates[0].Format != model.HLS || result.Candidates[1].Format != model.MP4 {
		t.Fatal("source formats lost")
	}
}

func TestTokenShapes(t *testing.T) {
	for _, body := range []string{`fixture`, `"fixture"`, `{"token":"fixture"}`, `{"data":"fixture"}`, `{"string":"fixture"}`, `{"result":"fixture"}`} {
		token, err := parseToken([]byte(body))
		if err != nil || token != "fixture" {
			t.Fatal("observed token shape rejected")
		}
	}
	for _, body := range []string{`<html>error</html>`, `{"token":42}`, `[]`, "one\ntwo", strings.Repeat("x", 4097)} {
		if _, err := parseToken([]byte(body)); err == nil {
			t.Fatal("invalid token accepted")
		}
	}
}

func TestCoordinateAndEndpointGuards(t *testing.T) {
	title := testTitle(model.TV)
	season, episode := 0, 8
	title.Season = &season
	title.Episode = &episode
	if embedPath(title) != "/tv/60625/0/8" {
		t.Fatal("specials remapped")
	}
	body := []byte(`<iframe id="player_iframe" data-api="/vs_src.php?type=tv&amp;id=60625&amp;season=0&amp;episode=8"></iframe>`)
	if _, err := gateEndpoint(body, title); err != nil {
		t.Fatal(err)
	}
	for _, replacement := range []string{"season=1", "season=0&amp;season=1"} {
		if _, err := gateEndpoint([]byte(strings.ReplaceAll(string(body), "season=0", replacement)), title); err == nil {
			t.Fatal("incorrect or duplicate season accepted")
		}
	}
	config := playerConfig{
		MediaType:  "tv",
		IDType:     "tmdb",
		MediaID:    "60625",
		TMDB:       "60625",
		Season:     &season,
		Episode:    &episode,
		StreamBase: dataOrigin + "/api.php?type=tv&tmdb=60625",
		APIToken:   "fixture",
	}
	endpoint, err := sourceEndpoint(config, title)
	if err != nil {
		t.Fatal(err)
	}
	parsedURL, _ := url.Parse(endpoint)
	if !matchesCoordinates(parsedURL.Query(), title, "tmdb") {
		t.Fatal("special coordinates not preserved in API")
	}
	for _, badBase := range []string{dataOrigin + "/api.php?type=tv&tmdb=1", "https://unrelated.example/api.php?type=tv&tmdb=60625", dataOrigin + "/api.php?type=tv&tmdb=60625&episode=1"} {
		config.StreamBase = badBase
		if _, err := sourceEndpoint(config, title); err == nil {
			t.Fatal("changed API or title accepted")
		}
	}
	if _, err := playerEndpoint("https://vidsrc.link/embed/tv/60625/0/8?vs=fixture", "/embed/tv/60625/0/8"); err == nil {
		t.Fatal("similarly named site substituted")
	}
}

func TestConfigMustBeLiteralJSON(t *testing.T) {
	var config playerConfig
	for _, body := range []string{`window.CONFIG = getConfig();`, `window.CONFIG = {"api":"one"}; window.CONFIG = {"api":"two"};`, `window.CONFIG = {api: "unquoted"};`} {
		if err := parseConfig([]byte(body), "CONFIG", &config); err == nil {
			t.Fatal("ambiguous or executable config accepted")
		}
	}
}

func TestSourceCoordinateMismatch(t *testing.T) {
	for _, coordinates := range []string{`"season":"1","episode":"2",`, `"season":2,"episode":1,`, `"season":"1.0","episode":"1",`, ``} {
		body := []byte(`{"status_code":"200","data":{` + coordinates + `"stream_urls":["https://cdn.example/master.m3u8"]}}`)
		if _, err := (&Adapter{}).sources(context.Background(), body, testTitle(model.TV)); err == nil || model.Category(err) != model.ProtocolChange {
			t.Fatal("TV coordinates were not checked before networking")
		}
	}
}

func TestStaticModuleEndpointGuard(t *testing.T) {
	for _, endpoint := range []string{"https://127.0.0.1/wasm.php?w=42&_=1", dataOrigin + "/wasm.php?w=43&_=1", dataOrigin + "/other.php?w=42&_=1", "https://unrelated.example/wasm.php?w=42&_=1"} {
		protection, _ := json.Marshal(map[string]any{
			"w":        42,
			"wasm_url": endpoint,
		})
		if _, err := (&Adapter{}).decrypt(context.Background(), "unused", protection); err == nil {
			t.Fatal("unsafe or mismatched static endpoint accepted")
		}
	}
	for _, encoded := range []string{"!not-base64", base64.StdEncoding.EncodeToString(make([]byte, 12))} {
		if _, err := decryptStreams(encoded, fixture(t, "rotating-layout.wasm")); err == nil {
			t.Fatal("invalid encrypted framing accepted")
		}
	}
}

type slowTokenClient struct {
	mutex        sync.Mutex
	requests     []string
	cancelParent context.CancelFunc
}

func (client *slowTokenClient) Fetch(ctx context.Context, request network.Request) (network.Response, error) {
	parsedURL, _ := url.Parse(request.URL)
	client.mutex.Lock()
	client.requests = append(client.requests, parsedURL.Hostname())
	client.mutex.Unlock()
	if parsedURL.Hostname() == "fast.example" {
		return network.Response{
			URL:    parsedURL,
			Status: 200,
			Body:   []byte("fixture-token"),
		}, nil
	}
	if client.cancelParent != nil {
		client.cancelParent()
	}
	<-ctx.Done()
	return network.Response{}, ctx.Err()
}

func TestSlowOptionalHostPreservesEarlierSources(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 600*time.Millisecond)
	defer cancel()
	client := &slowTokenClient{}
	body := []byte(`{"status_code":"200","data":{"stream_urls":["https://fast.example/master.m3u8","https://slow.example/master.m3u8","https://unvisited.example/master.m3u8"]}}`)
	result, err := (&Adapter{client: client}).sources(ctx, body, testTitle(model.Movie))
	if err != nil || ctx.Err() != nil {
		t.Fatalf("optional host consumed parent deadline and erased earlier sources: %v", err)
	}
	if len(result.Candidates) != 3 || !strings.HasSuffix(result.Candidates[0].URL, "?token=fixture-token") {
		t.Fatal("earlier tokenized source or untokened alternatives lost")
	}
	if len(client.requests) != 3 || len(result.Issues) != 2 {
		t.Fatal("parallel enrichment lost requests or optional failures")
	}
}

func TestOptionalTokenStillHonorsParentCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	client := &slowTokenClient{cancelParent: cancel}
	body := []byte(`{"status_code":"200","data":{"stream_urls":["https://fast.example/master.m3u8","https://slow.example/master.m3u8"]}}`)
	_, err := (&Adapter{client: client}).sources(ctx, body, testTitle(model.Movie))
	if !errors.Is(err, context.Canceled) {
		t.Fatal("caller cancellation treated as optional host failure")
	}
}

func TestDecryptorReuseKeepsExactWindowAndURLIdentity(t *testing.T) {
	client := &fixtureClient{testing: t, title: testTitle(model.Movie)}
	adapter := &Adapter{client: client}
	var envelope struct {
		Data struct {
			Streams string `json:"stream_urls"`
		} `json:"data"`
		Protection json.RawMessage `json:"vs"`
	}
	if err := json.Unmarshal(fixture(t, "encrypted.json"), &envelope); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		streams, err := adapter.decrypt(context.Background(), envelope.Data.Streams, envelope.Protection)
		if err != nil || len(streams) != 2 {
			t.Fatalf("cached decryptor changed streams: %v", err)
		}
	}
	if len(client.requests) != 1 {
		t.Fatal("identical static decryptor fetched twice")
	}
	var protection map[string]any
	if err := json.Unmarshal(envelope.Protection, &protection); err != nil {
		t.Fatal(err)
	}
	endpoint := protection["wasm_url"].(string)
	parsedURL, _ := url.Parse(endpoint)
	query := parsedURL.Query()
	query.Set("_", "different-version")
	parsedURL.RawQuery = query.Encode()
	protection["wasm_url"] = parsedURL.String()
	changed, _ := json.Marshal(protection)
	if _, err := adapter.decrypt(context.Background(), envelope.Data.Streams, changed); err != nil {
		t.Fatal(err)
	}
	if len(client.requests) != 2 {
		t.Fatal("different module identity reused a cached key")
	}
	if _, err := adapter.decrypt(context.Background(), "invalid", envelope.Protection); model.Category(err) != model.ProtocolChange {
		t.Fatal("cached decryptor bypassed ciphertext validation")
	}
	protection["w"] = 43
	mismatched, _ := json.Marshal(protection)
	if _, err := adapter.decrypt(context.Background(), envelope.Data.Streams, mismatched); model.Category(err) != model.ProtocolChange {
		t.Fatal("cached decryptor bypassed window validation")
	}
}

type parallelTokenClient struct {
	started chan struct{}
	release chan struct{}
}

func (client parallelTokenClient) Fetch(ctx context.Context, request network.Request) (network.Response, error) {
	select {
	case client.started <- struct{}{}:
	case <-ctx.Done():
		return network.Response{}, ctx.Err()
	}
	select {
	case <-client.release:
	case <-ctx.Done():
		return network.Response{}, ctx.Err()
	}
	parsedURL, _ := url.Parse(request.URL)
	return network.Response{URL: parsedURL, Status: 200, Body: []byte("fixture-token")}, nil
}

func TestOptionalTokensStartConcurrentlyAndDeduplicateOrigins(t *testing.T) {
	client := parallelTokenClient{started: make(chan struct{}), release: make(chan struct{})}
	adapter := &Adapter{client: client}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	done := make(chan map[string]hostToken, 1)
	go func() {
		done <- adapter.hostTokens(ctx, []string{
			"https://one.example/a.m3u8", "https://one.example/b.m3u8",
			"https://two.example/a.m3u8", "https://three.example/a.m3u8",
		})
	}()
	for range 3 {
		select {
		case <-client.started:
		case <-ctx.Done():
			<-done
			t.Fatal("independent host tokens serialized")
		}
	}
	close(client.release)
	outcomes := <-done
	if len(outcomes) != 3 {
		t.Fatal("token requests were not deduplicated by origin")
	}
	for _, outcome := range outcomes {
		if outcome.err != nil || outcome.token != "fixture-token" {
			t.Fatal("parallel token result lost")
		}
	}
}
