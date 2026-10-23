package vidcore

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"fmt"
	"net/http"
	"net/http/cookiejar"
	"net/url"
	"path"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// serverRecord represents an entry in the decrypted server list returned by VidCore.
type serverRecord struct {
	Name string `json:"name"`
	Data string `json:"data"`
}

var (
	actionNamePattern = regexp.MustCompile(`^[A-Za-z0-9_-]{1,128}$`)
	actionDataPattern = regexp.MustCompile(`^[A-Za-z0-9_-]+$`)
	identifierPattern = regexp.MustCompile(`^[A-Za-z0-9_]{1,64}$`)
)

// resolveNative executes the authenticated upstream handshake sequence:
//  1. Construct and send cryptographic challenge using the embed session token.
//  2. Transform challenge response into a catalog ticket.
//  3. Fetch and decrypt the available server list.
//  4. For each available server, post its unlock payload and decrypt stream candidate details.
//  5. Normalize candidates, verify content TMDB IDs, and attempt master playlist promotion.
func (a *Adapter) resolveNative(
	ctx context.Context,
	proto *protocol,
	embed Embed,
	referer string,
	jar *cookiejar.Jar,
	title model.Title,
) (model.AdapterResult, error) {
	result := model.AdapterResult{}

	fetchContext := model.FetchContext{
		Scopes: []model.FetchScope{
			{
				Origin:     origin,
				PathPrefix: proto.Base,
				Headers: map[string]string{
					"User-Agent":      userAgent,
					"Accept":          "*/*",
					"Accept-Language": "en-US,en;q=0.9",
					"Origin":          origin,
					"Referer":         referer,
				},
			},
		},
	}

	postProtocolAction := func(action, data string) (network.Response, error) {
		if len(data) == 0 || len(data) > 128*1024 || !actionNamePattern.MatchString(action) || !actionDataPattern.MatchString(data) {
			return network.Response{}, protocolError("invalid encoded protocol path")
		}

		targetURL := origin + proto.Base + "/" + action + "/" + data
		resp, err := a.client.Fetch(ctx, network.Request{
			URL:     targetURL,
			Method:  "POST",
			Context: fetchContext,
			Jar:     jar,
			Limit:   2_500_000,
		})
		if err != nil {
			return resp, upstreamError(ctx, action, err)
		}

		if resp.Status != http.StatusOK {
			return resp, model.Fail(model.UpstreamFailure, "protocol_post", fmt.Sprintf("provider HTTP %d", resp.Status))
		}

		return resp, nil
	}

	// Step 1: Challenge generation and submission.
	nonce, err := newNonce()
	if err != nil {
		return result, model.Fail(model.Unavailable, "challenge", "secure randomness unavailable")
	}

	challengeStr, err := proto.challenge(embed.Token, time.Now(), nonce)
	if err != nil {
		return result, err
	}

	challengeResp, err := postProtocolAction(proto.ChallengeAction, challengeStr)
	if err != nil {
		return result, err
	}

	// Step 2: Derive server-list ticket from challenge response.
	ticketStr, err := proto.ticket(challengeResp.Body)
	if err != nil {
		return result, err
	}

	// Step 3: Fetch and decrypt server list.
	listResp, err := postProtocolAction(proto.ListAction, ticketStr)
	if err != nil {
		return result, err
	}

	decryptedList, err := proto.decodeResponse(listResp.Body)
	if err != nil {
		return result, err
	}

	var servers []serverRecord
	if err := json.Unmarshal(decryptedList, &servers); err != nil || len(servers) > 32 {
		return result, protocolError("invalid decoded server list")
	}

	if len(servers) == 0 {
		return result, model.Fail(model.Unavailable, "catalog", "no servers for requested title")
	}

	// Validate the entire list before starting independent server unlocks.
	seenServers := make(map[string]bool)
	for _, server := range servers {
		if server.Name == "" || seenServers[server.Name] || len(server.Name) > 128 {
			return model.AdapterResult{}, protocolError("invalid or duplicate server identity")
		}
		seenServers[server.Name] = true
	}
	type unlockOutcome struct {
		candidate model.Candidate
		plaintext []byte
		err       error
		fatal     bool
	}
	outcomes := make([]unlockOutcome, len(servers))
	var cursor atomic.Int32
	var workers sync.WaitGroup
	for workerIndex := 0; workerIndex < min(3, len(servers)); workerIndex++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			for {
				index := int(cursor.Add(1)) - 1
				if index >= len(servers) || ctx.Err() != nil {
					return
				}
				server := servers[index]
				if server.Data == "" {
					continue
				}
				unlockResp, unlockErr := postProtocolAction(proto.UnlockAction, server.Data)
				if unlockErr != nil {
					outcomes[index].err = unlockErr
					continue
				}
				plaintext, decodeErr := proto.decodeResponse(unlockResp.Body)
				if decodeErr != nil {
					outcomes[index].err = decodeErr
					outcomes[index].fatal = true
					continue
				}
				candidate, normalizeErr := normalizeSource(plaintext, title, server.Name)
				if normalizeErr != nil {
					outcomes[index].err = normalizeErr
					outcomes[index].fatal = true
					continue
				}
				outcomes[index].plaintext = plaintext
				outcomes[index].candidate = a.promoteMaster(ctx, candidate)
			}
		}()
	}
	workers.Wait()
	if err := ctx.Err(); err != nil {
		return model.AdapterResult{}, err
	}
	// Preserve upstream order and invoke diagnostic observers serially. They
	// never run concurrently and cannot race with another server in this lookup.
	for _, outcome := range outcomes {
		if outcome.fatal {
			return model.AdapterResult{}, outcome.err
		}
		if outcome.err != nil {
			result.Issues = append(result.Issues, model.Failure{
				Category: model.Category(outcome.err),
				Stage:    "unlock",
				Detail:   "server unlock failed",
			})
			continue
		}
		if outcome.candidate.ID == "" {
			continue
		}
		if a.schemaObserver != nil {
			a.observeSchema(outcome.plaintext)
		}
		result.Candidates = append(result.Candidates, outcome.candidate)
	}

	if len(result.Candidates) == 0 {
		return result, model.Fail(model.Unavailable, "unlock", "no usable sources unlocked")
	}

	return result, nil
}

// observeSchema inspects top-level and nested track fields for offline protocol documentation.
func (a *Adapter) observeSchema(plaintext []byte) {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(plaintext, &fields); err != nil {
		return
	}

	var names []string
	for name := range fields {
		if identifierPattern.MatchString(name) {
			names = append(names, name)

			if name == "tracks" {
				var tracks []map[string]json.RawMessage
				if err := json.Unmarshal(fields[name], &tracks); err == nil {
					trackKeys := make(map[string]bool)
					for _, track := range tracks {
						for k := range track {
							if identifierPattern.MatchString(k) {
								trackKeys[k] = true
							}
						}
					}
					for k := range trackKeys {
						names = append(names, "tracks[]."+k)
					}
				}
			}
		}
	}

	sort.Strings(names)
	a.schemaObserver(names)
}

// normalizeSource validates decrypted JSON fields and transforms them into a model.Candidate.
func normalizeSource(plaintext []byte, title model.Title, serverName string) (model.Candidate, error) {
	var wire struct {
		URL        string            `json:"url"`
		Headers    map[string]string `json:"headers"`
		NoReferrer *bool             `json:"noReferrer"`
		ExpiresAt  *string           `json:"expiresAt"`
		Cookies    string            `json:"cookies"`
		CookiePath string            `json:"cookiePath"`
	}

	if err := json.Unmarshal(plaintext, &wire); err != nil || wire.URL == "" {
		return model.Candidate{}, protocolError("decoded media URL missing")
	}

	parsedURL, err := url.Parse(wire.URL)
	if err != nil || network.ValidateURL(parsedURL) != nil {
		return model.Candidate{}, protocolError("decoded media destination denied")
	}

	publicResources := &model.PublicResourceContext{
		Origin:  origin,
		Referer: origin + "/",
	}
	headers := map[string]string{
		"Referer": origin + "/",
		"Origin":  origin,
	}
	for name, value := range wire.Headers {
		headers[http.CanonicalHeaderKey(name)] = value
	}
	if wire.NoReferrer != nil && *wire.NoReferrer {
		delete(headers, "Referer")
		publicResources.Referer = ""
	}

	format := model.UnknownFormat
	switch strings.ToLower(path.Ext(parsedURL.Path)) {
	case ".m3u8":
		format = model.HLS
	case ".mp4":
		format = model.MP4
	case ".mpd":
		format = model.DASH
	}

	urlDigest := sha256.Sum256([]byte(wire.URL))
	candidate := model.Candidate{
		ID:       fmt.Sprintf("%s:%x", serverName, urlDigest[:12]),
		Provider: model.VidCore,
		Server:   serverName,
		Title:    title,
		URL:      wire.URL,
		Format:   format,
		Context: model.FetchContext{
			PublicResources: publicResources,
			Scopes: []model.FetchScope{
				{
					Origin:  network.Origin(parsedURL),
					Headers: headers,
				},
			},
		},
	}

	if wire.ExpiresAt != nil {
		expiryTime, err := time.Parse(time.RFC3339, *wire.ExpiresAt)
		if err != nil {
			return model.Candidate{}, protocolError("invalid explicit source expiration")
		}
		candidate.ExpiresAt = &expiryTime
	}

	if wire.Cookies != "" {
		if wire.CookiePath == "" {
			return model.Candidate{}, protocolError("source cookies lack explicit path scope")
		}
		candidate.Context.Scopes[0].Cookies = wire.Cookies
		candidate.Context.Scopes[0].CookiePath = wire.CookiePath
	}

	var rawAttributes map[string]json.RawMessage
	if err := json.Unmarshal(plaintext, &rawAttributes); err != nil {
		return model.Candidate{}, protocolError("invalid source attributes")
	}

	delete(rawAttributes, "url")
	delete(rawAttributes, "headers")
	delete(rawAttributes, "cookies")
	delete(rawAttributes, "cookiePath")

	// Validate title ID match if provided by upstream.
	if rawTMDB, exists := rawAttributes["tmdbId"]; exists && string(rawTMDB) != "null" {
		var tmdbID int64
		if err := json.Unmarshal(rawTMDB, &tmdbID); err != nil {
			var textVal string
			if err := json.Unmarshal(rawTMDB, &textVal); err != nil {
				return model.Candidate{}, protocolError("invalid source title identifier")
			}
			parsedID, parseErr := strconv.ParseInt(textVal, 10, 64)
			if parseErr != nil {
				return model.Candidate{}, protocolError("invalid source title identifier")
			}
			tmdbID = parsedID
		}

		if tmdbID != title.TMDBID {
			return model.Candidate{}, protocolError("source title identifier mismatch")
		}
	}

	// Observed tracks use file/label fields. Only WebVTT destinations have a known
	// subtitle representation; retain other payloads without guessing their type.
	if rawTracks, present := rawAttributes["tracks"]; present && string(rawTracks) != "null" {
		var tracks []struct {
			File  string `json:"file"`
			Label string `json:"label"`
		}
		if err := json.Unmarshal(rawTracks, &tracks); err != nil || len(tracks) > 64 {
			return model.Candidate{}, protocolError("invalid source tracks")
		}
		for _, track := range tracks {
			trackURL, err := url.Parse(track.File)
			if err != nil || network.ValidateURL(trackURL) != nil || !strings.EqualFold(path.Ext(trackURL.Path), ".vtt") {
				continue
			}
			language := ""
			switch strings.ToLower(track.Label) {
			case "english", "eng", "en":
				language = "en"
			case "japanese", "ja", "jpn":
				language = "ja"
			}
			candidate.Metadata.Tracks = append(candidate.Metadata.Tracks, model.Track{
				Kind:     "subtitles",
				Language: language,
				Name:     track.Label,
				URL:      trackURL.String(),
			})
		}
	}

	candidate.Metadata.ProviderData = rawAttributes
	return candidate, nil
}

// promoteMaster checks whether Supreme or Prime /sd/ stream URLs can be safely upgraded to master playlists.
// If master resolution fails, the proven original single-rendition source is left intact.
func (a *Adapter) promoteMaster(ctx context.Context, candidate model.Candidate) model.Candidate {
	if candidate.Server != "Supreme" && candidate.Server != "Prime" {
		return candidate
	}

	streamURL, err := url.Parse(candidate.URL)
	if err != nil {
		return candidate
	}

	sdIndex := strings.Index(streamURL.Path, "/sd/")
	if sdIndex < 0 {
		return candidate
	}

	streamURL.Path = streamURL.Path[:sdIndex] + "/master.m3u8"
	streamURL.RawPath = ""

	resp, err := a.client.Fetch(ctx, network.Request{
		URL:     streamURL.String(),
		Context: candidate.Context,
		Limit:   256 * 1024,
	})
	if err != nil || resp.Status != http.StatusOK || !strings.HasPrefix(string(resp.Body), "#EXTM3U") {
		return candidate
	}

	candidate.URL = streamURL.String()
	candidate.Format = model.HLS
	return candidate
}
