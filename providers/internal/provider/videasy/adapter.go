package videasy

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/provider"
)

const (
	baseURL = "https://api.wecollege.net"
	origin  = "https://vidy.st"
	referer = "https://vidy.st/"
)

var (
	// Ordered by priority; boise and miami provide 4K UHD manifests when available.
	activeServers = []string{
		"boise",
		"miami",
		"phoenix",
		"seattle",
		"atlanta",
		"portland",
		"vegas",
		"reno",
		"oakland",
	}
)

type protocolClient interface {
	Fetch(context.Context, network.Request) (network.Response, error)
}

// Adapter implements the provider.Adapter interface for Videasy.
type Adapter struct {
	client protocolClient
}

// New creates a new Videasy adapter instance bound to the provided network client.
func New(client *network.Client) *Adapter {
	adapter := &Adapter{}
	if client != nil {
		adapter.client = client
	}
	return adapter
}

// ID returns the ProviderID for Videasy.
func (*Adapter) ID() model.ProviderID {
	return model.Videasy
}

// Capabilities returns the media formats and identifier schemas supported by Videasy.
func (*Adapter) Capabilities() provider.Capabilities {
	return provider.Capabilities{
		MediaTypes:  []model.MediaType{model.Movie, model.TV},
		Identifiers: []string{"tmdb", "imdb"},
	}
}

type seedResponse struct {
	Seed  string `json:"seed"`
	TTLMs int    `json:"ttlMs"`
}

type upstreamSource struct {
	Quality string `json:"quality"`
	URL     string `json:"url"`
}

type upstreamSubtitle struct {
	Lang     string `json:"lang"`
	Language string `json:"language"`
	URL      string `json:"url"`
}

type upstreamPayload struct {
	Sources   []upstreamSource   `json:"sources"`
	Subtitles []upstreamSubtitle `json:"subtitles"`
	Playlist  string             `json:"playlist"`
}

// fetchContext returns the security headers required for Videasy endpoints and CDN playback.
func makeFetchContext() model.FetchContext {
	return model.FetchContext{
		PublicResources: &model.PublicResourceContext{
			Origin:  origin,
			Referer: referer,
		},
		Scopes: []model.FetchScope{
			{
				Origin: origin,
				Headers: map[string]string{
					"User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36",
					"Origin":     origin,
					"Referer":    referer,
					"Accept":     "*/*",
				},
			},
		},
	}
}

// fetchSeed queries the Videasy seed endpoint for the current media session.
func (a *Adapter) fetchSeed(ctx context.Context, tmdbID int64) (string, error) {
	seedURL := fmt.Sprintf("%s/seed?mediaId=%d", baseURL, tmdbID)
	req := network.Request{
		Method:  "GET",
		URL:     seedURL,
		Context: makeFetchContext(),
	}

	resp, err := a.client.Fetch(ctx, req)
	if err != nil {
		return "", fmt.Errorf("videasy seed request failed: %w", err)
	}

	if resp.Status != 200 {
		return "", fmt.Errorf("videasy seed returned status %d", resp.Status)
	}

	var sr seedResponse
	if err := json.Unmarshal(resp.Body, &sr); err != nil {
		return "", fmt.Errorf("unmarshal videasy seed failed: %w", err)
	}

	if sr.Seed == "" {
		return "", errors.New("empty seed in videasy response")
	}

	return sr.Seed, nil
}

// Resolve queries Videasy for playable stream sources matching the requested title.
func (a *Adapter) Resolve(ctx context.Context, lookup model.Lookup) (result model.AdapterResult, err error) {
	startTime := time.Now()
	defer func() {
		result.Duration = time.Since(startTime)
	}()

	if ctx == nil {
		return result, model.Fail(model.Unsupported, "context", "nil context provided")
	}
	if err := ctx.Err(); err != nil {
		return result, err
	}

	if err := lookup.Title.Validate(); err != nil {
		return result, model.Fail(model.Unsupported, "input", "invalid title identifiers or episode coordinates")
	}

	if a.client == nil {
		return result, model.Fail(model.Unavailable, "network", "VPN-controlled network client required")
	}

	seed, err := a.fetchSeed(ctx, lookup.Title.TMDBID)
	if err != nil {
		return result, model.Fail(model.Unavailable, "upstream", err.Error())
	}

	imdbID := lookup.Title.IMDbID

	type serverResult struct {
		server  string
		payload upstreamPayload
	}

	resultsChan := make(chan serverResult, len(activeServers))
	var wg sync.WaitGroup

	fCtx := makeFetchContext()

	for _, srv := range activeServers {
		wg.Add(1)
		go func(serverName string) {
			defer wg.Done()

			var queryURL string
			if lookup.Title.Type == model.TV {
				s := 1
				e := 1
				if lookup.Title.Season != nil {
					s = *lookup.Title.Season
				}
				if lookup.Title.Episode != nil {
					e = *lookup.Title.Episode
				}
				queryURL = fmt.Sprintf("%s/%s/sources?title=media&mediaType=tv&episodeId=%d&seasonId=%d&tmdbId=%d&imdbId=%s&enc=2&seed=%s",
					baseURL, serverName, e, s, lookup.Title.TMDBID, imdbID, seed)
			} else {
				queryURL = fmt.Sprintf("%s/%s/sources?title=media&mediaType=movie&tmdbId=%d&imdbId=%s&enc=2&seed=%s",
					baseURL, serverName, lookup.Title.TMDBID, imdbID, seed)
			}

			req := network.Request{
				Method:  "GET",
				URL:     queryURL,
				Context: fCtx,
			}

			resp, err := a.client.Fetch(ctx, req)
			if err != nil || resp.Status != 200 || len(resp.Body) < 20 {
				return
			}

			plain, err := DecryptPayload(string(resp.Body), seed, lookup.Title.TMDBID)
			if err != nil {
				return
			}

			var up upstreamPayload
			if err := json.Unmarshal(plain, &up); err != nil {
				return
			}

			if len(up.Sources) > 0 || up.Playlist != "" {
				resultsChan <- serverResult{server: serverName, payload: up}
			}
		}(srv)
	}

	wg.Wait()
	close(resultsChan)

	var candidates []model.Candidate
	seenURLs := make(map[string]bool)

	for res := range resultsChan {
		// Include master playlist if provided
		if res.payload.Playlist != "" && !seenURLs[res.payload.Playlist] {
			seenURLs[res.payload.Playlist] = true
			c := model.Candidate{
				ID:       fmt.Sprintf("videasy-%s-master", res.server),
				Provider: model.Videasy,
				Server:   res.server,
				Format:   model.HLS,
				URL:      res.payload.Playlist,
				Title:    lookup.Title.Clone(),
				Context:  fCtx,
				Metadata: model.Metadata{
					TracksKnown: len(res.payload.Subtitles) > 0,
					Tracks:      mapSubtitles(res.payload.Subtitles),
				},
				ContentVerified: true,
			}
			candidates = append(candidates, c)
		}

		// Include quality renditions (2160p 4K, 1080p, 720p, etc.)
		for idx, src := range res.payload.Sources {
			if src.URL == "" || seenURLs[src.URL] {
				continue
			}
			seenURLs[src.URL] = true

			variant := parseVariant(src.Quality, src.URL)
			c := model.Candidate{
				ID:                 fmt.Sprintf("videasy-%s-%d", res.server, idx),
				Provider:           model.Videasy,
				Server:             res.server,
				Format:             model.HLS,
				URL:                src.URL,
				SelectedVariantURL: src.URL,
				Title:              lookup.Title.Clone(),
				Context:            fCtx,
				Metadata: model.Metadata{
					Variants:    []model.Variant{variant},
					TracksKnown: len(res.payload.Subtitles) > 0,
					Tracks:      mapSubtitles(res.payload.Subtitles),
				},
				ContentVerified: true,
			}
			candidates = append(candidates, c)
		}
	}

	if len(candidates) == 0 {
		return result, model.Fail(model.Unavailable, "upstream", "no streams returned from Videasy servers")
	}

	result.Candidates = candidates
	return result, nil
}

func parseVariant(quality, streamURL string) model.Variant {
	v := model.Variant{URL: streamURL}
	q := strings.ToLower(quality)

	switch {
	case strings.Contains(q, "2160") || strings.Contains(q, "4k"):
		w, h := 3840, 2160
		v.Width = &w
		v.Height = &h
	case strings.Contains(q, "1080"):
		w, h := 1920, 1080
		v.Width = &w
		v.Height = &h
	case strings.Contains(q, "720"):
		w, h := 1280, 720
		v.Width = &w
		v.Height = &h
	case strings.Contains(q, "480"):
		w, h := 854, 480
		v.Width = &w
		v.Height = &h
	case strings.Contains(q, "360"):
		w, h := 640, 360
		v.Width = &w
		v.Height = &h
	}

	return v
}

func mapSubtitles(subs []upstreamSubtitle) []model.Track {
	tracks := make([]model.Track, 0, len(subs))
	for _, sub := range subs {
		if sub.URL == "" {
			continue
		}
		lang := sub.Language
		if lang == "" {
			lang = sub.Lang
		}
		tracks = append(tracks, model.Track{
			Kind:     "subtitles",
			Language: lang,
			Name:     lang,
			URL:      sub.URL,
		})
	}
	return tracks
}
