//
//  service.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Core catalog service orchestrator. Manages background refresh queues, durable job scheduling,
//  profile recommendation generation pipelines, and periodic sweeps for stale metadata.
//

package service

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/url"
	"regexp"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"stv-catalog/internal/db"
	"stv-catalog/internal/model"
	"stv-catalog/internal/recommend"
	"stv-catalog/internal/upstream"
)

// Standard caching and TTL durations for catalog items.
const (
	MetadataTTL = 24 * time.Hour
	ArtworkTTL  = 30 * 24 * time.Hour
	ShelfTTL    = 6 * time.Hour
	RetryDelay  = 30 * time.Minute
)

// Common operational errors returned by the catalog service.
var (
	ErrBusy                = errors.New("queue full or stopped")
	ErrScheduleUnavailable = errors.New("refresh schedule unavailable")
	ErrIncomplete          = errors.New("inputs incomplete")
	ErrInputsChanged       = errors.New("inputs changed during build")
)

type jobOutcome struct {
	due     int64
	success int64
	failed  int
}

var handlePattern = regexp.MustCompile(`^[A-Za-z0-9_-]{1,100}$`)

// Job specifies a discrete catalog refresh unit of work.
type Job struct {
	Kind string `json:"kind"`
	model.Identity
	Season  int    `json:"season,omitempty"`
	Handle  string `json:"handle,omitempty"`
	Artwork string `json:"artwork,omitempty"`
	Query   string `json:"query,omitempty"`
}

// Key returns a canonical unique string identifier for this job.
func (j Job) Key() string {
	switch j.Kind {
	case "title":
		return "title/" + j.Identity.Key()
	case "season":
		return "season/" + model.SeasonKey(j.ID, j.Season)
	case "recommendations":
		return "recommendations/" + j.Handle
	case "artwork":
		return "artwork/" + j.Artwork
	case "search":
		return "search/" + j.Query
	default:
		return j.Kind
	}
}

// Validate checks that job parameters adhere to safety bounds.
func (j Job) Validate() error {
	if len(j.Type) > 10 || len(j.Handle) > 100 || len(j.Artwork) > 200 || len(j.Query) > 200 {
		return errors.New("job field limit exceeded")
	}

	switch j.Kind {
	case "title":
		if j.Valid() {
			return nil
		}
	case "season":
		if j.Type == "tv" && j.ID > 0 && j.Season >= 0 && j.Season <= 1000 {
			return nil
		}
	case "browse":
		return nil
	case "recommendations":
		if handlePattern.MatchString(j.Handle) {
			return nil
		}
	case "artwork":
		return errors.New("artwork is delivered directly to clients")
	case "search":
		if len(j.Query) > 0 && len(j.Query) <= 200 {
			return nil
		}
	}
	return errors.New("invalid refresh job")
}

// JobState summarizes the current execution and scheduling status of a job.
type JobState struct {
	Key         string    `json:"key"`
	Due         time.Time `json:"next_attempt_at"`
	LastSuccess time.Time `json:"last_success_at"`
	Failed      bool      `json:"failed"`
	Pending     bool      `json:"pending"`
	Suspended   bool      `json:"suspended"`
}

// Service manages catalog metadata storage, background workers, and synchronization.
type Service struct {
	Store          *db.Store
	Up             *upstream.Upstream
	ctx            context.Context
	cancel         context.CancelFunc
	queue          chan Job
	mu             sync.Mutex
	pending        map[string]bool
	unsaved        map[string]jobOutcome
	stopped        atomic.Bool
	schedulePaused atomic.Bool
	wg             sync.WaitGroup
	sweepMu        sync.Mutex
	Now            func() time.Time
	JobTimeout     time.Duration
	SweepInterval  time.Duration
}

// NewService constructs a ready-to-run catalog service instance.
func NewService(s *db.Store, u *upstream.Upstream) *Service {
	ctx, cancel := context.WithCancel(context.Background())
	return &Service{
		Store:         s,
		Up:            u,
		ctx:           ctx,
		cancel:        cancel,
		queue:         make(chan Job, 64),
		pending:       map[string]bool{},
		unsaved:       map[string]jobOutcome{},
		Now:           time.Now,
		JobTimeout:    2 * time.Minute,
		SweepInterval: 15 * time.Minute,
	}
}

// Start launches worker goroutines and the periodic background sweep ticker.
func (s *Service) Start() {
	if s.Up.Token != "" {
		for _, spec := range upstream.OnboardingBrowse(s.Now()) {
			record, err := s.Store.Get(s.ctx, "shelf", spec.ID, s.Now())
			var shelf model.Shelf
			needsArtwork := err == nil &&
				json.Unmarshal(record.Data, &shelf) == nil &&
				(len(shelf.Titles) == 0 || shelf.Titles[0].ID != upstream.OnboardingArtworkTitles[spec.ID])

			if errors.Is(err, db.ErrMissing) || needsArtwork {
				s.makeDue(s.ctx, Job{Kind: "browse"})
				break
			}
		}

		if _, err := s.Store.Get(s.ctx, "shelf", "favorites-tv-animation", s.Now()); errors.Is(err, db.ErrMissing) {
			s.makeDue(s.ctx, Job{Kind: "browse"})
		}
		_, _ = s.Request(s.ctx, Job{Kind: "browse"})
	}

	for range 2 {
		s.wg.Add(1)
		go s.worker()
	}

	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		s.Sweep()

		ticker := time.NewTicker(s.SweepInterval)
		defer ticker.Stop()

		for {
			select {
			case <-ticker.C:
				s.Sweep()
			case <-s.ctx.Done():
				return
			}
		}
	}()
}

// Close signals graceful termination and awaits completion of running background jobs.
func (s *Service) Close(ctx context.Context) error {
	s.stopped.Store(true)
	s.cancel()

	done := make(chan struct{})
	go func() {
		s.wg.Wait()
		close(done)
	}()

	select {
	case <-done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// Request submits a refresh job, ensuring durability in SQLite and in-memory deduplication.
func (s *Service) Request(ctx context.Context, j Job) (string, error) {
	if err := j.Validate(); err != nil {
		return "", err
	}

	key := j.Key()
	data, _ := json.Marshal(j)

	s.mu.Lock()
	defer s.mu.Unlock()

	if s.stopped.Load() {
		return "", ErrBusy
	}
	if len(s.unsaved) > 0 {
		return "", ErrScheduleUnavailable
	}

	suspended := false
	if j.Kind == "recommendations" {
		active, err := s.recommendationActive(ctx, j.Handle)
		if err != nil {
			return "", err
		}
		suspended = !active
	}

	var count int
	if err := s.Store.DB().QueryRowContext(ctx, `SELECT COUNT(*) FROM jobs`).Scan(&count); err != nil {
		return "", err
	}
	if count >= s.Store.MaxRecords {
		var exists int
		if s.Store.DB().QueryRowContext(ctx, `SELECT 1 FROM jobs WHERE key=?`, key).Scan(&exists) != nil {
			return "", db.ErrCapacity
		}
	}

	if _, err := s.Store.DB().ExecContext(ctx, `INSERT INTO jobs(key,spec) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET spec=excluded.spec`, key, data); err != nil {
		return "", err
	}

	if suspended {
		return "suspended", nil
	}
	if s.pending[key] {
		return "deduplicated", nil
	}

	var due int64
	if err := s.Store.DB().QueryRowContext(ctx, `SELECT due FROM jobs WHERE key=?`, key).Scan(&due); err != nil {
		return "", err
	}
	if due > s.Now().UnixMilli() {
		return "scheduled", nil
	}

	select {
	case s.queue <- j:
		s.pending[key] = true
		return "queued", nil
	default:
		return "", ErrBusy
	}
}

// State returns the persistent schedule and run history for a given job key.
func (s *Service) State(ctx context.Context, key string) (JobState, error) {
	var due, success int64
	var failed bool
	err := s.Store.DB().QueryRowContext(ctx, `SELECT due,last_success,failed FROM jobs WHERE key=?`, key).Scan(&due, &success, &failed)
	if errors.Is(err, sql.ErrNoRows) {
		return JobState{}, db.ErrMissing
	}

	s.mu.Lock()
	pending := s.pending[key]
	s.mu.Unlock()

	suspended := false
	if strings.HasPrefix(key, "recommendations/") {
		active, activeErr := s.recommendationActive(ctx, strings.TrimPrefix(key, "recommendations/"))
		if activeErr != nil && !errors.Is(activeErr, db.ErrMissing) {
			return JobState{}, activeErr
		}
		suspended = !active
	}

	return JobState{
		Key:         key,
		Due:         time.UnixMilli(due).UTC(),
		LastSuccess: time.UnixMilli(success).UTC(),
		Failed:      failed,
		Pending:     pending,
		Suspended:   suspended,
	}, err
}

// worker loops continuously, executing queued refresh jobs with bounded timeouts.
func (s *Service) worker() {
	defer s.wg.Done()
	for {
		select {
		case <-s.ctx.Done():
			return
		case j := <-s.queue:
			if s.ctx.Err() != nil {
				return
			}

			s.mu.Lock()
			paused := len(s.unsaved) > 0
			if paused {
				delete(s.pending, j.Key())
			}
			s.mu.Unlock()

			if paused {
				continue
			}

			jobCtx, cancel := context.WithTimeout(s.ctx, s.JobTimeout)
			err := s.Run(jobCtx, j)
			cancel()

			s.finishJob(j, err)

			if len(s.queue) < 32 {
				s.admitDue()
			}
		}
	}
}

// Sweep checks for due recommendation profile updates and schedules pending jobs.
func (s *Service) Sweep() {
	s.admitDue()

	ctx := s.ctx
	if ctx.Err() != nil {
		return
	}

	inputs, err := s.Store.List(ctx, "inputs", s.Store.MaxRecords, s.Now())
	if err != nil {
		return
	}

	for _, r := range inputs {
		if ctx.Err() != nil {
			return
		}

		var p recommend.Inputs
		if json.Unmarshal(r.Data, &p) != nil || !p.Completed {
			continue
		}

		saved, err := s.Store.Get(ctx, "recommendations", r.Key, s.Now())
		var rec recommend.Recommendations
		due := err != nil ||
			json.Unmarshal(saved.Data, &rec) != nil ||
			rec.Fingerprint != p.Fingerprint() ||
			s.Now().Sub(saved.Updated) >= p.Cadence(s.Now())

		if due {
			s.makeDue(ctx, Job{Kind: "recommendations", Handle: r.Key})
			_, _ = s.Request(ctx, Job{Kind: "recommendations", Handle: r.Key})
		}
	}
}

func (s *Service) admitDue() {
	if !s.sweepMu.TryLock() {
		return
	}
	defer s.sweepMu.Unlock()

	ctx := s.ctx
	if ctx.Err() != nil {
		return
	}
	if !s.recoverSchedule(ctx) {
		return
	}

	rows, err := s.Store.DB().QueryContext(ctx, `SELECT j.spec FROM jobs j WHERE j.due<=? AND
 (json_extract(j.spec,'$.kind')!='recommendations' OR EXISTS
 (SELECT 1 FROM records r WHERE r.kind='inputs' AND r.key=json_extract(j.spec,'$.handle') AND json_extract(r.data,'$.completed')=1))
 ORDER BY j.due,j.key LIMIT 64`, s.Now().UnixMilli())
	if err != nil {
		return
	}

	jobs := []Job{}
	for rows.Next() {
		var b []byte
		var j Job
		if rows.Scan(&b) == nil && json.Unmarshal(b, &j) == nil {
			jobs = append(jobs, j)
		}
	}
	rows.Close()

	for _, j := range jobs {
		if ctx.Err() != nil {
			return
		}
		_, _ = s.Request(ctx, j)
	}
}

func (s *Service) makeDue(ctx context.Context, j Job) {
	_, _ = s.Store.DB().ExecContext(ctx, `UPDATE jobs SET due=0 WHERE key=? AND failed=0`, j.Key())
}

// SetInputs updates or inserts taste preferences for a user handle.
func (s *Service) SetInputs(ctx context.Context, handle string, p recommend.Inputs) error {
	if !handlePattern.MatchString(handle) {
		return errors.New("invalid handle")
	}
	if err := p.Validate(s.Now()); err != nil {
		return err
	}

	s.mu.Lock()
	if s.stopped.Load() {
		s.mu.Unlock()
		return ErrBusy
	}

	var prior recommend.Inputs
	old, err := s.Store.Get(ctx, "inputs", handle, s.Now())
	changed := err != nil || json.Unmarshal(old.Data, &prior) != nil || prior.Fingerprint() != p.Fingerprint()

	if err := s.Store.Put(ctx, "inputs", handle, p, s.Now(), 365*24*time.Hour); err != nil {
		s.mu.Unlock()
		return err
	}

	saved, err := s.Store.Get(ctx, "recommendations", handle, s.Now())
	overdue := err != nil || s.Now().Sub(saved.Updated) >= p.Cadence(s.Now())

	if p.Completed && (changed || overdue) {
		s.makeDue(ctx, Job{Kind: "recommendations", Handle: handle})
	}
	s.mu.Unlock()

	if p.Completed && (changed || overdue) {
		_, err := s.Request(ctx, Job{Kind: "recommendations", Handle: handle})
		return err
	}
	return nil
}

func (s *Service) saveList(ctx context.Context, kind, key string, data any, titles []model.Title, ttl time.Duration) error {
	return s.storeList(ctx, kind, key, data, titles, ttl)
}

func (s *Service) storeList(ctx context.Context, kind, key string, data any, titles []model.Title, ttl time.Duration) error {
	writes := []db.Write{{Kind: kind, Key: key, Data: data, TTL: ttl}}
	for _, t := range titles {
		writes = append(writes, db.Write{Kind: "title", Key: t.Key(), Data: t, TTL: MetadataTTL})
	}
	return s.Store.PutBatch(ctx, writes, s.Now())
}

// Run immediately executes a refresh job synchronously.
func (s *Service) Run(ctx context.Context, j Job) error {
	switch j.Kind {
	case "title":
		title, err := s.Up.Title(ctx, j.Identity)
		if err != nil {
			return err
		}
		return s.Store.Put(ctx, "title", title.Key(), title, s.Now(), MetadataTTL)

	case "season":
		season, err := s.Up.Season(ctx, j.ID, j.Season)
		if err != nil {
			return err
		}
		return s.Store.Put(ctx, "season", model.SeasonKey(j.ID, j.Season), season, s.Now(), MetadataTTL)

	case "browse":
		success := 0
		favorites, err := s.Up.FavoriteBrowse(ctx, s.Now())
		if err != nil {
			return err
		}

		specs := append(upstream.OnboardingBrowse(s.Now()), favorites...)
		specs = append(specs, upstream.Browse...)
		specs = append(specs, s.Up.TopicBrowse(ctx, s.Now())...)

		for _, spec := range specs {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			titles, err := s.Up.List(ctx, spec.Path, spec.Type)
			if err != nil || len(titles) == 0 {
				continue
			}

			if strings.HasPrefix(spec.ID, "favorites-") {
				for index := range titles {
					titles[index].Poster = strings.Replace(titles[index].Poster, "/artwork/w500/", "/artwork/w300/", 1)
				}
			}

			if preferred := upstream.OnboardingArtworkTitles[spec.ID]; preferred != 0 {
				if title, err := s.Up.Title(ctx, model.Identity{Type: "movie", ID: preferred}); err == nil {
					qualified := []model.Title{title}
					for _, candidate := range titles {
						if candidate.ID != preferred {
							qualified = append(qualified, candidate)
						}
					}
					titles = qualified
				}
			}

			shelf := model.Shelf{ID: spec.ID, Heading: spec.Heading, Titles: titles, Kind: "browse", Source: "tmdb"}
			if spec.ID == "trending" {
				shelf.Kind = "trending"
				shelf.Source = "tmdb-weekly"
			}
			if strings.HasPrefix(spec.ID, "topic-") {
				shelf.Kind = "topic"
				shelf.Source = "tmdb-discover"
			}
			if err = s.storeList(ctx, "shelf", spec.ID, shelf, titles, ShelfTTL); err == nil {
				success++
			}
		}

		if success < len(specs) {
			return errors.New("incomplete browse refresh; retained failed shelves")
		}
		return nil

	case "search":
		titles, err := s.Up.List(ctx, "search/multi?include_adult=false&query="+queryEscape(j.Query), "")
		if err != nil {
			return err
		}
		return s.saveList(ctx, "search", j.Query, titles, titles, time.Hour)

	case "recommendations":
		return s.buildRecommendations(ctx, j.Handle)
	}

	return errors.New("unknown job")
}

func (s *Service) buildRecommendations(ctx context.Context, handle string) error {
	record, err := s.Store.Get(ctx, "inputs", handle, s.Now())
	if err != nil {
		return err
	}

	var inputs recommend.Inputs
	if err = json.Unmarshal(record.Data, &inputs); err != nil {
		return err
	}
	if !inputs.Completed {
		return ErrIncomplete
	}

	fingerprint := inputs.Fingerprint()
	seeds := inputs.Seeds(s.Now())
	excluded := map[string]bool{}
	for _, seed := range seeds {
		excluded[seed.Key()] = true
	}

	lists := []recommend.Contribution{}
	sections := []model.Shelf{}
	all := []model.Title{}

	for _, g := range inputs.Genres {
		for _, typ := range []string{"movie", "tv"} {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			path, err := s.Up.DiscoverPath(ctx, g, typ, s.Now())
			if err != nil {
				continue
			}
			titles, err := s.Up.List(ctx, path, typ)
			if err != nil {
				continue
			}
			lists = append(lists, recommend.Contribution{Titles: titles, Weight: 2})
			sections = append(sections, model.Shelf{
				ID:      "taste-" + typ + "-" + g,
				Heading: upstream.GenreSpecs[g].Name + map[string]string{"movie": " movies", "tv": " series"}[typ],
				Kind:    "taste", Source: "tmdb-discover",
				Titles: clean(titles, excluded, 20),
			})
			all = append(all, titles...)
		}
	}

	reasons := 0
	for _, seed := range seeds {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		path := fmt.Sprintf("%s/%d/recommendations?language=en-US", seed.Type, seed.ID)
		titles, err := s.Up.List(ctx, path, seed.Type)
		if err != nil {
			continue
		}
		lists = append(lists, recommend.Contribution{Titles: titles, Weight: seed.Weight})
		all = append(all, titles...)
		if reasons < 3 {
			contextual := recommend.ContextualShelf(seed.Title, clean(titles, excluded, 20))
			contextual.ID = "like-" + seed.Key()
			contextual.Kind = "because"
			contextual.Source = "tmdb-recommendations"
			contextual.Reason = "watched"
			for _, favorite := range inputs.Favorites {
				if favorite.Key() == seed.Key() {
					contextual.Heading = "Because you liked " + seed.Title
					contextual.Reason = "liked"
					break
				}
			}
			if strings.TrimSpace(seed.Title) != "" && len(contextual.Titles) > 0 {
				sections = append(sections, contextual)
				reasons++
			}
		}
	}

	picks := recommend.Rank(lists, excluded)
	if len(picks) == 0 {
		return errors.New("no recommendation candidates")
	}

	sections = append([]model.Shelf{{ID: "for-you", Heading: "Picked for you", Kind: "personal", Source: "profile", Titles: picks}}, sections...)

	shelves, err := s.Store.List(ctx, "shelf", 100, s.Now())
	if err != nil {
		return err
	}
	for _, r := range shelves {
		var shelf model.Shelf
		if json.Unmarshal(r.Data, &shelf) == nil {
			sections = append(sections, shelf)
		}
	}

	s.mu.Lock()
	current, err := s.Store.Get(ctx, "inputs", handle, s.Now())
	var latest recommend.Inputs
	if err != nil || json.Unmarshal(current.Data, &latest) != nil || latest.Fingerprint() != fingerprint {
		s.mu.Unlock()
		return ErrInputsChanged
	}

	rec := recommend.Recommendations{
		Fingerprint: fingerprint,
		Pool:        picks,
		Sections:    sections,
	}
	err = s.saveRecommendation(ctx, handle, rec, all, inputs.Cadence(s.Now()))
	s.mu.Unlock()

	return err
}

func (s *Service) saveRecommendation(ctx context.Context, handle string, rec recommend.Recommendations, titles []model.Title, ttl time.Duration) error {
	writes := []db.Write{{Kind: "recommendations", Key: handle, Data: rec, TTL: ttl}}
	seen := map[string]bool{}
	for _, t := range titles {
		if seen[t.Key()] {
			continue
		}
		seen[t.Key()] = true
		writes = append(writes, db.Write{Kind: "title", Key: t.Key(), Data: t, TTL: MetadataTTL})
	}
	return s.Store.PutBatch(ctx, writes, s.Now())
}

func clean(titles []model.Title, excluded map[string]bool, limit int) []model.Title {
	out := []model.Title{}
	seen := map[string]bool{}
	for _, t := range titles {
		if !excluded[t.Key()] && !seen[t.Key()] && t.Poster != "" {
			seen[t.Key()] = true
			out = append(out, t)
			if len(out) == limit {
				break
			}
		}
	}
	return out
}

func queryEscape(q string) string {
	return url.QueryEscape(q)
}

func (s *Service) finishJob(j Job, runErr error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	defer delete(s.pending, j.Key())

	if s.ctx.Err() != nil {
		return
	}

	ttl := MetadataTTL
	switch j.Kind {
	case "artwork":
		ttl = ArtworkTTL
	case "browse":
		ttl = ShelfTTL
	case "search":
		ttl = time.Hour
	case "recommendations":
		record, err := s.Store.Get(s.ctx, "inputs", j.Handle, s.Now())
		var inputs recommend.Inputs
		if errors.Is(err, db.ErrMissing) {
			return
		}
		if err == nil && json.Unmarshal(record.Data, &inputs) == nil {
			if !inputs.Completed {
				return
			}
			ttl = inputs.Cadence(s.Now())
		}
	}

	out := jobOutcome{
		due:     s.Now().Add(ttl).UnixMilli(),
		success: s.Now().UnixMilli(),
	}

	if runErr != nil {
		out = jobOutcome{
			due:    s.Now().Add(RetryDelay).UnixMilli(),
			failed: 1,
		}
	}

	if errors.Is(runErr, ErrInputsChanged) || errors.Is(runErr, ErrIncomplete) {
		out = jobOutcome{}
	}

	if err := s.persistOutcome(s.ctx, j.Key(), out); err != nil {
		if len(s.unsaved) == 0 {
			log.Print("catalog refresh schedule persistence failed; updater paused")
		}
		s.unsaved[j.Key()] = out
		s.schedulePaused.Store(true)
	}
}

func (s *Service) persistOutcome(ctx context.Context, key string, out jobOutcome) error {
	_, err := s.Store.DB().ExecContext(ctx,
		`UPDATE jobs SET due=?,failed=?,last_success=CASE WHEN ?>0 THEN ? ELSE last_success END WHERE key=?`,
		out.due, out.failed, out.success, out.success, key)
	return err
}

func (s *Service) recoverSchedule(ctx context.Context) bool {
	s.mu.Lock()
	defer s.mu.Unlock()

	had := len(s.unsaved) > 0
	for key, out := range s.unsaved {
		if err := s.persistOutcome(ctx, key, out); err != nil {
			return false
		}
		delete(s.unsaved, key)
	}

	s.schedulePaused.Store(false)
	if had {
		log.Print("catalog refresh schedule recovered")
	}
	return true
}

func (s *Service) recommendationActive(ctx context.Context, handle string) (bool, error) {
	record, err := s.Store.Get(ctx, "inputs", handle, s.Now())
	if err != nil {
		return false, err
	}
	var inputs recommend.Inputs
	if err = json.Unmarshal(record.Data, &inputs); err != nil {
		return false, err
	}
	return inputs.Completed, nil
}

// ScheduleHealthy returns whether background schedule persistence is functioning normally.
func (s *Service) ScheduleHealthy() bool {
	return !s.schedulePaused.Load()
}
