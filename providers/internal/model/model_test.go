package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"testing"
	"time"
)

func TestTitleValidation(t *testing.T) {
	s1 := 1
	e1 := 1
	s0 := 0
	neg := -1
	posMal := int64(456)
	negMal := int64(-5)

	tests := []struct {
		name    string
		title   Title
		wantErr bool
	}{
		{
			name:    "valid movie",
			title:   NewMovie(100),
			wantErr: false,
		},
		{
			name:    "valid movie with valid imdb and mal",
			title:   Title{Type: Movie, TMDBID: 100, IMDbID: "tt1234567", MyAnimeListID: &posMal},
			wantErr: false,
		},
		{
			name:    "invalid movie with season",
			title:   Title{Type: Movie, TMDBID: 100, Season: &s1},
			wantErr: true,
		},
		{
			name:    "invalid movie with episode",
			title:   Title{Type: Movie, TMDBID: 100, Episode: &e1},
			wantErr: true,
		},
		{
			name:    "invalid tmdb id zero",
			title:   Title{Type: Movie, TMDBID: 0},
			wantErr: true,
		},
		{
			name:    "invalid tmdb id negative",
			title:   Title{Type: Movie, TMDBID: -10},
			wantErr: true,
		},
		{
			name:    "unsupported media type",
			title:   Title{Type: "anime", TMDBID: 100},
			wantErr: true,
		},
		{
			name:    "valid tv with season 1 episode 1",
			title:   NewTV(200, 1, 1),
			wantErr: false,
		},
		{
			name:    "valid tv with specials season 0 episode 1",
			title:   NewTV(200, 0, 1),
			wantErr: false,
		},
		{
			name:    "invalid tv missing season",
			title:   Title{Type: TV, TMDBID: 200, Episode: &e1},
			wantErr: true,
		},
		{
			name:    "invalid tv missing episode",
			title:   Title{Type: TV, TMDBID: 200, Season: &s1},
			wantErr: true,
		},
		{
			name:    "invalid tv negative season",
			title:   Title{Type: TV, TMDBID: 200, Season: &neg, Episode: &e1},
			wantErr: true,
		},
		{
			name:    "invalid tv episode zero",
			title:   Title{Type: TV, TMDBID: 200, Season: &s0, Episode: &s0},
			wantErr: true,
		},
		{
			name:    "invalid tv negative episode",
			title:   Title{Type: TV, TMDBID: 200, Season: &s1, Episode: &neg},
			wantErr: true,
		},
		{
			name:    "invalid negative mal id",
			title:   Title{Type: Movie, TMDBID: 100, MyAnimeListID: &negMal},
			wantErr: true,
		},
		{
			name:    "invalid imdb id too long",
			title:   Title{Type: Movie, TMDBID: 100, IMDbID: "tt123456789012345678901234567890123"},
			wantErr: true,
		},
		{
			name:    "invalid imdb id with whitespace",
			title:   Title{Type: Movie, TMDBID: 100, IMDbID: " tt1234567 "},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := tt.title.Validate()
			if (err != nil) != tt.wantErr {
				t.Errorf("Title.Validate() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestTitleSameContent(t *testing.T) {
	m1 := NewMovie(100)
	m1Dup := NewMovie(100)
	m2 := NewMovie(200)

	tv1 := NewTV(300, 1, 1)
	tv1Dup := NewTV(300, 1, 1)
	tvDiffEp := NewTV(300, 1, 2)
	tvDiffSeason := NewTV(300, 2, 1)
	tvDiffID := NewTV(400, 1, 1)

	if !m1.SameContent(m1Dup) {
		t.Errorf("expected same movies to match")
	}
	if m1.SameContent(m2) {
		t.Errorf("expected different movie tmdb IDs not to match")
	}
	if m1.SameContent(NewMovie(100)) == false {
		t.Errorf("expected movie same content")
	}
	if m1.SameContent(Title{Type: TV, TMDBID: 100}) {
		t.Errorf("expected different types not to match")
	}
	if !tv1.SameContent(tv1Dup) {
		t.Errorf("expected identical tv coordinates to match")
	}
	if tv1.SameContent(tvDiffEp) {
		t.Errorf("expected different tv episode not to match")
	}
	if tv1.SameContent(tvDiffSeason) {
		t.Errorf("expected different tv season not to match")
	}
	if tv1.SameContent(tvDiffID) {
		t.Errorf("expected different tv tmdb id not to match")
	}
	if tv1.SameContent(Title{Type: TV, TMDBID: 300}) {
		t.Errorf("expected tv with nil coordinates not to match")
	}
}

func TestTitleClone(t *testing.T) {
	origSeason := 2
	origEp := 5
	origMAL := int64(12345)
	orig := Title{
		Type:          TV,
		TMDBID:        555,
		IMDbID:        "tt5555555",
		MyAnimeListID: &origMAL,
		Season:        &origSeason,
		Episode:       &origEp,
	}

	cloned := orig.Clone()

	if !orig.SameContent(cloned) {
		t.Fatalf("cloned title content differs from original")
	}

	// Mutate clone pointers and verify original is unchanged
	*cloned.Season = 99
	*cloned.Episode = 88
	*cloned.MyAnimeListID = 99999

	if *orig.Season != 2 || *orig.Episode != 5 || *orig.MyAnimeListID != 12345 {
		t.Errorf("mutating cloned title affected original pointer targets")
	}
}

func TestPreferencesAndDevice(t *testing.T) {
	prefs := Preferences{
		AudioLanguages:    []string{"en", "ja"},
		SubtitleLanguages: []string{"en"},
		MaxHeight:         1080,
		RequireAudio:      true,
		RequireSubtitles:  false,
	}

	if err := prefs.Validate(); err != nil {
		t.Errorf("valid preferences failed validation: %v", err)
	}

	clonedPrefs := prefs.Clone()
	clonedPrefs.AudioLanguages[0] = "es"
	if prefs.AudioLanguages[0] != "en" {
		t.Errorf("mutating cloned preferences affected original slice")
	}

	invalidPrefs := Preferences{MaxHeight: -1}
	if err := invalidPrefs.Validate(); err == nil {
		t.Errorf("expected error for negative max height")
	}

	dev := Device{
		Formats:   []Format{HLS, MP4},
		Codecs:    []string{"avc1", "mp4a"},
		MaxHeight: 2160,
	}

	if err := dev.Validate(); err != nil {
		t.Errorf("valid device failed validation: %v", err)
	}

	clonedDev := dev.Clone()
	clonedDev.Formats[0] = DASH
	clonedDev.Codecs[0] = "hvc1"
	if dev.Formats[0] != HLS || dev.Codecs[0] != "avc1" {
		t.Errorf("mutating cloned device affected original slices")
	}

	invalidDevNoFormat := Device{Formats: nil}
	if err := invalidDevNoFormat.Validate(); err == nil {
		t.Errorf("expected error for device with no formats")
	}

	invalidDevBadFormat := Device{Formats: []Format{"flv"}}
	if err := invalidDevBadFormat.Validate(); err == nil {
		t.Errorf("expected error for unsupported device format")
	}
}

func TestLookupValidationAndClone(t *testing.T) {
	lookup := Lookup{
		Title:         NewTV(100, 1, 1),
		Preferences:   Preferences{AudioLanguages: []string{"en"}},
		Device:        Device{Formats: []Format{HLS}},
		StartupHeight: 1080,
		ContextKey:    "household-123",
	}

	if err := lookup.Validate(); err != nil {
		t.Fatalf("valid lookup failed validation: %v", err)
	}

	cloned := lookup.Clone()

	if cloned.ContextKey != "household-123" {
		t.Errorf("cloned lookup lost ContextKey: got %q", cloned.ContextKey)
	}

	*cloned.Title.Season = 5
	cloned.Preferences.AudioLanguages[0] = "fr"
	cloned.Device.Formats[0] = MP4

	if *lookup.Title.Season != 1 || lookup.Preferences.AudioLanguages[0] != "en" || lookup.Device.Formats[0] != HLS {
		t.Errorf("mutating cloned lookup affected original lookup data")
	}

	invalidLookup := lookup
	invalidLookup.StartupHeight = -5
	if err := invalidLookup.Validate(); err == nil {
		t.Errorf("expected error for negative startup height")
	}
}

func TestSourceAndCandidateClone(t *testing.T) {
	expiry := time.Now().Add(time.Hour)
	candidate := Candidate{
		ID:       "vidlink:123",
		Provider: VidLink,
		URL:      "https://media.example.com/stream.m3u8",
		Format:   HLS,
		Title:    NewMovie(500),
		Context: FetchContext{
			Scopes: []FetchScope{
				{
					Origin:  "https://vidlink.pro",
					Headers: map[string]string{"Referer": "https://vidlink.pro/"},
				},
			},
			PublicResources: &PublicResourceContext{
				Origin:  "https://vidlink.pro",
				Referer: "https://vidlink.pro/",
			},
		},
		ExpiresAt: &expiry,
		Metadata: Metadata{
			ProviderData: map[string]json.RawMessage{
				"raw": json.RawMessage(`{"key":"value"}`),
			},
			Tracks: []Track{
				{Kind: "audio", Language: "en", Default: true},
			},
		},
	}

	cloned := candidate.Clone()

	// Mutate clone map and verify original isolation
	cloned.Context.Scopes[0].Headers["Referer"] = "https://hacked.com"
	delete(cloned.Metadata.ProviderData, "raw")
	cloned.Metadata.Tracks[0].Language = "de"
	*cloned.ExpiresAt = time.Now()

	if candidate.Context.Scopes[0].Headers["Referer"] != "https://vidlink.pro/" {
		t.Errorf("mutating cloned scope headers affected original map")
	}
	if _, ok := candidate.Metadata.ProviderData["raw"]; !ok {
		t.Errorf("mutating cloned provider data affected original map")
	}
	if candidate.Metadata.Tracks[0].Language != "en" {
		t.Errorf("mutating cloned tracks affected original slice")
	}
	if *candidate.ExpiresAt == *cloned.ExpiresAt {
		t.Errorf("mutating cloned expires_at affected original time")
	}
}

func TestSelectionClone(t *testing.T) {
	sel := Selection{
		Version: ContractVersion,
		Selected: &Ranked{
			Score:   95.5,
			Reasons: []string{"highest_resolution"},
			Candidate: Probed{
				Source: Candidate{ID: "primary", Format: HLS},
			},
		},
		Backups: []Ranked{
			{
				Score:   80.0,
				Reasons: []string{"backup"},
				Candidate: Probed{
					Source: Candidate{ID: "backup1", Format: HLS},
				},
			},
		},
		Rejected: map[string]string{
			"unsupported_codec": "hevc rejected",
		},
		Providers: []ProviderStatus{
			{Provider: VidLink, CandidateCount: 1, Duration: 500 * time.Millisecond},
		},
	}

	cloned := sel.Clone()

	cloned.Selected.Score = 10.0
	cloned.Backups[0].Score = 5.0
	cloned.Rejected["new_reason"] = "testing"
	cloned.Providers[0].CandidateCount = 99

	if sel.Selected.Score != 95.5 {
		t.Errorf("mutating cloned selection selected score affected original")
	}
	if sel.Backups[0].Score != 80.0 {
		t.Errorf("mutating cloned backup score affected original")
	}
	if _, exists := sel.Rejected["new_reason"]; exists {
		t.Errorf("mutating cloned rejected map affected original")
	}
	if sel.Providers[0].CandidateCount != 1 {
		t.Errorf("mutating cloned providers affected original")
	}
}

func TestErrorsAndCategories(t *testing.T) {
	// Standard Failure
	err := Fail(Unavailable, "probe", "all mirrors timed out")
	if err.Error() != "unavailable: probe: all mirrors timed out" {
		t.Errorf("unexpected Error() string: %s", err.Error())
	}
	if Category(err) != Unavailable {
		t.Errorf("expected Category %v, got %v", Unavailable, Category(err))
	}

	// Wrapped error
	baseErr := errors.New("socket closed unexpectedly")
	wrapped := Wrap(UpstreamFailure, "socket", "network failed", baseErr)
	if !errors.Is(wrapped, baseErr) {
		t.Errorf("expected errors.Is to find underlying baseErr")
	}
	if Category(wrapped) != UpstreamFailure {
		t.Errorf("expected Category %v, got %v", UpstreamFailure, Category(wrapped))
	}

	// Context Deadline Exceeded
	deadlineErr := fmt.Errorf("lookup context: %w", context.DeadlineExceeded)
	if Category(deadlineErr) != Timeout {
		t.Errorf("expected Timeout category for DeadlineExceeded, got %v", Category(deadlineErr))
	}

	// Context Canceled
	cancelErr := fmt.Errorf("client dropped: %w", context.Canceled)
	if Category(cancelErr) != Cancelled {
		t.Errorf("expected Cancelled category for Canceled, got %v", Category(cancelErr))
	}

	// Arbitrary error defaults to UpstreamFailure
	unknownErr := errors.New("unknown error")
	if Category(unknownErr) != UpstreamFailure {
		t.Errorf("expected UpstreamFailure for arbitrary error, got %v", Category(unknownErr))
	}

	// Nil error category
	if Category(nil) != "" {
		t.Errorf("expected empty category for nil error, got %v", Category(nil))
	}

	// Failure Is matching
	targetMatch := &Failure{Category: Unavailable}
	if !errors.Is(err, targetMatch) {
		t.Errorf("expected Failure.Is to match target by category")
	}
	targetMismatch := &Failure{Category: ProtocolChange}
	if errors.Is(err, targetMismatch) {
		t.Errorf("expected Failure.Is not to match mismatched category")
	}

	// Nil receiver and nil target Failure.Is
	var nilFailure *Failure
	if nilFailure.Is(targetMatch) {
		t.Errorf("expected nil Failure receiver to return false from Is")
	}
	if targetMatch.Is(nil) {
		t.Errorf("expected Failure.Is(nil) to return false")
	}
}

func TestJSONRoundTrip(t *testing.T) {
	title := NewTV(1622, 1, 1)
	title.IMDbID = "tt0460681"

	data, err := json.Marshal(title)
	if err != nil {
		t.Fatalf("failed to marshal Title: %v", err)
	}

	var decoded Title
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("failed to unmarshal Title: %v", err)
	}

	if !title.SameContent(decoded) || decoded.IMDbID != "tt0460681" {
		t.Errorf("JSON round-trip mismatch: got %+v, want %+v", decoded, title)
	}
}
