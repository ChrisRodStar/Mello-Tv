package model

import (
	"encoding/json"
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
	Origin     string            `json:"origin"`
	PathPrefix string            `json:"path_prefix,omitempty"`
	Headers    map[string]string `json:"headers,omitempty"`
	Cookies    string            `json:"cookies,omitempty"`
	CookiePath string            `json:"cookie_path,omitempty"`
}

// PublicResourceContext contains adapter-declared, non-secret origin identification.
// These root-origin values may accompany media resources on validated CDNs.
// Cookies, authorization and arbitrary headers are intentionally not representable here.
type PublicResourceContext struct {
	Origin  string `json:"origin,omitempty"`
	Referer string `json:"referer,omitempty"`
}

// FetchContext separates public resource identification from origin-scoped credentials.
type FetchContext struct {
	Scopes          []FetchScope           `json:"scopes,omitempty"`
	PublicResources *PublicResourceContext `json:"public_resources,omitempty"`
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
	Width         *int     `json:"width,omitempty"`
	Height        *int     `json:"height,omitempty"`
	Bitrate       *int64   `json:"bitrate,omitempty"`
	Codecs        []string `json:"codecs,omitempty"`
	AudioGroup    string   `json:"audio_group,omitempty"`
	SubtitleGroup string   `json:"subtitle_group,omitempty"`
}

// Metadata holds tracks, variants, and raw upstream fields extracted from a resolver or playlist.
type Metadata struct {
	// ProviderData preserves unmapped upstream payload fields without treating them as verified facts.
	ProviderData map[string]json.RawMessage `json:"provider_data,omitempty"`
	Tracks       []Track                    `json:"tracks,omitempty"`
	TracksKnown  bool                       `json:"tracks_known"`
	Variants     []Variant                  `json:"variants,omitempty"`
}

// Candidate represents a candidate stream URL returned by an adapter before media probing.
type Candidate struct {
	ID       string     `json:"id"`
	Provider ProviderID `json:"provider"`
	Server   string     `json:"server,omitempty"`
	Title    Title      `json:"title"`

	// ContentVerified indicates whether audio/video frames have been independently verified.
	ContentVerified bool `json:"content_verified"`

	// URL remains the playable master for a probed HLS variant. Consumers must retain
	// its rendition associations and restrict playback to SelectedVariantURL when set.
	SelectedVariantURL string       `json:"selected_variant_url,omitempty"`
	URL                string       `json:"url"`
	Format             Format       `json:"format"`
	Context            FetchContext `json:"fetch_context"`
	ExpiresAt          *time.Time   `json:"expires_at,omitempty"`
	Metadata           Metadata     `json:"resolver_metadata"`
}

// Observation records empirical measurements from probing an active media stream.
type Observation struct {
	At               time.Time      `json:"at"`
	Accessible       bool           `json:"accessible"`
	Format           Format         `json:"format"`
	Metadata         Metadata       `json:"playlist_metadata"`
	Variant          *Variant       `json:"variant,omitempty"`
	FirstByte        *time.Duration `json:"first_byte,omitempty"`
	DeliveryBPS      *float64       `json:"delivery_bps,omitempty"`
	SamplesCompleted int            `json:"samples_completed"`
	SampleBytes      int64          `json:"sample_bytes"`
	Encrypted        bool           `json:"encrypted"`
	ProtectionKnown  bool           `json:"protection_known"`
	Reliability      *float64       `json:"reliability,omitempty"`
	Failure          ErrorCategory  `json:"failure,omitempty"`
}

// Probed pairs a candidate source with its empirical observation results.
type Probed struct {
	Source      Candidate   `json:"source"`
	Observation Observation `json:"observation"`
}
