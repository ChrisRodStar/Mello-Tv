//
//  handler.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Private HTTP REST API presentation layer for the Stream TV catalog service.
//  Implements internal endpoints for title lookups, episode coordinates, shelf browsing,
//  taste profile management, online snapshots, and constant-time token authentication.
//

package api

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"

	"stv-catalog/internal/db"
	"stv-catalog/internal/model"
	"stv-catalog/internal/recommend"
	"stv-catalog/internal/service"
	"stv-catalog/internal/upstream"
)

var handlePattern = regexp.MustCompile(`^[A-Za-z0-9_-]{1,100}$`)

// NewHandler sets up and returns the HTTP request multiplexer with rate-limiting and authorization middleware.
func NewHandler(s *service.Service, token string) http.Handler {
	mux := http.NewServeMux()

	mux.HandleFunc("GET /internal/v1/health/live", func(w http.ResponseWriter, r *http.Request) {
		send(w, http.StatusOK, map[string]any{
			"status":  "live",
			"version": "1",
		})
	})

	mux.HandleFunc("GET /internal/v1/health/ready", func(w http.ResponseWriter, r *http.Request) {
		if err := s.Store.Health(r.Context()); err != nil {
			problem(w, http.StatusServiceUnavailable, "storage_unavailable")
			return
		}
		if !s.ScheduleHealthy() {
			problem(w, http.StatusServiceUnavailable, "refresh_schedule_unavailable")
			return
		}
		send(w, http.StatusOK, map[string]any{
			"status":             "ready",
			"updater_configured": s.Up.Token != "",
		})
	})

	mux.HandleFunc("GET /internal/v1/titles/{type}/{id}", func(w http.ResponseWriter, r *http.Request) {
		id, err := requestIdentity(r)
		if err != nil {
			problem(w, http.StatusBadRequest, "invalid_identity")
			return
		}
		t0 := time.Now()
		record, err := s.Store.Title(r.Context(), id.Key(), s.Now())
		dbDur := float64(time.Since(t0).Microseconds()) / 1000.0
		w.Header().Set("Server-Timing", fmt.Sprintf("db;dur=%.3f", dbDur))
		if err != nil {
			readProblem(w, err)
			return
		}
		send(w, http.StatusOK, record)
	})

	mux.HandleFunc("PUT /internal/v1/titles/{type}/{id}/cross-ids", func(w http.ResponseWriter, r *http.Request) {
		id, err := requestIdentity(r)
		if err != nil {
			problem(w, http.StatusBadRequest, "invalid_identity")
			return
		}

		var ids struct {
			IMDb string `json:"imdb_id"`
			MAL  int    `json:"mal_id"`
		}
		if decode(w, r, &ids) != nil {
			return
		}

		if (ids.IMDb != "" && !validIMDb(ids.IMDb)) || ids.MAL < 0 {
			problem(w, http.StatusBadRequest, "invalid_cross_ids")
			return
		}

		if err = s.Store.Put(r.Context(), "cross_ids", id.Key(), ids, s.Now(), 36500*24*time.Hour); err != nil {
			writeProblem(w, err)
			return
		}
		send(w, http.StatusOK, map[string]string{"status": "saved"})
	})

	mux.HandleFunc("GET /internal/v1/tv/{id}/seasons/{season}", func(w http.ResponseWriter, r *http.Request) {
		id, err1 := positive(r.PathValue("id"))
		num, err2 := strconv.Atoi(r.PathValue("season"))
		if err1 != nil || err2 != nil || num < 0 || num > 1000 {
			problem(w, http.StatusBadRequest, "invalid_coordinates")
			return
		}
		readRecord(s, w, r, "season", model.SeasonKey(id, num))
	})

	mux.HandleFunc("GET /internal/v1/tv/{id}/seasons/{season}/episodes/{episode}", func(w http.ResponseWriter, r *http.Request) {
		id, err1 := positive(r.PathValue("id"))
		num, err2 := strconv.Atoi(r.PathValue("season"))
		epNum, err3 := positive(r.PathValue("episode"))
		if err1 != nil || err2 != nil || err3 != nil || num < 0 || num > 1000 {
			problem(w, http.StatusBadRequest, "invalid_coordinates")
			return
		}

		record, err := s.Store.Get(r.Context(), "season", model.SeasonKey(id, num), s.Now())
		if err != nil {
			readProblem(w, err)
			return
		}

		var season model.Season
		if json.Unmarshal(record.Data, &season) != nil {
			problem(w, http.StatusInternalServerError, "invalid_storage")
			return
		}

		for _, item := range season.Episodes {
			if item.Number == epNum {
				record.Data, _ = json.Marshal(struct {
					ShowID int `json:"show_id"`
					model.Episode
				}{
					ShowID:  id,
					Episode: item,
				})
				send(w, http.StatusOK, record)
				return
			}
		}
		problem(w, http.StatusNotFound, "missing")
	})

	mux.HandleFunc("GET /internal/v1/shelves", func(w http.ResponseWriter, r *http.Request) {
		rows, err := s.Store.List(r.Context(), "shelf", 100, s.Now())
		if err != nil {
			readProblem(w, err)
			return
		}
		send(w, http.StatusOK, map[string]any{"records": rows})
	})

	mux.HandleFunc("GET /internal/v1/shelves/{id}", func(w http.ResponseWriter, r *http.Request) {
		readRecord(s, w, r, "shelf", r.PathValue("id"))
	})

	mux.HandleFunc("GET /internal/v1/search", func(w http.ResponseWriter, r *http.Request) {
		query := strings.TrimSpace(r.URL.Query().Get("q"))
		offset := 0
		var err error
		if v := r.URL.Query().Get("offset"); v != "" {
			offset, err = strconv.Atoi(v)
		}
		if query == "" || len(query) > 200 || err != nil || offset < 0 || offset > 20000 {
			problem(w, http.StatusBadRequest, "invalid_search")
			return
		}

		rows, err := s.Store.Search(r.Context(), query, offset, s.Now())
		if err != nil {
			readProblem(w, err)
			return
		}
		send(w, http.StatusOK, map[string]any{
			"scope":   "cached_titles",
			"records": rows,
			"offset":  offset,
			"limit":   50,
		})
	})

	mux.HandleFunc("GET /internal/v1/search-results", func(w http.ResponseWriter, r *http.Request) {
		readRecord(s, w, r, "search", r.URL.Query().Get("q"))
	})

	mux.HandleFunc("POST /internal/v1/refresh", func(w http.ResponseWriter, r *http.Request) {
		var j service.Job
		if decode(w, r, &j) != nil {
			return
		}
		if j.Validate() != nil {
			problem(w, http.StatusBadRequest, "invalid_job")
			return
		}

		status, err := s.Request(r.Context(), j)
		if err != nil {
			writeProblem(w, err)
			return
		}
		send(w, http.StatusAccepted, map[string]any{
			"status":  status,
			"job_key": j.Key(),
		})
	})

	mux.HandleFunc("GET /internal/v1/refresh", func(w http.ResponseWriter, r *http.Request) {
		state, err := s.State(r.Context(), r.URL.Query().Get("key"))
		if err != nil {
			readProblem(w, err)
			return
		}
		send(w, http.StatusOK, state)
	})

	mux.HandleFunc("PUT /internal/v1/recommendations/{handle}/inputs", func(w http.ResponseWriter, r *http.Request) {
		var inputs recommend.Inputs
		if decode(w, r, &inputs) != nil {
			return
		}

		handle := r.PathValue("handle")
		if !handlePattern.MatchString(handle) || inputs.Validate(s.Now()) != nil {
			problem(w, http.StatusBadRequest, "invalid_inputs")
			return
		}

		if err := s.SetInputs(r.Context(), handle, inputs); err != nil {
			writeProblem(w, err)
			return
		}
		send(w, http.StatusAccepted, map[string]string{"status": "saved"})
	})

	mux.HandleFunc("GET /internal/v1/recommendations/{handle}/inputs", func(w http.ResponseWriter, r *http.Request) {
		readRecord(s, w, r, "inputs", r.PathValue("handle"))
	})

	mux.HandleFunc("DELETE /internal/v1/recommendations/{handle}", func(w http.ResponseWriter, r *http.Request) {
		handle := r.PathValue("handle")
		if !handlePattern.MatchString(handle) {
			problem(w, http.StatusBadRequest, "invalid_handle")
			return
		}

		tx, err := s.Store.DB().BeginTx(r.Context(), nil)
		if err == nil {
			defer tx.Rollback()
			_, err = tx.ExecContext(r.Context(), `DELETE FROM records WHERE kind IN ('inputs','recommendations') AND key=?`, handle)
			if err == nil {
				_, err = tx.ExecContext(r.Context(), `DELETE FROM jobs WHERE key=?`, "recommendations/"+handle)
			}
			if err == nil {
				_, err = tx.ExecContext(r.Context(), `UPDATE revision SET value=value+1`)
			}
			if err == nil {
				err = tx.Commit()
			}
		}

		if err != nil {
			writeProblem(w, err)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	})

	mux.HandleFunc("GET /internal/v1/recommendations/{handle}", func(w http.ResponseWriter, r *http.Request) {
		handle := r.PathValue("handle")
		category := r.URL.Query().Get("type")
		if category == "" {
			category = "all"
		}
		if category != "all" && category != "movie" && category != "tv" {
			problem(w, http.StatusBadRequest, "invalid_type")
			return
		}

		record, inputs, err := s.Store.Recommendation(r.Context(), handle, s.Now())
		if err != nil {
			readProblem(w, err)
			return
		}

		var result recommend.Recommendations
		if json.Unmarshal(record.Data, &result) != nil {
			problem(w, http.StatusInternalServerError, "invalid_storage")
			return
		}

		record.Data, _ = json.Marshal(result.View(category, inputs.Format))
		send(w, http.StatusOK, record)
	})

	mux.HandleFunc("GET /internal/v1/artwork/{size}/{filename}", func(w http.ResponseWriter, r *http.Request) {
		key := upstream.ArtworkKey(r.PathValue("size"), "/"+r.PathValue("filename"))
		if key == "" {
			problem(w, http.StatusBadRequest, "invalid_artwork")
			return
		}
		w.Header().Set("Cache-Control", "private, max-age=86400")
		http.Redirect(w, r, "https://image.tmdb.org/t/p/"+key, http.StatusFound)
	})

	mux.HandleFunc("GET /internal/v1/snapshot", func(w http.ResponseWriter, r *http.Request) {
		snapshot, err := s.Store.Export(r.Context(), s.Now())
		if err != nil {
			readProblem(w, err)
			return
		}
		etag := fmt.Sprintf(`"catalog-v1-%d"`, snapshot.Revision)
		w.Header().Set("X-Catalog-Revision", etag)
		send(w, http.StatusOK, snapshot)
	})

	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		problem(w, http.StatusNotFound, "unknown_route")
	})

	slots := make(chan struct{}, 16)

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		tw := &timingWriter{
			ResponseWriter: w,
			start:          time.Now(),
		}

		tw.Header().Set("X-Content-Type-Options", "nosniff")
		tw.Header().Set("Cache-Control", "no-store")

		if token != "" && subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte("Bearer "+token)) != 1 {
			problem(tw, http.StatusUnauthorized, "unauthorized")
			return
		}

		select {
		case slots <- struct{}{}:
			defer func() { <-slots }()
		default:
			problem(tw, http.StatusServiceUnavailable, "busy")
			return
		}

		ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
		defer cancel()

		mux.ServeHTTP(tw, r.WithContext(ctx))
	})
}

type timingWriter struct {
	http.ResponseWriter
	start       time.Time
	wroteHeader bool
}

func (tw *timingWriter) WriteHeader(code int) {
	if !tw.wroteHeader {
		tw.wroteHeader = true
		dur := float64(time.Since(tw.start).Microseconds()) / 1000.0
		existing := tw.Header().Get("Server-Timing")
		if existing != "" {
			tw.Header().Set("Server-Timing", fmt.Sprintf("%s, total;dur=%.3f", existing, dur))
		} else {
			tw.Header().Set("Server-Timing", fmt.Sprintf("total;dur=%.3f", dur))
		}
	}
	tw.ResponseWriter.WriteHeader(code)
}

func (tw *timingWriter) Write(b []byte) (int, error) {
	if !tw.wroteHeader {
		tw.WriteHeader(http.StatusOK)
	}
	return tw.ResponseWriter.Write(b)
}

func readRecord(s *service.Service, w http.ResponseWriter, r *http.Request, kind, key string) {
	t0 := time.Now()
	record, err := s.Store.Get(r.Context(), kind, key, s.Now())
	dbDur := float64(time.Since(t0).Microseconds()) / 1000.0
	w.Header().Set("Server-Timing", fmt.Sprintf("db;dur=%.3f", dbDur))
	if err != nil {
		readProblem(w, err)
		return
	}
	send(w, http.StatusOK, record)
}

func requestIdentity(r *http.Request) (model.Identity, error) {
	id, err := positive(r.PathValue("id"))
	identity := model.Identity{
		Type: r.PathValue("type"),
		ID:   id,
	}
	if err != nil || !identity.Valid() {
		return identity, errors.New("invalid identity")
	}
	return identity, nil
}

func positive(v string) (int, error) {
	n, err := strconv.Atoi(v)
	if err != nil || n <= 0 {
		return 0, errors.New("invalid number")
	}
	return n, nil
}

func validIMDb(s string) bool {
	if !strings.HasPrefix(s, "tt") || len(s) < 3 || len(s) > 20 {
		return false
	}
	_, err := strconv.ParseUint(s[2:], 10, 64)
	return err == nil
}

func decode(w http.ResponseWriter, r *http.Request, v any) error {
	r.Body = http.MaxBytesReader(w, r.Body, 256<<10)
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()

	if err := dec.Decode(v); err != nil {
		problem(w, http.StatusBadRequest, "invalid_json")
		return err
	}

	var extra any
	if err := dec.Decode(&extra); err != io.EOF {
		problem(w, http.StatusBadRequest, "invalid_json")
		return errors.New("trailing JSON")
	}
	return nil
}

func send(w http.ResponseWriter, status int, data any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(data)
}

func problem(w http.ResponseWriter, status int, code string) {
	send(w, status, map[string]string{"error": code})
}

func readProblem(w http.ResponseWriter, err error) {
	if errors.Is(err, db.ErrMissing) {
		problem(w, http.StatusNotFound, "missing")
	} else {
		problem(w, http.StatusServiceUnavailable, "storage_unavailable")
	}
}

func writeProblem(w http.ResponseWriter, err error) {
	if errors.Is(err, db.ErrMissing) {
		problem(w, http.StatusNotFound, "missing")
		return
	}
	if errors.Is(err, service.ErrScheduleUnavailable) {
		w.Header().Set("Retry-After", "60")
		problem(w, http.StatusServiceUnavailable, "refresh_schedule_unavailable")
		return
	}
	if errors.Is(err, service.ErrBusy) {
		w.Header().Set("Retry-After", "60")
		problem(w, http.StatusServiceUnavailable, "busy")
	} else if errors.Is(err, db.ErrCapacity) {
		problem(w, 507, "storage_capacity")
	} else {
		problem(w, http.StatusServiceUnavailable, "storage_unavailable")
	}
}
