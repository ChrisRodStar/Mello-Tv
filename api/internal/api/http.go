package api

import (
	"database/sql"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"
)

func (s *Service) route(w http.ResponseWriter, r *http.Request) error {
	path := r.URL.Path
	method := r.Method
	if path == "/health/livez" && method == "GET" {
		write(w, 200, map[string]string{"version": "api.v1", "status": "alive"})
		return nil
	}
	if path == "/health/readyz" && method == "GET" {
		if e := s.db.PingContext(r.Context()); e != nil {
			return e
		}
		write(w, 200, map[string]any{"version": "api.v1", "status": "ready"})
		return nil
	}
	if strings.HasPrefix(path, "/api/v1/auth/") || path == "/api/v1/accounts" {
		return missing()
	}
	if !strings.HasPrefix(path, "/api/") && s.cfg.WebDir != "" && method == "GET" {
		http.FileServer(http.Dir(s.cfg.WebDir)).ServeHTTP(w, r)
		return nil
	}
	account := s.household
	if path == "/api/v1/status" && method == "GET" {
		return s.status(w, r)
	}
	if path == "/api/v1/profiles" {
		return s.profiles(w, r, account)
	}
	if strings.HasPrefix(path, "/api/v1/profiles/") {
		rest := strings.TrimPrefix(path, "/api/v1/profiles/")
		p := strings.Split(rest, "/")
		if len(p) == 0 || !safeID(p[0]) {
			return missing()
		}
		profile, e := s.profile(r, account, p[0])
		if e != nil {
			return e
		}
		if len(p) == 1 {
			return s.editProfile(w, r, account, profile)
		}
		switch p[1] {
		case "preferences":
			if len(p) == 2 && method == "PUT" {
				var prefs Preferences
				if e := body(r, &prefs); e != nil {
					return e
				}
				if !prefs.Valid() {
					return bad()
				}
				return s.updateProfile(w, r, profile, &prefs)
			}
		case "activity":
			if len(p) == 2 && method == "POST" {
				e := s.touch(r, profile.ID)
				if e != nil {
					return e
				}
				w.WriteHeader(204)
				return nil
			}
		case "library", "favorites", "recommendation-hidden", "recommendation-watched", "recommendation-ignored":
			return s.list(w, r, profile, p[1], p[2:])
		case "history", "resume", "progress":
			return s.history(w, r, profile, p[1], p[2:])
		case "recommendation-feedback":
			if len(p) == 2 && method == "GET" {
				return s.recommendationFeedback(w, r, profile)
			}
		case "recommendation-topics":
			return s.recommendationTopics(w, r, profile, p[2:])
		case "recommendations":
			if len(p) == 2 && method == "GET" {
				return s.recommendations(w, r, profile)
			}
		case "home":
			if len(p) == 2 && method == "GET" {
				return s.home(w, r, profile)
			}
		case "playback":
			return s.playback(w, r, profile, p[2:])
		}
		return missing()
	}
	if method == "GET" || method == "HEAD" {
		return s.catalogRead(w, r)
	}
	return missing()
}
func (s *Service) profiles(w http.ResponseWriter, r *http.Request, account string) error {
	if r.Method == "GET" {
		rows, e := s.db.QueryContext(r.Context(), "SELECT data FROM profiles WHERE account=? ORDER BY id", account)
		if e != nil {
			return e
		}
		defer rows.Close()
		out := []Profile{}
		for rows.Next() {
			var b []byte
			var p Profile
			if e = rows.Scan(&b); e != nil {
				return e
			}
			if e = decode(b, &p); e != nil {
				return e
			}
			out = append(out, p)
		}
		if e = rows.Err(); e != nil {
			return e
		}
		write(w, 200, map[string]any{"profiles": out})
		return nil
	}
	if r.Method != "POST" {
		return missing()
	}
	var in struct {
		Name  string `json:"name"`
		Color string `json:"color"`
	}
	if e := body(r, &in); e != nil {
		return e
	}
	if !validProfile(in.Name, in.Color) {
		return bad()
	}
	p := Profile{ID: id(), Name: in.Name, Color: in.Color, Preferences: Preferences{Format: "both", Genres: []string{}}}
	e := transaction(r.Context(), s.db, func(tx *sql.Tx) error {
		var n int
		if e := tx.QueryRow("SELECT count(*) FROM profiles").Scan(&n); e != nil {
			return e
		}
		if n >= 128 {
			return &fault{409, "profile_limit"}
		}
		_, e := tx.Exec("INSERT INTO profiles(id,account,data) VALUES(?,?,?)", p.ID, account, marshal(p))
		return e
	})
	if e != nil {
		return e
	}
	write(w, 201, p)
	return nil
}
func validProfile(name, color string) bool {
	return len(strings.TrimSpace(name)) > 0 && len(name) <= 80 && len(color) <= 32 && len(color) > 0
}
func (s *Service) editProfile(w http.ResponseWriter, r *http.Request, account string, p Profile) error {
	switch r.Method {
	case "GET":
		write(w, 200, p)
		return nil
	case "PUT":
		return s.updateProfile(w, r, p, nil)
	case "DELETE":
		e := transaction(r.Context(), s.db, func(tx *sql.Tx) error {
			if _, e := tx.Exec("INSERT OR IGNORE INTO deletions VALUES(?)", p.ID); e != nil {
				return e
			}
			if _, e := tx.Exec("UPDATE playbacks SET revoked=1 WHERE profile=?", p.ID); e != nil {
				return e
			}
			_, e := tx.Exec("DELETE FROM profiles WHERE id=? AND account=?", p.ID, account)
			return e
		})
		if e != nil {
			return e
		}
		w.WriteHeader(204)
		return nil
	}
	return missing()
}
func (s *Service) updateProfile(w http.ResponseWriter, r *http.Request, p Profile, prefs *Preferences) error {
	// Read/modify/write the latest profile in the transaction to avoid lost updates
	// between name edits and independently submitted preference changes.
	var name, color string
	if prefs == nil {
		var in struct {
			Name  string `json:"name"`
			Color string `json:"color"`
		}
		if e := body(r, &in); e != nil {
			return e
		}
		if !validProfile(in.Name, in.Color) {
			return bad()
		}
		name, color = in.Name, in.Color
	}
	e := transaction(r.Context(), s.db, func(tx *sql.Tx) error {
		var b []byte
		if e := tx.QueryRow("SELECT data FROM profiles WHERE id=?", p.ID).Scan(&b); e != nil {
			return missing()
		}
		if e := decode(b, &p); e != nil {
			return e
		}
		if prefs != nil {
			p.Preferences = *prefs
		} else {
			p.Name, p.Color = name, color
		}
		_, e := tx.Exec("UPDATE profiles SET data=?,version=version+1 WHERE id=?", marshal(p), p.ID)
		return e
	})
	if e != nil {
		return e
	}
	write(w, 200, p)
	return nil
}
func (s *Service) touch(r *http.Request, pid string) error {
	return transaction(r.Context(), s.db, func(tx *sql.Tx) error { return touchTx(tx, pid) })
}
func touchTx(tx *sql.Tx, pid string) error {
	_, e := tx.Exec("INSERT OR IGNORE INTO activity VALUES(?,?)", pid, time.Now().UTC().Format("2006-01-02"))
	if e != nil {
		return e
	}
	_, e = tx.Exec("UPDATE profiles SET version=version+1 WHERE id=?", pid)
	return e
}
func (s *Service) list(w http.ResponseWriter, r *http.Request, p Profile, kind string, parts []string) error {
	if len(parts) == 0 && r.Method == "GET" {
		rows, e := s.db.QueryContext(r.Context(), "SELECT data FROM lists WHERE profile=? AND kind=? ORDER BY target LIMIT 1000", p.ID, kind)
		if e != nil {
			return e
		}
		defer rows.Close()
		out := []Title{}
		for rows.Next() {
			var b []byte
			var t Title
			if e = rows.Scan(&b); e != nil {
				return e
			}
			if e = decode(b, &t); e != nil {
				return e
			}
			out = append(out, t)
		}
		if e = rows.Err(); e != nil {
			return e
		}
		rows.Close()
		for i, t := range out {
			cached := s.cachedTitle(t.Identity)
			if cached.Title != "" {
				t = cached
			}
			out[i] = publicTitle(t)
		}
		write(w, 200, map[string]any{"titles": out})
		return nil
	}
	if len(parts) != 2 {
		return missing()
	}
	n, e := strconv.ParseInt(parts[1], 10, 64)
	if e != nil {
		return bad()
	}
	i := Identity{parts[0], n}
	if !i.Valid() {
		return bad()
	}
	if r.Method != "PUT" && r.Method != "DELETE" {
		return missing()
	}
	var t Title
	if r.Method == "PUT" {
		t = s.cachedTitle(i)
		if t.Title == "" {
			t = Title{Identity: i}
		}
	}
	e = transaction(r.Context(), s.db, func(tx *sql.Tx) error {
		if r.Method == "DELETE" {
			if _, e := tx.Exec("DELETE FROM lists WHERE profile=? AND kind=? AND target=?", p.ID, kind, i.Key()); e != nil {
				return e
			}
		} else {
			var count int
			if e := tx.QueryRow("SELECT count(*) FROM lists WHERE profile=? AND kind=? AND target<>?", p.ID, kind, i.Key()).Scan(&count); e != nil {
				return e
			}
			limit := 1000
			if kind == "favorites" {
				limit = 50
			}
			if count >= limit {
				return &fault{409, "list_limit"}
			}
			if _, e := tx.Exec("INSERT INTO lists VALUES(?,?,?,?) ON CONFLICT(profile,kind,target) DO UPDATE SET data=excluded.data", p.ID, kind, i.Key(), marshal(t)); e != nil {
				return e
			}
		}
		_, e := tx.Exec("UPDATE profiles SET version=version+1 WHERE id=?", p.ID)
		return e
	})
	if e != nil {
		return e
	}
	w.WriteHeader(204)
	return nil
}
func targetQuery(r *http.Request) (Target, error) {
	n, e := strconv.ParseInt(r.URL.Query().Get("id"), 10, 64)
	if e != nil {
		return Target{}, bad()
	}
	t := Target{Identity: Identity{r.URL.Query().Get("type"), n}}
	if v := r.URL.Query().Get("season"); v != "" {
		n, e := strconv.Atoi(v)
		if e != nil {
			return t, bad()
		}
		t.Season = &n
	}
	if v := r.URL.Query().Get("episode"); v != "" {
		n, e := strconv.Atoi(v)
		if e != nil {
			return t, bad()
		}
		t.Episode = &n
	}
	if !t.Valid() {
		return t, bad()
	}
	return t, nil
}
func (s *Service) history(w http.ResponseWriter, r *http.Request, p Profile, kind string, parts []string) error {
	if len(parts) != 0 {
		return missing()
	}
	if kind == "progress" && r.Method == "PUT" {
		return s.updateProgress(w, r, p)
	}
	if r.Method != "GET" {
		return missing()
	}
	if kind == "resume" {
		t, e := targetQuery(r)
		if e != nil {
			return e
		}
		var b []byte
		e = s.db.QueryRowContext(r.Context(), "SELECT data FROM progress WHERE profile=? AND target=?", p.ID, t.Key()).Scan(&b)
		if errors.Is(e, sql.ErrNoRows) {
			return missing()
		}
		if e != nil {
			return e
		}
		var v Progress
		if e = decode(b, &v); e != nil {
			return e
		}
		write(w, 200, v)
		return nil
	}
	offset, e := parseOffset(r)
	if e != nil {
		return e
	}
	rows, e := s.db.QueryContext(r.Context(), "SELECT data FROM progress WHERE profile=? ORDER BY json_extract(data,'$.updated_at') DESC,target LIMIT 100 OFFSET ?", p.ID, offset)
	if e != nil {
		return e
	}
	defer rows.Close()
	out := []Progress{}
	for rows.Next() {
		var b []byte
		var v Progress
		if e = rows.Scan(&b); e != nil {
			return e
		}
		if e = decode(b, &v); e != nil {
			return e
		}
		out = append(out, v)
	}
	if e = rows.Err(); e != nil {
		return e
	}
	write(w, 200, map[string]any{"records": out, "offset": offset, "limit": 100})
	return nil
}
func parseOffset(r *http.Request) (int, error) {
	v := r.URL.Query().Get("offset")
	if v == "" {
		return 0, nil
	}
	n, e := strconv.Atoi(v)
	if e != nil || n < 0 || n > 20000 {
		return 0, bad()
	}
	return n, nil
}
