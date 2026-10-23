//
//  store.go
//  Stream TV Catalog Service
//
//  Created by ChrisRodStar.
//
//  Summary:
//  Persistence tier using SQLite in Write-Ahead Logging (WAL) mode.
//  Implements transactional updates, schema migrations, size/quota limits,
//  consistent exports, and non-blocking online backups via VACUUM INTO.
//

package db

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"time"

	_ "modernc.org/sqlite"

	"stv-catalog/internal/model"
	"stv-catalog/internal/recommend"
)

var (
	ErrMissing  = errors.New("missing")
	ErrCapacity = errors.New("storage capacity")
)

// Record represents a persisted key-value entry with TTL expiration, revision metadata, and freshness state.
type Record struct {
	Kind     string          `json:"kind"`
	Key      string          `json:"key"`
	Data     json.RawMessage `json:"data"`
	Updated  time.Time       `json:"updated_at"`
	Expires  time.Time       `json:"expires_at"`
	Revision int64           `json:"revision"`
	State    string          `json:"state,omitempty"`
}

// Write represents an individual write operation in a batch.
type Write struct {
	Kind string
	Key  string
	Data any
	TTL  time.Duration
}

// Export contains a complete point-in-time snapshot of the catalog database.
type Export struct {
	Version  string    `json:"version"`
	Revision int64     `json:"revision"`
	Exported time.Time `json:"exported_at"`
	Records  []Record  `json:"records"`
}

// CrossIDs stores external identifier mappings (IMDb, MyAnimeList).
type CrossIDs struct {
	IMDb string `json:"imdb_id"`
	MAL  int    `json:"mal_id"`
}

// Store encapsulates SQLite database operations and safety limits.
type Store struct {
	db         *sql.DB
	MaxBytes   int64
	MaxRecords int
}

// DB returns the underlying database handle.
func (s *Store) DB() *sql.DB {
	return s.db
}

// Open initializes or connects to a SQLite database at path with WAL configuration.
func Open(path string) (*Store, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, err
	}
	f, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, err
	}
	_ = f.Close()
	if err = os.Chmod(path, 0o600); err != nil {
		return nil, err
	}

	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)

	s := &Store{
		db:         db,
		MaxBytes:   128 << 20,
		MaxRecords: 20000,
	}

	if err = s.Migrate(); err != nil {
		_ = db.Close()
		return nil, err
	}
	if err = s.removeArtwork(); err != nil {
		_ = db.Close()
		return nil, err
	}
	return s, nil
}

func (s *Store) removeArtwork() error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()

	if _, err = tx.Exec(`DELETE FROM jobs WHERE json_extract(spec,'$.kind')='artwork'`); err != nil {
		return err
	}
	result, err := tx.Exec(`DELETE FROM records WHERE kind='artwork'`)
	if err != nil {
		return err
	}
	count, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if count > 0 {
		if _, err = tx.Exec(`UPDATE revision SET value=value+1`); err != nil {
			return err
		}
	}
	if err = tx.Commit(); err != nil {
		return err
	}
	if count > 0 {
		_, err = s.db.Exec(`VACUUM`)
		if err == nil {
			_, _ = s.db.Exec(`PRAGMA wal_checkpoint(TRUNCATE)`)
		}
	}
	return err
}

// Close flushes connections and shuts down the SQLite handle.
func (s *Store) Close() error {
	return s.db.Close()
}

// Migrate executes initial schema creation and ensures pragmas are set.
func (s *Store) Migrate() error {
	if _, err := s.db.Exec(`PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_size_limit=16777216; PRAGMA mmap_size=268435456; PRAGMA cache_size=-64000;`); err != nil {
		return err
	}

	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()

	if _, err = tx.Exec(`CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY);`); err != nil {
		return err
	}

	var version int
	if err = tx.QueryRow(`SELECT COALESCE(MAX(version),0) FROM schema_migrations`).Scan(&version); err != nil {
		return err
	}

	if version > 2 {
		return errors.New("database schema newer than service")
	}

	switch version {
	case 0:
		_, err = tx.Exec(`
			CREATE TABLE records (
				kind TEXT NOT NULL,
				key TEXT NOT NULL,
				data BLOB NOT NULL,
				updated INTEGER NOT NULL,
				expires INTEGER NOT NULL,
				revision INTEGER NOT NULL,
				PRIMARY KEY(kind, key)
			);
			CREATE INDEX records_expiry ON records(expires, kind);
			CREATE INDEX records_title_search ON records(kind, lower(json_extract(data, '$.title')), key);
			CREATE TABLE revision(value INTEGER NOT NULL);
			INSERT INTO revision VALUES(0);
			CREATE TABLE jobs (
				key TEXT PRIMARY KEY,
				spec BLOB NOT NULL,
				due INTEGER NOT NULL DEFAULT 0,
				last_success INTEGER NOT NULL DEFAULT 0,
				failed INTEGER NOT NULL DEFAULT 0
			);
			CREATE INDEX jobs_due ON jobs(due);
			INSERT INTO schema_migrations VALUES(2);
		`)
		if err != nil {
			return err
		}
	case 1:
		_, err = tx.Exec(`
			CREATE INDEX IF NOT EXISTS records_title_search ON records(kind, lower(json_extract(data, '$.title')), key);
			INSERT INTO schema_migrations VALUES(2);
		`)
		if err != nil {
			return err
		}
	}
	return tx.Commit()
}

func scanRecord(row interface{ Scan(...any) error }, now time.Time) (Record, error) {
	var r Record
	var updated, expires int64
	err := row.Scan(&r.Kind, &r.Key, &r.Data, &updated, &expires, &r.Revision)
	if errors.Is(err, sql.ErrNoRows) {
		return r, ErrMissing
	}
	r.Updated = time.UnixMilli(updated).UTC()
	r.Expires = time.UnixMilli(expires).UTC()
	r.State = "fresh"
	if !now.Before(r.Expires) {
		r.State = "stale"
	}
	return r, err
}

// Get fetches a single record by kind and key, checking expiry against now.
func (s *Store) Get(ctx context.Context, kind, key string, now time.Time) (Record, error) {
	return scanRecord(s.db.QueryRowContext(ctx, `SELECT kind,key,data,updated,expires,revision FROM records WHERE kind=? AND key=?`, kind, key), now)
}

// Put saves a single record under an atomic transaction.
func (s *Store) Put(ctx context.Context, kind, key string, data any, now time.Time, ttl time.Duration) error {
	return s.PutBatch(ctx, []Write{{Kind: kind, Key: key, Data: data, TTL: ttl}}, now)
}

// PutBatch applies a list of writes atomically within a single transaction and increments revision.
func (s *Store) PutBatch(ctx context.Context, writes []Write, now time.Time) error {
	for _, write := range writes {
		if write.Kind == "artwork" {
			return errors.New("artwork storage is disabled")
		}
	}

	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()

	if _, err = tx.ExecContext(ctx, `UPDATE revision SET value=value+1`); err != nil {
		return err
	}

	var rev int64
	if err = tx.QueryRowContext(ctx, `SELECT value FROM revision`).Scan(&rev); err != nil {
		return err
	}

	for _, w := range writes {
		b, err := json.Marshal(w.Data)
		if err != nil {
			return err
		}

		if w.Kind == "title" {
			var priorJSON []byte
			err := tx.QueryRowContext(ctx, `SELECT data FROM records WHERE kind='title' AND key=?`, w.Key).Scan(&priorJSON)
			if err != nil && !errors.Is(err, sql.ErrNoRows) {
				return err
			}
			if err == nil {
				var prior, next model.Title
				if err = json.Unmarshal(priorJSON, &prior); err != nil {
					return err
				}
				if err = json.Unmarshal(b, &next); err != nil {
					return err
				}
				if prior.Details && !next.Details {
					continue
				}
				if next.IMDb == "" {
					next.IMDb = prior.IMDb
				}
				if next.MAL == 0 {
					next.MAL = prior.MAL
				}
				b, err = json.Marshal(next)
				if err != nil {
					return err
				}
			}
		}

		if _, err = tx.ExecContext(ctx, `INSERT INTO records VALUES(?,?,?,?,?,?) ON CONFLICT(kind,key) DO UPDATE SET data=excluded.data,updated=excluded.updated,expires=excluded.expires,revision=excluded.revision`, w.Kind, w.Key, b, now.UnixMilli(), now.Add(w.TTL).UnixMilli(), rev); err != nil {
			return err
		}
	}

	var count int
	var total int64
	if err = tx.QueryRowContext(ctx, `SELECT COUNT(*),COALESCE(SUM(length(data)),0) FROM records`).Scan(&count, &total); err != nil {
		return err
	}
	if count > s.MaxRecords || total > s.MaxBytes {
		return ErrCapacity
	}
	return tx.Commit()
}

// List returns records of a given kind up to limit ordered by key.
func (s *Store) List(ctx context.Context, kind string, limit int, now time.Time) ([]Record, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT kind,key,data,updated,expires,revision FROM records WHERE kind=? ORDER BY key LIMIT ?`, kind, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	out := []Record{}
	for rows.Next() {
		r, err := scanRecord(rows, now)
		if err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// Search queries titles matching a case-insensitive substring search.
func (s *Store) Search(ctx context.Context, q string, offset int, now time.Time) ([]Record, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT kind,key,data,updated,expires,revision FROM records INDEXED BY records_title_search WHERE kind='title' AND instr(lower(json_extract(data,'$.title')),lower(?))>0 ORDER BY key LIMIT 50 OFFSET ?`, q, offset)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	out := []Record{}
	for rows.Next() {
		r, err := scanRecord(rows, now)
		if err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// Health verifies the database connection and returns error if inaccessible.
func (s *Store) Health(ctx context.Context) error {
	var v int
	return s.db.QueryRowContext(ctx, `SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1`).Scan(&v)
}

func applyCrossIDs(r Record, ids Record) Record {
	var t model.Title
	var cross CrossIDs
	if json.Unmarshal(r.Data, &t) != nil || json.Unmarshal(ids.Data, &cross) != nil {
		return r
	}
	if cross.IMDb != "" {
		t.IMDb = cross.IMDb
	}
	if cross.MAL > 0 {
		t.MAL = cross.MAL
	}
	r.Data, _ = json.Marshal(t)
	r.Revision = max(r.Revision, ids.Revision)
	return r
}

func annotateRecommendation(r Record, p recommend.Inputs, now time.Time) Record {
	var rec recommend.Recommendations
	if json.Unmarshal(r.Data, &rec) != nil {
		r.State = "stale"
		return r
	}
	r.Expires = r.Updated.Add(p.Cadence(now))
	if p.Fingerprint() != rec.Fingerprint || !now.Before(r.Expires) {
		r.State = "stale"
	} else {
		r.State = "fresh"
	}
	return r
}

// Title returns a title record enriched with cross-IDs from external catalogs.
func (s *Store) Title(ctx context.Context, key string, now time.Time) (Record, error) {
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return Record{}, err
	}
	defer tx.Rollback()

	r, err := scanRecord(tx.QueryRowContext(ctx, `SELECT kind,key,data,updated,expires,revision FROM records WHERE kind='title' AND key=?`, key), now)
	if err != nil {
		return r, err
	}

	ids, err := scanRecord(tx.QueryRowContext(ctx, `SELECT kind,key,data,updated,expires,revision FROM records WHERE kind='cross_ids' AND key=?`, key), now)
	if err == nil {
		r = applyCrossIDs(r, ids)
	} else if !errors.Is(err, ErrMissing) {
		return r, err
	}
	return r, tx.Commit()
}

// Recommendation fetches recommendation records annotated with freshness and cadence.
func (s *Store) Recommendation(ctx context.Context, handle string, now time.Time) (Record, recommend.Inputs, error) {
	var p recommend.Inputs
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return Record{}, p, err
	}
	defer tx.Rollback()

	r, err := scanRecord(tx.QueryRowContext(ctx, `SELECT kind,key,data,updated,expires,revision FROM records WHERE kind='recommendations' AND key=?`, handle), now)
	if err != nil {
		return r, p, err
	}

	var b []byte
	if err = tx.QueryRowContext(ctx, `SELECT data FROM records WHERE kind='inputs' AND key=?`, handle).Scan(&b); err != nil {
		return r, p, err
	}
	if err = json.Unmarshal(b, &p); err != nil {
		return r, p, err
	}

	r = annotateRecommendation(r, p, now)
	return r, p, tx.Commit()
}

// Export dumps an atomic, read-only snapshot of all catalog records.
func (s *Store) Export(ctx context.Context, now time.Time) (Export, error) {
	out := Export{Version: "1", Exported: now, Records: []Record{}}
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return out, err
	}
	defer tx.Rollback()

	if err = tx.QueryRowContext(ctx, `SELECT value FROM revision`).Scan(&out.Revision); err != nil {
		return out, err
	}

	rows, err := tx.QueryContext(ctx, `SELECT kind,key,data,updated,expires,revision FROM records ORDER BY kind,key`)
	if err != nil {
		return out, err
	}
	defer rows.Close()

	for rows.Next() {
		r, err := scanRecord(rows, now)
		if err != nil {
			return out, err
		}
		out.Records = append(out.Records, r)
	}
	if err = rows.Err(); err != nil {
		return out, err
	}

	inputs := map[string]recommend.Inputs{}
	for _, r := range out.Records {
		if r.Kind == "inputs" {
			var p recommend.Inputs
			if json.Unmarshal(r.Data, &p) == nil {
				inputs[r.Key] = p
			}
		}
	}

	crossIDs := map[string]Record{}
	for _, r := range out.Records {
		if r.Kind == "cross_ids" {
			crossIDs[r.Key] = r
		}
	}

	filtered := make([]Record, 0, len(out.Records))
	for _, r := range out.Records {
		if err := ctx.Err(); err != nil {
			return out, err
		}
		if r.Kind == "inputs" {
			continue
		}
		if r.Kind == "title" {
			if ids, ok := crossIDs[r.Key]; ok {
				r = applyCrossIDs(r, ids)
			}
		}
		if r.Kind == "recommendations" {
			p, ok := inputs[r.Key]
			if !ok {
				return out, errors.New("recommendation inputs missing")
			}
			r = annotateRecommendation(r, p, now)
			var rec recommend.Recommendations
			if err := json.Unmarshal(r.Data, &rec); err != nil {
				return out, err
			}
			r.Data, err = json.Marshal(rec.View("all", p.Format))
			if err != nil {
				return out, err
			}
		}
		filtered = append(filtered, r)
	}

	out.Records = filtered
	return out, tx.Commit()
}

// Backup creates a point-in-time SQLite database copy using VACUUM INTO.
func (s *Store) Backup(ctx context.Context, path string) error {
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		return errors.New("backup destination must not exist")
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}

	_, err := s.db.ExecContext(ctx, `VACUUM INTO ?`, path)
	if err != nil {
		_ = os.Remove(path)
		return errors.New("backup failed")
	}
	return os.Chmod(path, 0o600)
}
