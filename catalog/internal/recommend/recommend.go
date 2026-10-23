//
//  recommend.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Personalization and recommendation engine for the Stream TV catalog service.
//  Includes taste profile input validation, stable fingerprint hashing,
//  viewing decay calculation, multi-signal candidate ranking, sequel spacing
//  diversification, and presentation views.
//

package recommend

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"math"
	"sort"
	"strings"
	"time"

	"stv-catalog/internal/model"
)

// Recommendations packages the final picks and shelves generated for a profile.
type Recommendations struct {
	Fingerprint string        `json:"fingerprint"`
	Pool        []model.Title `json:"pool"`
	Sections    []model.Shelf `json:"sections"`
}

// ViewingSignal represents caller-supplied aggregate measured viewing metrics for a title.
type ViewingSignal struct {
	model.Identity
	Title              string    `json:"title"`
	Watched            float64   `json:"watched_seconds"`
	Duration           float64   `json:"duration_seconds"`
	Completed          bool      `json:"completed"`
	MeaningfulEpisodes int       `json:"meaningful_episodes"`
	UpdatedAt          time.Time `json:"updated_at"`
}

// Favorite captures a seed title curated by a profile along with its display title.
type Favorite struct {
	model.Identity
	Title string `json:"title"`
}

// Inputs captures profile taste inputs used to synthesize personalized recommendations.
type Inputs struct {
	Completed    bool            `json:"completed"`
	Format       string          `json:"format"`
	Genres       []string        `json:"genres"`
	Favorites    []Favorite      `json:"favorites"`
	Signals      []ViewingSignal `json:"signals"`
	ActivityDays []string        `json:"activity_days"`
}

// Validate ensures inputs conform to bounds, allowed formats, and valid dates.
func (p Inputs) Validate(now time.Time) error {
	if p.Format != "both" && p.Format != "movie" && p.Format != "tv" {
		return errors.New("invalid format")
	}
	if len(p.Genres) > 9 || len(p.Favorites) > 50 || len(p.Signals) > 100 || len(p.ActivityDays) > 30 {
		return errors.New("input limit exceeded")
	}

	seen := map[string]bool{}
	for _, g := range p.Genres {
		if !model.ValidGenre(g) || seen[g] {
			return errors.New("invalid or duplicate genre")
		}
		seen[g] = true
	}

	seen = map[string]bool{}
	for _, f := range p.Favorites {
		if !f.Valid() || len(f.Title) > 300 || seen[f.Key()] {
			return errors.New("invalid favorite")
		}
		seen[f.Key()] = true
	}

	seen = map[string]bool{}
	for _, s := range p.Signals {
		if !s.Valid() || seen[s.Key()] || s.Watched < 0 || s.Duration < 0 || s.MeaningfulEpisodes < 0 ||
			s.MeaningfulEpisodes > 100000 || s.UpdatedAt.IsZero() || s.UpdatedAt.After(now) ||
			math.IsNaN(s.Watched) || math.IsInf(s.Watched, 0) || math.IsNaN(s.Duration) || math.IsInf(s.Duration, 0) ||
			len(s.Title) > 300 {
			return errors.New("invalid viewing signal")
		}
		seen[s.Key()] = true
	}

	seen = map[string]bool{}
	for _, d := range p.ActivityDays {
		t, err := time.Parse("2006-01-02", d)
		if err != nil || t.After(now) || now.Sub(t) > 31*24*time.Hour || seen[d] {
			return errors.New("invalid activity day")
		}
		seen[d] = true
	}
	return nil
}

// Fingerprint produces a stable SHA-256 hash representing the exact taste inputs.
func (p Inputs) Fingerprint() string {
	p.ActivityDays = nil

	genres := make([]string, len(p.Genres))
	copy(genres, p.Genres)
	sort.Strings(genres)
	p.Genres = genres

	favs := make([]Favorite, len(p.Favorites))
	copy(favs, p.Favorites)
	sort.Slice(favs, func(i, j int) bool {
		return favs[i].Key() < favs[j].Key()
	})
	p.Favorites = favs

	sigs := make([]ViewingSignal, len(p.Signals))
	copy(sigs, p.Signals)
	sort.Slice(sigs, func(i, j int) bool {
		return sigs[i].Key() < sigs[j].Key()
	})

	qualified := make([]bool, len(sigs))
	for i := range sigs {
		qualified[i] = Qualifies(sigs[i])
		sigs[i].Watched = math.Floor(sigs[i].Watched/600) * 600
		sigs[i].UpdatedAt = time.Time{}
	}
	p.Signals = sigs

	b, _ := json.Marshal(struct {
		Version   int
		Inputs    Inputs
		Qualified []bool
	}{
		Version:   2,
		Inputs:    p,
		Qualified: qualified,
	})
	h := sha256.Sum256(b)
	return hex.EncodeToString(h[:])
}

// Cadence calculates how frequently recommendations should refresh (daily vs weekly).
func (p Inputs) Cadence(now time.Time) time.Duration {
	n := 0
	start := now.UTC().Truncate(24 * time.Hour).Add(-48 * time.Hour)
	seen := map[string]bool{}
	for _, d := range p.ActivityDays {
		t, err := time.Parse("2006-01-02", d)
		if err == nil && !t.Before(start) && !t.After(now) && !seen[d] {
			n++
			seen[d] = true
		}
	}
	if n >= 2 {
		return 24 * time.Hour
	}
	return 7 * 24 * time.Hour
}

// WeightedSeed represents a candidate title seed paired with an initial relevance weight.
type WeightedSeed struct {
	Favorite
	Weight float64
}

// Seeds extracts taste seeds from favorites and viewing signals using half-life decay.
func (p Inputs) Seeds(now time.Time) []WeightedSeed {
	out := make([]WeightedSeed, 0, len(p.Favorites)+min(20, len(p.Signals)))
	seen := make(map[string]bool, len(p.Favorites)+len(p.Signals))
	for _, f := range p.Favorites {
		out = append(out, WeightedSeed{Favorite: f, Weight: 5})
		seen[f.Key()] = true
	}

	history := make([]WeightedSeed, 0, len(p.Signals))
	for _, s := range p.Signals {
		age := now.Sub(s.UpdatedAt).Hours() / 24
		if age < 0 || age > 60 || seen[s.Key()] {
			continue
		}
		qual := Qualifies(s)
		w := s.Watched/1800 + 1
		if s.Type == "tv" {
			w = float64(s.MeaningfulEpisodes + 1)
		}
		if qual {
			history = append(history, WeightedSeed{
				Favorite: Favorite{Identity: s.Identity, Title: s.Title},
				Weight:   math.Min(5, w) * math.Pow(0.5, age/14),
			})
		}
	}

	sort.Slice(history, func(i, j int) bool {
		if history[i].Weight == history[j].Weight {
			return history[i].Key() < history[j].Key()
		}
		return history[i].Weight > history[j].Weight
	})

	if len(history) > 20 {
		history = history[:20]
	}
	return append(out, history...)
}

// Contribution pairs a recommendation candidate list from TMDB with an originating seed weight.
type Contribution struct {
	Titles []model.Title
	Weight float64
}

// Rank scores and orders candidate titles across all seed contributions with pre-allocated memory pools.
func Rank(lists []Contribution, excluded map[string]bool) []model.Title {
	type entry struct {
		t       model.Title
		score   float64
		matches int
	}

	totalExpected := 0
	for _, l := range lists {
		totalExpected += len(l.Titles)
	}

	pool := make(map[string]*entry, totalExpected)
	for _, l := range lists {
		seen := make(map[string]bool, len(l.Titles))
		for i, t := range l.Titles {
			k := t.Key()
			if excluded[k] || seen[k] || t.Poster == "" {
				continue
			}
			seen[k] = true
			e := pool[k]
			if e == nil {
				e = &entry{t: t}
				pool[k] = e
			}
			e.score += l.Weight / math.Sqrt(float64(i+1))
			e.matches++
		}
	}

	entries := make([]*entry, 0, len(pool))
	for _, e := range pool {
		e.score += math.Log2(float64(e.matches+1))*1.5 + math.Max(0, e.t.Rating-5)*0.15
		entries = append(entries, e)
	}

	sort.Slice(entries, func(i, j int) bool {
		if entries[i].score == entries[j].score {
			if entries[i].t.ID == entries[j].t.ID {
				return entries[i].t.Type < entries[j].t.Type
			}
			return entries[i].t.ID > entries[j].t.ID
		}
		return entries[i].score > entries[j].score
	})

	out := make([]model.Title, len(entries))
	for i, e := range entries {
		out[i] = e.t
	}
	return out
}

// Family extracts the common franchise name from a title to prevent sequel clustering.
func Family(t model.Title) string {
	s := strings.ToLower(t.Title)
	parts := strings.FieldsFunc(s, func(r rune) bool {
		return r == ':' || r == '–' || r == '—' || r == '-'
	})
	if len(parts) == 0 {
		return s
	}
	s = parts[0]
	f := strings.Fields(s)
	if len(f) > 1 {
		last := f[len(f)-1]
		if strings.Trim(last, "0123456789") == "" {
			f = f[:len(f)-1]
		}
	}
	return strings.Join(f, " ")
}

// Diversify applies anti-clustering logic across an entire title pool.
func Diversify(titles []model.Title) []model.Title {
	return DiversifyLimit(titles, len(titles))
}

// DiversifyLimit selects up to limit titles, ensuring titles from the same franchise are spaced out.
func DiversifyLimit(titles []model.Title, limit int) []model.Title {
	n := len(titles)
	target := min(limit, n)
	if target == 0 {
		return []model.Title{}
	}

	families := make([]string, n)
	for i, t := range titles {
		families[i] = Family(t)
	}

	used := make([]bool, n)
	out := make([]model.Title, 0, target)
	chosen := make([]string, 0, target)

	for len(out) < target {
		recent := map[string]bool{}
		for i := max(0, len(chosen)-3); i < len(chosen); i++ {
			recent[chosen[i]] = true
		}

		pick, fallback := -1, -1
		for i := range titles {
			if used[i] {
				continue
			}
			if fallback < 0 {
				fallback = i
			}
			if !recent[families[i]] {
				pick = i
				break
			}
		}

		if pick < 0 {
			pick = fallback
		}
		used[pick] = true
		out = append(out, titles[pick])
		chosen = append(chosen, families[pick])
	}
	return out
}

// SerendipityMix blends high-confidence taste picks (70%) with discovery/critically acclaimed picks (30%)
// to prevent recommendation echo chambers while keeping suggestions deeply relevant.
func SerendipityMix(taste []model.Title, discovery []model.Title, limit int) []model.Title {
	if len(discovery) == 0 {
		return DiversifyLimit(taste, limit)
	}
	if len(taste) == 0 {
		return DiversifyLimit(discovery, limit)
	}

	target := min(limit, len(taste)+len(discovery))
	out := make([]model.Title, 0, target)
	seen := make(map[string]bool, target)

	tIdx, dIdx := 0, 0
	for len(out) < target && (tIdx < len(taste) || dIdx < len(discovery)) {
		for step := 0; step < 7 && tIdx < len(taste) && len(out) < target; step++ {
			t := taste[tIdx]
			tIdx++
			k := t.Key()
			if !seen[k] {
				seen[k] = true
				out = append(out, t)
			}
		}
		for step := 0; step < 3 && dIdx < len(discovery) && len(out) < target; step++ {
			d := discovery[dIdx]
			dIdx++
			k := d.Key()
			if !seen[k] {
				seen[k] = true
				out = append(out, d)
			}
		}
	}
	return DiversifyLimit(out, limit)
}

// ContextualShelf generates a personalized thematic shelf (e.g. "Because you watched Whiplash").
func ContextualShelf(seedTitle string, titles []model.Title) model.Shelf {
	return model.Shelf{
		ID:      "because-" + strings.ToLower(strings.ReplaceAll(strings.TrimSpace(seedTitle), " ", "-")),
		Heading: "Because you watched " + seedTitle,
		Titles:  DiversifyLimit(titles, 20),
	}
}

// View filters recommendation items by preferred category (all, movie, tv).
func (r Recommendations) View(category, format string) Recommendations {
	r.Sections = append([]model.Shelf(nil), r.Sections...)
	filter := func(in []model.Title) []model.Title {
		out := []model.Title{}
		for _, t := range in {
			if category == "all" || t.Type == category {
				out = append(out, t)
			}
		}
		return out
	}

	pool := filter(r.Pool)
	if category == "all" && format != "both" {
		sort.SliceStable(pool, func(i, j int) bool {
			return pool[i].Type == format && pool[j].Type != format
		})
	}

	picks := DiversifyLimit(pool, 24)
	for i := range r.Sections {
		if r.Sections[i].ID == "for-you" {
			r.Sections[i].Titles = picks
		} else {
			r.Sections[i].Titles = filter(r.Sections[i].Titles)
		}
	}
	return r
}

// Qualifies evaluates whether a viewing signal meets engagement thresholds for recommendation seeding.
func Qualifies(s ViewingSignal) bool {
	if s.Type == "tv" {
		return s.Watched >= 1200 && (s.MeaningfulEpisodes >= 2 || s.Watched >= 2700)
	}
	return s.Watched >= 600 && (s.Completed || (s.Duration > 0 && s.Watched >= s.Duration*0.2))
}
