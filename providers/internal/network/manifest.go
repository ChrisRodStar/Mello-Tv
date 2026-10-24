package network

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"stv-providers/internal/cache"
	"stv-providers/internal/model"
)

type manifestCacheKey struct{}

type manifestEntry struct {
	response Response
	expires  time.Time
}

type manifestCache struct {
	mutex   sync.Mutex
	entries map[[32]byte]manifestEntry
	bytes   int
}

// WithManifestCache shares bounded, short-lived manifest bodies only within one
// resolution round. Session jars, media samples, and failures are never cached.
func WithManifestCache(ctx context.Context) context.Context {
	return context.WithValue(ctx, manifestCacheKey{}, &manifestCache{
		entries: make(map[[32]byte]manifestEntry),
	})
}

// Fetch retains the guarded transport for every miss and checks current VPN
// health on hits too. Cache identity includes the complete credential context.
func (c *Client) Fetch(ctx context.Context, request Request) (Response, error) {
	if c == nil {
		return Response{}, errors.New("nil network client")
	}

	roundCache, _ := ctx.Value(manifestCacheKey{}).(*manifestCache)
	parsedURL, err := url.Parse(request.URL)
	eligible := roundCache != nil && err == nil && ValidateURL(parsedURL) == nil &&
		strings.HasSuffix(strings.ToLower(parsedURL.Path), ".m3u8") &&
		(request.Method == "" || request.Method == http.MethodGet) &&
		request.Jar == nil && !request.Sample && len(request.Body) == 0
	if !eligible {
		return c.fetch(ctx, request)
	}

	identity, err := json.Marshal(struct {
		URL     string
		Context model.FetchContext
		Headers map[string]string
	}{
		URL:     request.URL,
		Context: request.Context,
		Headers: request.Headers,
	})
	if err != nil {
		return c.fetch(ctx, request)
	}

	key := sha256.Sum256(identity)
	limit := request.Limit
	if limit <= 0 || limit > c.maxBytes {
		limit = c.maxBytes
	}

	roundCache.mutex.Lock()
	entry, found := roundCache.entries[key]
	roundCache.mutex.Unlock()

	if found && time.Now().Before(entry.expires) && int64(len(entry.response.Body)) <= limit {
		if err := ctx.Err(); err != nil {
			return Response{}, err
		}
		if c.guard == nil {
			return Response{}, errors.New("VPN route unavailable")
		}
		if err := c.guard.Check(ctx); err != nil {
			return Response{}, errors.New("VPN route unavailable")
		}
		return cloneResponse(entry.response), nil
	}

	response, err := c.fetch(ctx, request)
	if err != nil || response.Status != http.StatusOK ||
		!bytes.HasPrefix(response.Body, []byte("#EXTM3U")) ||
		len(response.Header.Values("Set-Cookie")) != 0 ||
		strings.Contains(strings.ToLower(response.Header.Get("Cache-Control")), "no-store") {
		return response, err
	}

	// Live media playlists may advance; keep reuse below two seconds. Masters
	// are reusable for this round, which itself has a fixed overall deadline.
	lifetime := 2 * time.Second
	if bytes.Contains(response.Body, []byte("#EXT-X-STREAM-INF:")) {
		lifetime = 30 * time.Second
	}
	lifetime = cache.AssetLifetime(response.Header, lifetime)
	if lifetime <= 0 {
		return response, nil
	}

	cloned := cloneResponse(response)
	roundCache.mutex.Lock()
	defer roundCache.mutex.Unlock()

	now := time.Now()
	// Purge expired entries to keep memory and entry count strictly bounded.
	for k, e := range roundCache.entries {
		if !now.Before(e.expires) {
			roundCache.bytes -= len(e.response.Body)
			delete(roundCache.entries, k)
		}
	}

	if previous, exists := roundCache.entries[key]; exists {
		roundCache.bytes -= len(previous.response.Body)
		delete(roundCache.entries, key)
	}

	// Hard caps prevent signed URLs or large manifests accumulating in a round.
	if len(roundCache.entries) < 16 && roundCache.bytes+len(cloned.Body) <= 2*1024*1024 {
		roundCache.entries[key] = manifestEntry{response: cloned, expires: now.Add(lifetime)}
		roundCache.bytes += len(cloned.Body)
	}

	return response, nil
}

func cloneResponse(response Response) Response {
	cloned := response
	cloned.Header = response.Header.Clone()
	cloned.Body = bytes.Clone(response.Body)
	if response.URL != nil {
		copiedURL := *response.URL
		cloned.URL = &copiedURL
	}
	return cloned
}
