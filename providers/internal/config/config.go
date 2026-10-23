// Package config bootstraps and coordinates service components into a unified resolution engine.
package config

import (
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/probe"
	"family-stream-providers/internal/provider"
	"family-stream-providers/internal/provider/vidcore"
	"family-stream-providers/internal/provider/vidfast"
	"family-stream-providers/internal/provider/vidlink"
	"family-stream-providers/internal/provider/vidsrc"
	"family-stream-providers/internal/search"
)

// NewEngine wires all four native providers into the registry
// and initializes network clients, HLS stream probers, and the search coordinator.
//
// Note: Deployment must establish and attest a fail-closed Mullvad VPN network before supplying options.
func NewEngine(options network.Options, diagnostic func(model.ProviderStatus)) (*search.Coordinator, func(), error) {
	client, err := network.New(options)
	if err != nil {
		return nil, nil, err
	}

	registry, err := provider.NewRegistry(
		vidcore.New(client),
		vidlink.New(client),
		vidsrc.New(client),
		vidfast.New(client),
	)
	if err != nil {
		client.Close()
		return nil, nil, err
	}

	prober, err := probe.New(client, probe.DefaultLimits())
	if err != nil {
		client.Close()
		return nil, nil, err
	}

	coordinator, err := search.New(registry, prober, search.DefaultLimits(), diagnostic)
	if err != nil {
		client.Close()
		return nil, nil, err
	}

	cleanup := func() {
		coordinator.Close()
		client.Close()
	}

	return coordinator, cleanup, nil
}
