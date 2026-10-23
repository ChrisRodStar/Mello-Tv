// Package vidsrc implements the statically traced vidsrc.me -> vidsrc.sh protocol.
package vidsrc

import (
	"context"
	"fmt"
	"time"

	"family-stream-providers/internal/cache"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/provider"
)

type fetcher interface {
	Fetch(context.Context, network.Request) (network.Response, error)
}

// Adapter returns sources to shared probing and selection without ranking them.
type Adapter struct {
	decryptors cache.Values[[]byte]
	keys       cache.Values[[]byte]
	client     fetcher
}

var _ provider.Adapter = (*Adapter)(nil)

// New requires the controlled client for every nested request.
func New(client *network.Client) *Adapter {
	adapter := &Adapter{}
	if client != nil {
		adapter.client = client
	}
	return adapter
}

func (*Adapter) ID() model.ProviderID {
	return model.VidSrc
}

func (*Adapter) Capabilities() provider.Capabilities {
	return provider.Capabilities{
		MediaTypes:  []model.MediaType{model.Movie, model.TV},
		Identifiers: []string{"tmdb"},
	}
}

func (a *Adapter) Resolve(ctx context.Context, lookup model.Lookup) (result model.AdapterResult, err error) {
	startedAt := time.Now()
	defer func() {
		result.Duration = time.Since(startedAt)
	}()

	if err := ctx.Err(); err != nil {
		return result, fmt.Errorf("resolve VidSrc: %w", err)
	}
	if err := lookup.Title.Validate(); err != nil {
		return result, fmt.Errorf("validate VidSrc lookup: %w: %w",
			model.Fail(model.Unsupported, "input", "invalid title identifiers or episode coordinates"), err)
	}
	if a.client == nil {
		return result, model.Fail(model.Unavailable, "network", "VPN-controlled network client required")
	}

	lookupContext, cancel := context.WithTimeout(ctx, 18*time.Second)
	defer cancel()
	return a.resolve(lookupContext, lookup.Title)
}
