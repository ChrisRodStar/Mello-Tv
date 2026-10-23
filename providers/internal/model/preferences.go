package model

// Preferences encapsulates audio, subtitle, and resolution preferences for stream selection.
type Preferences struct {
	AudioLanguages    []string `json:"audio_languages,omitempty"`
	SubtitleLanguages []string `json:"subtitle_languages,omitempty"`
	RequireAudio      bool     `json:"require_audio"`
	RequireSubtitles  bool     `json:"require_subtitles"`
	MaxHeight         int      `json:"max_height,omitempty"`
}

// Device encapsulates client hardware and player capabilities for media compatibility checking.
type Device struct {
	Formats                []Format `json:"formats"`
	Codecs                 []string `json:"codecs,omitempty"`
	MaxHeight              int      `json:"max_height,omitempty"`
	AllowUnknownCodecs     bool     `json:"allow_unknown_codecs"`
	AllowUnknownResolution bool     `json:"allow_unknown_resolution"`
	SupportsEncryptedHLS   bool     `json:"supports_encrypted_hls"`
}

// Lookup combines title identification, user preferences, and client device capabilities.
type Lookup struct {
	// Refresh bypasses cached adapter results for a source refresh; concurrent
	// identical refreshes still coalesce within their own flight.
	// StartupHeight opts into a verified startup rendition; zero waits for all probes.
	StartupHeight int `json:"startup_height,omitempty"`
	// WaitForComplete joins the same flight but waits for its complete backup set.
	WaitForComplete bool        `json:"wait_for_complete,omitempty"`
	Refresh         bool        `json:"refresh,omitempty"`
	Title           Title       `json:"title"`
	Preferences     Preferences `json:"preferences"`
	Device          Device      `json:"device"`

	// ContextKey isolates contexts that must not share upstream sessions (e.g. household authorization).
	ContextKey string `json:"-"`
}
