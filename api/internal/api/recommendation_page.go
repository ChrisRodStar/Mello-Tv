package api

import (
	"crypto/sha256"
	"encoding/hex"
	"net/http"
	"sort"
	"strings"
	"time"
)

// Home is assembled from retained catalog data and current profile feedback.
// It never contacts TMDB, providers or the relay.
type recommendationPage struct {
	Sections []Shelf `json:"sections"`
	Featured []Title `json:"featured"`
	Version  string  `json:"page_version"`
}

func (s *Service) recommendationExclusions(r *http.Request, pid string) (map[string]bool, map[string]bool, error) {
	excluded, topics := map[string]bool{}, map[string]bool{}
	rows, e := s.db.QueryContext(r.Context(), "SELECT kind,target FROM lists WHERE profile=? AND kind IN ('recommendation-hidden','recommendation-watched','recommendation-topics')", pid)
	if e != nil {
		return nil, nil, e
	}
	for rows.Next() {
		var kind, key string
		if e = rows.Scan(&kind, &key); e != nil {
			rows.Close()
			return nil, nil, e
		}
		if kind == "recommendation-topics" {
			topics[key] = true
		} else {
			excluded[key] = true
		}
	}
	e = rows.Err()
	rows.Close()
	if e != nil {
		return nil, nil, e
	}
	// Completed movies should not be presented as new discoveries. Completing one
	// TV episode must not hide an entire ongoing series.
	rows, e = s.db.QueryContext(r.Context(), "SELECT data FROM progress WHERE profile=? AND type='movie'", pid)
	if e != nil {
		return nil, nil, e
	}
	defer rows.Close()
	for rows.Next() {
		var b []byte
		if e = rows.Scan(&b); e != nil {
			return nil, nil, e
		}
		var p Progress
		if e = decode(b, &p); e != nil {
			return nil, nil, e
		}
		if p.Completed {
			excluded[p.Identity.Key()] = true
		}
	}
	return excluded, topics, rows.Err()
}

func eligibleTitles(titles []Title, category string, excluded map[string]bool, now time.Time) []Title {
	out := []Title{}
	seen := map[string]bool{}
	for _, t := range titles {
		if (category != "all" && t.Type != category) || excluded[t.Key()] || seen[t.Key()] || t.Poster == "" {
			continue
		}
		// An undated title has unknown release status; do not invent a release date.
		if date, e := time.Parse("2006-01-02", t.Date); e == nil && date.After(now) {
			continue
		}
		seen[t.Key()] = true
		out = append(out, t)
	}
	return out
}

func (s *Service) recommendationPage(r *http.Request, p Profile) (recommendationPage, error) {
	category := r.URL.Query().Get("type")
	if category == "" {
		category = "all"
	}
	if category != "all" && category != "movie" && category != "tv" {
		return recommendationPage{}, bad()
	}
	topic := r.URL.Query().Get("topic")
	if topic == "" {
		topic = "all"
	}
	if topic != "all" && topic != "short" && topic != "classics" && topic != "discovery" {
		return recommendationPage{}, bad()
	}
	shelfID := r.URL.Query().Get("shelf")
	if len(shelfID) > 200 || shelfID != "" && !safeID(shelfID) && !(strings.HasPrefix(shelfID, "like-") && identityKey(strings.TrimPrefix(shelfID, "like-"))) {
		return recommendationPage{}, bad()
	}
	excluded, hiddenTopics, e := s.recommendationExclusions(r, p.ID)
	if e != nil {
		return recommendationPage{}, e
	}
	now := time.Now().UTC()
	personal := []Shelf{}
	pool := []Title{}
	if rec, ok := s.record("recommendations", p.ID); ok {
		var v Recommendations
		if e = decode(rec.Data, &v); e != nil {
			return recommendationPage{}, e
		}
		v.Pool = eligibleTitles(v.Pool, category, excluded, now)
		for i := range v.Sections {
			v.Sections[i].Titles = eligibleTitles(v.Sections[i].Titles, category, excluded, now)
		}
		v = recommendationView(v, category, p.Preferences.Format)
		pool = v.Pool
		personal = v.Sections
	}
	// Keep genuinely personal rows ahead of general copies. General rows copied
	// into a daily recommendation snapshot must not shadow fresher browse data.
	candidates := []Shelf{}
	for _, row := range personal {
		if row.ID == "for-you" || strings.HasPrefix(row.ID, "like-") || strings.HasPrefix(row.ID, "taste-") || row.Kind == "personal" || row.Kind == "because" || row.Kind == "taste" {
			candidates = append(candidates, row)
		}
	}
	for _, rec := range s.records("shelf") {
		var row Shelf
		if e = decode(rec.Data, &row); e != nil {
			return recommendationPage{}, e
		}
		candidates = append(candidates, row)
	}
	// Index only titles present in candidate rows, avoiding a full snapshot scan
	// for every poster. No catalog requests are made on this path.
	wanted := map[string]bool{}
	for _, row := range candidates {
		for _, title := range row.Titles {
			wanted[title.Key()] = true
		}
	}
	details := map[string]Title{}
	for _, rec := range s.records("title") {
		if !wanted[rec.Key] {
			continue
		}
		var title Title
		if decode(rec.Data, &title) == nil && title.Details {
			details[title.Key()] = title
		}
	}
	seenRows := map[string]bool{}
	rows := []Shelf{}
	for _, row := range candidates {
		if seenRows[row.ID] || hiddenTopics[row.ID] || strings.HasPrefix(row.ID, "onboarding-") || strings.HasPrefix(row.ID, "favorites-") {
			continue
		}
		seenRows[row.ID] = true
		// Release/airing lists belong in catalog browsing, not watch-tonight suggestions.
		if row.ID == "movie-upcoming" || row.ID == "movie-now-playing" || row.ID == "tv-airing-today" || row.ID == "tv-on-air" {
			continue
		}
		// Enrich copies only: saved snapshot slices must never be mutated.
		enriched := append([]Title(nil), row.Titles...)
		for i, title := range enriched {
			if cached, ok := details[title.Key()]; ok {
				enriched[i] = cached
			}
		}
		row.Titles = publicTitles(eligibleTitles(enriched, category, excluded, now))
		if row.ID == "topic-classics" {
			older := []Title{}
			for _, title := range row.Titles {
				date, err := time.Parse("2006-01-02", title.Date)
				if err != nil || date.Year() < 2000 {
					older = append(older, title)
				}
			}
			row.Titles = older
		}
		if row.ID == "topic-short-movies" {
			short := []Title{}
			for _, title := range row.Titles {
				// Unknown runtime is allowed here because this shelf was retrieved
				// using TMDB's runtime filter. Known conflicting details win.
				if title.Type == "movie" && title.Runtime < 100 {
					short = append(short, title)
				}
			}
			row.Titles = short
		}
		if len(row.Titles) == 0 {
			continue
		}
		if row.Kind == "" {
			switch {
			case row.ID == "for-you":
				row.Kind = "personal"
			case strings.HasPrefix(row.ID, "like-"):
				row.Kind = "because"
			case strings.HasPrefix(row.ID, "taste-"):
				row.Kind = "taste"
			case strings.HasPrefix(row.ID, "topic-"):
				row.Kind = "topic"
			default:
				row.Kind = "browse"
			}
		}
		if row.ID == "trending" {
			row.Kind = "trending"
			row.Source = "tmdb-weekly"
			row.Heading = "Trending on TMDB this week"
		}
		rows = append(rows, row)
	}
	// Discovery is explicitly outside the ranked taste pool, rather than a
	// claimed confidence percentage or random high-rating list.
	familiar := map[string]bool{}
	for _, t := range pool {
		familiar[t.Key()] = true
	}
	discovery := []Title{}
	discoverySeen := map[string]bool{}
	for _, row := range rows {
		if row.Kind != "topic" && row.Kind != "trending" && row.Kind != "browse" {
			continue
		}
		for _, t := range row.Titles {
			if !familiar[t.Key()] && !discoverySeen[t.Key()] {
				discovery = append(discovery, t)
				discoverySeen[t.Key()] = true
			}
		}
	}
	if len(familiar) > 0 && len(discovery) >= 4 {
		rows = append(rows, Shelf{ID: "discovery", Heading: "Try something different", Kind: "discovery", Source: "outside-taste-pool", Titles: discovery})
	}
	// Prefer specific, complementary rows; title scoring and decay are unchanged.
	affinities := map[string]int{}
	priority := func(row Shelf) int {
		switch row.Kind {
		case "personal":
			return 0
		case "because":
			return 10
		case "topic":
			if affinities[row.ID] > 0 {
				return 20
			}
			return 40
		case "taste":
			return 30
		case "discovery":
			return 35
		case "trending":
			return 45
		}
		return 50
	}
	for _, row := range rows {
		matches := 0
		for _, title := range row.Titles {
			if familiar[title.Key()] {
				matches += 2
			}
			for _, genre := range title.Genres {
				value := strings.ToLower(genre)
				for _, preferred := range p.Preferences.Genres {
					match := value == preferred && preferred != "animation" ||
						preferred == "scifi" && (strings.Contains(value, "sci") || value == "fantasy") ||
						preferred == "crime" && value == "mystery" || preferred == "action" && strings.Contains(value, "adventure") ||
						preferred == "documentary" && value == "documentary"
					if match {
						matches++
						break
					}
				}
			}
		}
		affinities[row.ID] = matches * 100 / len(row.Titles)
	}
	sort.SliceStable(rows, func(i, j int) bool {
		a, b := priority(rows[i]), priority(rows[j])
		if a != b {
			return a < b
		}
		if affinities[rows[i].ID] != affinities[rows[j].ID] {
			return affinities[rows[i].ID] > affinities[rows[j].ID]
		}
		return rows[i].ID < rows[j].ID
	})
	page := recommendationPage{Sections: []Shelf{}, Featured: []Title{}}
	used := map[string]bool{}
	counts := map[string]int{}
	limits := map[string]int{"personal": 1, "because": 2, "topic": 3, "taste": 2, "browse": 2, "trending": 1, "discovery": 1}
	for _, row := range rows {
		if shelfID != "" && row.ID != shelfID {
			continue
		}
		if topic == "short" && row.ID != "topic-short-movies" {
			continue
		}
		if topic == "classics" && row.ID != "topic-classics" {
			continue
		}
		if topic == "discovery" && row.Kind != "discovery" {
			continue
		}
		if counts[row.Kind] >= limits[row.Kind] {
			continue
		}
		titles := []Title{}
		limit := 12
		if shelfID != "" || topic != "all" {
			limit = 100
		}
		for _, t := range row.Titles {
			if !used[t.Key()] {
				titles = append(titles, t)
			}
			if len(titles) >= limit {
				break
			}
		}
		minimum := 3
		if row.Kind == "personal" || topic != "all" || shelfID != "" {
			minimum = 1
		}
		if len(titles) < minimum {
			continue
		}
		for _, t := range titles {
			used[t.Key()] = true
		}
		row.Titles = titles
		page.Sections = append(page.Sections, row)
		counts[row.Kind]++
		if len(page.Sections) >= 12 {
			break
		}
	}
	// A warming catalog may have fewer than three usable titles. Keep one
	// honest, short row rather than declaring the entire catalog empty.
	if topic == "all" && shelfID == "" && len(page.Sections) == 0 && len(rows) > 0 {
		row := rows[0]
		row.Titles = row.Titles[:min(12, len(row.Titles))]
		page.Sections = append(page.Sections, row)
	}
	// Feature one title, reserving it so it is not repeated in a visible shelf.
	if topic == "all" && shelfID == "" && len(page.Sections) > 0 && len(page.Sections[0].Titles) >= 4 {
		first := &page.Sections[0]
		page.Featured = append(page.Featured, first.Titles[0])
		first.Titles = first.Titles[1:]
	}
	digest := sha256.Sum256(marshal(struct {
		Sections []Shelf
		Featured []Title
	}{page.Sections, page.Featured}))
	page.Version = hex.EncodeToString(digest[:8])
	return page, nil
}

func (s *Service) recommendationTopics(w http.ResponseWriter, r *http.Request, p Profile, parts []string) error {
	if len(parts) == 0 && r.Method == "GET" {
		_, topics, e := s.recommendationExclusions(r, p.ID)
		if e != nil {
			return e
		}
		out := []string{}
		for id := range topics {
			out = append(out, id)
		}
		sort.Strings(out)
		write(w, 200, map[string]any{"topics": out})
		return nil
	}
	if len(parts) != 1 || !safeID(parts[0]) || (!strings.HasPrefix(parts[0], "topic-") && !strings.HasPrefix(parts[0], "taste-")) {
		return bad()
	}
	if r.Method != "PUT" && r.Method != "DELETE" {
		return missing()
	}
	if r.Method == "DELETE" {
		_, e := s.db.ExecContext(r.Context(), "DELETE FROM lists WHERE profile=? AND kind='recommendation-topics' AND target=?", p.ID, parts[0])
		if e != nil {
			return e
		}
	} else {
		// Unknown topic IDs cannot fill profile storage with arbitrary records.
		known := false
		for _, rec := range s.records("shelf") {
			if rec.Key == parts[0] {
				known = true
				break
			}
		}
		if !known {
			if rec, ok := s.record("recommendations", p.ID); ok {
				var v Recommendations
				if decode(rec.Data, &v) == nil {
					for _, row := range v.Sections {
						if row.ID == parts[0] {
							known = true
							break
						}
					}
				}
			}
		}
		if !known {
			return missing()
		}
		_, e := s.db.ExecContext(r.Context(), "INSERT INTO lists VALUES(?,'recommendation-topics',?,?) ON CONFLICT(profile,kind,target) DO NOTHING", p.ID, parts[0], marshal(map[string]string{"id": parts[0]}))
		if e != nil {
			return e
		}
	}
	w.WriteHeader(204)
	return nil
}

// A single read supplies the reversible controls on web and iOS.
func (s *Service) recommendationFeedback(w http.ResponseWriter, r *http.Request, p Profile) error {
	lists := map[string][]Title{"hidden": {}, "watched": {}, "ignored": {}, "favorites": {}}
	topics := []string{}
	rows, e := s.db.QueryContext(r.Context(), "SELECT kind,target,data FROM lists WHERE profile=? AND kind IN ('recommendation-hidden','recommendation-watched','recommendation-ignored','favorites','recommendation-topics') ORDER BY kind,target", p.ID)
	if e != nil {
		return e
	}
	for rows.Next() {
		var kind, target string
		var data []byte
		if e = rows.Scan(&kind, &target, &data); e != nil {
			rows.Close()
			return e
		}
		if kind == "recommendation-topics" {
			topics = append(topics, target)
			continue
		}
		var title Title
		if e = decode(data, &title); e != nil {
			rows.Close()
			return e
		}
		kind = strings.TrimPrefix(kind, "recommendation-")
		lists[kind] = append(lists[kind], title)
	}
	e = rows.Err()
	rows.Close()
	if e != nil {
		return e
	}
	for kind, titles := range lists {
		for i, title := range titles {
			if cached := s.cachedTitle(title.Identity); cached.Title != "" {
				title = cached
			}
			titles[i] = publicTitle(title)
		}
		lists[kind] = titles
	}
	write(w, 200, map[string]any{"hidden": lists["hidden"], "watched": lists["watched"], "ignored": lists["ignored"], "favorites": lists["favorites"], "topics": topics})
	return nil
}
