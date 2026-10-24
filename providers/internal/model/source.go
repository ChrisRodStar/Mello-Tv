package model

import (
	"bytes"
	"encoding/json"
	"slices"
	"time"
)

// ProviderID identifies a distinct upstream streaming provider.
type ProviderID string

const (
	VidCore ProviderID = "vidcore"
	VidLink ProviderID = "vidlink"
	VidSrc  ProviderID = "vidsrc"
	VidFast ProviderID = "vidfast"
)

// PlannedProviders lists the four intended providers supported by the service architecture.
var PlannedProviders = [...]ProviderID{
	VidCore,
	VidLink,
	VidSrc,
	VidFast,
}

// Format identifies the streaming container or manifest format.
type Format string

const (
	UnknownFormat Format = "unknown"
	HLS           Format = "hls"
	DASH          Format = "dash"
	MP4           Format = "mp4"
)

// FetchScope restricts HTTP headers and cookies to an explicit origin and path prefix.
// This prevents credentials from leaking to external CDNs or across redirects.
type FetchScope struct {
	Headers    map[string]string `json:"headers,omitempty"`
	Origin     string            `json:"origin"`
	PathPrefix string            `json:"path_prefix,omitempty"`
	Cookies    string            `json:"cookies,omitempty"`
	CookiePath string            `json:"cookie_path,omitempty"`
}

// Clone returns an isolated deep copy of FetchScope with cloned header map.
func (s FetchScope) Clone() FetchScope {
	cloned := s
	if s.Headers != nil {
		cloned.Headers = make(map[string]string, len(s.Headers))
		for k, v := range s.Headers {
			cloned.Headers[k] = v
		}
	}
	return cloned
}

// PublicResourceContext contains adapter-declared, non-secret origin identification.
// These root-origin values may accompany media resources on validated CDNs.
// Cookies, authorization and arbitrary headers are intentionally not representable here.
type PublicResourceContext struct {
	Origin  string `json:"origin,omitempty"`
	Referer string `json:"referer,omitempty"`
}

// Clone returns an isolated copy of PublicResourceContext.
func (p PublicResourceContext) Clone() PublicResourceContext {
	return p
}

// FetchContext separates public resource identification from origin-scoped credentials.
type FetchContext struct {
	Scopes          []FetchScope           `json:"scopes,omitempty"`
	PublicResources *PublicResourceContext `json:"public_resources,omitempty"`
}

// Clone returns an isolated deep copy of FetchContext.
func (fc FetchContext) Clone() FetchContext {
	cloned := fc
	if fc.Scopes != nil {
		cloned.Scopes = make([]FetchScope, len(fc.Scopes))
		for i, s := range fc.Scopes {
			cloned.Scopes[i] = s.Clone()
		}
	}
	if fc.PublicResources != nil {
		pr := *fc.PublicResources
		cloned.PublicResources = &pr
	}
	return cloned
}

// Track represents an audio or subtitle rendition within an adaptive stream.
type Track struct {
	Kind     string `json:"kind"`
	Language string `json:"language,omitempty"`
	Name     string `json:"name,omitempty"`
	Group    string `json:"group,omitempty"`
	URL      string `json:"url,omitempty"`
	Default  bool   `json:"default"`
}

// Variant represents a specific bitrate and resolution rendition within an adaptive playlist.
type Variant struct {
	URL           string   `json:"url"`
	AudioGroup    string   `json:"audio_group,omitempty"`
	SubtitleGroup string   `json:"subtitle_group,omitempty"`
	Codecs        []string `json:"codecs,omitempty"`
	Bitrate       *int64   `json:"bitrate,omitempty"`
	Width         *int     `json:"width,omitempty"`
	Height        *int     `json:"height,omitempty"`
}

// Clone returns an isolated deep copy of Variant with cloned pointers and codecs.
func (v Variant) Clone() Variant {
	cloned := v
	if v.Bitrate != nil {
		b := *v.Bitrate
		cloned.Bitrate = &b
	}
	if v.Width != nil {
		w := *v.Width
		cloned.Width = &w
	}
	if v.Height != nil {
		h := *v.Height
		cloned.Height = &h
	}
	cloned.Codecs = slices.Clone(v.Codecs)
	return cloned
}

// Metadata holds tracks, variants, and raw upstream fields extracted from a resolver or playlist.
type Metadata struct {
	// ProviderData preserves unmapped upstream payload fields without treating them as verified facts.
	ProviderData map[string]json.RawMessage `json:"provider_data,omitempty"`
	Tracks       []Track                    `json:"tracks,omitempty"`
	Variants     []Variant                  `json:"variants,omitempty"`
	TracksKnown  bool                       `json:"tracks_known"`
}

// Clone returns an isolated deep copy of Metadata with cloned maps and slices.
func (m Metadata) Clone() Metadata {
	cloned := m
	if m.ProviderData != nil {
		cloned.ProviderData = make(map[string]json.RawMessage, len(m.ProviderData))
		for k, v := range m.ProviderData {
			cloned.ProviderData[k] = bytes.Clone(v)
		}
	}
	cloned.Tracks = slices.Clone(m.Tracks)
	if m.Variants != nil {
		cloned.Variants = make([]Variant, len(m.Variants))
		for i, v := range m.Variants {
			cloned.Variants[i] = v.Clone()
		}
	}
	return cloned
}

// Candidate represents a candidate stream URL returned by an adapter before media probing.
type Candidate struct {
	ExpiresAt          *time.Time   `json:"expires_at,omitempty"`
	ID                 string       `json:"id"`
	Server             string       `json:"server,omitempty"`
	SelectedVariantURL string       `json:"selected_variant_url,omitempty"`
	URL                string       `json:"url"`
	Provider           ProviderID   `json:"provider"`
	Format             Format       `json:"format"`
	Title              Title        `json:"title"`
	Context            FetchContext `json:"fetch_context"`
	Metadata           Metadata     `json:"resolver_metadata"`
	ContentVerified    bool         `json:"content_verified"`
}

// Clone returns an isolated deep copy of Candidate with independent memory allocations.
func (c Candidate) Clone() Candidate {
	cloned := c
	cloned.Title = c.Title.Clone()
	cloned.Context = c.Context.Clone()
	cloned.Metadata = c.Metadata.Clone()
	if c.ExpiresAt != nil {
		exp := *c.ExpiresAt
		cloned.ExpiresAt = &exp
	}
	return cloned
}

// Observation records empirical measurements from probing an active media stream.
type Observation struct {
	At               time.Time      `json:"at"`
	DeliveryBPS      *float64       `json:"delivery_bps,omitempty"`
	Reliability      *float64       `json:"reliability,omitempty"`
	Variant          *Variant       `json:"variant,omitempty"`
	FirstByte        *time.Duration `json:"first_byte,omitempty"`
	SampleBytes      int64          `json:"sample_bytes"`
	Format           Format         `json:"format"`
	Failure          ErrorCategory  `json:"failure,omitempty"`
	Metadata         Metadata       `json:"playlist_metadata"`
	SamplesCompleted int            `json:"samples_completed"`
	Accessible       bool           `json:"accessible"`
	Encrypted        bool           `json:"encrypted"`
	ProtectionKnown  bool           `json:"protection_known"`
}

// Clone returns an isolated deep copy of Observation with independent pointers.
func (o Observation) Clone() Observation {
	cloned := o
	cloned.Metadata = o.Metadata.Clone()
	if o.Variant != nil {
		v := o.Variant.Clone()
		cloned.Variant = &v
	}
	if o.FirstByte != nil {
		fb := *o.FirstByte
		cloned.FirstByte = &fb
	}
	if o.DeliveryBPS != nil {
		db := *o.DeliveryBPS
		cloned.DeliveryBPS = &db
	}
	if o.Reliability != nil {
		r := *o.Reliability
		cloned.Reliability = &r
	}
	return cloned
}

// Probed pairs a candidate source with its empirical observation results.
type Probed struct {
	Source      Candidate   `json:"source"`
	Observation Observation `json:"observation"`
}

// Clone returns an isolated deep copy of Probed.
func (p Probed) Clone() Probed {
	return Probed{
		Source:      p.Source.Clone(),
		Observation: p.Observation.Clone(),
	}
}
