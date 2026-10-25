// Package config bootstraps and coordinates service components into a unified resolution engine.
package config

import (
	"sync"

	"stv-providers/internal/model"
	"stv-providers/internal/network"
	"stv-providers/internal/probe"
	"stv-providers/internal/provider"
	"stv-providers/internal/provider/vidcore"
	"stv-providers/internal/provider/videasy"
	"stv-providers/internal/provider/vidfast"
	"stv-providers/internal/provider/vidlink"
	"stv-providers/internal/provider/vidsrc"
	"stv-providers/internal/search"
)

// NewEngine wires all native providers into the registry
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
		videasy.New(client),
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

	var once sync.Once
	cleanup := func() {
		once.Do(func() {
			coordinator.Close()
			client.Close()
		})
	}

	return coordinator, cleanup, nil
}
