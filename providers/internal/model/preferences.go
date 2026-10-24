package model

import (
	"fmt"
	"slices"
)

// Preferences encapsulates audio, subtitle, and resolution preferences for stream selection.
// Fields are ordered for 64-bit memory alignment.
type Preferences struct {
	AudioLanguages    []string `json:"audio_languages,omitempty"`
	SubtitleLanguages []string `json:"subtitle_languages,omitempty"`
	MaxHeight         int      `json:"max_height,omitempty"`
	RequireAudio      bool     `json:"require_audio"`
	RequireSubtitles  bool     `json:"require_subtitles"`
}

// Validate verifies that language preferences and height limits are within bounded limits.
func (p Preferences) Validate() error {
	if p.MaxHeight < 0 {
		return fmt.Errorf("max_height cannot be negative")
	}

	for _, langs := range [][]string{p.AudioLanguages, p.SubtitleLanguages} {
		if len(langs) > 32 {
			return fmt.Errorf("language preference list exceeds maximum limit of 32 entries")
		}
		for _, lang := range langs {
			if len(lang) == 0 || len(lang) > 64 {
				return fmt.Errorf("language code must be between 1 and 64 characters")
			}
		}
	}

	return nil
}

// Clone returns an isolated deep copy of Preferences with cloned slices.
func (p Preferences) Clone() Preferences {
	cloned := p
	cloned.AudioLanguages = slices.Clone(p.AudioLanguages)
	cloned.SubtitleLanguages = slices.Clone(p.SubtitleLanguages)
	return cloned
}

// Device encapsulates client hardware and player capabilities for media compatibility checking.
// Fields are ordered for 64-bit memory alignment.
type Device struct {
	Formats                []Format `json:"formats"`
	Codecs                 []string `json:"codecs,omitempty"`
	MaxHeight              int      `json:"max_height,omitempty"`
	AllowUnknownCodecs     bool     `json:"allow_unknown_codecs"`
	AllowUnknownResolution bool     `json:"allow_unknown_resolution"`
	SupportsEncryptedHLS   bool     `json:"supports_encrypted_hls"`
}

// Validate verifies that client device capabilities adhere to supported formats and boundaries.
func (d Device) Validate() error {
	if len(d.Formats) == 0 {
		return fmt.Errorf("device must declare at least one supported streaming format")
	}
	if len(d.Formats) > 4 {
		return fmt.Errorf("device declared too many streaming formats (max 4)")
	}
	for _, format := range d.Formats {
		switch format {
		case HLS, MP4, DASH:
		default:
			return fmt.Errorf("unsupported streaming format: %s", format)
		}
	}

	if len(d.Codecs) > 32 {
		return fmt.Errorf("codec list exceeds maximum limit of 32 entries")
	}
	for _, codec := range d.Codecs {
		if len(codec) == 0 || len(codec) > 64 {
			return fmt.Errorf("codec string must be between 1 and 64 characters")
		}
	}

	if d.MaxHeight < 0 {
		return fmt.Errorf("device max_height cannot be negative")
	}

	return nil
}

// Clone returns an isolated deep copy of Device with cloned slices.
func (d Device) Clone() Device {
	cloned := d
	cloned.Formats = slices.Clone(d.Formats)
	cloned.Codecs = slices.Clone(d.Codecs)
	return cloned
}

// Lookup combines title identification, user preferences, and client device capabilities.
// Fields are ordered for 64-bit memory alignment.
type Lookup struct {
	Title           Title       `json:"title"`
	Preferences     Preferences `json:"preferences"`
	Device          Device      `json:"device"`
	ContextKey      string      `json:"-"`
	StartupHeight   int         `json:"startup_height,omitempty"`
	WaitForComplete bool        `json:"wait_for_complete,omitempty"`
	Refresh         bool        `json:"refresh,omitempty"`
}

// Validate verifies that title, preferences, and device requirements in the lookup are valid.
func (l Lookup) Validate() error {
	if err := l.Title.Validate(); err != nil {
		return fmt.Errorf("invalid title: %w", err)
	}
	if err := l.Preferences.Validate(); err != nil {
		return fmt.Errorf("invalid preferences: %w", err)
	}
	if err := l.Device.Validate(); err != nil {
		return fmt.Errorf("invalid device: %w", err)
	}
	if l.StartupHeight < 0 || l.StartupHeight > 16384 {
		return fmt.Errorf("startup_height must be between 0 and 16384")
	}
	return nil
}

// Clone creates an isolated deep copy of Lookup with independent pointers and slices.
// Preserves ContextKey across concurrent flights without JSON serialization overhead.
func (l Lookup) Clone() Lookup {
	cloned := l
	cloned.Title = l.Title.Clone()
	cloned.Preferences = l.Preferences.Clone()
	cloned.Device = l.Device.Clone()
	return cloned
}
