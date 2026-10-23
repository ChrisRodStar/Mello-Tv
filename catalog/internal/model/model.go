//
//  model.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Pure domain models and data contracts for media catalog entities.
//  Includes canonical title identities, movie and TV show structures,
//  season summaries, and episode coordinates.
//

package model

import "strconv"

// Identity represents the unique canonical coordinate for a media item.
// Type is either "movie" or "tv", and ID is the positive TMDB identifier.
type Identity struct {
	Type string `json:"type"`
	ID   int    `json:"id"`
}

// Key formats the canonical identity key (e.g. "movie:27205" or "tv:60625").
func (i Identity) Key() string {
	return i.Type + ":" + strconv.Itoa(i.ID)
}

// Valid checks that the identity type is either "movie" or "tv" and the TMDB ID is positive.
func (i Identity) Valid() bool {
	return (i.Type == "movie" || i.Type == "tv") && i.ID > 0
}

// SeasonSummary outlines episode counts for a season entry within a TV show detail payload.
type SeasonSummary struct {
	Number       int    `json:"season"`
	Name         string `json:"name"`
	EpisodeCount int    `json:"episode_count"`
}

// Title encapsulates a full movie or TV series domain model.
type Title struct {
	Identity
	Title    string          `json:"title"`
	Overview string          `json:"overview"`
	Date     string          `json:"date,omitempty"`
	Rating   float64         `json:"rating"`
	Poster   string          `json:"poster,omitempty"`
	Backdrop string          `json:"backdrop,omitempty"`
	IMDb     string          `json:"imdb_id,omitempty"`
	MAL      int             `json:"mal_id,omitempty"`
	Runtime  int             `json:"runtime,omitempty"`
	Genres   []string        `json:"genres,omitempty"`
	Seasons  []SeasonSummary `json:"seasons,omitempty"`
	Details  bool            `json:"details"`
}

// Episode defines coordinate and metadata attributes for an individual TV episode.
type Episode struct {
	ID       int    `json:"tmdb_episode_id"`
	Season   int    `json:"season"`
	Number   int    `json:"episode"`
	Name     string `json:"name"`
	Overview string `json:"overview"`
	Runtime  int    `json:"runtime"`
	Still    string `json:"still,omitempty"`
	Date     string `json:"air_date,omitempty"`
}

// Season represents a full TV season containing all its episode coordinate objects.
type Season struct {
	ShowID   int       `json:"show_id"`
	Number   int       `json:"season"`
	Name     string    `json:"name"`
	Episodes []Episode `json:"episodes"`
}

// Shelf represents an ordered horizontal collection of titles displayed on clients.
type Shelf struct {
	ID      string  `json:"id"`
	Heading string  `json:"heading"`
	Kind    string  `json:"kind,omitempty"`
	Source  string  `json:"source,omitempty"`
	Reason  string  `json:"reason,omitempty"`
	Titles  []Title `json:"titles"`
}

// SeasonKey returns the canonical storage key for a TV season coordinate.
func SeasonKey(showID, seasonNumber int) string {
	return "tv:" + strconv.Itoa(showID) + ":" + strconv.Itoa(seasonNumber)
}

// CanonicalGenres defines the set of recognized onboarding and discovery genre slugs.
var CanonicalGenres = map[string]string{
	"action":      "Action & adventure",
	"comedy":      "Comedy",
	"drama":       "Drama",
	"crime":       "Crime & mystery",
	"scifi":       "Sci-fi & fantasy",
	"animation":   "Anime",
	"documentary": "Documentaries",
	"horror":      "Horror",
	"family":      "Family",
}

// ValidGenre checks whether a given genre slug is supported by the catalog.
func ValidGenre(genre string) bool {
	_, ok := CanonicalGenres[genre]
	return ok
}
