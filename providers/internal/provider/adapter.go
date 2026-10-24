package provider

import (
	"context"
	"slices"

	"stv-providers/internal/model"
)

// Capabilities describes the media types and identifier systems supported by an adapter.
type Capabilities struct {
	MediaTypes  []model.MediaType
	Identifiers []string
}

// SupportsMediaType reports whether the capabilities list includes the given media type.
func (c Capabilities) SupportsMediaType(mediaType model.MediaType) bool {
	return slices.Contains(c.MediaTypes, mediaType)
}

// SupportsIdentifier reports whether the capabilities list includes the given identifier schema.
func (c Capabilities) SupportsIdentifier(idType string) bool {
	return slices.Contains(c.Identifiers, idType)
}

// Clone returns an isolated deep copy of the capabilities.
func (c Capabilities) Clone() Capabilities {
	return Capabilities{
		MediaTypes:  slices.Clone(c.MediaTypes),
		Identifiers: slices.Clone(c.Identifiers),
	}
}

// Adapter defines the uniform interface implemented by all upstream streaming provider plugins.
type Adapter interface {
	// ID returns the unique ProviderID assigned to this adapter.
	ID() model.ProviderID

	// Capabilities returns the supported media types and identifier schemas.
	Capabilities() Capabilities

	// Resolve locates candidate streaming sources for a given title lookup.
	Resolve(ctx context.Context, lookup model.Lookup) (model.AdapterResult, error)
}
