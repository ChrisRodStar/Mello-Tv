package provider

import (
	"context"
	"fmt"
	"slices"

	"stv-providers/internal/model"
)

// Registry manages the set of active provider adapters for coordinated search.
// It is immutable after creation and safe for concurrent use across multiple goroutines.
type Registry struct {
	adapters []Adapter
}

// NewRegistry creates a provider registry using explicit injection to avoid import cycles.
// Any unimplemented planned providers default to a stub that returns an explicit unsupported failure.
func NewRegistry(adapters ...Adapter) (*Registry, error) {
	registered := make(map[model.ProviderID]Adapter, len(adapters))

	for _, adapter := range adapters {
		if adapter == nil {
			return nil, fmt.Errorf("nil adapter provided")
		}

		id := adapter.ID()
		if !slices.Contains(model.PlannedProviders[:], id) {
			return nil, fmt.Errorf("unknown adapter: %s", id)
		}
		if registered[id] != nil {
			return nil, fmt.Errorf("duplicate adapter: %s", id)
		}

		registered[id] = adapter
	}

	registry := &Registry{
		adapters: make([]Adapter, 0, len(model.PlannedProviders)),
	}
	for _, plannedID := range model.PlannedProviders {
		adapter := registered[plannedID]
		if adapter == nil {
			adapter = absentAdapter{id: plannedID}
		}
		registry.adapters = append(registry.adapters, adapter)
	}

	return registry, nil
}

// Adapters returns a slice copy of all registered and stubbed adapters in planned order.
func (r *Registry) Adapters() []Adapter {
	if r == nil {
		return nil
	}
	return slices.Clone(r.adapters)
}

// Get returns the adapter registered for the given provider ID, or false if absent.
func (r *Registry) Get(id model.ProviderID) (Adapter, bool) {
	if r == nil {
		return nil, false
	}
	for _, adapter := range r.adapters {
		if adapter.ID() == id {
			return adapter, true
		}
	}
	return nil, false
}

// Len returns the total number of registered and stubbed adapters in the registry.
func (r *Registry) Len() int {
	if r == nil {
		return 0
	}
	return len(r.adapters)
}

// InstalledCount returns the number of active, implemented adapters in the registry.
func (r *Registry) InstalledCount() int {
	if r == nil {
		return 0
	}
	count := 0
	for _, adapter := range r.adapters {
		if IsInstalled(adapter) {
			count++
		}
	}
	return count
}

// IsInstalled reports whether the adapter is an active implementation rather than an absent stub.
func IsInstalled(adapter Adapter) bool {
	if adapter == nil {
		return false
	}
	_, absent := adapter.(absentAdapter)
	return !absent
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
	if ctx == nil {
		return model.AdapterResult{}, model.Fail(model.Unsupported, "context", "nil context provided")
	}
	if err := ctx.Err(); err != nil {
		return model.AdapterResult{}, err
	}
	return model.AdapterResult{}, model.Fail(model.Unsupported, "adapter", "provider implementation not installed")
}
