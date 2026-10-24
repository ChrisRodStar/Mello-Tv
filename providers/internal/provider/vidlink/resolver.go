package vidlink

import (
	"context"
	"crypto/sha256"
	"fmt"
	"net/http"
	"net/http/cookiejar"
	"net/url"
	"strconv"
	"time"

	"stv-providers/internal/cache"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
)

const (
	origin       = "https://vidlink.pro"
	playerDigest = "27b65f7f7df985942df6197868896adbbdaf230fac7778089a8a0fa259a38f56"
	userAgent    = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36"
)

// requestError retains the private cause for errors.Is/As but formats only the
// categorized safe message. Transport errors may contain signed request URLs.
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
	path := "/movie/" + strconv.FormatInt(title.TMDBID, 10)
	if title.Type == model.TV {
		path = "/tv/" + strconv.FormatInt(title.TMDBID, 10) + "/" + strconv.Itoa(*title.Season) + "/" + strconv.Itoa(*title.Episode)
	}
	return path
}

// fetch requires the exact requested endpoint after shared redirect validation.
func (a *Adapter) fetch(
	ctx context.Context,
	request network.Request,
	stage string,
) (network.Response, error) {
	response, err := a.client.Fetch(ctx, request)
	if err != nil {
		return network.Response{}, &requestError{
			failure: model.Fail(model.UpstreamFailure, stage, "upstream request or bounded read rejected"),
			cause:   err,
		}
	}

	expectedURL, err := url.Parse(request.URL)
	if err != nil || response.URL == nil || response.URL.String() != expectedURL.String() {
		return network.Response{}, model.Fail(model.UpstreamFailure, stage, "upstream endpoint changed")
	}

	if response.Status == http.StatusNotFound || response.Status == http.StatusGone {
		return network.Response{}, model.Fail(model.Unavailable, stage, "upstream resource unavailable")
	}
	if response.Status != http.StatusOK {
		return network.Response{}, model.Fail(model.UpstreamFailure, stage, "upstream HTTP response rejected")
	}

	return response, nil
}

func (a *Adapter) resolve(ctx context.Context, title model.Title) (model.AdapterResult, error) {
	jar, err := cookiejar.New(nil)
	if err != nil {
		return model.AdapterResult{}, fmt.Errorf("create VidLink session: %w", err)
	}

	path := embedPath(title)
	fetchContext := model.FetchContext{
		Scopes: []model.FetchScope{
			{
				Origin: origin,
				Headers: map[string]string{
					"User-Agent":             userAgent,
					"Accept":                 "*/*",
					"Origin":                 origin,
					"Referer":                origin + path,
					"X-Playback-Environment": "hls",
				},
			},
		},
	}

	_, err = a.fetch(ctx, network.Request{
		URL:     origin + path,
		Context: fetchContext,
		Jar:     jar,
		Limit:   512 * 1024,
	}, "embed")
	if err != nil {
		return model.AdapterResult{}, err
	}

	playerURL := origin + "/fu.wasm"
	if _, verified := a.players.Get(playerURL); !verified {
		player, fetchErr := a.fetch(ctx, network.Request{
			URL: playerURL, Context: fetchContext, Jar: jar, Limit: 3 * 1024 * 1024,
		}, "player")
		if fetchErr != nil {
			return model.AdapterResult{}, fetchErr
		}
		if fmt.Sprintf("%x", sha256.Sum256(player.Body)) != playerDigest {
			return model.AdapterResult{}, model.Fail(model.ProtocolChange, "player", "unrecognized static token algorithm version")
		}
		// Never skip a response that installs per-lookup session cookies.
		if len(player.Header.Values("Set-Cookie")) == 0 {
			a.players.Put(playerURL, true, cache.AssetLifetime(player.Header, 2*time.Minute), 1)
		}
	}

	token, err := generateToken(title.TMDBID, time.Now())
	if err != nil {
		return model.AdapterResult{}, err
	}

	apiPath := "/api/b/movie/" + token
	if title.Type == model.TV {
		apiPath = "/api/b/tv/" + token + "/" + strconv.Itoa(*title.Season) + "/" + strconv.Itoa(*title.Episode)
	}

	response, err := a.fetch(ctx, network.Request{
		URL:     origin + apiPath + "?multiLang=1",
		Context: fetchContext,
		Jar:     jar,
		Limit:   1024 * 1024,
	}, "sources")
	if err != nil {
		return model.AdapterResult{}, err
	}

	return normalize(response.Body, title)
}
