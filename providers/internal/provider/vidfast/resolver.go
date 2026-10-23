package vidfast

import (
	"context"
	"encoding/json"
	"net/http"
	"regexp"
	"strconv"
	"strings"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
)

const (
	origin        = "https://vidfast.vc"
	maxEmbedSize  = 512 * 1024
	maxFlightSize = 128 * 1024
)

// Only literal JSON strings inside the observed Flight push framing are read.
// This is data extraction, never JavaScript evaluation or a Flight interpreter.
var scriptPattern = regexp.MustCompile(`src="(/_next/static/chunks/365-[a-zA-Z0-9-]+\.js)"`)

var flightPattern = regexp.MustCompile(`self\.__next_f\.push\(\[1,("(?:[^"\\]|\\.)*")\]\)`)

type bootstrap struct {
	Title        model.Title
	Token        string
	PlayerScript string
}

func protocolError(stage, detail string) error {
	return model.Fail(model.ProtocolChange, stage, detail)
}

// requestError preserves cancellation causes without formatting private URLs.
type requestError struct {
	cause error
}

func (*requestError) Error() string {
	return "VidFast bootstrap request or bounded read rejected"
}

func (e *requestError) Unwrap() error {
	return e.cause
}

func embedPath(title model.Title) string {
	path := "/" + string(title.Type) + "/" + strconv.FormatInt(title.TMDBID, 10)
	if title.Type == model.TV {
		path += "/" + strconv.Itoa(*title.Season) + "/" + strconv.Itoa(*title.Episode)
	}
	return path
}

func (a *Adapter) bootstrap(ctx context.Context, title model.Title) (bootstrap, error) {
	targetURL := origin + embedPath(title)
	response, err := a.client.Fetch(ctx, network.Request{
		URL: targetURL,
		Context: model.FetchContext{
			Scopes: []model.FetchScope{
				{
					Origin: origin,
					Headers: map[string]string{
						"Referer": origin + "/",
					},
				},
			},
		},
		Limit: maxEmbedSize,
	})
	if err != nil {
		return bootstrap{}, &requestError{cause: err}
	}
	if response.URL == nil || response.URL.String() != targetURL {
		return bootstrap{}, protocolError("embed", "upstream endpoint changed")
	}
	if response.Status == http.StatusNotFound || response.Status == http.StatusGone {
		return bootstrap{}, model.Fail(model.Unavailable, "embed", "upstream title unavailable")
	}
	if response.Status != http.StatusOK {
		return bootstrap{}, model.Fail(model.UpstreamFailure, "embed", "upstream HTTP response rejected")
	}
	return parseBootstrap(response.Body, title)
}

func parseBootstrap(body []byte, title model.Title) (bootstrap, error) {
	if len(body) > maxEmbedSize {
		return bootstrap{}, protocolError("embed", "embed exceeds bounded parser size")
	}
	if err := title.Validate(); err != nil {
		return bootstrap{}, protocolError("embed", "invalid title coordinates")
	}

	var flight strings.Builder
	for _, match := range flightPattern.FindAllSubmatch(body, -1) {
		var chunk string
		if json.Unmarshal(match[1], &chunk) != nil || len(chunk) > maxFlightSize-flight.Len() {
			return bootstrap{}, protocolError("bootstrap", "invalid or oversized Flight data")
		}
		flight.WriteString(chunk)
	}

	var found *bootstrap
	for _, line := range strings.Split(flight.String(), "\n") {
		_, payload, framed := strings.Cut(line, ":")
		if !framed || !strings.HasPrefix(payload, `["$","$L`) {
			continue
		}
		var component []json.RawMessage
		if json.Unmarshal([]byte(payload), &component) != nil || len(component) != 4 {
			return bootstrap{}, protocolError("bootstrap", "malformed player component")
		}
		var properties map[string]json.RawMessage
		if json.Unmarshal(component[3], &properties) != nil {
			return bootstrap{}, protocolError("bootstrap", "malformed player properties")
		}
		if _, carriesToken := properties["en"]; !carriesToken {
			continue
		}
		if found != nil {
			return bootstrap{}, protocolError("bootstrap", "ambiguous player component")
		}

		readString := func(name string) string {
			var value string
			_ = json.Unmarshal(properties[name], &value)
			return value
		}
		if readString("host") != "vidfast.vc" || readString("id") != strconv.FormatInt(title.TMDBID, 10) {
			return bootstrap{}, protocolError("bootstrap", "player host or title mismatch")
		}
		if title.Type == model.TV {
			if readString("season") != strconv.Itoa(*title.Season) || readString("episode") != strconv.Itoa(*title.Episode) {
				return bootstrap{}, protocolError("bootstrap", "player episode coordinates mismatch")
			}
		} else if properties["season"] != nil || properties["episode"] != nil {
			return bootstrap{}, protocolError("bootstrap", "movie carries episode coordinates")
		}

		token := readString("en")
		if len(token) < 16 || len(token) > 4096 || strings.IndexFunc(token, func(character rune) bool {
			return !(character >= 'A' && character <= 'Z' || character >= 'a' && character <= 'z' ||
				character >= '0' && character <= '9' || character == '-' || character == '_')
		}) >= 0 {
			return bootstrap{}, protocolError("bootstrap", "opaque bootstrap token malformed")
		}
		found = &bootstrap{
			Title: title,
			Token: token,
		}
	}
	if found == nil {
		return bootstrap{}, protocolError("bootstrap", "static player properties absent")
	}
	scripts := scriptPattern.FindAllSubmatch(body, -1)
	if len(scripts) != 1 {
		return bootstrap{}, protocolError("bootstrap", "unrecognized shared player script")
	}
	found.PlayerScript = origin + string(scripts[0][1])
	return *found, nil
}
