package vidsrc

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"html"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"

	"stv-providers/internal/cache"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

const (
	origin       = "https://vidsrc.sh"
	playerOrigin = "https://stellarconductornexus.com"
	dataOrigin   = "https://data.vidsrc.sh"
	userAgent    = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36"
)

var (
	iframePattern    = regexp.MustCompile(`(?is)<iframe\b[^>]*>`)
	attributePattern = regexp.MustCompile(`(?i)\b(id|data-api)\s*=\s*["']([^"']*)["']`)
	configPattern    = regexp.MustCompile(`(?s)window\.(CFG|CONFIG)\s*=\s*(\{[^<]*?\})\s*;`)
)

func protocolError(stage, detail string) error {
	return model.Fail(model.ProtocolChange, stage, detail)
}

// requestError keeps private transport causes for cancellation and errors.Is,
// but formats only sanitized text, never signed URLs or private tokens.
type requestError struct {
	failure error
	cause   error
}

func (e *requestError) Error() string {
	return e.failure.Error()
}

func (e *requestError) Unwrap() []error {
	return []error{e.failure, e.cause}
}

func embedPath(title model.Title) string {
	path := "/" + string(title.Type) + "/" + strconv.FormatInt(title.TMDBID, 10)
	if title.Type == model.TV {
		path += "/" + strconv.Itoa(*title.Season) + "/" + strconv.Itoa(*title.Episode)
	}
	return path
}

func requestContext(destination, referer string) model.FetchContext {
	return model.FetchContext{
		Scopes: []model.FetchScope{
			{
				Origin: destination,
				Headers: map[string]string{
					"User-Agent": userAgent,
					"Accept":     "*/*",
					"Referer":    referer,
				},
			},
		},
	}
}

func (a *Adapter) fetch(ctx context.Context, rawURL, referer, stage string, limit int64) (network.Response, error) {
	response, err := a.client.Fetch(ctx, network.Request{
		URL:     rawURL,
		Context: requestContext(rawURL, referer),
		Limit:   limit,
	})
	if err != nil {
		return network.Response{}, &requestError{
			failure: model.Fail(model.UpstreamFailure, stage, "upstream request or bounded read rejected"),
			cause:   err,
		}
	}
	// Shared transport validates every redirect and pins every socket. A changed
	// protocol endpoint still needs mapping; do not parse an unrelated final page.
	if response.URL == nil || response.URL.String() != rawURL {
		return network.Response{}, protocolError(stage, "upstream endpoint changed")
	}
	if response.Status == http.StatusNotFound || response.Status == http.StatusGone {
		return network.Response{}, model.Fail(model.Unavailable, stage, "upstream resource unavailable")
	}
	if response.Status != http.StatusOK {
		return network.Response{}, model.Fail(model.UpstreamFailure, stage, "upstream HTTP response rejected")
	}
	return response, nil
}

func parseConfig(body []byte, name string, target any) error {
	matches := configPattern.FindAllSubmatch(body, -1)
	var payload []byte
	for _, match := range matches {
		if string(match[1]) != name {
			continue
		}
		if payload != nil {
			return protocolError("player", "ambiguous player configuration")
		}
		payload = match[2]
	}
	if payload == nil || json.Unmarshal(payload, target) != nil {
		return protocolError("player", "static JSON player configuration missing or malformed")
	}
	return nil
}

func gateEndpoint(body []byte, title model.Title) (string, error) {
	var endpoint string
	for _, tag := range iframePattern.FindAll(body, -1) {
		attributes := make(map[string]string)
		for _, attribute := range attributePattern.FindAllSubmatch(tag, -1) {
			name := strings.ToLower(string(attribute[1]))
			if _, duplicate := attributes[name]; duplicate {
				return "", protocolError("embed", "ambiguous player attributes")
			}
			attributes[name] = html.UnescapeString(string(attribute[2]))
		}
		if attributes["id"] != "player_iframe" {
			continue
		}
		if endpoint != "" {
			return "", protocolError("embed", "ambiguous player frame")
		}
		endpoint = attributes["data-api"]
	}
	parsedURL, err := url.Parse(endpoint)
	if err != nil || parsedURL.IsAbs() || parsedURL.Host != "" || parsedURL.Path != "/vs_src.php" || parsedURL.Fragment != "" {
		return "", protocolError("embed", "unrecognized gate lookup")
	}
	query, err := url.ParseQuery(parsedURL.RawQuery)
	if err != nil || !matchesCoordinates(query, title, "id") {
		return "", protocolError("embed", "gate title coordinates mismatch")
	}
	expectedFields := 2
	if title.Type == model.TV {
		expectedFields = 4
	}
	if len(query) != expectedFields {
		return "", protocolError("embed", "unrecognized gate parameters")
	}
	return origin + endpoint, nil
}

func matchesCoordinates(query url.Values, title model.Title, identifier string) bool {
	if !singleValue(query, "type", string(title.Type)) || !singleValue(query, identifier, strconv.FormatInt(title.TMDBID, 10)) {
		return false
	}
	if title.Type == model.TV {
		return singleValue(query, "season", strconv.Itoa(*title.Season)) && singleValue(query, "episode", strconv.Itoa(*title.Episode))
	}
	return len(query["season"]) == 0 && len(query["episode"]) == 0
}

func singleValue(query url.Values, name, expected string) bool {
	return len(query[name]) == 1 && query[name][0] == expected
}

func playerEndpoint(rawURL, expectedPath string) (string, error) {
	baseURL, _ := url.Parse(playerOrigin)
	parsedURL, err := baseURL.Parse(rawURL)
	if err != nil || network.ValidateURL(parsedURL) != nil || !sameOrigin(parsedURL, playerOrigin) ||
		parsedURL.Path != expectedPath || parsedURL.Fragment != "" {
		return "", protocolError("player", "unrecognized player endpoint or title coordinates")
	}
	query, err := url.ParseQuery(parsedURL.RawQuery)
	if err != nil || len(query) != 1 || len(query["vs"]) != 1 || query.Get("vs") == "" || len(query.Get("vs")) > 2048 {
		return "", protocolError("player", "host-bound gate token missing or changed")
	}
	return parsedURL.String(), nil
}

type playerConfig struct {
	MediaType  string `json:"mediaType"`
	IDType     string `json:"idType"`
	MediaID    string `json:"mediaId"`
	TMDB       string `json:"tmdb"`
	Season     *int   `json:"season"`
	Episode    *int   `json:"episode"`
	API        string `json:"api"`
	StreamBase string `json:"streamBase"`
	APIToken   string `json:"apiToken"`
}

func sourceEndpoint(config playerConfig, title model.Title) (string, error) {
	if config.MediaType != string(title.Type) || config.IDType != "tmdb" ||
		config.MediaID != strconv.FormatInt(title.TMDBID, 10) || config.TMDB != config.MediaID {
		return "", protocolError("player", "player title identifiers mismatch")
	}
	if title.Type == model.TV {
		if config.Season == nil || config.Episode == nil || *config.Season != *title.Season || *config.Episode != *title.Episode {
			return "", protocolError("player", "player episode coordinates mismatch")
		}
	} else if config.Season != nil || config.Episode != nil {
		return "", protocolError("player", "movie carries episode coordinates")
	}

	rawURL := config.API
	if title.Type == model.TV {
		if rawURL != "" {
			return "", protocolError("player", "unrecognized TV API configuration")
		}
		rawURL = config.StreamBase
	}
	parsedURL, err := url.Parse(rawURL)
	if err != nil || network.ValidateURL(parsedURL) != nil || !sameOrigin(parsedURL, dataOrigin) || parsedURL.Path != "/api.php" || parsedURL.Fragment != "" {
		return "", protocolError("player", "unrecognized stream API")
	}
	query, err := url.ParseQuery(parsedURL.RawQuery)
	if err != nil {
		return "", protocolError("player", "malformed stream API query")
	}
	if title.Type == model.TV {
		if len(query) != 2 || !singleValue(query, "type", "tv") || !singleValue(query, "tmdb", strconv.FormatInt(title.TMDBID, 10)) {
			return "", protocolError("player", "TV stream base changed")
		}
		query.Set("season", strconv.Itoa(*title.Season))
		query.Set("episode", strconv.Itoa(*title.Episode))
		query.Set("stream_urls", "")
	}
	expectedFields := 3
	if title.Type == model.TV {
		expectedFields = 5
	}
	if len(query) != expectedFields || !matchesCoordinates(query, title, "tmdb") || !singleValue(query, "stream_urls", "") {
		return "", protocolError("player", "stream API title coordinates mismatch")
	}
	if config.APIToken == "" || len(config.APIToken) > 4096 || strings.ContainsAny(config.APIToken, "\r\n\x00") {
		return "", protocolError("player", "single-use API token absent or malformed")
	}
	query.Set("api_token", config.APIToken)
	parsedURL.RawQuery = query.Encode()
	return parsedURL.String(), nil
}

func (a *Adapter) resolve(ctx context.Context, title model.Title) (model.AdapterResult, error) {
	path := embedPath(title)
	embed, err := a.fetch(ctx, origin+"/embed"+path, origin+"/", "embed", 256*1024)
	if err != nil {
		return model.AdapterResult{}, err
	}
	gateURL, err := gateEndpoint(embed.Body, title)
	if err != nil {
		return model.AdapterResult{}, err
	}
	gate, err := a.fetch(ctx, gateURL, origin+"/embed"+path, "gate", 8*1024)
	if err != nil {
		return model.AdapterResult{}, err
	}
	var gateResponse struct {
		Src string `json:"src"`
	}
	if json.Unmarshal(gate.Body, &gateResponse) != nil {
		return model.AdapterResult{}, protocolError("gate", "malformed gate envelope")
	}
	landingURL, err := playerEndpoint(gateResponse.Src, "/embed"+path)
	if err != nil {
		return model.AdapterResult{}, err
	}
	landing, err := a.fetch(ctx, landingURL, origin+"/", "landing", 256*1024)
	if err != nil {
		return model.AdapterResult{}, err
	}
	var landingConfig struct {
		PlayerURL string `json:"playerUrl"`
	}
	if err := parseConfig(landing.Body, "CFG", &landingConfig); err != nil {
		return model.AdapterResult{}, err
	}
	playerURL, err := playerEndpoint(landingConfig.PlayerURL, "/embed/player"+path)
	if err != nil {
		return model.AdapterResult{}, err
	}
	player, err := a.fetch(ctx, playerURL, playerOrigin+"/", "player", 256*1024)
	if err != nil {
		return model.AdapterResult{}, err
	}
	var config playerConfig
	if err := parseConfig(player.Body, "CONFIG", &config); err != nil {
		return model.AdapterResult{}, err
	}
	apiURL, err := sourceEndpoint(config, title)
	if err != nil {
		return model.AdapterResult{}, err
	}
	response, err := a.fetch(ctx, apiURL, playerOrigin+"/", "sources", 256*1024)
	if err != nil {
		return model.AdapterResult{}, err
	}
	return a.sources(ctx, response.Body, title)
}

func (a *Adapter) decrypt(ctx context.Context, encoded string, protection json.RawMessage) ([]string, error) {
	var envelope struct {
		Window  *int64 `json:"w"`
		WASMURL string `json:"wasm_url"`
		WASM    string `json:"wasm"`
	}
	if json.Unmarshal(protection, &envelope) != nil || envelope.Window == nil || *envelope.Window < 0 {
		return nil, protocolError("decryptor", "encrypted stream envelope missing")
	}
	var module []byte
	lifetime := time.Minute
	if envelope.WASMURL != "" {
		parsedURL, err := url.Parse(envelope.WASMURL)
		if err != nil || network.ValidateURL(parsedURL) != nil || !sameOrigin(parsedURL, dataOrigin) || parsedURL.Path != "/wasm.php" || parsedURL.Fragment != "" {
			return nil, protocolError("decryptor", "unrecognized static module endpoint")
		}
		query, err := url.ParseQuery(parsedURL.RawQuery)
		if err != nil || !singleValue(query, "w", strconv.FormatInt(*envelope.Window, 10)) || len(query) != 2 || len(query["_"]) != 1 {
			return nil, protocolError("decryptor", "static module window mismatch")
		}
		// Use the exact declared URL, including its window and cache-bust value;
		// no assumption is made that distinct module URLs contain the same key.
		if cached, found := a.decryptors.Get(envelope.WASMURL); found {
			streams, err := decryptStreamsWithKey(encoded, cached)
			if err != nil {
				return nil, protocolError("decryptor", "encrypted stream list rejected: "+err.Error())
			}
			return streams, nil
		}
		response, err := a.fetch(ctx, envelope.WASMURL, playerOrigin+"/", "decryptor", 128*1024)
		if err != nil {
			return nil, err
		}
		module = response.Body
		lifetime = cache.AssetLifetime(response.Header, time.Minute)
	} else {
		var err error
		module, err = base64.StdEncoding.Strict().DecodeString(envelope.WASM)
		if err != nil {
			return nil, protocolError("decryptor", "invalid inline static module")
		}
	}
	digest := fmt.Sprintf("%x", sha256.Sum256(module))
	key, found := a.keys.Get(digest)
	if !found {
		var err error
		key, err = moduleKey(module)
		if err != nil {
			return nil, protocolError("decryptor", "unrecognized static cipher: "+err.Error())
		}
		a.keys.Put(digest, key, 2*time.Minute, 16)
	}
	streams, err := decryptStreamsWithKey(encoded, key)
	if err != nil {
		return nil, protocolError("decryptor", "encrypted stream list rejected: "+err.Error())
	}
	if envelope.WASMURL != "" {
		a.decryptors.Put(envelope.WASMURL, key, lifetime, 16)
	}
	return streams, nil
}

func sameOrigin(parsedURL *url.URL, origin string) bool {
	expectedURL, err := url.Parse(origin)
	return err == nil && network.Origin(parsedURL) == network.Origin(expectedURL)
}
