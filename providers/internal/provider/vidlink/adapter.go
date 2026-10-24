// Package vidlink implements the statically traced VidLink protocol in native Go.
package vidlink

import (
	"context"
	"fmt"
	"time"

	"stv-providers/internal/cache"
	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/provider"
)

// fetcher is consumed here to allow deterministic offline protocol fixtures.
type fetcher interface {
	Fetch(context.Context, network.Request) (network.Response, error)
}

// Adapter resolves upstream candidates without choosing a playback winner.
type Adapter struct {
	players cache.Values[bool]
	client  fetcher
}

var _ provider.Adapter = (*Adapter)(nil)

// New binds resolution to the existing controlled network client.
func New(client *network.Client) *Adapter {
	adapter := &Adapter{}
	if client != nil {
		adapter.client = client
	}
	return adapter
}

// ID identifies this provider independently of upstream mirror/server identities.
func (*Adapter) ID() model.ProviderID {
	return model.VidLink
}

// Capabilities describes the traced movie/TV path. The separate MAL anime path
// is not mapped; anime titles can currently use their TMDB TV coordinates only.
func (*Adapter) Capabilities() provider.Capabilities {
	return provider.Capabilities{
		MediaTypes: []model.MediaType{
			model.Movie,
			model.TV,
		},
		Identifiers: []string{"tmdb"},
	}
}

// Resolve validates the shared input and preserves cancellation/error causes.
// The caller's deadline and the provider window bound all upstream work.
func (a *Adapter) Resolve(
	ctx context.Context,
	lookup model.Lookup,
) (result model.AdapterResult, err error) {
	startedAt := time.Now()
	defer func() {
		result.Duration = time.Since(startedAt)
	}()

	if ctx == nil {
		return result, model.Fail(model.Unsupported, "context", "nil context provided")
	}
	if err := ctx.Err(); err != nil {
		return result, fmt.Errorf("resolve VidLink: %w", err)
	}

	if err := lookup.Title.Validate(); err != nil {
		return result, fmt.Errorf(
			"validate VidLink lookup: %w: %w",
			model.Fail(model.Unsupported, "input", "invalid title identifiers or episode coordinates"),
			err,
		)
	}

	if a.client == nil {
		return result, model.Fail(model.Unavailable, "network", "VPN-controlled network client required")
	}

	lookupContext, cancel := context.WithTimeout(ctx, 18*time.Second)
	defer cancel()

	return a.resolve(lookupContext, lookup.Title)
}
