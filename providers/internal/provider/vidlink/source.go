package vidlink

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"path"
	"regexp"
	"sort"
	"strconv"
	"strings"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

type source struct {
	Type     string            `json:"type"`
	URL      string            `json:"url"`
	Playlist string            `json:"playlist"`
	Headers  map[string]string `json:"headers"`
	Season   *int              `json:"se"`
	Episode  *int              `json:"ep"`
}

var sourceIDPattern = regexp.MustCompile(`^[A-Za-z0-9_-]{1,128}$`)

func protocolError(detail string) error {
	return model.Fail(model.ProtocolChange, "sources", detail)
}

// normalize maps the observed response without applying the browser's external
// proxy URL transformation. Native consumers retain direct URLs and scoped
// context; requiresProxy/TTL/codecName remain unverified provider metadata.
func normalize(body []byte, title model.Title) (model.AdapterResult, error) {
	var envelope struct {
		SourceID string          `json:"sourceId"`
		Stream   json.RawMessage `json:"stream"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return model.AdapterResult{}, protocolError("malformed source envelope")
	}
	if strings.TrimSpace(string(body)) == "null" || string(envelope.Stream) == "null" {
		return model.AdapterResult{}, model.Fail(model.Unavailable, "sources", "no source returned")
	}
	if len(envelope.Stream) == 0 || !sourceIDPattern.MatchString(envelope.SourceID) {
		return model.AdapterResult{}, protocolError("source identity or stream missing")
	}

	var stream struct {
		Playlist        string                     `json:"playlist"`
		DeliveryType    string                     `json:"deliveryType"`
		PlaylistHeaders map[string]string          `json:"playlistHeaders"`
		Qualities       map[string]json.RawMessage `json:"qualities"`
		Alternates      map[string]json.RawMessage `json:"alternates"`
		Captions        []json.RawMessage          `json:"captions"`
		TTL             json.RawMessage            `json:"TTL"`
	}
	if err := json.Unmarshal(envelope.Stream, &stream); err != nil {
		return model.AdapterResult{}, protocolError("invalid stream schema")
	}
	if len(stream.Qualities)+len(stream.Alternates) > 31 || len(stream.Captions) > 64 {
		return model.AdapterResult{}, protocolError("source count outside supported bounds")
	}

	captionsJSON, err := json.Marshal(stream.Captions)
	if err != nil {
		return model.AdapterResult{}, protocolError("invalid caption metadata")
	}
	result := model.AdapterResult{}
	appendSource := func(identity string, record source, raw json.RawMessage) {
		candidate, err := normalizeSource(record, title, envelope.SourceID, identity)
		if err != nil {
			result.Issues = append(result.Issues, model.Failure{
				Category: model.Category(err),
				Stage:    "sources",
				Detail:   "source record rejected",
			})
			return
		}

		candidate.Metadata.ProviderData = map[string]json.RawMessage{
			"source_record": raw,
			"captions":      captionsJSON,
		}
		if len(stream.TTL) > 0 {
			candidate.Metadata.ProviderData["ttl"] = stream.TTL
		}
		// SRT captions are retained privately rather than misrepresented as WebVTT
		// or used to satisfy required playable subtitles. Audio is still unknown.
		result.Candidates = append(result.Candidates, candidate)
	}

	if stream.Playlist != "" {
		appendSource("primary", source{
			URL:     stream.Playlist,
			Type:    stream.DeliveryType,
			Headers: stream.PlaylistHeaders,
		}, envelope.Stream)
	}

	for _, collection := range []struct {
		Prefix  string
		Records map[string]json.RawMessage
	}{
		{
			Prefix:  "quality",
			Records: stream.Qualities,
		},
		{
			Prefix:  "alternate",
			Records: stream.Alternates,
		},
	} {
		identities := make([]string, 0, len(collection.Records))
		for identity := range collection.Records {
			identities = append(identities, identity)
		}
		sort.Strings(identities)

		for _, identity := range identities {
			raw := collection.Records[identity]
			var record source
			if err := json.Unmarshal(raw, &record); err != nil {
				result.Issues = append(result.Issues, model.Failure{
					Category: model.ProtocolChange,
					Stage:    "sources",
					Detail:   "malformed source record rejected",
				})
				continue
			}
			if record.URL == "" {
				record.URL = record.Playlist
			}
			appendSource(collection.Prefix+":"+identity, record, raw)
		}
	}

	if len(result.Candidates) == 0 {
		if len(result.Issues) > 0 {
			return result, protocolError("all source records rejected")
		}
		return result, model.Fail(model.Unavailable, "sources", "no source returned")
	}

	return result, nil
}

func normalizeSource(record source, title model.Title, server, identity string) (model.Candidate, error) {
	resourceURL, err := url.Parse(record.URL)
	if err != nil || network.ValidateURL(resourceURL) != nil {
		return model.Candidate{}, protocolError("source destination denied")
	}
	if title.Type == model.TV && ((record.Season != nil && *record.Season != *title.Season) ||
		(record.Episode != nil && *record.Episode != *title.Episode)) {
		return model.Candidate{}, protocolError("source episode coordinates mismatch")
	}

	format := model.UnknownFormat
	switch strings.ToLower(record.Type) {
	case "mp4":
		format = model.MP4
	case "hls":
		format = model.HLS
	case "dash":
		format = model.DASH
	default:
		return model.Candidate{}, model.Fail(model.Unsupported, "sources", "unmapped source format")
	}

	fetchContext, err := sourceContext(resourceURL, record.Headers)
	if err != nil {
		return model.Candidate{}, err
	}
	digest := sha256.Sum256([]byte(identity + "\x00" + record.URL))
	candidate := model.Candidate{
		ID:       fmt.Sprintf("%x", digest[:12]),
		Provider: model.VidLink,
		Server:   server,
		Title:    title,
		URL:      record.URL,
		Format:   format,
		Context:  fetchContext,
	}

	// The quality key declares a resolution tier, not a measured width or bitrate.
	if quality, found := strings.CutPrefix(identity, "quality:"); found {
		if height, err := strconv.Atoi(quality); err == nil && height > 0 && height <= 16384 {
			candidate.Metadata.Variants = []model.Variant{
				{
					URL:    record.URL,
					Height: &height,
				},
			}
		}
	}
	return candidate, nil
}

func sourceContext(resourceURL *url.URL, headers map[string]string) (model.FetchContext, error) {
	publicResources := &model.PublicResourceContext{}
	scope := model.FetchScope{
		Origin:     network.Origin(resourceURL),
		PathPrefix: resourceURL.Path,
		Headers:    make(map[string]string),
	}
	for name, value := range headers {
		if strings.ContainsAny(name+value, "\r\n\x00") {
			return model.FetchContext{}, protocolError("invalid source header")
		}
		switch strings.ToLower(name) {
		case "cookie":
			scope.Cookies = value
			scope.CookiePath = resourceURL.Path
		case "referer", "origin":
			identityURL, err := url.Parse(value)
			isPublicRoot := err == nil && network.ValidateURL(identityURL) == nil &&
				identityURL.RawQuery == "" && !identityURL.ForceQuery && identityURL.Fragment == "" &&
				(identityURL.Path == "" || identityURL.Path == "/")
			if isPublicRoot {
				if strings.EqualFold(name, "origin") {
					publicResources.Origin = value
				} else {
					publicResources.Referer = value
				}
				continue
			}
			scope.Headers[http.CanonicalHeaderKey(name)] = value
		case "authorization", "user-agent", "accept", "accept-language":
			scope.Headers[http.CanonicalHeaderKey(name)] = value
		default:
			return model.FetchContext{}, protocolError("unmapped source request header")
		}
	}
	if len(headers) == 0 {
		return model.FetchContext{}, nil
	}

	// Credentials never inherit the provider origin or another CDN. Path scope is
	// the exact resource path because no wider credential scope was demonstrated.
	if (scope.Cookies != "" || scope.Headers["Authorization"] != "") &&
		(scope.PathPrefix == "" || path.Clean(scope.PathPrefix) == "/") {
		return model.FetchContext{}, protocolError("source cookie path scope unresolved")
	}
	fetchContext := model.FetchContext{}
	if len(scope.Headers) > 0 || scope.Cookies != "" {
		fetchContext.Scopes = []model.FetchScope{scope}
	}
	if publicResources.Origin != "" || publicResources.Referer != "" {
		fetchContext.PublicResources = publicResources
	}
	return fetchContext, nil
}
