package provider

import (
	"context"
	"family-stream-providers/internal/model"
	"fmt"
)

// Registry manages the set of active provider adapters for coordinated search.
type Registry struct {
	adapters []Adapter
}

// NewRegistry creates a provider registry using explicit injection to avoid import cycles.
// Any unimplemented planned providers default to a stub that returns an explicit unsupported failure.
func NewRegistry(adapters ...Adapter) (*Registry, error) {
	registered := make(map[model.ProviderID]Adapter)

	for _, adapter := range adapters {
		if adapter == nil {
			return nil, fmt.Errorf("nil adapter provided")
		}

		isKnown := false
		for _, plannedID := range model.PlannedProviders {
			if adapter.ID() == plannedID {
				isKnown = true
				break
			}
		}

		if !isKnown || registered[adapter.ID()] != nil {
			return nil, fmt.Errorf("unknown or duplicate adapter: %s", adapter.ID())
		}

		registered[adapter.ID()] = adapter
	}

	registry := &Registry{}
	for _, plannedID := range model.PlannedProviders {
		adapter := registered[plannedID]
		if adapter == nil {
			adapter = absentAdapter{id: plannedID}
		}
		registry.adapters = append(registry.adapters, adapter)
	}

	return registry, nil
}

// Adapters returns a slice of all registered and stubbed adapters.
func (r *Registry) Adapters() []Adapter {
	return append([]Adapter(nil), r.adapters...)
}

// absentAdapter represents a planned provider whose implementation is not yet installed.
type absentAdapter struct {
	id model.ProviderID
}

func (a absentAdapter) ID() model.ProviderID {
	return a.id
}

func (a absentAdapter) Capabilities() Capabilities {
	return Capabilities{}
}

func (a absentAdapter) Resolve(ctx context.Context, _ model.Lookup) (model.AdapterResult, error) {
	if err := ctx.Err(); err != nil {
		return model.AdapterResult{}, err
	}
	return model.AdapterResult{}, model.Fail(model.Unsupported, "adapter", "provider implementation not installed")
}
