package api

import (
	"encoding/json"
	"fmt"
	"math"
	"regexp"
	"time"
)

type Identity struct {
	Type string `json:"type"`
	ID   int64  `json:"id"`
}

func (i Identity) Valid() bool { return (i.Type == "movie" || i.Type == "tv") && i.ID > 0 }
func (i Identity) Key() string { return fmt.Sprintf("%s:%d", i.Type, i.ID) }

type Title struct {
	Identity
	Title    string          `json:"title"`
	Overview string          `json:"overview"`
	Date     string          `json:"date,omitempty"`
	Rating   float64         `json:"rating"`
	Poster   string          `json:"poster,omitempty"`
	Backdrop string          `json:"backdrop,omitempty"`
	IMDb     string          `json:"imdb_id,omitempty"`
	MAL      int64           `json:"mal_id,omitempty"`
	Runtime  int             `json:"runtime,omitempty"`
	Genres   []string        `json:"genres,omitempty"`
	Seasons  []SeasonSummary `json:"seasons,omitempty"`
	Details  bool            `json:"details"`
}
type SeasonSummary struct {
	Number int    `json:"season"`
	Name   string `json:"name"`
	Count  int    `json:"episode_count"`
}
type Episode struct {
	ID       int64  `json:"tmdb_episode_id"`
	Season   int    `json:"season"`
	Number   int    `json:"episode"`
	Name     string `json:"name"`
	Overview string `json:"overview"`
	Runtime  int    `json:"runtime"`
	Still    string `json:"still,omitempty"`
	Date     string `json:"air_date,omitempty"`
}
type Season struct {
	ShowID   int64     `json:"show_id"`
	Number   int       `json:"season"`
	Name     string    `json:"name"`
	Episodes []Episode `json:"episodes"`
}
type Shelf struct {
	ID      string  `json:"id"`
	Heading string  `json:"heading"`
	Kind    string  `json:"kind,omitempty"`
	Source  string  `json:"source,omitempty"`
	Reason  string  `json:"reason,omitempty"`
	Titles  []Title `json:"titles"`
}
type Recommendations struct {
	Fingerprint string  `json:"fingerprint"`
	Pool        []Title `json:"pool"`
	Sections    []Shelf `json:"sections"`
}
type Record struct {
	Kind     string          `json:"kind"`
	Key      string          `json:"key"`
	Data     json.RawMessage `json:"data"`
	Updated  time.Time       `json:"updated_at"`
	Expires  time.Time       `json:"expires_at"`
	Revision int64           `json:"revision"`
	State    string          `json:"state"`
}
type Snapshot struct {
	Version  string    `json:"version"`
	Revision int64     `json:"revision"`
	Exported time.Time `json:"exported_at"`
	Records  []Record  `json:"records"`
}
type Preferences struct {
	Completed bool     `json:"completed"`
	Format    string   `json:"format"`
	Genres    []string `json:"genres"`
}

var genres = map[string]bool{"action": true, "comedy": true, "drama": true, "crime": true, "scifi": true, "animation": true, "documentary": true, "horror": true, "family": true}

func (p Preferences) Valid() bool {
	if p.Format != "both" && p.Format != "movie" && p.Format != "tv" {
		return false
	}
	if len(p.Genres) > 9 {
		return false
	}
	seen := map[string]bool{}
	for _, g := range p.Genres {
		if !genres[g] || seen[g] {
			return false
		}
		seen[g] = true
	}
	return true
}

type Profile struct {
	ID          string      `json:"id"`
	Name        string      `json:"name"`
	Color       string      `json:"color"`
	Preferences Preferences `json:"preferences"`
}
type Target struct {
	Identity
	Season  *int `json:"season,omitempty"`
	Episode *int `json:"episode,omitempty"`
}

func (t Target) Valid() bool {
	if !t.Identity.Valid() {
		return false
	}
	if t.Type == "movie" {
		return t.Season == nil && t.Episode == nil
	}
	return t.Season != nil && t.Episode != nil && *t.Season >= 0 && *t.Season <= 10000 && *t.Episode > 0 && *t.Episode <= 100000
}
func (t Target) Key() string {
	if t.Type == "movie" {
		return t.Identity.Key()
	}
	return fmt.Sprintf("%s:%d:%d", t.Identity.Key(), *t.Season, *t.Episode)
}

type Device struct {
	Formats                []string `json:"formats"`
	Codecs                 []string `json:"codecs,omitempty"`
	MaxHeight              int      `json:"max_height,omitempty"`
	AllowUnknownCodecs     bool     `json:"allow_unknown_codecs"`
	AllowUnknownResolution bool     `json:"allow_unknown_resolution"`
	EncryptedHLS           bool     `json:"supports_encrypted_hls"`
}
type PlaybackPreferences struct {
	Audio            []string `json:"audio_languages,omitempty"`
	Subtitles        []string `json:"subtitle_languages,omitempty"`
	RequireAudio     bool     `json:"require_audio"`
	RequireSubtitles bool     `json:"require_subtitles"`
	MaxHeight        int      `json:"max_height,omitempty"`
}
type PlaybackRequest struct {
	Target
	Device      *Device              `json:"device,omitempty"`
	Preferences *PlaybackPreferences `json:"preferences,omitempty"`
	IMDb        string               `json:"imdb_id,omitempty"`
	MAL         *int64               `json:"mal_id,omitempty"`
}

var imdbPattern = regexp.MustCompile(`^tt[0-9]{1,20}$`)

func (p PlaybackRequest) Valid() bool {
	if p.Device != nil {
		if len(p.Device.Formats) == 0 || len(p.Device.Formats) > 2 || len(p.Device.Codecs) > 16 || p.Device.MaxHeight < 0 || p.Device.MaxHeight > 16384 {
			return false
		}
		for _, f := range p.Device.Formats {
			if f != "hls" && f != "mp4" {
				return false
			}
		}
		for _, c := range p.Device.Codecs {
			if len(c) > 32 {
				return false
			}
		}
	}
	if p.Preferences != nil {
		if len(p.Preferences.Audio) > 16 || len(p.Preferences.Subtitles) > 16 || p.Preferences.MaxHeight < 0 {
			return false
		}
		for _, v := range append(append([]string{}, p.Preferences.Audio...), p.Preferences.Subtitles...) {
			if len(v) > 32 {
				return false
			}
		}
	}
	return p.Target.Valid() && (p.IMDb == "" || imdbPattern.MatchString(p.IMDb)) && (p.MAL == nil || *p.MAL > 0)
}

type Progress struct {
	Target
	Position  float64   `json:"position_seconds"`
	Duration  float64   `json:"duration_seconds"`
	Watched   float64   `json:"watched_seconds"`
	Completed bool      `json:"completed"`
	Updated   time.Time `json:"updated_at"`
}

func finite(v float64) bool { return !math.IsInf(v, 0) && !math.IsNaN(v) && v >= 0 }

type RelayReady struct {
	AvailableHeights []int     `json:"available_heights,omitempty"`
	Version          string    `json:"contract_version"`
	SID              string    `json:"session_id"`
	Entry            string    `json:"entry_path"`
	Format           string    `json:"format"`
	Expires          time.Time `json:"expires_at"`
	Revision         uint64    `json:"revision"`
	AuthRevision     uint64    `json:"auth_revision"`
	Generation       uint64    `json:"source_generation"`
	State            string    `json:"state"`
	Resume           bool      `json:"resume_required"`
}
type Playback struct {
	ID      string          `json:"id"`
	Request PlaybackRequest `json:"title"`
	Ready   RelayReady      `json:"-"`
	Revoked bool            `json:"-"`
}
type fault struct {
	status int
	code   string
}

func (f *fault) Error() string { return f.code }
func bad() error               { return &fault{400, "invalid_request"} }
func missing() error           { return &fault{404, "missing"} }
func conflict() error          { return &fault{409, "revision_conflict"} }
