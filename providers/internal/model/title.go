package model

import (
	"fmt"
	"strings"
)

// MediaType represents the category of video content (movie or episodic TV).
type MediaType string

const (
	Movie MediaType = "movie"
	TV    MediaType = "tv"
)

// Title specifies media coordinates for a lookup across provider catalogs.
// Fields are ordered for 64-bit memory alignment with zero internal padding.
type Title struct {
	Type          MediaType `json:"type"`
	TMDBID        int64     `json:"tmdb_id"`
	IMDbID        string    `json:"imdb_id,omitempty"`
	MyAnimeListID *int64    `json:"my_anime_list_id,omitempty"`
	Season        *int      `json:"season,omitempty"`
	Episode       *int      `json:"episode,omitempty"`
}

// NewMovie returns a Title representation for a movie with canonical TMDB ID.
func NewMovie(tmdbID int64) Title {
	return Title{
		Type:   Movie,
		TMDBID: tmdbID,
	}
}

// NewTV returns a Title representation for a TV episode with coordinates.
func NewTV(tmdbID int64, season, episode int) Title {
	return Title{
		Type:    TV,
		TMDBID:  tmdbID,
		Season:  &season,
		Episode: &episode,
	}
}

// Validate verifies that title identifiers and episode coordinates are valid.
func (t Title) Validate() error {
	if t.TMDBID <= 0 {
		return fmt.Errorf("positive TMDB ID required")
	}

	switch t.Type {
	case Movie:
		if t.Season != nil || t.Episode != nil {
			return fmt.Errorf("movie cannot carry episode coordinates")
		}
	case TV:
		if t.Season == nil || t.Episode == nil || *t.Season < 0 || *t.Episode < 1 {
			return fmt.Errorf("TV requires nonnegative season and positive episode")
		}
	default:
		return fmt.Errorf("unsupported media type")
	}

	if t.MyAnimeListID != nil && *t.MyAnimeListID <= 0 {
		return fmt.Errorf("invalid MyAnimeList ID")
	}

	if len(t.IMDbID) > 32 {
		return fmt.Errorf("IMDb ID exceeds maximum length of 32 characters")
	}

	if len(t.IMDbID) > 0 && strings.TrimSpace(t.IMDbID) != t.IMDbID {
		return fmt.Errorf("IMDb ID must not contain surrounding whitespace")
	}

	return nil
}

// SameContent returns true if both Title descriptors refer to the exact same content item.
func (t Title) SameContent(other Title) bool {
	if t.Type != other.Type || t.TMDBID != other.TMDBID {
		return false
	}

	if t.Type == Movie {
		return true
	}

	if t.Season == nil || other.Season == nil || t.Episode == nil || other.Episode == nil {
		return false
	}

	return *t.Season == *other.Season && *t.Episode == *other.Episode
}

// Clone returns an isolated deep copy of Title with cloned pointer coordinates.
func (t Title) Clone() Title {
	cloned := t
	if t.MyAnimeListID != nil {
		malID := *t.MyAnimeListID
		cloned.MyAnimeListID = &malID
	}
	if t.Season != nil {
		s := *t.Season
		cloned.Season = &s
	}
	if t.Episode != nil {
		e := *t.Episode
		cloned.Episode = &e
	}
	return cloned
}
