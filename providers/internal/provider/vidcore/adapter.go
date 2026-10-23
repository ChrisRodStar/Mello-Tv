package vidcore

import (
	"context"
	"family-stream-providers/internal/cache"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/provider"
	"strconv"
	"time"
)

type protocolClient interface {
	Fetch(context.Context, network.Request) (network.Response, error)
}

// Adapter implements the provider.Adapter interface for VidCore.
type Adapter struct {
	protocols      cache.Values[*protocol]
	client         protocolClient
	schemaObserver func([]string)
}

// New creates a new VidCore adapter instance bound to the provided network client.
func New(client *network.Client) *Adapter {
	adapter := &Adapter{}
	if client != nil {
		adapter.client = client
	}
	return adapter
}

// ID returns the ProviderID for VidCore.
func (*Adapter) ID() model.ProviderID {
	return model.VidCore
}

// Capabilities returns the media formats and identifier schemas supported by VidCore.
func (*Adapter) Capabilities() provider.Capabilities {
	return provider.Capabilities{
		MediaTypes:  []model.MediaType{model.Movie, model.TV},
		Identifiers: []string{"tmdb"},
	}
}

// embedPath builds the VidCore embed URL path from title coordinates.
func embedPath(title model.Title) (string, error) {
	if err := title.Validate(); err != nil {
		return "", model.Fail(model.Unsupported, "input", "invalid title identifiers or episode coordinates")
	}

	tmdbIDStr := strconv.FormatInt(title.TMDBID, 10)
	if title.Type == model.TV {
		return "/tv/" + tmdbIDStr + "/" + strconv.Itoa(*title.Season) + "/" + strconv.Itoa(*title.Episode), nil
	}

	return "/movie/" + tmdbIDStr, nil
}

// Resolve queries VidCore for playable stream sources matching the requested title.
func (a *Adapter) Resolve(ctx context.Context, lookup model.Lookup) (result model.AdapterResult, err error) {
	startTime := time.Now()
	defer func() {
		result.Duration = time.Since(startTime)
	}()

	path, err := embedPath(lookup.Title)
	if err != nil {
		return result, err
	}

	if a.client == nil {
		return result, model.Fail(model.Unavailable, "network", "VPN-controlled network client required")
	}

	lookupCtx, cancel := context.WithTimeout(ctx, 18*time.Second)
	defer cancel()

	return a.resolve(lookupCtx, path, lookup.Title)
}

// NewWithSchemaObserver creates an adapter with an observer hook for research on newly seen payload fields.
func NewWithSchemaObserver(client *network.Client, observer func([]string)) *Adapter {
	adapter := New(client)
	adapter.schemaObserver = observer
	return adapter
}
