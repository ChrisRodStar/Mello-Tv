// Package vidfast implements the statically recognized vidfast.vc movie/TV protocol.
package vidfast

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

// Adapter returns native sources without executing downloaded player code.
type Adapter struct {
	materials cache.Values[protocolMaterial]
	client    fetcher
	// Package-private injection permits synthetic offline bundle fixtures.
	// New always installs the production fingerprint verifier.
	recognize func([]byte) (protocolMaterial, error)
}

var _ provider.Adapter = (*Adapter)(nil)

// New requires the shared VPN-controlled client, including redirect handling.
func New(client *network.Client) *Adapter {
	adapter := &Adapter{
		recognize: recognizePlayer,
	}
	if client != nil {
		adapter.client = client
	}
	return adapter
}

func (*Adapter) ID() model.ProviderID {
	return model.VidFast
}

// Capabilities describes the canonical identifier routes.
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
		return result, fmt.Errorf("resolve VidFast: %w", err)
	}
	if err := lookup.Title.Validate(); err != nil {
		return result, model.Fail(model.Unsupported, "input", "invalid title identifiers or episode coordinates")
	}
	if a.client == nil {
		return result, model.Fail(model.Unavailable, "network", "VPN-controlled network client required")
	}

	lookupContext, cancel := context.WithTimeout(ctx, 16*time.Second)
	defer cancel()

	return a.resolve(lookupContext, lookup.Title)
}
