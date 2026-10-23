package api

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"database/sql"
	"encoding/json"
	"errors"
	"golang.org/x/sys/unix"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

type Service struct {
	owner             *os.File
	closeOnce         sync.Once
	closeErr          error
	lifecycleMu       sync.Mutex
	cfg               Config
	db                *sql.DB
	client            *http.Client
	signer            ed25519.PrivateKey
	encryption        []byte
	tokens            map[string]string
	household         string
	ctx               context.Context
	cancel            context.CancelFunc
	wg                sync.WaitGroup
	slots             chan struct{}
	playLocks         playbackLocks
	enrichmentSlots   chan struct{}
	enrichmentCancels map[string]context.CancelFunc
	snapMu            sync.RWMutex
	snapshot          Snapshot
	stopping          atomic.Bool
	syncMu            sync.Mutex
	refreshWake       chan struct{}
	snapshotWake      chan struct{}
}

func New(c Config) (*Service, error) {
	signer, e := loadSigner(c.SigningKeyFile)
	if e != nil {
		return nil, errors.New("signing key unavailable")
	}
	key, e := privateFile(c.EncryptionKeyFile, 32)
	if e != nil || len(key) != 32 {
		return nil, errors.New("encryption key unavailable")
	}
	s := &Service{cfg: c, signer: signer, encryption: key, tokens: map[string]string{}, slots: make(chan struct{}, 32), enrichmentSlots: make(chan struct{}, 2)}
	s.refreshWake = make(chan struct{}, 1)
	s.snapshotWake = make(chan struct{}, 1)
	for name, path := range map[string]string{"catalog": c.CatalogTokenFile, "provider": c.ProviderTokenFile, "relay": c.RelayTokenFile} {
		b, e := privateFile(path, 4096)
		if e != nil || !validServiceToken(b) {
			return nil, errors.New("service secret unavailable")
		}
		s.tokens[name] = string(b)
	}
	if e = os.MkdirAll(filepath.Dir(c.DBPath), 0700); e != nil {
		return nil, errors.New("storage unavailable")
	}
	s.owner, e = os.OpenFile(c.DBPath+".owner", os.O_CREATE|os.O_RDWR, 0600)
	if e != nil {
		return nil, errors.New("storage owner unavailable")
	}
	if e = unix.Flock(int(s.owner.Fd()), unix.LOCK_EX|unix.LOCK_NB); e != nil {
		s.owner.Close()
		return nil, errors.New("storage already owned")
	}
	s.db, e = openStore(c.DBPath)
	if e != nil {
		s.owner.Close()
		return nil, errors.New("storage unavailable")
	}
	var sentinel []byte
	e = s.db.QueryRow("SELECT value FROM settings WHERE key='encryption-check'").Scan(&sentinel)
	if errors.Is(e, sql.ErrNoRows) {
		sentinel, e = seal(key, "api-key-check", []byte("family-stream-api.v1"))
		if e == nil {
			_, e = s.db.Exec("INSERT INTO settings VALUES('encryption-check',?)", sentinel)
		}
	} else if e == nil {
		var plain []byte
		plain, e = unseal(key, "api-key-check", sentinel)
		if e == nil && string(plain) != "family-stream-api.v1" {
			e = errors.New("key mismatch")
		}
	}
	if e != nil {
		s.db.Close()
		s.owner.Close()
		return nil, errors.New("state key rejected")
	}
	s.household, e = s.selectHousehold(c.HouseholdAccountID)
	if e != nil {
		s.db.Close()
		s.owner.Close()
		return nil, e
	}
	s.ctx, s.cancel = context.WithCancel(context.Background())
	transport := &http.Transport{Proxy: nil, DialContext: (&net.Dialer{Timeout: 3 * time.Second, KeepAlive: 30 * time.Second}).DialContext, MaxConnsPerHost: 4, MaxIdleConns: 8, MaxIdleConnsPerHost: 4, ResponseHeaderTimeout: 32 * time.Second, MaxResponseHeaderBytes: 16384, DisableCompression: true}
	s.client = &http.Client{Transport: transport, Timeout: 35 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return errors.New("redirect rejected") }}
	var raw []byte
	e = s.db.QueryRow("SELECT data FROM snapshot WHERE id=1").Scan(&raw)
	if e == nil {
		if e = decode(raw, &s.snapshot); e == nil {
			previousCount := len(s.snapshot.Records)
			s.snapshot = metadataSnapshot(s.snapshot)
			e = validateSnapshot(s.snapshot)
			if e == nil && previousCount != len(s.snapshot.Records) {
				_, e = s.db.Exec("UPDATE snapshot SET data=? WHERE id=1", marshal(s.snapshot))
				if e == nil {
					_, e = s.db.Exec("VACUUM")
					if e == nil {
						_, e = s.db.Exec("PRAGMA wal_checkpoint(TRUNCATE)")
					}
				}
			}
		}
	}
	if e != nil && !errors.Is(e, sql.ErrNoRows) {
		s.db.Close()
		s.owner.Close()
		s.cancel()
		return nil, errors.New("retained snapshot invalid")
	}
	return s, nil
}
func (s *Service) Start() {
	s.startWorker(func(ctx context.Context) { _ = s.SyncSnapshot(ctx) }, s.snapshotWake)
	s.startWorker(func(ctx context.Context) {
		s.syncMu.Lock()
		defer s.syncMu.Unlock()
		_ = s.syncRecommendations(ctx)
	})
	s.startWorker(func(ctx context.Context) { s.flushRefresh(ctx) }, s.refreshWake)
	s.startWorker(func(ctx context.Context) { _ = s.retryRevocations(ctx); s.cleanup(ctx) })
}
func (s *Service) startWorker(work func(context.Context), wakeChannels ...<-chan struct{}) {
	var wake <-chan struct{}
	if len(wakeChannels) > 0 {
		wake = wakeChannels[0]
	}
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		tick := time.NewTicker(time.Minute)
		defer tick.Stop()
		for {
			ctx, cancel := context.WithTimeout(s.ctx, 40*time.Second)
			work(ctx)
			cancel()
			select {
			case <-s.ctx.Done():
				return
			case <-tick.C:
			case <-wake:
				// Coalesce demand and bound refresh work while clients await
				// newly requested title/season metadata.
				timer := time.NewTimer(500 * time.Millisecond)
				select {
				case <-s.ctx.Done():
					timer.Stop()
					return
				case <-timer.C:
				}
			}
		}
	}()
}

func notifyWorker(wake chan struct{}) {
	select {
	case wake <- struct{}{}:
	default:
	}
}
func (s *Service) cleanup(ctx context.Context) {
	now := time.Now().Unix()
	_, _ = s.db.ExecContext(ctx, "DELETE FROM sessions WHERE expires < ?", now)
	_, _ = s.db.ExecContext(ctx, "DELETE FROM activity WHERE day < ?", time.Now().UTC().AddDate(0, 0, -31).Format("2006-01-02"))
	_, _ = s.db.ExecContext(ctx, "DELETE FROM playbacks WHERE expires < ?", now-7*86400)
}

func (s *Service) Close() error {
	s.closeOnce.Do(func() {
		s.StopAdmission()
		s.wg.Wait()
		s.client.CloseIdleConnections()
		s.closeErr = s.db.Close()
		_ = s.owner.Close()
	})
	return s.closeErr
}

func (s *Service) StopAdmission() {
	s.lifecycleMu.Lock()
	defer s.lifecycleMu.Unlock()
	s.stopping.Store(true)
	s.cancel()
}

func (s *Service) private(ctx context.Context, name, method, path string, body any, idem string, limit int64) ([]byte, error) {
	timeout := 12 * time.Second
	if name == "provider" {
		timeout = 35 * time.Second
	}
	if name == "catalog" {
		timeout = 3 * time.Second
		if path == "/internal/v1/snapshot" {
			timeout = 30 * time.Second
		}
	}
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	base := map[string]string{"catalog": s.cfg.CatalogURL, "provider": s.cfg.ProviderURL, "relay": s.cfg.RelayURL}[name]
	var reader io.Reader
	if body != nil {
		reader = bytes.NewReader(marshal(body))
	}
	r, e := http.NewRequestWithContext(ctx, method, base+path, reader)
	if e != nil {
		return nil, e
	}
	r.Header.Set("Authorization", "Bearer "+s.tokens[name])
	r.Header.Set("Content-Type", "application/json")
	if idem != "" {
		r.Header.Set("Idempotency-Key", idem)
	}
	resp, e := s.client.Do(r)
	if e != nil {
		return nil, &fault{503, name + "_unavailable"}
	}
	defer resp.Body.Close()
	b, e := io.ReadAll(io.LimitReader(resp.Body, limit+1))
	if e != nil || int64(len(b)) > limit {
		return nil, &fault{502, name + "_invalid_response"}
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		code := name + "_unavailable"
		status := 503
		if name == "relay" {
			var p struct {
				Code string `json:"code"`
			}
			_ = json.Unmarshal(b, &p)
			switch p.Code {
			case "invalid_input", "invalid_selection", "unsupported_format", "revision_conflict", "source_refresh_required", "session_expired", "source_expired", "revoked_grant", "vpn_unavailable", "request_limit", "draining":
				code = p.Code
			}
			if resp.StatusCode == 400 || resp.StatusCode == 413 || resp.StatusCode == 415 || resp.StatusCode == 422 {
				status = 422
			}
			if resp.StatusCode == 409 {
				status = 409
			}
			if resp.StatusCode == 404 {
				status = 404
			}
			if resp.StatusCode == 410 {
				status = 410
			}
		}
		return nil, &fault{status, code}
	}
	return b, nil
}

var identifier = regexp.MustCompile(`^[A-Za-z0-9_-]{1,128}$`)

func safeID(v string) bool { return identifier.MatchString(v) }
func decode(b []byte, v any) error {
	if !json.Valid(b) || checkJSON(b) != nil {
		return bad()
	}
	d := json.NewDecoder(bytes.NewReader(b))
	d.DisallowUnknownFields()
	d.UseNumber()
	if e := d.Decode(v); e != nil {
		return bad()
	}
	return nil
}
func (s *Service) Handler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "private, no-store")
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		s.lifecycleMu.Lock()
		if s.stopping.Load() {
			s.lifecycleMu.Unlock()
			writeError(w, &fault{503, "draining"})
			return
		}
		s.wg.Add(1)
		s.lifecycleMu.Unlock()
		defer s.wg.Done()
		select {
		case s.slots <- struct{}{}:
			defer func() { <-s.slots }()
		default:
			writeError(w, &fault{429, "overloaded"})
			return
		}
		if len(r.URL.Path) > 4096 || len(r.URL.RawQuery) > 4096 {
			writeError(w, bad())
			return
		}
		query, e := url.ParseQuery(r.URL.RawQuery)
		if e != nil {
			writeError(w, bad())
			return
		}
		for _, values := range query {
			if len(values) != 1 {
				writeError(w, bad())
				return
			}
		}
		ctx, cancel := context.WithTimeout(r.Context(), 40*time.Second)
		defer cancel()
		stop := context.AfterFunc(s.ctx, cancel)
		defer stop()
		r = r.WithContext(ctx)
		if r.Method != "GET" && r.Method != "HEAD" {
			if o := r.Header.Get("Origin"); o != "" && o != s.cfg.Origin {
				writeError(w, &fault{403, "origin_rejected"})
				return
			}
			if r.Header.Get("Sec-Fetch-Site") == "cross-site" {
				writeError(w, &fault{403, "origin_rejected"})
				return
			}
		}
		if e := s.route(w, r); e != nil {
			writeError(w, e)
		}
	})
}
func body(r *http.Request, v any) error {
	if r.Header.Get("Content-Type") != "application/json" {
		return &fault{415, "json_required"}
	}
	b, e := io.ReadAll(io.LimitReader(r.Body, 32769))
	if e != nil || len(b) > 32768 {
		return bad()
	}
	if strings.HasSuffix(r.URL.Path, "/progress") && !requiredKeys(b, "playback_id", "sequence", "position_seconds", "duration_seconds", "watched_seconds") {
		return bad()
	}
	if strings.HasSuffix(r.URL.Path, "/preferences") && !requiredKeys(b, "completed", "format", "genres") {
		return bad()
	}
	if e := requestShape(b, reflect.TypeOf(v)); e != nil {
		return e
	}
	return decode(b, v)
}
func write(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if v != nil {
		_ = json.NewEncoder(w).Encode(v)
	}
}
func writeError(w http.ResponseWriter, e error) {
	var f *fault
	if !errors.As(e, &f) {
		f = &fault{503, "storage_unavailable"}
	}
	write(w, f.status, map[string]any{"error": f.code})
}

func validServiceToken(b []byte) bool {
	if len(b) < 32 || len(b) > 4096 {
		return false
	}
	for _, c := range b {
		if c < 33 || c > 126 {
			return false
		}
	}
	return true
}
