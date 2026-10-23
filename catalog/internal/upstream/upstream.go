//
//  upstream.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  HTTP client and parser for upstream metadata providers (The Movie Database / TMDB).
//  Implements bounded concurrency, rate-limiting cooldowns, response size limits,
//  and translation of upstream payloads into domain catalog models.
//

package upstream

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/rand/v2"
	"net"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"stv-catalog/internal/model"
)

// Upstream coordinates rate-limited, fail-safe HTTP communication with TMDB.
type Upstream struct {
	Base      string
	ImageBase string
	Token     string
	Client    *http.Client
	Interval  time.Duration

	mu      sync.Mutex
	next    time.Time
	blocked time.Time
	sem     chan struct{}
}

// NewUpstream initializes a TMDB API client with connection pooling, keep-alives, and conservative concurrency.
func NewUpstream(token string) *Upstream {
	transport := &http.Transport{
		Proxy: http.ProxyFromEnvironment,
		DialContext: (&net.Dialer{
			Timeout:   5 * time.Second,
			KeepAlive: 30 * time.Second,
		}).DialContext,
		ForceAttemptHTTP2:     true,
		MaxIdleConns:          20,
		MaxIdleConnsPerHost:   10,
		IdleConnTimeout:       90 * time.Second,
		TLSHandshakeTimeout:   5 * time.Second,
		ExpectContinueTimeout: 1 * time.Second,
	}

	return &Upstream{
		Base:      "https://api.themoviedb.org/3/",
		ImageBase: "https://image.tmdb.org/t/p/",
		Token:     token,
		Client: &http.Client{
			Transport: transport,
			Timeout:   8 * time.Second,
			CheckRedirect: func(*http.Request, []*http.Request) error {
				return errors.New("redirect rejected")
			},
		},
		sem:      make(chan struct{}, 6),
		Interval: 200 * time.Millisecond,
	}
}

type semCloser struct {
	io.ReadCloser
	sem  chan struct{}
	once sync.Once
}

func (s *semCloser) Close() error {
	err := s.ReadCloser.Close()
	s.once.Do(func() {
		<-s.sem
	})
	return err
}

func (u *Upstream) execute(ctx context.Context, path string, art bool) (*http.Response, int64, error) {
	if art {
		return nil, 0, errors.New("artwork is delivered directly to clients")
	}
	if u.Token == "" {
		return nil, 0, errors.New("updater unconfigured")
	}

	select {
	case u.sem <- struct{}{}:
	case <-ctx.Done():
		return nil, 0, ctx.Err()
	}
	semAcquired := true
	defer func() {
		if semAcquired {
			<-u.sem
		}
	}()

	u.mu.Lock()
	now := time.Now()
	if now.Before(u.blocked) {
		u.mu.Unlock()
		return nil, 0, errors.New("upstream cooldown")
	}

	slot := u.next
	if slot.Before(now) {
		slot = now
	}
	u.next = slot.Add(u.Interval)
	u.mu.Unlock()

	timer := time.NewTimer(time.Until(slot))
	defer timer.Stop()

	select {
	case <-timer.C:
	case <-ctx.Done():
		return nil, 0, ctx.Err()
	}

	u.mu.Lock()
	blocked := time.Now().Before(u.blocked)
	u.mu.Unlock()
	if blocked {
		return nil, 0, errors.New("upstream cooldown")
	}

	base := u.Base
	limit := int64(4 << 20) // 4 MB limit for metadata responses
	if art {
		base = u.ImageBase
		limit = 8 << 20 // 8 MB limit for artwork responses
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, base+path, nil)
	if err != nil {
		return nil, 0, errors.New("invalid upstream path")
	}

	if !art {
		req.Header.Set("Authorization", "Bearer "+u.Token)
		req.Header.Set("Accept", "application/json")
	}

	res, err := u.Client.Do(req)
	if err != nil {
		return nil, 0, errors.New("upstream request failed")
	}

	if res.StatusCode != http.StatusOK {
		defer res.Body.Close()
		if res.StatusCode == http.StatusTooManyRequests || res.StatusCode >= 500 {
			delay := 30 * time.Minute
			if res.StatusCode == http.StatusTooManyRequests {
				if secs, err := strconv.Atoi(res.Header.Get("Retry-After")); err == nil {
					delay = max(time.Minute, min(time.Hour, time.Duration(secs)*time.Second))
				} else if t, err := http.ParseTime(res.Header.Get("Retry-After")); err == nil {
					delay = max(time.Minute, min(time.Hour, time.Until(t)))
				}
			}
			// Randomized jitter prevents thundering herds across concurrent workers
			jitter := time.Duration(rand.IntN(5000)) * time.Millisecond
			delay += jitter

			u.mu.Lock()
			u.blocked = time.Now().Add(delay)
			u.mu.Unlock()
		}
		return nil, 0, fmt.Errorf("upstream status %d", res.StatusCode)
	}

	if res.ContentLength > limit {
		defer res.Body.Close()
		return nil, 0, errors.New("upstream response too large")
	}

	res.Body = &semCloser{ReadCloser: res.Body, sem: u.sem}
	semAcquired = false
	return res, limit, nil
}

// Fetch executes an HTTP GET request to TMDB with rate limiting, concurrency bounding, and payload size controls.
func (u *Upstream) Fetch(ctx context.Context, path string, art bool) ([]byte, string, error) {
	res, limit, err := u.execute(ctx, path, art)
	if err != nil {
		return nil, "", err
	}
	defer res.Body.Close()

	body, err := io.ReadAll(io.LimitReader(res.Body, limit+1))
	if err != nil || int64(len(body)) > limit {
		return nil, "", errors.New("invalid upstream response size")
	}

	contentType := strings.Split(res.Header.Get("Content-Type"), ";")[0]
	return body, contentType, nil
}

// JSON fetches and unmarshals JSON from an upstream API endpoint using a streaming decoder.
func (u *Upstream) JSON(ctx context.Context, path string, out any) error {
	res, limit, err := u.execute(ctx, path, false)
	if err != nil {
		return err
	}
	defer res.Body.Close()

	dec := json.NewDecoder(io.LimitReader(res.Body, limit+1))
	if err = dec.Decode(out); err != nil {
		return errors.New("malformed upstream JSON")
	}
	return nil
}

var artPath = regexp.MustCompile(`^/[A-Za-z0-9_-]+\.(jpg|jpeg|png|webp)$`)

// ArtworkKey validates that the requested image size and filename are allowed.
func ArtworkKey(size, path string) string {
	if (size != "w300" && size != "w500" && size != "w1280") || !artPath.MatchString(path) {
		return ""
	}
	return size + path
}

// ArtworkURL formats the canonical local redirect URL for artwork.
func ArtworkURL(size, path string) string {
	key := ArtworkKey(size, path)
	if key == "" {
		return ""
	}
	return "/internal/v1/artwork/" + key
}

// ArtKeyFromURL parses the artwork key from an internal URL path.
func ArtKeyFromURL(path string) string {
	const prefix = "/internal/v1/artwork/"
	if !strings.HasPrefix(path, prefix) {
		return ""
	}
	key := strings.TrimPrefix(path, prefix)
	size, filename, ok := strings.Cut(key, "/")
	if !ok {
		return ""
	}
	return ArtworkKey(size, "/"+filename)
}

type tmdbImageItem struct {
	FilePath    string  `json:"file_path"`
	Iso6391     *string `json:"iso_639_1"`
	VoteAverage float64 `json:"vote_average"`
	VoteCount   int     `json:"vote_count"`
	Width       int     `json:"width"`
	Height      int     `json:"height"`
}

type tmdbImages struct {
	Posters []tmdbImageItem `json:"posters"`
}

// selectEnglishPoster selects the highest-voted English poster with title typography, falling back if none match.
func selectEnglishPoster(posters []tmdbImageItem, fallback string) string {
	var bestPath string
	var bestScore float64
	bestVotes := -1

	for _, p := range posters {
		if p.FilePath == "" || !artPath.MatchString(p.FilePath) {
			continue
		}
		if p.Iso6391 != nil && strings.EqualFold(*p.Iso6391, "en") {
			score := float64(p.VoteCount)*10.0 + p.VoteAverage
			if p.VoteCount > bestVotes || (p.VoteCount == bestVotes && score > bestScore) {
				bestPath = p.FilePath
				bestScore = score
				bestVotes = p.VoteCount
			}
		}
	}
	if bestPath != "" {
		return bestPath
	}
	return fallback
}

type tmdbTitle struct {
	ID             int        `json:"id"`
	Type           string     `json:"media_type"`
	Title          string     `json:"title"`
	Name           string     `json:"name"`
	Overview       string     `json:"overview"`
	Date           string     `json:"release_date"`
	AirDate        string     `json:"first_air_date"`
	Rating         float64    `json:"vote_average"`
	Poster         string     `json:"poster_path"`
	Backdrop       string     `json:"backdrop_path"`
	Images         tmdbImages `json:"images"`
	Runtime        int        `json:"runtime"`
	EpisodeRuntime []int      `json:"episode_run_time"`
	IMDb           string     `json:"imdb_id"`
	External       struct {
		IMDb string `json:"imdb_id"`
	} `json:"external_ids"`
	GenreIDs []int `json:"genre_ids"`
	Genres   []struct {
		Name string `json:"name"`
	} `json:"genres"`
	Seasons []struct {
		Number int    `json:"season_number"`
		Name   string `json:"name"`
		Count  int    `json:"episode_count"`
	} `json:"seasons"`
}

var genreNames = map[int]string{
	28: "Action", 12: "Adventure", 16: "Animation", 35: "Comedy", 80: "Crime",
	99: "Documentary", 18: "Drama", 10751: "Family", 14: "Fantasy", 36: "History",
	27: "Horror", 10402: "Music", 9648: "Mystery", 10749: "Romance",
	878: "Science Fiction", 10770: "TV Movie", 53: "Thriller", 10752: "War", 37: "Western",
	10759: "Action & Adventure", 10762: "Kids", 10763: "News", 10764: "Reality",
	10765: "Sci-Fi & Fantasy", 10766: "Soap", 10767: "Talk", 10768: "War & Politics",
}

// convert transforms a raw TMDB title payload into a canonical domain Title.
func (t tmdbTitle) convert(typ string, details bool) (model.Title, error) {
	if t.Type != "" {
		typ = t.Type
	}
	name := t.Title
	if name == "" {
		name = t.Name
	}
	id := model.Identity{Type: typ, ID: t.ID}
	if !id.Valid() || strings.TrimSpace(name) == "" || len(name) > 1000 || t.Rating < 0 || t.Rating > 10 || len(t.Seasons) > 1000 {
		return model.Title{}, errors.New("invalid title response")
	}

	date := t.Date
	if date == "" {
		date = t.AirDate
	}

	poster := t.Poster
	if len(t.Images.Posters) > 0 {
		poster = selectEnglishPoster(t.Images.Posters, t.Poster)
	}

	out := model.Title{
		Identity: id,
		Title:    name,
		Overview: t.Overview,
		Date:     date,
		Rating:   t.Rating,
		Poster:   ArtworkURL("w500", poster),
		Backdrop: ArtworkURL("w1280", t.Backdrop),
		IMDb:     t.IMDb,
		Runtime:  t.Runtime,
		Details:  details,
	}

	if out.IMDb == "" {
		out.IMDb = t.External.IMDb
	}
	if out.Runtime == 0 && len(t.EpisodeRuntime) > 0 {
		out.Runtime = t.EpisodeRuntime[0]
	}
	for _, g := range t.Genres {
		out.Genres = append(out.Genres, g.Name)
	}

	// List responses use genre IDs; detail responses use named genres.
	if len(out.Genres) == 0 {
		for _, genreID := range t.GenreIDs {
			if name := genreNames[genreID]; name != "" {
				out.Genres = append(out.Genres, name)
			}
		}
	}
	seen := map[int]bool{}
	for _, s := range t.Seasons {
		if s.Number < 0 || s.Count < 0 || seen[s.Number] {
			return model.Title{}, errors.New("invalid seasons")
		}
		seen[s.Number] = true
		out.Seasons = append(out.Seasons, model.SeasonSummary{
			Number:       s.Number,
			Name:         s.Name,
			EpisodeCount: s.Count,
		})
	}

	return out, nil
}

// Title fetches full detail for a specific movie or TV series from TMDB.
func (u *Upstream) Title(ctx context.Context, id model.Identity) (model.Title, error) {
	var fields map[string]json.RawMessage
	path := fmt.Sprintf("%s/%d?append_to_response=external_ids,images&include_image_language=en,null&language=en-US", id.Type, id.ID)
	if err := u.JSON(ctx, path, &fields); err != nil {
		return model.Title{}, err
	}

	required := []string{"genres", "overview", "poster_path", "backdrop_path"}
	if id.Type == "movie" {
		required = append(required, "runtime")
	} else {
		required = append(required, "seasons")
	}

	for _, key := range required {
		if _, ok := fields[key]; !ok {
			return model.Title{}, errors.New("incomplete title details")
		}
	}

	if bytes.Equal(bytes.TrimSpace(fields["genres"]), []byte("null")) ||
		(id.Type == "tv" && bytes.Equal(bytes.TrimSpace(fields["seasons"]), []byte("null"))) {
		return model.Title{}, errors.New("invalid title detail arrays")
	}

	data, err := json.Marshal(fields)
	if err != nil {
		return model.Title{}, errors.New("invalid title details")
	}

	var raw tmdbTitle
	if json.Unmarshal(data, &raw) != nil {
		return model.Title{}, errors.New("malformed title details")
	}

	title, err := raw.convert(id.Type, true)
	if err == nil && title.Identity != id {
		return model.Title{}, errors.New("title identity mismatch")
	}

	return title, err
}

// List fetches a collection of titles from a TMDB list or discover endpoint.
func (u *Upstream) List(ctx context.Context, path, typ string) ([]model.Title, error) {
	var raw struct {
		Results *[]tmdbTitle `json:"results"`
	}
	if err := u.JSON(ctx, path, &raw); err != nil {
		return nil, err
	}
	if raw.Results == nil || len(*raw.Results) > 1000 {
		return nil, errors.New("invalid title list")
	}

	out := []model.Title{}
	seen := map[string]bool{}
	for _, v := range *raw.Results {
		if v.Type != "" && v.Type != "movie" && v.Type != "tv" {
			continue
		}
		title, err := v.convert(typ, false)
		if err != nil {
			return nil, err
		}
		if !seen[title.Key()] {
			seen[title.Key()] = true
			out = append(out, title)
		}
	}
	return out, nil
}

// Season fetches full episode coordinates and summaries for a specific TV season.
func (u *Upstream) Season(ctx context.Context, id, number int) (model.Season, error) {
	var raw struct {
		Number   *int   `json:"season_number"`
		Name     string `json:"name"`
		Episodes *[]struct {
			ID       int    `json:"id"`
			Season   *int   `json:"season_number"`
			Number   int    `json:"episode_number"`
			Name     string `json:"name"`
			Overview string `json:"overview"`
			Runtime  int    `json:"runtime"`
			Still    string `json:"still_path"`
			Date     string `json:"air_date"`
		} `json:"episodes"`
	}

	path := fmt.Sprintf("tv/%d/season/%d", id, number)
	if err := u.JSON(ctx, path, &raw); err != nil {
		return model.Season{}, err
	}

	if raw.Number == nil || *raw.Number != number || raw.Episodes == nil || len(*raw.Episodes) > 1000 {
		return model.Season{}, errors.New("invalid season response")
	}

	out := model.Season{
		ShowID:   id,
		Number:   number,
		Name:     raw.Name,
		Episodes: []model.Episode{},
	}

	seen := map[int]bool{}
	for _, v := range *raw.Episodes {
		if v.ID <= 0 || v.Number <= 0 || v.Season == nil || *v.Season != number || v.Name == "" || seen[v.Number] {
			return model.Season{}, errors.New("invalid episode coordinates")
		}
		seen[v.Number] = true
		out.Episodes = append(out.Episodes, model.Episode{
			ID:       v.ID,
			Season:   number,
			Number:   v.Number,
			Name:     v.Name,
			Overview: v.Overview,
			Runtime:  v.Runtime,
			Still:    ArtworkURL("w300", v.Still),
			Date:     v.Date,
		})
	}

	return out, nil
}

// GenreSpec defines TMDB genre mapping attributes for discovery.
type GenreSpec struct {
	Name  string
	Movie string
	TV    string
}

// GenreSpecs maps standard genre keys to TMDB category IDs.
var GenreSpecs = map[string]GenreSpec{
	"action":      {"Action & adventure", "28|12", "10759"},
	"comedy":      {"Comedy", "35", "35"},
	"drama":       {"Drama", "18", "18"},
	"crime":       {"Crime & mystery", "80|9648", "80|9648"},
	"scifi":       {"Sci-fi & fantasy", "878|14", "10765"},
	"animation":   {"Anime", "16", "16"},
	"documentary": {"Documentaries", "99", "99"},
	"horror":      {"Horror", "27", ""},
	"family":      {"Family", "10751", "10751"},
}

// DiscoverPath constructs a TMDB discover URL for a given genre, media type, and timestamp.
func (u *Upstream) DiscoverPath(ctx context.Context, genre, typ string, now time.Time) (string, error) {
	g := GenreSpecs[genre]
	id := g.Movie
	if typ == "tv" {
		id = g.TV
	}

	p := url.Values{
		"include_adult":  {"false"},
		"sort_by":        {"popularity.desc"},
		"vote_count.gte": {"50"},
		"language":       {"en-US"},
	}

	if genre == "animation" {
		p.Set("with_original_language", "ja")
	}

	if id != "" {
		p.Set("with_genres", id)
	} else {
		var raw struct {
			Results []struct {
				ID   int    `json:"id"`
				Name string `json:"name"`
			} `json:"results"`
		}
		if err := u.JSON(ctx, "search/keyword?query=horror", &raw); err != nil {
			return "", err
		}
		for _, k := range raw.Results {
			if strings.EqualFold(k.Name, "horror") && k.ID > 0 {
				p.Set("with_keywords", strconv.Itoa(k.ID))
				break
			}
		}
		if p.Get("with_keywords") == "" {
			return "", errors.New("horror keyword missing")
		}
	}

	dateField := "primary_release_date"
	if typ == "tv" {
		dateField = "first_air_date"
	}
	p.Set(dateField+".lte", now.UTC().Format("2006-01-02"))

	return "discover/" + typ + "?" + p.Encode(), nil
}

// BrowseSpec outlines query parameters and display heading for curated shelves.
type BrowseSpec struct {
	ID      string
	Heading string
	Path    string
	Type    string
}

// OnboardingArtworkTitles anchors specific memorable titles for profile taste selection.
var OnboardingArtworkTitles = map[string]int{
	"onboarding-action":      299536, // Avengers: Infinity War
	"onboarding-comedy":      18785,  // The Hangover
	"onboarding-drama":       244786, // Whiplash
	"onboarding-crime":       414906, // The Batman
	"onboarding-scifi":       157336, // Interstellar
	"onboarding-animation":   129,    // Spirited Away
	"onboarding-documentary": 664280, // David Attenborough: A Life on Our Planet
	"onboarding-horror":      138843, // The Conjuring
}

// OnboardingBrowse builds discovery shelf queries for onboarding artwork selections.
func OnboardingBrowse(now time.Time) []BrowseSpec {
	specs := make([]BrowseSpec, 0, 8)
	for _, genre := range []string{"action", "comedy", "drama", "crime", "scifi", "animation", "documentary", "horror"} {
		language := "en"
		if genre == "animation" {
			language = "ja"
		}
		params := url.Values{
			"include_adult":            {"false"},
			"sort_by":                  {"popularity.desc"},
			"with_original_language":   {language},
			"with_genres":              {GenreSpecs[genre].Movie},
			"vote_count.gte":           {"100"},
			"primary_release_date.gte": {"2000-01-01"},
			"primary_release_date.lte": {now.UTC().Format("2006-01-02")},
			"language":                 {"en-US"},
		}
		specs = append(specs, BrowseSpec{
			ID:      "onboarding-" + genre,
			Heading: GenreSpecs[genre].Name,
			Path:    "discover/movie?" + params.Encode(),
			Type:    "movie",
		})
	}
	return specs
}

// FavoriteBrowse generates candidate seed shelf discovery queries.
func (u *Upstream) FavoriteBrowse(ctx context.Context, now time.Time) ([]BrowseSpec, error) {
	specs := make([]BrowseSpec, 0, 16)
	for _, genre := range []string{"action", "comedy", "drama", "crime", "scifi", "animation", "documentary", "horror"} {
		for _, typ := range []string{"movie", "tv"} {
			path, err := u.DiscoverPath(ctx, genre, typ, now)
			if err != nil {
				return nil, err
			}
			parsed, err := url.Parse(path)
			if err != nil {
				return nil, err
			}
			params := parsed.Query()
			params.Set("sort_by", "vote_count.desc")
			params.Set("vote_average.gte", "6.5")
			params.Set("vote_count.gte", "300")
			params.Set("language", "en-US")
			dateField := "primary_release_date"
			if typ == "tv" {
				dateField = "first_air_date"
			}
			params.Set(dateField+".gte", "1980-01-01")
			parsed.RawQuery = params.Encode()
			specs = append(specs, BrowseSpec{
				ID:      "favorites-" + typ + "-" + genre,
				Heading: GenreSpecs[genre].Name,
				Path:    parsed.String(),
				Type:    typ,
			})
		}
	}
	return specs, nil
}

// Browse holds the standard set of default curated home page shelves.
var Browse = []BrowseSpec{
	{"trending", "Trending this week", "trending/all/week?language=en-US", ""},
	{"movie-popular", "Popular movies", "movie/popular?language=en-US", "movie"},
	{"movie-top-rated", "Top rated movies", "movie/top_rated?language=en-US", "movie"},
	{"movie-now-playing", "In theaters", "movie/now_playing?language=en-US", "movie"},
	{"movie-upcoming", "Upcoming movies", "movie/upcoming?language=en-US", "movie"},
	{"movie-action", "Action & adventure", "discover/movie?with_genres=28&include_adult=false&sort_by=popularity.desc&language=en-US", "movie"},
	{"movie-comedy", "Comedy movies", "discover/movie?with_genres=35&include_adult=false&sort_by=popularity.desc&language=en-US", "movie"},
	{"tv-popular", "Popular TV shows", "tv/popular?language=en-US", "tv"},
	{"tv-top-rated", "Top rated TV shows", "tv/top_rated?language=en-US", "tv"},
	{"tv-on-air", "On the air", "tv/on_the_air?language=en-US", "tv"},
	{"tv-airing-today", "Airing today", "tv/airing_today?language=en-US", "tv"},
	{"tv-drama", "Drama series", "discover/tv?with_genres=18&include_adult=false&sort_by=popularity.desc&language=en-US", "tv"},
	{"tv-crime", "Crime series", "discover/tv?with_genres=80&include_adult=false&sort_by=popularity.desc&language=en-US", "tv"},
}

// TopicBrowse describes a small, factual discovery library. Missing keyword
// lookups omit only that topic; saved shelves survive upstream errors.
func (u *Upstream) TopicBrowse(ctx context.Context, now time.Time) []BrowseSpec {
	type topic struct{ id, heading, typ, genres, keyword string }
	topics := []topic{
		{"heists", "Heist movies", "movie", "", "heist"},
		{"time-travel", "Time travel", "movie", "", "time travel"},
		{"detectives", "Detective stories", "tv", "", "detective"},
		{"romantic-comedy", "Romantic comedies", "movie", "35,10749", ""},
		{"historical-drama", "Historical dramas", "movie", "18,36", ""},
		{"animated-adventures", "Animated adventures", "movie", "16,12", ""},
		{"documentaries", "Documentaries", "movie", "99", ""},
		{"mysteries", "Mystery series", "tv", "9648", ""},
		{"fantasy", "Fantasy films", "movie", "14", ""},
		{"short-movies", "Under 100 minutes", "movie", "", ""},
		{"classics", "Before 2000", "movie", "", ""},
	}
	out := []BrowseSpec{}
	for _, t := range topics {
		if ctx.Err() != nil {
			break
		}
		p := url.Values{"include_adult": {"false"}, "language": {"en-US"},
			"sort_by": {"popularity.desc"}, "vote_count.gte": {"50"}}
		date := "primary_release_date"
		if t.typ == "tv" {
			date = "first_air_date"
		}
		p.Set(date+".lte", now.UTC().Format("2006-01-02"))
		if t.genres != "" {
			p.Set("with_genres", t.genres)
		}
		if t.id == "short-movies" {
			p.Set("with_runtime.gte", "1")
			p.Set("with_runtime.lte", "99")
		}
		if t.id == "classics" {
			p.Set("primary_release_date.lte", "1999-12-31")
		}
		if t.keyword != "" {
			var raw struct {
				Results []struct {
					ID   int    `json:"id"`
					Name string `json:"name"`
				} `json:"results"`
			}
			if u.JSON(ctx, "search/keyword?query="+url.QueryEscape(t.keyword), &raw) != nil {
				continue
			}
			for _, k := range raw.Results {
				if k.ID > 0 && strings.EqualFold(k.Name, t.keyword) {
					p.Set("with_keywords", strconv.Itoa(k.ID))
					break
				}
			}
			if p.Get("with_keywords") == "" {
				continue
			}
		}
		out = append(out, BrowseSpec{ID: "topic-" + t.id, Heading: t.heading,
			Path: "discover/" + t.typ + "?" + p.Encode(), Type: t.typ})
	}
	return out
}
