package provider

import (
	"context"
	"errors"
	"testing"

	"stv-providers/internal/model"
)

type mockAdapter struct {
	id   model.ProviderID
	caps Capabilities
}

func (m mockAdapter) ID() model.ProviderID {
	return m.id
}

func (m mockAdapter) Capabilities() Capabilities {
	return m.caps
}

func (m mockAdapter) Resolve(ctx context.Context, _ model.Lookup) (model.AdapterResult, error) {
	if ctx != nil {
		if err := ctx.Err(); err != nil {
			return model.AdapterResult{}, err
		}
	}
	return model.AdapterResult{}, nil
}

func TestNewRegistryWithPlannedProviders(t *testing.T) {
	vidcore := mockAdapter{id: model.VidCore}
	vidlink := mockAdapter{id: model.VidLink}
	vidsrc := mockAdapter{id: model.VidSrc}
	vidfast := mockAdapter{id: model.VidFast}

	reg, err := NewRegistry(vidcore, vidlink, vidsrc, vidfast)
	if err != nil {
		t.Fatalf("unexpected error creating registry: %v", err)
	}

	adapters := reg.Adapters()
	if len(adapters) != len(model.PlannedProviders) {
		t.Fatalf("got %d adapters, want %d", len(adapters), len(model.PlannedProviders))
	}

	for i, plannedID := range model.PlannedProviders {
		if adapters[i].ID() != plannedID {
			t.Fatalf("position %d got ID %s, want %s", i, adapters[i].ID(), plannedID)
		}
		if !IsInstalled(adapters[i]) {
			t.Fatalf("adapter %s should be marked as installed", plannedID)
		}
	}
}

func TestNewRegistryWithPartialProviders(t *testing.T) {
	vidcore := mockAdapter{id: model.VidCore}
	reg, err := NewRegistry(vidcore)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	adapters := reg.Adapters()
	if len(adapters) != len(model.PlannedProviders) {
		t.Fatalf("got %d adapters, want %d", len(adapters), len(model.PlannedProviders))
	}

	for _, adapter := range adapters {
		if adapter.ID() == model.VidCore {
			if !IsInstalled(adapter) {
				t.Fatal("VidCore should be installed")
			}
		} else {
			if IsInstalled(adapter) {
				t.Fatalf("adapter %s should be an absent stub", adapter.ID())
			}
		}
	}
}

func TestNewRegistryRejections(t *testing.T) {
	t.Run("nil adapter", func(t *testing.T) {
		if _, err := NewRegistry(nil); err == nil {
			t.Fatal("expected error for nil adapter")
		}
	})

	t.Run("duplicate adapter", func(t *testing.T) {
		v1 := mockAdapter{id: model.VidCore}
		v2 := mockAdapter{id: model.VidCore}
		if _, err := NewRegistry(v1, v2); err == nil {
			t.Fatal("expected error for duplicate adapter")
		}
	})

	t.Run("unknown adapter", func(t *testing.T) {
		unknown := mockAdapter{id: model.ProviderID("unknown-provider")}
		if _, err := NewRegistry(unknown); err == nil {
			t.Fatal("expected error for unknown adapter")
		}
	})
}

func TestRegistryGetAndNilSafety(t *testing.T) {
	vidlink := mockAdapter{id: model.VidLink}
	reg, err := NewRegistry(vidlink)
	if err != nil {
		t.Fatal(err)
	}

	adapter, found := reg.Get(model.VidLink)
	if !found || adapter == nil || adapter.ID() != model.VidLink {
		t.Fatal("failed to get registered adapter")
	}

	absent, found := reg.Get(model.VidSrc)
	if !found || absent == nil || IsInstalled(absent) {
		t.Fatal("absent adapter should still be retrieved from registry")
	}

	_, found = reg.Get(model.ProviderID("nonexistent"))
	if found {
		t.Fatal("nonexistent adapter should not be found")
	}

	if reg.Len() != len(model.PlannedProviders) || reg.InstalledCount() != 1 {
		t.Fatalf("expected Len %d and InstalledCount 1, got %d and %d", len(model.PlannedProviders), reg.Len(), reg.InstalledCount())
	}

	var nilReg *Registry
	if nilReg.Adapters() != nil {
		t.Fatal("nil registry Adapters should return nil")
	}
	if _, found := nilReg.Get(model.VidCore); found {
		t.Fatal("nil registry Get should return false")
	}
	if nilReg.Len() != 0 || nilReg.InstalledCount() != 0 {
		t.Fatal("nil registry Len and InstalledCount should return 0")
	}
	if IsInstalled(nil) {
		t.Fatal("nil adapter should not be reported as installed")
	}
}

func TestAbsentAdapterBehavior(t *testing.T) {
	reg, err := NewRegistry()
	if err != nil {
		t.Fatal(err)
	}

	if reg.InstalledCount() != 0 {
		t.Fatal("empty registry should have 0 installed adapters")
	}

	for _, adapter := range reg.Adapters() {
		if IsInstalled(adapter) {
			t.Fatalf("expected stub adapter, got installed for %s", adapter.ID())
		}
		if len(adapter.Capabilities().MediaTypes) != 0 {
			t.Fatal("absent adapter capabilities should be empty")
		}

		res, err := adapter.Resolve(context.Background(), model.Lookup{})
		if err == nil {
			t.Fatal("absent adapter should fail resolve")
		}
		if res.Candidates != nil {
			t.Fatal("absent adapter should return zero result")
		}

		// Nil context safety
		var nilCtx context.Context
		if _, err := adapter.Resolve(nilCtx, model.Lookup{}); err == nil {
			t.Fatal("absent adapter should fail resolve with nil context")
		}

		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if _, err := adapter.Resolve(ctx, model.Lookup{}); !errors.Is(err, context.Canceled) {
			t.Fatalf("expected context.Canceled, got %v", err)
		}
	}
}

func TestCapabilitiesHelpers(t *testing.T) {
	caps := Capabilities{
		MediaTypes:  []model.MediaType{model.Movie, model.TV},
		Identifiers: []string{"tmdb", "imdb"},
	}

	if !caps.SupportsMediaType(model.Movie) || !caps.SupportsMediaType(model.TV) {
		t.Fatal("capabilities should support Movie and TV")
	}
	if caps.SupportsMediaType(model.MediaType("other")) {
		t.Fatal("capabilities should not support other media types")
	}

	if !caps.SupportsIdentifier("tmdb") || !caps.SupportsIdentifier("imdb") {
		t.Fatal("capabilities should support tmdb and imdb")
	}
	if caps.SupportsIdentifier("mal") {
		t.Fatal("capabilities should not support mal")
	}

	cloned := caps.Clone()
	cloned.MediaTypes[0] = model.MediaType("other")
	cloned.Identifiers[0] = "mal"
	if caps.MediaTypes[0] == model.MediaType("other") || caps.Identifiers[0] == "mal" {
		t.Fatal("modifying cloned capabilities mutated original")
	}

	var emptyCaps Capabilities
	if emptyCaps.SupportsMediaType(model.Movie) || emptyCaps.SupportsIdentifier("tmdb") {
		t.Fatal("empty capabilities should not support anything")
	}
	emptyCloned := emptyCaps.Clone()
	if emptyCloned.MediaTypes != nil || emptyCloned.Identifiers != nil {
		t.Fatal("empty capabilities clone should have nil slices")
	}
}

func BenchmarkNewRegistry(b *testing.B) {
	vidcore := mockAdapter{id: model.VidCore}
	vidlink := mockAdapter{id: model.VidLink}
	vidsrc := mockAdapter{id: model.VidSrc}
	vidfast := mockAdapter{id: model.VidFast}

	b.ReportAllocs()
	for b.Loop() {
		reg, err := NewRegistry(vidcore, vidlink, vidsrc, vidfast)
		if err != nil || reg == nil {
			b.Fatal("failed NewRegistry")
		}
	}
}

func BenchmarkRegistryGet(b *testing.B) {
	vidcore := mockAdapter{id: model.VidCore}
	vidlink := mockAdapter{id: model.VidLink}
	vidsrc := mockAdapter{id: model.VidSrc}
	vidfast := mockAdapter{id: model.VidFast}

	reg, err := NewRegistry(vidcore, vidlink, vidsrc, vidfast)
	if err != nil {
		b.Fatal(err)
	}

	b.ReportAllocs()
	for b.Loop() {
		adapter, found := reg.Get(model.VidFast)
		if !found || adapter == nil {
			b.Fatal("not found")
		}
	}
}

func BenchmarkRegistryAdapters(b *testing.B) {
	vidcore := mockAdapter{id: model.VidCore}
	vidlink := mockAdapter{id: model.VidLink}
	vidsrc := mockAdapter{id: model.VidSrc}
	vidfast := mockAdapter{id: model.VidFast}

	reg, err := NewRegistry(vidcore, vidlink, vidsrc, vidfast)
	if err != nil {
		b.Fatal(err)
	}

	b.ReportAllocs()
	for b.Loop() {
		adapters := reg.Adapters()
		if len(adapters) != 4 {
			b.Fatal("unexpected len")
		}
	}
}

func BenchmarkCapabilitiesSupports(b *testing.B) {
	caps := Capabilities{
		MediaTypes:  []model.MediaType{model.Movie, model.TV},
		Identifiers: []string{"tmdb", "imdb", "mal"},
	}

	b.ReportAllocs()
	for b.Loop() {
		if !caps.SupportsMediaType(model.TV) || !caps.SupportsIdentifier("mal") {
			b.Fatal("unexpected capability mismatch")
		}
	}
}
