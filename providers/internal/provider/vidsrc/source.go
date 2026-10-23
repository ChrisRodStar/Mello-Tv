package vidsrc

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"net/url"
	"path"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
)

func (a *Adapter) sources(ctx context.Context, body []byte, title model.Title) (model.AdapterResult, error) {
	var envelope struct {
		StatusCode string `json:"status_code"`
		Data       struct {
			IMDbID  string          `json:"imdb_id"`
			Season  *json.Number    `json:"season"`
			Episode *json.Number    `json:"episode"`
			Streams json.RawMessage `json:"stream_urls"`
		} `json:"data"`
		VS json.RawMessage `json:"vs"`
	}
	if json.Unmarshal(body, &envelope) != nil {
		return model.AdapterResult{}, protocolError("sources", "malformed stream envelope")
	}
	switch envelope.StatusCode {
	case "200":
	case "404", "410":
		return model.AdapterResult{}, model.Fail(model.Unavailable, "sources", "stream API returned no usable result")
	case "401", "403", "429", "500", "503":
		return model.AdapterResult{}, model.Fail(model.UpstreamFailure, "sources", "stream API rejected the lookup")
	default:
		return model.AdapterResult{}, protocolError("sources", "unrecognized stream API status")
	}
	if title.IMDbID != "" && envelope.Data.IMDbID != "" && title.IMDbID != envelope.Data.IMDbID {
		return model.AdapterResult{}, protocolError("sources", "source IMDb cross-ID mismatch")
	}
	if title.Type == model.TV {
		if envelope.Data.Season == nil || envelope.Data.Episode == nil || envelope.Data.Season.String() != strconv.Itoa(*title.Season) || envelope.Data.Episode.String() != strconv.Itoa(*title.Episode) {
			return model.AdapterResult{}, protocolError("sources", "source episode coordinates mismatch")
		}
	} else if envelope.Data.Season != nil || envelope.Data.Episode != nil {
		return model.AdapterResult{}, protocolError("sources", "movie source carries episode coordinates")
	}

	var streams []string
	if len(envelope.Data.Streams) == 0 || string(envelope.Data.Streams) == "null" {
		return model.AdapterResult{}, model.Fail(model.Unavailable, "sources", "stream list empty")
	}
	if envelope.Data.Streams[0] == '"' {
		var encoded string
		if json.Unmarshal(envelope.Data.Streams, &encoded) != nil {
			return model.AdapterResult{}, protocolError("sources", "encrypted stream list malformed")
		}
		var err error
		streams, err = a.decrypt(ctx, encoded, envelope.VS)
		if err != nil {
			return model.AdapterResult{}, err
		}
	} else if json.Unmarshal(envelope.Data.Streams, &streams) != nil {
		return model.AdapterResult{}, protocolError("sources", "unrecognized stream list schema")
	}
	if len(streams) == 0 {
		return model.AdapterResult{}, model.Fail(model.Unavailable, "sources", "stream list empty")
	}
	if len(streams) > 16 {
		return model.AdapterResult{}, protocolError("sources", "stream count outside supported bounds")
	}

	var metadata map[string]json.RawMessage
	if json.Unmarshal(body, &metadata) != nil {
		return model.AdapterResult{}, protocolError("sources", "source metadata malformed")
	}
	// Keep reported subtitle/protection/expiry fields as opaque provider metadata.
	// There is no evidence that these establish playable tracks or an absolute TTL.
	result := model.AdapterResult{}
	// Optional enrichment has both a total window and a per-host timeout. Keep
	// a return margin before the provider deadline so usable earlier sources are
	// handed back to the coordinator instead of being erased by a later stall.
	tokenDeadline := time.Now().Add(3 * time.Second)
	if deadline, found := ctx.Deadline(); found && deadline.Add(-250*time.Millisecond).Before(tokenDeadline) {
		tokenDeadline = deadline.Add(-250 * time.Millisecond)
	}
	tokenContext, cancelTokens := context.WithDeadline(ctx, tokenDeadline)
	defer cancelTokens()

	tokenResults := a.hostTokens(tokenContext, streams)
	tokens := make(map[string]string)
	seen := make(map[string]bool)
	for _, rawURL := range streams {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		if seen[rawURL] {
			continue
		}
		seen[rawURL] = true
		resourceURL, err := url.Parse(rawURL)
		if err != nil || network.ValidateURL(resourceURL) != nil || resourceURL.Scheme != "https" || resourceURL.Fragment != "" || len(rawURL) > 8192 {
			result.Issues = append(result.Issues, model.Failure{
				Category: model.ProtocolChange,
				Stage:    "sources",
				Detail:   "source destination rejected",
			})
			continue
		}
		resourceOrigin := network.Origin(resourceURL)
		token, found := tokens[resourceOrigin]
		if !found {
			outcome := tokenResults[resourceOrigin]
			token = outcome.token
			if outcome.err != nil {
				if ctx.Err() != nil {
					return result, ctx.Err()
				}
				result.Issues = append(result.Issues, model.Failure{
					Category: model.Category(outcome.err),
					Stage:    "media_token",
					Detail:   "optional media host token unavailable",
				})
			}
			tokens[resourceOrigin] = token
		}
		if strings.Contains(rawURL, "__TOKEN__") {
			if token == "" {
				continue
			}
			rawURL = strings.ReplaceAll(rawURL, "__TOKEN__", url.QueryEscape(token))
		} else if token != "" {
			separator := "?"
			if strings.Contains(rawURL, "?") {
				separator = "&"
			}
			rawURL += separator + "token=" + url.QueryEscape(token)
		}
		resourceURL, err = url.Parse(rawURL)
		if err != nil || network.ValidateURL(resourceURL) != nil || network.Origin(resourceURL) != resourceOrigin {
			result.Issues = append(result.Issues, model.Failure{
				Category: model.ProtocolChange,
				Stage:    "sources",
				Detail:   "tokenized source destination rejected",
			})
			continue
		}
		format := model.UnknownFormat
		if strings.EqualFold(path.Ext(resourceURL.Path), ".m3u8") {
			format = model.HLS
		} else if strings.EqualFold(path.Ext(resourceURL.Path), ".mp4") {
			format = model.MP4
		}
		digest := sha256.Sum256([]byte(rawURL))
		result.Candidates = append(result.Candidates, model.Candidate{
			ID:       fmt.Sprintf("%x", digest[:12]),
			Provider: model.VidSrc,
			Server:   resourceURL.Hostname(),
			Title:    title,
			URL:      rawURL,
			Format:   format,
			Context: model.FetchContext{
				PublicResources: &model.PublicResourceContext{
					Referer: playerOrigin + "/",
				},
			},
			Metadata: model.Metadata{
				ProviderData: metadata,
			},
		})
	}
	if len(result.Candidates) == 0 {
		return result, protocolError("sources", "all source records rejected")
	}
	return result, nil
}

func parseToken(body []byte) (string, error) {
	text := strings.TrimSpace(string(body))
	if strings.HasPrefix(text, "\"") {
		if json.Unmarshal(body, &text) != nil {
			return "", protocolError("media_token", "malformed media token")
		}
	} else if strings.HasPrefix(text, "{") {
		var fields map[string]json.RawMessage
		if json.Unmarshal(body, &fields) != nil {
			return "", protocolError("media_token", "malformed media token")
		}
		text = ""
		for _, name := range []string{"token", "data", "string", "result"} {
			if len(fields[name]) == 0 {
				continue
			}
			if json.Unmarshal(fields[name], &text) != nil {
				return "", protocolError("media_token", "unrecognized media token schema")
			}
			if text != "" {
				break
			}
		}
	}
	if len(text) > 4096 || strings.ContainsAny(text, "\r\n\x00<>[]{} \t") {
		return "", protocolError("media_token", "invalid media token")
	}
	return text, nil
}

// Optional host-token requests are independent. Keep the existing total and
// per-host deadlines while overlapping up to three distinct origins.
type hostToken struct {
	token string
	err   error
}

func (a *Adapter) hostTokens(ctx context.Context, streams []string) map[string]hostToken {
	seen := make(map[string]bool)
	var origins []string
	for _, rawURL := range streams {
		parsed, err := url.Parse(rawURL)
		if err != nil || network.ValidateURL(parsed) != nil || parsed.Scheme != "https" || parsed.Fragment != "" || len(rawURL) > 8192 {
			continue
		}
		origin := network.Origin(parsed)
		if !seen[origin] {
			seen[origin] = true
			origins = append(origins, origin)
		}
	}
	outcomes := make([]hostToken, len(origins))
	var cursor atomic.Int32
	var workers sync.WaitGroup
	for workerIndex := 0; workerIndex < min(3, len(origins)); workerIndex++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			for {
				index := int(cursor.Add(1)) - 1
				if index >= len(origins) {
					return
				}
				hostContext, cancel := context.WithTimeout(ctx, time.Second)
				fetchErr := hostContext.Err()
				var response network.Response
				if fetchErr == nil {
					response, fetchErr = a.fetch(hostContext, origins[index]+"/generate.php", playerOrigin+"/", "media_token", 8*1024)
				}
				cancel()
				if fetchErr == nil {
					outcomes[index].token, fetchErr = parseToken(response.Body)
				}
				outcomes[index].err = fetchErr
			}
		}()
	}
	workers.Wait()
	results := make(map[string]hostToken, len(origins))
	for index, origin := range origins {
		results[origin] = outcomes[index]
	}
	return results
}
