package vidfast

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"path"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"family-stream-providers/internal/cache"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
)

func normalize(body []byte, title model.Title, serverName string) (model.Candidate, error) {
	var record struct {
		URL        string            `json:"url"`
		TMDBID     json.Number       `json:"tmdbId"`
		MP4        *bool             `json:"mp4"`
		NoReferrer *bool             `json:"noReferrer"`
		Tracks     []json.RawMessage `json:"tracks"`
		Season     *json.Number      `json:"season"`
		Episode    *json.Number      `json:"episode"`
	}
	var metadata map[string]json.RawMessage
	if json.Unmarshal(body, &record) != nil || json.Unmarshal(body, &metadata) != nil || metadata == nil {
		return model.Candidate{}, protocolError("sources", "source schema malformed")
	}
	if record.TMDBID.String() != strconv.FormatInt(title.TMDBID, 10) {
		return model.Candidate{}, protocolError("sources", "source title identifier mismatch")
	}
	if record.Season != nil || record.Episode != nil {
		if title.Type != model.TV || record.Season == nil || record.Episode == nil ||
			record.Season.String() != strconv.Itoa(*title.Season) || record.Episode.String() != strconv.Itoa(*title.Episode) {
			return model.Candidate{}, protocolError("sources", "source episode coordinates mismatch")
		}
	}

	resourceURL, err := url.Parse(record.URL)
	if err != nil || network.ValidateURL(resourceURL) != nil || len(record.URL) > 8192 || resourceURL.Fragment != "" {
		return model.Candidate{}, protocolError("sources", "source destination rejected")
	}
	if len(record.Tracks) > 64 {
		return model.Candidate{}, protocolError("sources", "subtitle count exceeds bound")
	}
	format := model.UnknownFormat
	switch strings.ToLower(path.Ext(resourceURL.Path)) {
	case ".m3u8":
		format = model.HLS
	case ".mp4":
		format = model.MP4
	case ".mpd":
		format = model.DASH
	}
	if record.MP4 != nil && *record.MP4 {
		if format != model.UnknownFormat && format != model.MP4 {
			return model.Candidate{}, protocolError("sources", "source format declarations conflict")
		}
		format = model.MP4
	}
	digest := sha256.Sum256([]byte(serverName + "\x00" + record.URL))
	candidate := model.Candidate{
		ID:       fmt.Sprintf("%x", digest[:12]),
		Provider: model.VidFast,
		Server:   serverName,
		Title:    title,
		URL:      record.URL,
		Format:   format,
		Metadata: model.Metadata{ProviderData: metadata},
	}
	if record.NoReferrer == nil {
		return model.Candidate{}, protocolError("sources", "media referrer policy unknown")
	}
	if !*record.NoReferrer {
		candidate.Context.PublicResources = &model.PublicResourceContext{Referer: origin + "/"}
	}
	// The upstream labels alone do not establish WebVTT compatibility or audio
	// language. Keep tracks and englishTrackIndex in private provider metadata;
	// shared probing can establish actual HLS rendition tracks independently.
	return candidate, nil
}

func (a *Adapter) resolve(ctx context.Context, title model.Title) (model.AdapterResult, error) {
	bootstrap, err := a.bootstrap(ctx, title)
	if err != nil {
		return model.AdapterResult{}, err
	}
	material, cached := a.materials.Get(bootstrap.PlayerScript)
	if !cached {
		bundle, fetchErr := a.protocolFetch(ctx, bootstrap.PlayerScript, http.MethodGet, protocolMaterial{}, 3*1024*1024)
		if fetchErr != nil {
			return model.AdapterResult{}, fetchErr
		}
		if bundle.Status != http.StatusOK {
			return model.AdapterResult{}, model.Fail(model.UpstreamFailure, "bundle", "player bundle unavailable")
		}
		recognize := a.recognize
		if recognize == nil {
			recognize = recognizePlayer
		}
		material, err = recognize(bundle.Body)
		if err != nil {
			return model.AdapterResult{}, err
		}
		a.materials.Put(bootstrap.PlayerScript, material, cache.AssetLifetime(bundle.Header, 2*time.Minute), 8)
	}

	servers, err := a.probeServers(ctx, bootstrap, material)
	if err != nil {
		return model.AdapterResult{}, err
	}
	return a.sources(ctx, title, servers, material)
}

func (a *Adapter) sources(ctx context.Context, title model.Title, servers []server, material protocolMaterial) (model.AdapterResult, error) {
	if len(servers) == 0 {
		return model.AdapterResult{}, model.Fail(model.Unavailable, "servers", "no servers returned")
	}
	if len(servers) > 16 {
		return model.AdapterResult{}, protocolError("servers", "too many servers")
	}
	// Return a margin before the provider deadline, so a stalled alternative
	// cannot erase candidates already resolved by other bounded workers.
	unlockContext := ctx
	cancel := func() {}
	if deadline, found := ctx.Deadline(); found {
		unlockContext, cancel = context.WithDeadline(ctx, deadline.Add(-250*time.Millisecond))
	}
	defer cancel()
	type outcome struct {
		candidate model.Candidate
		err       error
	}
	outcomes := make([]outcome, len(servers))
	var cursor atomic.Int32
	var workers sync.WaitGroup
	for worker := 0; worker < min(3, len(servers)); worker++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			for {
				index := int(cursor.Add(1)) - 1
				if index >= len(servers) {
					return
				}
				serverContext, stop := context.WithTimeout(unlockContext, 3*time.Second)
				plaintext, err := a.unlock(serverContext, material, servers[index])
				stop()
				if err == nil {
					outcomes[index].candidate, err = normalize(plaintext, title, servers[index].Name)
				}
				outcomes[index].err = err
			}
		}()
	}
	workers.Wait()
	result := model.AdapterResult{}
	for _, outcome := range outcomes {
		if outcome.err != nil {
			result.Issues = append(result.Issues, model.Failure{
				Category: model.Category(outcome.err),
				Stage:    "unlock",
				Detail:   "server alternative unavailable or rejected",
			})
			continue
		}
		result.Candidates = append(result.Candidates, outcome.candidate)
	}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	if len(result.Candidates) > 0 {
		return result, nil
	}
	return result, model.Fail(model.Unavailable, "sources", "no usable server alternatives")
}
