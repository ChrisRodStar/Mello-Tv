package api

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"math"
	"sort"
	"time"
)

type favorite struct {
	Identity
	Title string `json:"title"`
}
type signal struct {
	Identity
	Title     string    `json:"title"`
	Watched   float64   `json:"watched_seconds"`
	Duration  float64   `json:"duration_seconds"`
	Completed bool      `json:"completed"`
	Episodes  int       `json:"meaningful_episodes"`
	Updated   time.Time `json:"updated_at"`
}
type inputs struct {
	Completed bool       `json:"completed"`
	Format    string     `json:"format"`
	Genres    []string   `json:"genres"`
	Favorites []favorite `json:"favorites"`
	Signals   []signal   `json:"signals"`
	Activity  []string   `json:"activity_days"`
}

func (s *Service) derived(ctx context.Context, pid string) (inputs, int64, error) {
	var out inputs
	var version int64
	e := transaction(ctx, s.db, func(tx *sql.Tx) error {
		var b []byte
		var p Profile
		if e := tx.QueryRow("SELECT data,version FROM profiles WHERE id=?", pid).Scan(&b, &version); e != nil {
			return e
		}
		if e := decode(b, &p); e != nil {
			return e
		}
		out = inputs{Completed: p.Preferences.Completed, Format: p.Preferences.Format, Genres: p.Preferences.Genres, Favorites: []favorite{}, Signals: []signal{}, Activity: []string{}}
		rows, e := tx.Query("SELECT data FROM lists WHERE profile=? AND kind='favorites' ORDER BY target LIMIT 50", pid)
		if e != nil {
			return e
		}
		for rows.Next() {
			var b []byte
			var t Title
			if e = rows.Scan(&b); e != nil {
				rows.Close()
				return e
			}
			if e = decode(b, &t); e != nil {
				rows.Close()
				return e
			}
			out.Favorites = append(out.Favorites, favorite{t.Identity, t.Title})
		}
		e = rows.Err()
		rows.Close()
		if e != nil {
			return e
		}
		rows, e = tx.Query(`SELECT type,title_id,SUM(json_extract(data,'$.watched_seconds')),MAX(json_extract(data,'$.duration_seconds')),MAX(json_extract(data,'$.completed')),SUM(CASE WHEN type='tv' AND json_extract(data,'$.watched_seconds')>=600 THEN 1 ELSE 0 END),MAX(json_extract(data,'$.updated_at')) FROM progress WHERE profile=? GROUP BY type,title_id ORDER BY MAX(json_extract(data,'$.updated_at')) DESC,type,title_id LIMIT 100`, pid)
		if e != nil {
			return e
		}
		for rows.Next() {
			var v signal
			var updated string
			if e = rows.Scan(&v.Type, &v.ID, &v.Watched, &v.Duration, &v.Completed, &v.Episodes, &updated); e != nil {
				rows.Close()
				return e
			}
			v.Updated, e = time.Parse(time.RFC3339Nano, updated)
			if e != nil {
				rows.Close()
				return e
			}
			out.Signals = append(out.Signals, v)
		}
		e = rows.Err()
		rows.Close()
		if e != nil {
			return e
		}
		rows, e = tx.Query("SELECT day FROM activity WHERE profile=? AND day>=? ORDER BY day DESC LIMIT 30", pid, time.Now().UTC().AddDate(0, 0, -30).Format("2006-01-02"))
		if e != nil {
			return e
		}
		for rows.Next() {
			var day string
			if e = rows.Scan(&day); e != nil {
				rows.Close()
				return e
			}
			out.Activity = append(out.Activity, day)
		}
		e = rows.Err()
		rows.Close()
		return e
	})
	if e != nil {
		return out, version, e
	}
	rows, e := s.db.QueryContext(ctx, "SELECT target FROM lists WHERE profile=? AND kind IN ('recommendation-hidden','recommendation-ignored')", pid)
	if e != nil {
		return out, version, e
	}
	ignored := map[string]bool{}
	for rows.Next() {
		var key string
		if e = rows.Scan(&key); e != nil {
			rows.Close()
			return out, version, e
		}
		ignored[key] = true
	}
	e = rows.Err()
	rows.Close()
	if e != nil {
		return out, version, e
	}
	favorites := []favorite{}
	for _, f := range out.Favorites {
		if !ignored[f.Key()] {
			favorites = append(favorites, f)
		}
	}
	out.Favorites = favorites
	signals := []signal{}
	for _, v := range out.Signals {
		if !ignored[v.Key()] {
			signals = append(signals, v)
		}
	}
	out.Signals = signals
	for i := range out.Favorites {
		t := s.cachedTitle(out.Favorites[i].Identity)
		if t.Title != "" {
			out.Favorites[i].Title = t.Title
		}
	}
	for i := range out.Signals {
		out.Signals[i].Title = s.cachedTitle(out.Signals[i].Identity).Title
	}
	return out, version, nil
}
func (p inputs) fingerprint() string {
	p.Activity = nil
	p.Genres = append([]string(nil), p.Genres...)
	sort.Strings(p.Genres)
	p.Favorites = append([]favorite(nil), p.Favorites...)
	sort.Slice(p.Favorites, func(i, j int) bool { return p.Favorites[i].Key() < p.Favorites[j].Key() })
	p.Signals = append([]signal(nil), p.Signals...)
	sort.Slice(p.Signals, func(i, j int) bool { return p.Signals[i].Key() < p.Signals[j].Key() })
	qualified := make([]bool, len(p.Signals))
	for i, v := range p.Signals {
		if v.Type == "tv" {
			qualified[i] = v.Watched >= 1200 && (v.Episodes >= 2 || v.Watched >= 2700)
		} else {
			qualified[i] = v.Watched >= 600 && (v.Completed || v.Duration > 0 && v.Watched >= v.Duration*.2)
		}
		p.Signals[i].Watched = math.Floor(v.Watched/600) * 600
		p.Signals[i].Updated = time.Time{}
	}
	b := marshal(struct {
		Version   int
		Inputs    inputs
		Qualified []bool
	}{2, p, qualified})
	h := sha256.Sum256(b)
	return hex.EncodeToString(h[:])
}
func (s *Service) syncRecommendations(ctx context.Context) error {
	// One joined owner executes PUT and deletion outbox in order. If profile
	// deletion races a PUT, its durable tombstone is processed immediately after.
	rows, e := s.db.QueryContext(ctx, "SELECT id FROM profiles ORDER BY synced=version,last_sync,id LIMIT 16")
	if e != nil {
		return e
	}
	ids := []string{}
	for rows.Next() {
		var pid string
		if e = rows.Scan(&pid); e != nil {
			rows.Close()
			return e
		}
		ids = append(ids, pid)
	}
	e = rows.Err()
	rows.Close()
	if e != nil {
		return e
	}

	profiles := ids
	rows, e = s.db.QueryContext(ctx, "SELECT handle FROM deletions ORDER BY handle LIMIT 128")
	if e != nil {
		return e
	}
	ids = nil
	for rows.Next() {
		var h string
		if e = rows.Scan(&h); e != nil {
			rows.Close()
			return e
		}
		ids = append(ids, h)
	}
	e = rows.Err()
	rows.Close()
	if e != nil {
		return e
	}
	for _, pid := range ids {
		if _, e = s.private(ctx, "catalog", "DELETE", "/internal/v1/recommendations/"+pid, nil, "", 8192); e == nil {
			_, _ = s.db.ExecContext(ctx, "DELETE FROM deletions WHERE handle=?", pid)
		}
	}
	ids = profiles
	for _, pid := range ids {
		_, _ = s.db.ExecContext(ctx, "UPDATE profiles SET last_sync=? WHERE id=?", time.Now().Unix(), pid)
		in, version, e := s.derived(ctx, pid)
		if e != nil {
			continue
		}
		if _, e = s.private(ctx, "catalog", "PUT", "/internal/v1/recommendations/"+pid+"/inputs", in, "", 8192); e == nil {
			_, _ = s.db.ExecContext(ctx, "UPDATE profiles SET synced=? WHERE id=? AND version=?", version, pid, version)
		}
	}
	return ctx.Err()
}
func (s *Service) flushRefresh(ctx context.Context) {
	rows, e := s.db.QueryContext(ctx, "SELECT key,data FROM refresh_jobs LIMIT 32")
	if e != nil {
		return
	}
	type job struct {
		k string
		b []byte
	}
	jobs := []job{}
	for rows.Next() {
		var j job
		if rows.Scan(&j.k, &j.b) == nil {
			jobs = append(jobs, j)
		}
	}
	rows.Close()
	for _, j := range jobs {
		var raw any
		if decode(j.b, &raw) != nil {
			continue
		}
		if _, e := s.private(ctx, "catalog", "POST", "/internal/v1/refresh", raw, "", 8192); e == nil {
			_, _ = s.db.ExecContext(ctx, "DELETE FROM refresh_jobs WHERE key=?", j.k)
			notifyWorker(s.snapshotWake)
		}
	}
}
