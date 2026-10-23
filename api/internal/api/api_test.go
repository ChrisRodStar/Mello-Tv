package api

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/x509"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

type fixture struct {
	t                *testing.T
	s                *Service
	c                Config
	server           *httptest.Server
	catalog          *httptest.Server
	provider         *httptest.Server
	relay            *httptest.Server
	mu               sync.Mutex
	snapshot         Snapshot
	catalogMode      string
	providerCalls    int
	pendingSelection bool
	completeRelease  <-chan struct{}
	completeStarted  chan struct{}
	backupEnriched   chan struct{}
	lastLookup       map[string]any
	ready            RelayReady
	relayCreates     int
	relayMutations   int
	loseReply        bool
	noBackups        bool
	loseCreate       bool
	deleted          bool
	inputs           map[string]inputs
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	f := &fixture{t: t, inputs: map[string]inputs{}}
	now := time.Now().UTC().Add(-time.Minute)
	f.snapshot = Snapshot{Version: "1", Revision: 1, Exported: now, Records: []Record{}}
	f.catalog = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		if r.Header.Get("Authorization") != "Bearer "+strings.Repeat("c", 32) {
			t.Error("catalog auth")
		}
		switch {
		case r.URL.Path == "/internal/v1/snapshot":
			switch f.catalogMode {
			case "outage":
				w.WriteHeader(503)
			case "malformed":
				_, _ = w.Write([]byte(`{"version":`))
			case "interrupted":
				w.Header().Set("Content-Length", "999999")
				_, _ = w.Write([]byte(`{}`))
			default:
				write(w, 200, f.snapshot)
			}
		case strings.HasSuffix(r.URL.Path, "/inputs"):
			var in inputs
			_ = json.NewDecoder(r.Body).Decode(&in)
			f.inputs[strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, "/internal/v1/recommendations/"), "/inputs")] = in
			w.WriteHeader(202)
		case r.Method == "DELETE":
			delete(f.inputs, strings.TrimPrefix(r.URL.Path, "/internal/v1/recommendations/"))
			w.WriteHeader(204)
		default:
			w.WriteHeader(202)
		}
	}))
	f.provider = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		if r.Header.Get("Authorization") != "Bearer "+strings.Repeat("p", 32) {
			t.Error("provider auth")
		}
		f.providerCalls++
		_ = json.NewDecoder(r.Body).Decode(&f.lastLookup)
		lookup := f.lastLookup["lookup"].(map[string]any)
		complete, _ := lookup["wait_for_complete"].(bool)
		if complete && f.completeRelease != nil {
			release, started := f.completeRelease, f.completeStarted
			f.mu.Unlock()
			select {
			case started <- struct{}{}:
			default:
			}
			select {
			case <-release:
			case <-r.Context().Done():
			}
			f.mu.Lock()
		}
		source := map[string]any{"title": lookup["title"], "url": "http://private.example/master.m3u8", "resolver_metadata": map[string]any{"signed": "SECRET-PRIVATE-METADATA"}}
		ranked := map[string]any{"candidate": map[string]any{"source": source, "observation": map[string]any{"format": "hls"}}, "score": 4}
		write(w, 200, map[string]any{"version": "providers.v1", "pending": f.pendingSelection && !complete, "selected": ranked, "backups": []any{ranked, ranked}, "providers": []any{}, "rejected": map[string]any{}})
	}))
	f.ready = RelayReady{Version: "relay.v1", SID: strings.Repeat("s", 24), Entry: "/stream/" + strings.Repeat("s", 24) + "/entry", Format: "hls", Expires: time.Now().UTC().Add(time.Hour).Truncate(time.Second), Revision: 1, AuthRevision: 7, Generation: 1, State: "active"}
	f.relay = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		if r.Header.Get("Authorization") != "Bearer "+strings.Repeat("r", 32) {
			t.Error("relay auth")
		}
		switch {
		case r.Method == "POST" && strings.HasSuffix(r.URL.Path, "/backups"):
			var payload struct {
				Generation uint64 `json:"expected_generation"`
			}
			_ = json.NewDecoder(r.Body).Decode(&payload)
			if payload.Generation != f.ready.Generation {
				t.Error("backup enrichment changed generation")
			}
			select {
			case f.backupEnriched <- struct{}{}:
			default:
			}
			w.WriteHeader(204)
		case r.Method == "DELETE":
			f.deleted = true
			f.ready.State = "revoked"
			w.WriteHeader(204)
		case r.Method == "POST" && r.URL.Path == "/internal/v1/sessions":
			f.relayCreates++
			var payload map[string]any
			_ = json.NewDecoder(r.Body).Decode(&payload)
			if payload["owner_id"] == nil || payload["selection"] == nil {
				t.Error("missing selection")
			}
			if f.loseCreate {
				f.loseCreate = false
				w.Header().Set("Content-Length", "999")
				_, _ = w.Write([]byte("{"))
				return
			}
			write(w, 201, f.ready)
		case r.Method == "POST":
			var payload struct {
				Revision  uint64          `json:"expected_revision"`
				Owner     string          `json:"owner_id"`
				Selection json.RawMessage `json:"selection"`
				Expires   time.Time       `json:"expires_at"`
			}
			_ = json.NewDecoder(r.Body).Decode(&payload)
			if payload.Revision != f.ready.Revision {
				write(w, 409, map[string]string{"code": "revision_conflict"})
				return
			}
			if strings.HasSuffix(r.URL.Path, "/failover") && f.noBackups {
				write(w, 409, map[string]string{"code": "source_refresh_required"})
				return
			}
			f.relayMutations++
			f.ready.Revision++
			if strings.HasSuffix(r.URL.Path, "/renew") {
				f.ready.Expires = payload.Expires
				f.ready.Resume = false
			} else {
				f.ready.Generation++
				f.ready.Resume = true
			}
			if f.loseReply {
				f.loseReply = false
				w.Header().Set("Content-Length", "999")
				_, _ = w.Write([]byte("{"))
				return
			}
			write(w, 200, f.ready)
		default:
			write(w, 200, f.ready)
		}
	}))
	dir := t.TempDir()
	secret := func(name string, b []byte) string {
		p := filepath.Join(dir, name)
		if e := os.WriteFile(p, b, 0600); e != nil {
			t.Fatal(e)
		}
		return p
	}
	seed := bytes.Repeat([]byte{7}, 32)
	key := ed25519.NewKeyFromSeed(seed)
	der, _ := x509.MarshalPKCS8PrivateKey(key)
	f.c = Config{Listen: "127.0.0.1:8090", DBPath: filepath.Join(dir, "api.sqlite"), Origin: "http://127.0.0.1:8090", CatalogURL: f.catalog.URL, ProviderURL: f.provider.URL, RelayURL: f.relay.URL, CatalogTokenFile: secret("catalog", bytes.Repeat([]byte{'c'}, 32)), ProviderTokenFile: secret("provider", bytes.Repeat([]byte{'p'}, 32)), RelayTokenFile: secret("relay", bytes.Repeat([]byte{'r'}, 32)), SetupTokenFile: secret("setup", bytes.Repeat([]byte{'b'}, 32)), SigningKeyFile: secret("signer", der), EncryptionKeyFile: secret("encryption", bytes.Repeat([]byte{3}, 32)), KID: "test-key"}
	f.open()
	t.Cleanup(func() { f.server.Close(); _ = f.s.Close(); f.catalog.Close(); f.provider.Close(); f.relay.Close() })
	return f
}
func (f *fixture) open() {
	s, e := New(f.c)
	if e != nil {
		f.t.Fatal(e)
	}
	f.s = s
	f.server = httptest.NewServer(s.Handler())
}
func (f *fixture) restart() {
	f.server.Close()
	if e := f.s.Close(); e != nil {
		f.t.Fatal(e)
	}
	f.open()
}
func (f *fixture) request(method, path, token string, v any, key string) (int, []byte) {
	f.t.Helper()
	var reader io.Reader
	if v != nil {
		reader = bytes.NewReader(marshal(v))
	}
	r, e := http.NewRequest(method, f.server.URL+path, reader)
	if e != nil {
		f.t.Fatal(e)
	}
	r.Header.Set("Content-Type", "application/json")
	if token != "" {
		r.Header.Set("Authorization", "Bearer "+token)
	}
	if key != "" {
		r.Header.Set("Idempotency-Key", key)
	}
	client := *f.server.Client()
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	response, e := client.Do(r)
	if e != nil {
		f.t.Fatal(e)
	}
	defer response.Body.Close()
	b, e := io.ReadAll(response.Body)
	if e != nil {
		f.t.Fatal(e)
	}
	return response.StatusCode, b
}
func (f *fixture) want(status int, method, path, token string, v any, key string) []byte {
	f.t.Helper()
	code, b := f.request(method, path, token, v, key)
	if code != status {
		f.t.Fatalf("%s %s: status %d expected %d: %s", method, path, code, status, b)
	}
	return b
}

// Household initialization belongs to service startup, with no client session.
func (f *fixture) setup() string { return "" }
func (f *fixture) profile(token string) Profile {
	b := f.want(201, "POST", "/api/v1/profiles", token, map[string]string{"name": "Person", "color": "blue"}, "")
	var p Profile
	_ = decode(b, &p)
	return p
}
func (f *fixture) play(token, pid string, target any, key string) string {
	b := f.want(200, "POST", "/api/v1/profiles/"+pid+"/playback", token, target, key)
	var out struct {
		ID string `json:"id"`
	}
	_ = json.Unmarshal(b, &out)
	if out.ID == "" {
		f.t.Fatal("missing playback")
	}
	if bytes.Contains(b, []byte("private.example")) || bytes.Contains(b, []byte("SECRET")) {
		f.t.Fatal("leaked source")
	}
	return out.ID
}
func TestHouseholdAccessAndRestart(t *testing.T) {
	f := newFixture(t)
	f.want(200, "GET", "/api/v1/browse", "", nil, "")
	p := f.profile("")
	owner := f.s.household
	for _, path := range []string{"/api/v1/auth/setup", "/api/v1/auth/login", "/api/v1/accounts"} {
		f.want(404, "POST", path, "", nil, "")
	}
	f.want(204, "PUT", "/api/v1/profiles/"+p.ID+"/library/movie/7", "", nil, "")
	f.restart()
	if f.s.household != owner {
		t.Fatal("household owner changed")
	}
	f.want(200, "GET", "/api/v1/profiles/"+p.ID, "", nil, "")
	b := f.want(200, "GET", "/api/v1/profiles/"+p.ID+"/library", "", nil, "")
	if !bytes.Contains(b, []byte(`"id":7`)) {
		t.Fatal("library lost")
	}
	f.want(404, "PUT", "/api/v1/auth/password", "", nil, "")
}
func (f *fixture) seedSnapshot(pid string) {
	var b bytes.Buffer
	_ = png.Encode(&b, image.NewRGBA(image.Rect(0, 0, 2, 2)))
	now := time.Now().UTC().Add(-time.Hour)
	title := Title{Identity: Identity{"movie", 7}, Title: "Example", Poster: "/internal/v1/artwork/w500/example.png", Runtime: 120, IMDb: "tt1234", MAL: 42, Details: true}
	rec := func(kind, key string, data any) Record {
		return Record{Kind: kind, Key: key, Data: marshal(data), Updated: now, Expires: now.Add(30 * time.Minute), Revision: 1, State: "fresh"}
	}
	f.mu.Lock()
	f.snapshot = Snapshot{Version: "1", Revision: 1, Exported: now.Add(time.Minute), Records: []Record{rec("title", "movie:7", title), rec("title", "tv:7", Title{Identity: Identity{"tv", 7}, Title: "Series"}), rec("season", "tv:7:0", Season{ShowID: 7, Number: 0, Episodes: []Episode{{Season: 0, Number: 1, Name: "Special", Runtime: 20}}}), rec("shelf", "home", Shelf{ID: "home", Titles: []Title{title}}), rec("search", "Example", []Title{title}), rec("artwork", "w500/example.png", artwork{MIME: "image/png", Bytes: b.Bytes()})}}
	f.mu.Unlock()
	if e := f.s.SyncSnapshot(context.Background()); e != nil {
		f.t.Fatal(e)
	}
}
func TestDurableSnapshotValidationAndOutages(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	f.seedSnapshot(p.ID)
	for _, mode := range []string{"outage", "malformed", "interrupted"} {
		f.mu.Lock()
		f.catalogMode = mode
		f.mu.Unlock()
		if e := f.s.SyncSnapshot(context.Background()); e == nil {
			t.Fatal("bad snapshot accepted")
		}
		f.restart()
		for _, path := range []string{"/titles/movie/7", "/titles/tv/7", "/tv/7/seasons/0/episodes/1", "/browse", "/search?q=Example", "/search-results?q=Example"} {
			b := f.want(200, "GET", "/api/v1"+path, token, nil, "")
			if bytes.Contains(b, []byte("/internal/v1/")) {
				t.Fatal("private path exposed")
			}
		}
	}
	b := f.want(200, "GET", "/api/v1/titles/movie/7", token, nil, "")
	if !bytes.Contains(b, []byte(`"state":"stale"`)) {
		t.Fatal("freshness not recomputed")
	}
	f.want(404, "GET", "/api/v1/titles/movie/8", token, nil, "")
	f.mu.Lock()
	f.catalogMode = ""
	f.snapshot.Records[0].Key = "movie:8"
	f.mu.Unlock()
	if e := f.s.SyncSnapshot(context.Background()); e == nil {
		t.Fatal("mismatched identity accepted")
	}
	f.want(200, "GET", "/api/v1/titles/movie/7", token, nil, "")
}
func TestSnapshotRejectsPartialAndMalformedData(t *testing.T) {
	f := newFixture(t)
	f.seedSnapshot("")
	f.mu.Lock()
	original := marshal(f.snapshot)
	f.mu.Unlock()
	for _, mutate := range []func(*Snapshot){func(s *Snapshot) { s.Records = append(s.Records, s.Records[0]) }, func(s *Snapshot) {
		s.Records[0].Data = marshal(map[string]any{"type": "movie", "id": 7, "poster": "https://secret.example/a"})
	}, func(s *Snapshot) { s.Records[0].Revision = 2 }, func(s *Snapshot) { s.Version = "2" }, func(s *Snapshot) { s.Records = nil }, func(s *Snapshot) { s.Records[0].Kind = "unknown" }} {
		var snap Snapshot
		_ = decode(original, &snap)
		mutate(&snap)
		f.mu.Lock()
		f.snapshot = snap
		f.mu.Unlock()
		if e := f.s.SyncSnapshot(context.Background()); e == nil {
			t.Fatal("bad export accepted")
		}
	}
}
func TestPlaybackIdentityGrantsAndRecovery(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	f.seedSnapshot(p.ID)
	f.catalog.Close()
	pb := f.play(token, p.ID, map[string]any{"type": "tv", "id": 1429, "season": 0, "episode": 1, "imdb_id": "tt2560140", "mal_id": 16498}, "specials")
	_ = pb
}
func TestMoviePlaybackGrantAndIdempotency(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	f.seedSnapshot(p.ID)
	pb := f.play(token, p.ID, map[string]any{"type": "movie", "id": 7}, "create-one")
	base := "/api/v1/profiles/" + p.ID + "/playback/" + pb
	b := f.want(200, "GET", base, token, nil, "")
	var out struct {
		URL string `json:"url"`
	}
	_ = json.Unmarshal(b, &out)
	parts := strings.Split(strings.Split(out.URL, "?st=")[1], ".")
	claims, _ := base64.RawURLEncoding.DecodeString(parts[1])
	var c struct {
		ARV int `json:"arv"`
	}
	_ = json.Unmarshal(claims, &c)
	if c.ARV != 7 {
		t.Fatal("wrong auth revision")
	}
	sig, _ := base64.RawURLEncoding.DecodeString(parts[2])
	if !ed25519.Verify(f.s.signer.Public().(ed25519.PublicKey), []byte(parts[0]+"."+parts[1]), sig) {
		t.Fatal("bad signature")
	}
	f.play(token, p.ID, map[string]any{"type": "movie", "id": 7}, "create-one")
	f.mu.Lock()
	if f.providerCalls != 1 || f.relayCreates != 1 {
		t.Error("duplicate setup")
	}
	lookup := f.lastLookup["lookup"].(map[string]any)["title"].(map[string]any)
	if lookup["tmdb_id"] != float64(7) || lookup["my_anime_list_id"] != float64(42) {
		t.Error("cross-ID translation")
	}
	f.mu.Unlock()
	f.want(409, "POST", "/api/v1/profiles/"+p.ID+"/playback", token, map[string]any{"type": "movie", "id": 8}, "create-one")
	f.restart()
	f.want(200, "GET", base, token, nil, "")
	f.mu.Lock()
	f.loseReply = true
	f.mu.Unlock()
	f.want(502, "POST", base+"/backup", token, map[string]any{"expected_revision": 1}, "switch-one")
	f.want(200, "GET", base, token, nil, "")
	f.restart()
	f.want(200, "POST", base+"/backup", token, map[string]any{"expected_revision": 1}, "switch-one")
	f.mu.Lock()
	if f.relayMutations != 1 {
		t.Error("double switch after lost reply")
	}
	f.mu.Unlock()
	f.want(200, "POST", base+"/refresh", token, map[string]any{"expected_revision": 2}, "refresh-one")
	f.want(200, "POST", base+"/renew", token, map[string]any{"expected_revision": 3}, "renew-one")
	f.want(204, "DELETE", base, token, nil, "")
	f.want(410, "GET", base, token, nil, "")
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.deleted {
		t.Fatal("not revoked")
	}
}
func TestViewingCountersConcurrentAndSeeking(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	pb := f.play(token, p.ID, map[string]any{"type": "movie", "id": 7}, "watch")
	_, e := f.s.db.Exec("UPDATE playbacks SET created=? WHERE id=?", time.Now().Add(-time.Hour).Unix(), pb)
	if e != nil {
		t.Fatal(e)
	}
	path := "/api/v1/profiles/" + p.ID + "/progress"
	update := map[string]any{"playback_id": pb, "sequence": 1, "position_seconds": 7000, "duration_seconds": 7200, "watched_seconds": 600}
	var wg sync.WaitGroup
	var failed atomic.Bool
	for range 12 {
		wg.Go(func() {
			code, _ := f.request("PUT", path, token, update, "")
			if code != 200 {
				failed.Store(true)
			}
		})
	}
	wg.Wait()
	if failed.Load() {
		t.Fatal("concurrent update failed")
	}
	update["sequence"] = 2
	update["position_seconds"] = 5
	update["watched_seconds"] = 900
	f.want(200, "PUT", path, token, update, "")
	f.want(200, "PUT", path, token, map[string]any{"playback_id": pb, "sequence": 1, "position_seconds": 7200, "duration_seconds": 7200, "watched_seconds": 600}, "")
	b := f.want(200, "GET", "/api/v1/profiles/"+p.ID+"/resume?type=movie&id=7", token, nil, "")
	var progress Progress
	_ = decode(b, &progress)
	if progress.Watched != 900 || progress.Position != 5 || progress.Completed {
		t.Fatalf("incorrect measured viewing: %+v", progress)
	}
	in, _, e := f.s.derived(context.Background(), p.ID)
	if e != nil {
		t.Fatal(e)
	}
	if len(in.Signals) != 1 || in.Signals[0].Watched != 900 {
		t.Fatal("wrong aggregate")
	}
	f.restart()
	f.want(200, "PUT", path, token, update, "")
	b = f.want(200, "GET", "/api/v1/profiles/"+p.ID+"/resume?type=movie&id=7", token, nil, "")
	_ = decode(b, &progress)
	if progress.Watched != 900 {
		t.Fatal("restart double counted")
	}
}
func TestDeletionRevokesAmbiguousCreateAndCatalogInputs(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	f.mu.Lock()
	f.loseCreate = true
	f.mu.Unlock()
	path := "/api/v1/profiles/" + p.ID
	f.want(502, "POST", path+"/playback", token, map[string]any{"type": "movie", "id": 7}, "lost-create")
	if e := f.s.syncRecommendations(context.Background()); e != nil {
		t.Fatal(e)
	}
	f.want(204, "DELETE", path, token, nil, "")
	if e := f.s.retryRevocations(context.Background()); e != nil {
		t.Fatal(e)
	}
	if e := f.s.syncRecommendations(context.Background()); e != nil {
		t.Fatal(e)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.deleted || len(f.inputs) != 0 {
		t.Fatal("deletion outbox incomplete")
	}
}
func TestExhaustedBackupAllowsRefresh(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	pb := f.play(token, p.ID, map[string]any{"type": "movie", "id": 7}, "one")
	path := "/api/v1/profiles/" + p.ID + "/playback/" + pb
	f.mu.Lock()
	f.noBackups = true
	f.mu.Unlock()
	f.want(409, "POST", path+"/backup", token, map[string]any{"expected_revision": 1}, "backup")
	f.want(200, "POST", path+"/refresh", token, map[string]any{"expected_revision": 1}, "refresh")
}
func TestKeyMismatchAndSchemaFailClosed(t *testing.T) {
	f := newFixture(t)
	f.server.Close()
	_ = f.s.Close()
	if e := os.WriteFile(f.c.EncryptionKeyFile, bytes.Repeat([]byte{5}, 32), 0600); e != nil {
		t.Fatal(e)
	}
	if s, e := New(f.c); e == nil {
		s.Close()
		t.Fatal("wrong key accepted")
	}
	_ = os.WriteFile(f.c.EncryptionKeyFile, bytes.Repeat([]byte{3}, 32), 0600)
	f.open()
	_, e := f.s.db.Exec("PRAGMA user_version=99")
	if e != nil {
		t.Fatal(e)
	}
	f.server.Close()
	f.s.Close()
	if s, e := New(f.c); e == nil {
		s.Close()
		t.Fatal("unknown schema accepted")
	}
	db, e := openRawForTest(f.c.DBPath)
	if e != nil {
		t.Fatal(e)
	}
	_, _ = db.Exec("PRAGMA user_version=1")
	db.Close()
	f.open()
}
func openRawForTest(path string) (*sql.DB, error) { return sql.Open("sqlite", path) }
func TestJSONBoundsAndCSRF(t *testing.T) {
	for _, b := range []string{`{"id":1,"id":2}`, `{"id":1,"ID":2}`, `{} {}`, strings.Repeat("[", 34) + strings.Repeat("]", 34)} {
		if checkJSON([]byte(b)) == nil {
			t.Fatal("unsafe JSON accepted")
		}
	}
	f := newFixture(t)
	token := f.setup()
	req, _ := http.NewRequest("POST", f.server.URL+"/api/v1/profiles", strings.NewReader(`{"name":"X","color":"blue"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Origin", "https://attacker.example")
	resp, e := f.server.Client().Do(req)
	if e != nil {
		t.Fatal(e)
	}
	resp.Body.Close()
	if resp.StatusCode != 403 {
		t.Fatal("CSRF allowed")
	}
}

func TestSnapshotCommitFailurePreservesPrior(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	f.seedSnapshot("")
	if _, e := f.s.db.Exec("CREATE TRIGGER reject_snapshot BEFORE UPDATE ON snapshot BEGIN SELECT RAISE(ABORT,'synthetic'); END"); e != nil {
		t.Fatal(e)
	}
	f.mu.Lock()
	f.snapshot.Revision = 2
	f.snapshot.Exported = time.Now().UTC()
	f.snapshot.Records[0].Data = marshal(Title{Identity: Identity{"movie", 7}, Title: "Replacement"})
	f.mu.Unlock()
	if e := f.s.SyncSnapshot(context.Background()); e == nil {
		t.Fatal("commit failure ignored")
	}
	b := f.want(200, "GET", "/api/v1/titles/movie/7", token, nil, "")
	if bytes.Contains(b, []byte("Replacement")) {
		t.Fatal("uncommitted snapshot published")
	}
	f.restart()
	f.want(302, "GET", "/api/v1/artwork/w500/example.png", token, nil, "")
}
func TestOwnedShutdownCancelsAndJoinsRequests(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	entered := make(chan struct{})
	exited := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		close(entered)
		select {
		case <-r.Context().Done():
		case <-time.After(3 * time.Second):
		}
		close(exited)
	}))
	defer server.Close()
	f.s.cfg.ProviderURL = server.URL
	done := make(chan struct{})
	go func() {
		defer close(done)
		_, _ = f.request("POST", "/api/v1/profiles/"+p.ID+"/playback", token, map[string]any{"type": "movie", "id": 7}, "shutdown")
	}()
	<-entered
	closed := make(chan struct{})
	go func() { _ = f.s.Close(); close(closed) }()
	select {
	case <-closed:
	case <-time.After(time.Second):
		t.Fatal("service did not join owned handler")
	}
	select {
	case <-exited:
	case <-time.After(time.Second):
		t.Fatal("private request not canceled")
	}
	<-done
}
func TestRecommendationViewsAndFingerprint(t *testing.T) {
	r := Recommendations{Sections: []Shelf{{ID: "for-you"}}}
	for i := 1; i <= 30; i++ {
		r.Pool = append(r.Pool, Title{Identity: Identity{"movie", int64(i)}, Title: fmt.Sprintf("Movie %c", 'A'+i)})
	}
	r.Pool = append(r.Pool, Title{Identity: Identity{"tv", 42}, Title: "Series"})
	v := recommendationView(r, "tv", "both")
	if len(v.Sections[0].Titles) != 1 || v.Sections[0].Titles[0].Type != "tv" {
		t.Fatal("category view lost pool titles")
	}
	v = recommendationView(r, "all", "tv")
	if len(v.Sections[0].Titles) != 24 || v.Sections[0].Titles[0].Type != "tv" {
		t.Fatal("format preference ignored")
	}
	p := inputs{Format: "both", Signals: []signal{{Identity: Identity{"tv", 1}, Watched: 2699, Updated: time.Now()}}}
	before := p.fingerprint()
	p.Signals[0].Watched = 2700
	if before == p.fingerprint() {
		t.Fatal("qualification crossing did not invalidate fingerprint")
	}
}
func TestProgressCannotUseUnestablishedPlayback(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	pbid := id()
	_, e := f.s.db.Exec("INSERT INTO playbacks(id,profile,request,expires,created,idem,fingerprint) VALUES(?,?,?,?,?,?,?)", pbid, p.ID, marshal(PlaybackRequest{Target: Target{Identity: Identity{"movie", 7}}}), time.Now().Add(time.Hour).Unix(), time.Now().Unix(), "pending", "fingerprint")
	if e != nil {
		t.Fatal(e)
	}
	f.want(409, "PUT", "/api/v1/profiles/"+p.ID+"/progress", token, map[string]any{"playback_id": pbid, "sequence": 1, "position_seconds": 10, "duration_seconds": 7200, "watched_seconds": 10}, "")
}
func TestConfigValidation(t *testing.T) {
	f := newFixture(t)
	path := filepath.Join(t.TempDir(), "config.json")
	save := func(c Config) {
		t.Helper()
		if e := os.WriteFile(path, marshal(c), 0600); e != nil {
			t.Fatal(e)
		}
	}
	save(f.c)
	if _, e := LoadConfig(path); e != nil {
		t.Fatal(e)
	}
	for _, alter := range []func(*Config){func(c *Config) { c.ProviderURL = c.CatalogURL }, func(c *Config) { c.RelayURL = "http://outside.example:8094" }, func(c *Config) { c.Listen = "0.0.0.0:8090" }, func(c *Config) { c.Origin = "http://unsafe.example" }} {
		c := f.c
		alter(&c)
		save(c)
		if _, e := LoadConfig(path); e == nil {
			t.Fatal("invalid config accepted")
		}
	}
}
func TestSavedTitlesJoinSnapshotAndRewriteArtwork(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	path := "/api/v1/profiles/" + p.ID
	for _, kind := range []string{"library", "favorites"} {
		f.want(204, "PUT", path+"/"+kind+"/movie/7", token, nil, "")
	}
	f.seedSnapshot(p.ID)
	for _, kind := range []string{"library", "favorites"} {
		b := f.want(200, "GET", path+"/"+kind, token, nil, "")
		if bytes.Contains(b, []byte("/internal/v1/")) || !bytes.Contains(b, []byte("https://image.tmdb.org/t/p/")) || !bytes.Contains(b, []byte("Example")) {
			t.Fatal("saved metadata was not joined/rewritten")
		}
	}
	in, _, e := f.s.derived(context.Background(), p.ID)
	if e != nil {
		t.Fatal(e)
	}
	if in.Favorites[0].Title != "Example" {
		t.Fatal("derived favorite name stale")
	}
}
func TestPrivatePayloadPreservesLargeJSONNumbers(t *testing.T) {
	b := []byte(`{"selection":{"unknown":{"counter":9007199254740993},"duration":25000000000}}`)
	var v any
	if e := decode(b, &v); e != nil {
		t.Fatal(e)
	}
	if !bytes.Contains(marshal(v), []byte("9007199254740993")) {
		t.Fatal("private metadata number rounded")
	}
}
func TestTVMeasuredEpisodeAggregationAndProfileIsolation(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	other := f.profile(token)
	path := "/api/v1/profiles/" + p.ID
	for _, ep := range []int{1, 2} {
		pb := f.play(token, p.ID, map[string]any{"type": "tv", "id": 7, "season": 0, "episode": ep}, fmt.Sprintf("ep%d", ep))
		_, e := f.s.db.Exec("UPDATE playbacks SET created=? WHERE id=?", time.Now().Add(-time.Hour).Unix(), pb)
		if e != nil {
			t.Fatal(e)
		}
		update := map[string]any{"playback_id": pb, "sequence": 1, "position_seconds": 2000, "duration_seconds": 2400, "watched_seconds": 600}
		f.want(200, "PUT", path+"/progress", token, update, "")
		f.want(404, "PUT", "/api/v1/profiles/"+other.ID+"/progress", token, update, "")
	}
	in, _, e := f.s.derived(context.Background(), p.ID)
	if e != nil {
		t.Fatal(e)
	}
	if len(in.Signals) != 1 || in.Signals[0].Watched != 1200 || in.Signals[0].Episodes != 2 {
		t.Fatal("distinct episode aggregation incorrect")
	}
	unrelated, _, e := f.s.derived(context.Background(), other.ID)
	if e != nil || len(unrelated.Signals) != 0 {
		t.Fatal("profile signals leaked")
	}
}
func TestNoSourceDoesNotCreateRelaySession(t *testing.T) {
	f := newFixture(t)
	token := f.setup()
	p := f.profile(token)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		write(w, 200, map[string]any{"version": "providers.v1", "backups": []any{}, "rejected": map[string]string{}, "providers": []any{}})
	}))
	defer server.Close()
	f.s.cfg.ProviderURL = server.URL
	f.want(422, "POST", "/api/v1/profiles/"+p.ID+"/playback", token, map[string]any{"type": "movie", "id": 7}, "no-source")
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.relayCreates != 0 {
		t.Fatal("empty selection imported")
	}
}
func TestExclusiveDatabaseOwner(t *testing.T) {
	f := newFixture(t)
	if s, e := New(f.c); e == nil {
		s.Close()
		t.Fatal("two API processes could own the same database")
	}
	f.restart()
	f.want(200, "GET", "/health/readyz", "", nil, "")
}
func TestBackgroundWorkerShutdown(t *testing.T) {
	f := newFixture(t)
	entered := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/internal/v1/snapshot" {
			close(entered)
			select {
			case <-r.Context().Done():
			case <-time.After(3 * time.Second):
			}
		} else {
			w.WriteHeader(503)
		}
	}))
	defer server.Close()
	f.s.cfg.CatalogURL = server.URL
	f.s.Start()
	<-entered
	done := make(chan struct{})
	go func() { f.s.Close(); close(done) }()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("background workers did not join")
	}
}

// Legacy snapshot fixture used to prove artwork payloads are discarded.
type artwork struct {
	MIME  string `json:"mime"`
	Bytes []byte `json:"bytes"`
}
