package api

import (
	"sort"
	"strings"
)

// Match the catalog's view semantics using its complete ranked pool. This never
// ranks provider sources or treats catalog discovery as playback availability.
func recommendationView(r Recommendations, category, format string) Recommendations {
	filter := func(in []Title) []Title {
		out := []Title{}
		for _, t := range in {
			if category == "all" || t.Type == category {
				out = append(out, t)
			}
		}
		return out
	}
	pool := filter(r.Pool)
	if category == "all" && format != "both" {
		sort.SliceStable(pool, func(i, j int) bool { return pool[i].Type == format && pool[j].Type != format })
	}
	family := func(t Title) string {
		s := strings.ToLower(t.Title)
		p := strings.FieldsFunc(s, func(r rune) bool { return r == ':' || r == '–' || r == '—' || r == '-' })
		if len(p) == 0 {
			return s
		}
		f := strings.Fields(p[0])
		if len(f) > 1 && strings.Trim(f[len(f)-1], "0123456789") == "" {
			f = f[:len(f)-1]
		}
		return strings.Join(f, " ")
	}
	used := make([]bool, len(pool))
	families := make([]string, len(pool))
	for i, t := range pool {
		families[i] = family(t)
	}
	picks := []Title{}
	chosen := []string{}
	for len(picks) < min(24, len(pool)) {
		recent := map[string]bool{}
		for _, f := range chosen[max(0, len(chosen)-3):] {
			recent[f] = true
		}
		pick, fallback := -1, -1
		for i := range pool {
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
		picks = append(picks, pool[pick])
		chosen = append(chosen, families[pick])
	}
	for i := range r.Sections {
		if r.Sections[i].ID == "for-you" {
			r.Sections[i].Titles = publicTitles(picks)
		} else {
			r.Sections[i].Titles = publicTitles(filter(r.Sections[i].Titles))
		}
	}
	r.Pool = publicTitles(r.Pool)
	return r
}
