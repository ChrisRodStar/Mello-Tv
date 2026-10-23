package api

import (
	"context"
	"crypto/ed25519"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"time"
)

func (s *Service) playback(w http.ResponseWriter, r *http.Request, p Profile, parts []string) error {
	if len(parts) == 0 && r.Method == "POST" {
		return s.createPlayback(w, r, p)
	}
	if len(parts) < 1 || len(parts) > 2 || !safeID(parts[0]) {
		return missing()
	}
	release, e := s.playLocks.acquire(r.Context(), "session/"+parts[0], time.Second)
	if e != nil {
		return e
	}
	defer release()
	pb, e := s.loadPlayback(r.Context(), parts[0], p.ID)
	if e != nil {
		return e
	}
	if len(parts) == 1 {
		if r.Method == "DELETE" {
			if _, e = s.db.ExecContext(r.Context(), "UPDATE playbacks SET revoked=1 WHERE id=?", pb.ID); e != nil {
				return e
			}
			s.cancelBackups(pb.ID)
			if e = s.revokeOne(r.Context(), pb); e != nil {
				return e
			}
			w.WriteHeader(204)
			return nil
		}
		if r.Method == "GET" {
			if pb.Revoked {
				return &fault{410, "revoked"}
			}
			if pb.Ready.SID == "" {
				return &fault{409, "setup_pending"}
			}
			ready, e := s.relayState(r.Context(), pb.Ready.SID)
			if e != nil {
				return e
			}
			if e = s.saveReady(r.Context(), pb.ID, ready); e != nil {
				return e
			}
			pb.Ready = ready
			return s.playbackReply(w, r, pb, false)
		}
		return missing()
	}
	if r.Method != "POST" {
		return missing()
	}
	kind := parts[1]
	if kind != "renew" && kind != "backup" && kind != "refresh" {
		return missing()
	}
	return s.mutatePlayback(w, r, pb, kind)
}
func (s *Service) loadPlayback(ctx context.Context, pbid, pid string) (Playback, error) {
	var p Playback
	p.ID = pbid
	var req, ready []byte
	e := s.db.QueryRowContext(ctx, "SELECT request,ready,revoked FROM playbacks WHERE id=? AND profile=?", pbid, pid).Scan(&req, &ready, &p.Revoked)
	if errors.Is(e, sql.ErrNoRows) {
		return p, missing()
	}
	if e != nil {
		return p, e
	}
	if e = decode(req, &p.Request); e != nil {
		return p, e
	}
	if len(ready) > 0 {
		e = decode(ready, &p.Ready)
	}
	return p, e
}
func (s *Service) createPlayback(w http.ResponseWriter, r *http.Request, p Profile) error {
	key := r.Header.Get("Idempotency-Key")
	if !safeID(key) {
		return bad()
	}
	release, e := s.playLocks.acquire(r.Context(), "create/"+p.ID+"/"+key, time.Second)
	if e != nil {
		return e
	}
	defer release()
	var in PlaybackRequest
	if e := body(r, &in); e != nil {
		return e
	}
	if !in.Valid() {
		return bad()
	}
	fingerprint := hash(string(marshal(in)))
	var pbid, previous string
	e = s.db.QueryRowContext(r.Context(), "SELECT id,fingerprint FROM playbacks WHERE profile=? AND idem=?", p.ID, key).Scan(&pbid, &previous)
	if e == nil {
		if previous != fingerprint {
			return &fault{409, "idempotency_conflict"}
		}
	} else if errors.Is(e, sql.ErrNoRows) {
		// Enrich only from durable local metadata; known-ID playback makes no
		// synchronous catalog request, even when the catalog is healthy.
		title := s.cachedTitle(in.Identity)
		if in.IMDb == "" {
			in.IMDb = title.IMDb
		}
		if in.MAL == nil && title.MAL > 0 {
			n := title.MAL
			in.MAL = &n
		}
		pbid = id()
		now := time.Now()
		e = transaction(r.Context(), s.db, func(tx *sql.Tx) error {
			var n int
			if e := tx.QueryRow("SELECT count(*) FROM playbacks").Scan(&n); e != nil {
				return e
			}
			if n >= 4096 {
				return &fault{429, "playback_limit"}
			}
			var exists int
			if e := tx.QueryRow("SELECT count(*) FROM profiles WHERE id=?", p.ID).Scan(&exists); e != nil {
				return e
			}
			if exists != 1 {
				return missing()
			}
			_, e := tx.Exec("INSERT INTO playbacks(id,profile,request,expires,created,idem,fingerprint) VALUES(?,?,?,?,?,?,?)", pbid, p.ID, marshal(in), now.Add(6*time.Hour).Unix(), now.Unix(), key, fingerprint)
			return e
		})
		if e != nil {
			return e
		}
	} else {
		return e
	}
	releaseSession, e := s.playLocks.acquire(r.Context(), "session/"+pbid, time.Second)
	if e != nil {
		return e
	}
	defer releaseSession()
	pb, e := s.loadPlayback(r.Context(), pbid, p.ID)
	if e != nil {
		return e
	}
	if pb.Revoked {
		return &fault{410, "revoked"}
	}
	if pb.Ready.SID != "" {
		return s.playbackReply(w, r, pb, false)
	}
	var stage []byte
	var expires int64
	if e = s.db.QueryRowContext(r.Context(), "SELECT stage,expires FROM playbacks WHERE id=?", pbid).Scan(&stage, &expires); e != nil {
		return e
	}
	if expires <= time.Now().Unix() {
		return &fault{410, "session_expired"}
	}
	if len(stage) == 0 {
		selection, e := s.resolve(r.Context(), p.ID, pb.Request, false)
		if e != nil {
			return e
		}
		payload := map[string]any{"contract_version": "relay.v1", "owner_id": p.ID, "expires_at": time.Unix(expires, 0).UTC(), "selection": json.RawMessage(selection)}
		stage, e = seal(s.encryption, pbid, marshal(payload))
		if e != nil {
			return e
		}
		if _, e = s.db.ExecContext(r.Context(), "UPDATE playbacks SET stage=? WHERE id=?", stage, pbid); e != nil {
			return e
		}
	}
	raw, e := unseal(s.encryption, pbid, stage)
	if e != nil {
		return e
	}
	var payload any
	if e = decode(raw, &payload); e != nil {
		return e
	}
	b, e := s.private(r.Context(), "relay", "POST", "/internal/v1/sessions", payload, pbid, 32768)
	if e != nil {
		return e
	}
	var ready RelayReady
	if e = decode(b, &ready); e != nil {
		return e
	}
	if e = s.saveReady(r.Context(), pbid, ready); e != nil {
		return e
	}
	pb.Ready = ready
	var staged struct {
		Selection json.RawMessage `json:"selection"`
	}
	if json.Unmarshal(raw, &staged) == nil {
		s.scheduleBackups(pb, p.ID, staged.Selection)
	}
	return s.playbackReply(w, r, pb, false)
}
func (s *Service) resolve(ctx context.Context, pid string, in PlaybackRequest, refresh bool) ([]byte, error) {
	return s.resolveMode(ctx, pid, in, refresh, false)
}

func (s *Service) resolveMode(ctx context.Context, pid string, in PlaybackRequest, refresh, complete bool) ([]byte, error) {
	title := map[string]any{"type": in.Type, "tmdb_id": in.ID}
	if in.IMDb != "" {
		title["imdb_id"] = in.IMDb
	}
	if in.MAL != nil {
		title["my_anime_list_id"] = *in.MAL
	}
	if in.Season != nil {
		title["season"] = *in.Season
		title["episode"] = *in.Episode
	}
	device := any(map[string]any{"formats": []string{"hls", "mp4"}, "codecs": []string{"avc1", "mp4a"}, "supports_encrypted_hls": true})
	if in.Device != nil {
		device = in.Device
	}
	prefs := any(map[string]any{"require_audio": false, "require_subtitles": false})
	if in.Preferences != nil {
		prefs = in.Preferences
	}
	lookup := map[string]any{"title": title, "refresh": refresh, "preferences": prefs, "device": device}
	if !refresh {
		lookup["startup_height"] = 1080
	}
	if complete {
		lookup["wait_for_complete"] = true
	}
	req := map[string]any{"version": "providers.v1", "context_key": pid, "lookup": lookup}
	b, e := s.private(ctx, "provider", "POST", "/internal/v1/resolve", req, "", 2<<20)
	if e != nil {
		return nil, e
	}
	// Inspect only routing identity; preserve the full unmodified private envelope
	// and its ordered backups for the relay's authoritative import validation.
	type ranked struct {
		Candidate struct {
			Source struct {
				Title struct {
					Type    string `json:"type"`
					ID      int64  `json:"tmdb_id"`
					IMDb    string `json:"imdb_id"`
					MAL     *int64 `json:"my_anime_list_id"`
					Season  *int   `json:"season"`
					Episode *int   `json:"episode"`
				} `json:"title"`
			} `json:"source"`
		} `json:"candidate"`
	}
	var selection struct {
		Version  string   `json:"version"`
		Selected *ranked  `json:"selected"`
		Backups  []ranked `json:"backups"`
	}
	if e = json.Unmarshal(b, &selection); e != nil || selection.Version != "providers.v1" {
		return nil, &fault{502, "invalid_selection"}
	}
	if selection.Selected == nil {
		return nil, &fault{422, "no_source"}
	}
	if len(selection.Backups) > 127 {
		return nil, &fault{502, "selection_limit"}
	}
	all := append([]ranked{*selection.Selected}, selection.Backups...)
	for _, v := range all {
		t := v.Candidate.Source.Title
		target := Target{Identity: Identity{t.Type, t.ID}, Season: t.Season, Episode: t.Episode}
		if !target.Valid() || target.Key() != in.Target.Key() || (in.IMDb != "" && t.IMDb != in.IMDb) || (in.MAL != nil && (t.MAL == nil || *t.MAL != *in.MAL)) {
			return nil, &fault{502, "selection_identity_mismatch"}
		}
	}
	return b, nil
}
func validateReady(r RelayReady) error {
	if len(r.AvailableHeights) > 32 || (len(r.AvailableHeights) > 0 && r.Format != "hls") {
		return &fault{502, "relay_invalid_response"}
	}
	seen := make(map[int]bool)
	for _, height := range r.AvailableHeights {
		if height <= 0 || height > 16384 || seen[height] {
			return &fault{502, "relay_invalid_response"}
		}
		seen[height] = true
	}
	if r.Version != "relay.v1" || !safeID(r.SID) || r.Entry != "/stream/"+r.SID+"/entry" || (r.Format != "hls" && r.Format != "mp4") || r.Revision == 0 || r.AuthRevision == 0 || r.Generation == 0 || r.Expires.IsZero() {
		return &fault{502, "relay_invalid_response"}
	}
	switch r.State {
	case "active", "draining", "expired", "revoked":
	default:
		return &fault{502, "relay_invalid_response"}
	}
	return nil
}
func (s *Service) saveReady(ctx context.Context, pbid string, r RelayReady) error {
	if e := validateReady(r); e != nil {
		return e
	}
	_, e := s.db.ExecContext(ctx, "UPDATE playbacks SET ready=?,expires=?,stage=NULL WHERE id=?", marshal(r), r.Expires.Unix(), pbid)
	return e
}
func (s *Service) relayState(ctx context.Context, sid string) (RelayReady, error) {
	var ready RelayReady
	b, e := s.private(ctx, "relay", "GET", "/internal/v1/sessions/"+sid, nil, "", 32768)
	if e != nil {
		return ready, e
	}
	if e = decode(b, &ready); e != nil {
		return ready, e
	}
	if ready.SID != sid {
		return ready, &fault{502, "relay_identity_mismatch"}
	}
	return ready, validateReady(ready)
}
func (s *Service) playbackReply(w http.ResponseWriter, r *http.Request, p Playback, reload bool) error {
	var revoked bool
	var pid string
	var exists int
	e := s.db.QueryRowContext(r.Context(), "SELECT b.revoked,b.profile,(SELECT count(*) FROM profiles WHERE id=b.profile) FROM playbacks b WHERE b.id=?", p.ID).Scan(&revoked, &pid, &exists)
	if e != nil {
		return e
	}
	if revoked || exists != 1 {
		return &fault{410, "revoked"}
	}
	switch p.Ready.State {
	case "revoked":
		return &fault{410, "revoked"}
	case "expired":
		return &fault{410, "session_expired"}
	case "draining":
		return &fault{503, "draining"}
	}
	if e = validateReady(p.Ready); e != nil {
		return e
	}
	if !p.Ready.Expires.After(time.Now()) {
		return &fault{410, "session_expired"}
	}
	token, e := s.grant(p.Ready, pid)
	if e != nil {
		return e
	}
	position := 0.0
	var data []byte
	if e = s.db.QueryRowContext(r.Context(), "SELECT data FROM progress WHERE profile=? AND target=?", pid, p.Request.Target.Key()).Scan(&data); e == nil {
		var v Progress
		if decode(data, &v) == nil {
			position = v.Position
		}
	} else if !errors.Is(e, sql.ErrNoRows) {
		return e
	}
	write(w, 200, map[string]any{"id": p.ID, "title": p.Request.Target, "url": p.Ready.Entry + "?st=" + token, "format": p.Ready.Format, "available_heights": p.Ready.AvailableHeights, "expires_at": p.Ready.Expires, "revision": p.Ready.Revision, "source_generation": p.Ready.Generation, "reload_required": reload || p.Ready.Resume, "resume_seconds": position})
	return nil
}
func (s *Service) grant(r RelayReady, owner string) (string, error) {
	now := time.Now().Unix()
	exp := r.Expires.Unix()
	if exp > now+21600 {
		exp = now + 21600
	}
	enc := base64.RawURLEncoding.EncodeToString
	header := enc(marshal(map[string]string{"alg": "EdDSA", "typ": "JWT", "kid": s.cfg.KID}))
	claims := enc(marshal(map[string]any{"iss": "family-stream-api", "aud": "family-stream-relay", "sub": owner, "sid": r.SID, "arv": r.AuthRevision, "iat": now, "nbf": now, "exp": exp, "jti": id()}))
	message := header + "." + claims
	return message + "." + enc(ed25519.Sign(s.signer, []byte(message))), nil
}
func (s *Service) mutatePlayback(w http.ResponseWriter, r *http.Request, p Playback, kind string) error {
	if p.Revoked || p.Ready.SID == "" {
		return &fault{410, "revoked"}
	}
	key := r.Header.Get("Idempotency-Key")
	if !safeID(key) {
		return bad()
	}
	var in struct {
		Revision uint64 `json:"expected_revision"`
	}
	if e := body(r, &in); e != nil {
		return e
	}
	if in.Revision == 0 {
		return bad()
	}
	var priorKind string
	var priorRevision uint64
	var priorResult []byte
	e := s.db.QueryRowContext(r.Context(), "SELECT kind,revision,result FROM mutation_history WHERE playback=? AND idem=?", p.ID, key).Scan(&priorKind, &priorRevision, &priorResult)
	if e == nil {
		if priorKind != kind || priorRevision != in.Revision {
			return &fault{409, "idempotency_conflict"}
		}
		var ready RelayReady
		if e = decode(priorResult, &ready); e != nil {
			return e
		}
		if ready.Revision != p.Ready.Revision {
			return &fault{409, "operation_superseded"}
		}
		p.Ready = ready
		return s.playbackReply(w, r, p, kind != "renew")
	}
	if !errors.Is(e, sql.ErrNoRows) {
		return e
	}
	var pendingKey, pendingKind string
	var expected uint64
	var payload, base []byte
	e = s.db.QueryRowContext(r.Context(), "SELECT idem,kind,revision,payload,base FROM mutations WHERE playback=?", p.ID).Scan(&pendingKey, &pendingKind, &expected, &payload, &base)
	if e == nil {
		if decode(base, &p.Ready) != nil {
			return &fault{503, "invalid_pending_state"}
		}
		if pendingKey != key || pendingKind != kind || expected != in.Revision {
			return &fault{409, "operation_pending"}
		}
	} else if errors.Is(e, sql.ErrNoRows) {
		var mutations int
		if e := s.db.QueryRowContext(r.Context(), "SELECT count(*) FROM mutation_history WHERE playback=?", p.ID).Scan(&mutations); e != nil {
			return e
		}
		if mutations >= 256 {
			return &fault{409, "mutation_limit"}
		}
		if in.Revision != p.Ready.Revision {
			return conflict()
		}
		payload, e = seal(s.encryption, p.ID+"/mutation", marshal(map[string]any{"owner_id": s.profileForPlayback(r.Context(), p.ID), "expected_revision": in.Revision}))
		if e != nil {
			return e
		}
		if _, e = s.db.ExecContext(r.Context(), "INSERT INTO mutations(playback,idem,kind,revision,payload,base) VALUES(?,?,?,?,?,?)", p.ID, key, kind, in.Revision, payload, marshal(p.Ready)); e != nil {
			return e
		}
	} else {
		return e
	}
	// Reconcile a lost reply before retrying: this API is the exclusive relay
	// controller for its sessions, and only one durable mutation is admitted.
	current, e := s.relayState(r.Context(), p.Ready.SID)
	if e != nil {
		return e
	}
	if current.Revision > in.Revision {
		return s.completeMutation(w, r, p, key, kind, in.Revision, current)
	}
	if current.Revision != in.Revision {
		return conflict()
	}
	raw, e := unseal(s.encryption, p.ID+"/mutation", payload)
	if e != nil {
		return e
	}
	var mutation map[string]any
	if e = decode(raw, &mutation); e != nil {
		return e
	}
	// Save prepared selection/expiry before issuing control. Retry uses these
	// exact bytes; no source lookup or expiry change follows an ambiguous reply.
	if kind == "refresh" && mutation["selection"] == nil {
		selection, e := s.resolve(r.Context(), s.profileForPlayback(r.Context(), p.ID), p.Request, true)
		if e != nil {
			return e
		}
		mutation["selection"] = json.RawMessage(selection)
	}
	if kind == "renew" && mutation["expires_at"] == nil {
		expiry := time.Now().Add(6 * time.Hour).UTC().Truncate(time.Second)
		if !expiry.After(p.Ready.Expires) {
			_, _ = s.db.ExecContext(r.Context(), "DELETE FROM mutations WHERE playback=?", p.ID)
			return &fault{409, "renew_too_early"}
		}
		mutation["expires_at"] = expiry
	}
	payload, e = seal(s.encryption, p.ID+"/mutation", marshal(mutation))
	if e != nil {
		return e
	}
	if _, e = s.db.ExecContext(r.Context(), "UPDATE mutations SET payload=? WHERE playback=?", payload, p.ID); e != nil {
		return e
	}
	operation := kind
	if operation == "backup" {
		operation = "failover"
	}
	if operation == "refresh" {
		operation = "source"
	}
	b, e := s.private(r.Context(), "relay", "POST", "/internal/v1/sessions/"+p.Ready.SID+"/"+operation, mutation, "", 32768)
	if e != nil {
		var f *fault
		if errors.As(e, &f) && (f.code == "source_refresh_required" || f.code == "invalid_input" || f.code == "invalid_selection" || f.code == "unsupported_format") {
			_, _ = s.db.ExecContext(r.Context(), "DELETE FROM mutations WHERE playback=?", p.ID)
		}
		return e
	}
	var ready RelayReady
	if e = decode(b, &ready); e != nil {
		return e
	}
	return s.completeMutation(w, r, p, key, kind, in.Revision, ready)
}
func (s *Service) profileForPlayback(ctx context.Context, pbid string) string {
	var pid string
	_ = s.db.QueryRowContext(ctx, "SELECT profile FROM playbacks WHERE id=?", pbid).Scan(&pid)
	return pid
}
func (s *Service) completeMutation(w http.ResponseWriter, r *http.Request, p Playback, key, kind string, revision uint64, ready RelayReady) error {
	if e := validateReady(ready); e != nil {
		return e
	}
	if ready.SID != p.Ready.SID || ready.Revision != revision+1 || ready.AuthRevision != p.Ready.AuthRevision {
		return &fault{502, "relay_invalid_transition"}
	}
	if kind == "renew" && (ready.Generation != p.Ready.Generation || !ready.Expires.After(p.Ready.Expires)) {
		return &fault{502, "relay_invalid_transition"}
	}
	if kind != "renew" && ready.Generation <= p.Ready.Generation {
		return &fault{502, "relay_invalid_transition"}
	}
	e := transaction(r.Context(), s.db, func(tx *sql.Tx) error {

		if _, e := tx.Exec("INSERT INTO mutation_history VALUES(?,?,?,?,?)", p.ID, key, kind, revision, marshal(ready)); e != nil {
			return e
		}
		if _, e := tx.Exec("UPDATE playbacks SET ready=?,expires=? WHERE id=?", marshal(ready), ready.Expires.Unix(), p.ID); e != nil {
			return e
		}
		_, e := tx.Exec("DELETE FROM mutations WHERE playback=?", p.ID)
		return e
	})
	if e != nil {
		return e
	}
	p.Ready = ready
	return s.playbackReply(w, r, p, kind != "renew")
}
func (s *Service) revokeOne(ctx context.Context, p Playback) error {
	if p.Ready.SID == "" {
		var stage []byte
		if e := s.db.QueryRowContext(ctx, "SELECT stage FROM playbacks WHERE id=?", p.ID).Scan(&stage); e != nil {
			return e
		}
		if len(stage) == 0 {
			_, e := s.db.ExecContext(ctx, "UPDATE playbacks SET revoked_done=1 WHERE id=?", p.ID)
			return e
		}
		raw, e := unseal(s.encryption, p.ID, stage)
		if e != nil {
			return e
		}
		var payload any
		if e = decode(raw, &payload); e != nil {
			return e
		}
		b, e := s.private(ctx, "relay", "POST", "/internal/v1/sessions", payload, p.ID, 32768)
		if e != nil {
			return e
		}
		if e = decode(b, &p.Ready); e != nil {
			return e
		}
		if e = s.saveReady(ctx, p.ID, p.Ready); e != nil {
			return e
		}
	}
	_, e := s.private(ctx, "relay", "DELETE", "/internal/v1/sessions/"+p.Ready.SID, nil, "", 8192)
	if e != nil {
		return e
	}
	_, e = s.db.ExecContext(ctx, "UPDATE playbacks SET stage=NULL,revoked_done=1 WHERE id=?", p.ID)
	return e
}
func (s *Service) retryRevocations(ctx context.Context) error {
	rows, e := s.db.QueryContext(ctx, "SELECT id,profile FROM playbacks WHERE revoked=1 AND revoked_done=0 AND expires>? LIMIT 64", time.Now().Unix())
	if e != nil {
		return e
	}
	type ref struct{ id, pid string }
	refs := []ref{}
	for rows.Next() {
		var v ref
		if e = rows.Scan(&v.id, &v.pid); e != nil {
			rows.Close()
			return e
		}
		refs = append(refs, v)
	}
	e = rows.Err()
	rows.Close()
	if e != nil {
		return e
	}
	for _, v := range refs {
		release, lockErr := s.playLocks.acquire(ctx, "session/"+v.id, 0)
		if lockErr != nil {
			continue
		}
		p, loadErr := s.loadPlayback(ctx, v.id, v.pid)
		if loadErr == nil {
			_ = s.revokeOne(ctx, p)
		}
		release()
	}
	return ctx.Err()
}
