package api

import (
	"database/sql"
	"errors"
	"math"
	"net/http"
	"time"
)

func (s *Service) updateProgress(w http.ResponseWriter, r *http.Request, p Profile) error {
	var in struct {
		PlaybackID string  `json:"playback_id"`
		Sequence   int64   `json:"sequence"`
		Position   float64 `json:"position_seconds"`
		Duration   float64 `json:"duration_seconds"`
		Watched    float64 `json:"watched_seconds"`
	}
	if e := body(r, &in); e != nil {
		return e
	}
	if !safeID(in.PlaybackID) || in.Sequence <= 0 || !finite(in.Position) || !finite(in.Duration) || !finite(in.Watched) || in.Position > 86400 || in.Duration > 86400 || in.Watched > 604800 {
		return bad()
	}
	var result Progress
	e := transaction(r.Context(), s.db, func(tx *sql.Tx) error {
		var req, ready []byte
		var total float64
		var sequence, created, expires int64
		var revoked bool
		e := tx.QueryRow("SELECT request,total,sequence,created,expires,revoked,ready FROM playbacks WHERE id=? AND profile=?", in.PlaybackID, p.ID).Scan(&req, &total, &sequence, &created, &expires, &revoked, &ready)
		if errors.Is(e, sql.ErrNoRows) {
			return missing()
		}
		if e != nil {
			return e
		}
		if len(ready) == 0 {
			return &fault{409, "setup_pending"}
		}
		if revoked || time.Now().Unix() > expires+300 {
			return &fault{410, "session_expired"}
		}
		var pr PlaybackRequest
		if e = decode(req, &pr); e != nil {
			return e
		}
		var b []byte
		e = tx.QueryRow("SELECT data FROM progress WHERE profile=? AND target=?", p.ID, pr.Target.Key()).Scan(&b)
		if e == nil {
			if e = decode(b, &result); e != nil {
				return e
			}
		} else if !errors.Is(e, sql.ErrNoRows) {
			return e
		} else {
			result.Target = pr.Target
		}
		if in.Sequence <= sequence {
			return nil
		}
		if in.Watched < total {
			return &fault{409, "counter_regression"}
		}
		// Cumulative measured play time is independent of the seekable position.
		// Two clients use distinct playback IDs and monotonic counters per ID.
		if in.Watched > math.Max(0, float64(time.Now().Unix()-created))*2+30 {
			return bad()
		}
		result.Watched += in.Watched - total
		result.Position = in.Position
		result.Duration = in.Duration
		if in.Duration > 0 {
			result.Position = math.Min(in.Position, in.Duration)
		}
		result.Completed = result.Completed || (in.Duration > 0 && result.Position >= in.Duration*.9 && result.Watched >= in.Duration*.6)
		result.Updated = time.Now().UTC()
		if _, e = tx.Exec("INSERT INTO progress VALUES(?,?,?,?,?) ON CONFLICT(profile,target) DO UPDATE SET data=excluded.data", p.ID, pr.Target.Key(), pr.Type, pr.ID, marshal(result)); e != nil {
			return e
		}
		if _, e = tx.Exec("UPDATE playbacks SET total=?,sequence=? WHERE id=?", in.Watched, in.Sequence, in.PlaybackID); e != nil {
			return e
		}
		return touchTx(tx, p.ID)
	})
	if e != nil {
		return e
	}
	write(w, 200, result)
	return nil
}
