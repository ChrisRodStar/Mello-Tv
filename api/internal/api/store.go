package api

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	_ "modernc.org/sqlite"
	"os"
	"path/filepath"
)

const schema = `
CREATE TABLE settings(key TEXT PRIMARY KEY,value BLOB NOT NULL);
CREATE TABLE accounts(id TEXT PRIMARY KEY,username TEXT UNIQUE NOT NULL,password BLOB NOT NULL,salt BLOB NOT NULL,admin INTEGER NOT NULL);
CREATE TABLE sessions(hash TEXT PRIMARY KEY,account TEXT NOT NULL REFERENCES accounts(id),expires INTEGER NOT NULL);
CREATE INDEX session_expiry ON sessions(expires);
CREATE TABLE profiles(id TEXT PRIMARY KEY,account TEXT NOT NULL REFERENCES accounts(id),data BLOB NOT NULL,version INTEGER NOT NULL DEFAULT 1,synced INTEGER NOT NULL DEFAULT 0,last_sync INTEGER NOT NULL DEFAULT 0);
CREATE TABLE lists(profile TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,kind TEXT NOT NULL,target TEXT NOT NULL,data BLOB NOT NULL,PRIMARY KEY(profile,kind,target));
CREATE TABLE progress(profile TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,target TEXT NOT NULL,type TEXT NOT NULL,title_id INTEGER NOT NULL,data BLOB NOT NULL,PRIMARY KEY(profile,target));
CREATE INDEX progress_title ON progress(profile,type,title_id);
CREATE TABLE activity(profile TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,day TEXT NOT NULL,PRIMARY KEY(profile,day));
CREATE TABLE playbacks(id TEXT PRIMARY KEY,profile TEXT NOT NULL,request BLOB NOT NULL,ready BLOB,revoked INTEGER NOT NULL DEFAULT 0,revoked_done INTEGER NOT NULL DEFAULT 0,expires INTEGER NOT NULL,total REAL NOT NULL DEFAULT 0,sequence INTEGER NOT NULL DEFAULT 0,created INTEGER NOT NULL,idem TEXT NOT NULL,fingerprint TEXT NOT NULL,stage BLOB,UNIQUE(profile,idem));
CREATE INDEX playback_expiry ON playbacks(expires);
CREATE TABLE mutations(playback TEXT PRIMARY KEY REFERENCES playbacks(id) ON DELETE CASCADE,idem TEXT NOT NULL,kind TEXT NOT NULL,revision INTEGER NOT NULL,payload BLOB,base BLOB NOT NULL,done INTEGER NOT NULL DEFAULT 0,result BLOB);
CREATE TABLE mutation_history(playback TEXT NOT NULL REFERENCES playbacks(id) ON DELETE CASCADE,idem TEXT NOT NULL,kind TEXT NOT NULL,revision INTEGER NOT NULL,result BLOB NOT NULL,PRIMARY KEY(playback,idem));
CREATE TABLE deletions(handle TEXT PRIMARY KEY);
CREATE TABLE refresh_jobs(key TEXT PRIMARY KEY,data BLOB NOT NULL);
CREATE TABLE snapshot(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL,data BLOB NOT NULL);
PRAGMA user_version=1;`

func openStore(path string) (*sql.DB, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return nil, err
	}
	f, e := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0600)
	if e != nil {
		return nil, e
	}
	f.Close()
	if e = os.Chmod(path, 0600); e != nil {
		return nil, e
	}
	db, e := sql.Open("sqlite", path)
	if e != nil {
		return nil, e
	}
	db.SetMaxOpenConns(1)
	fail := func(e error) (*sql.DB, error) { db.Close(); return nil, e }
	for _, q := range []string{"PRAGMA foreign_keys=ON", "PRAGMA journal_mode=WAL", "PRAGMA synchronous=FULL", "PRAGMA busy_timeout=3000"} {
		if _, e = db.Exec(q); e != nil {
			return fail(e)
		}
	}
	var version int
	if e = db.QueryRow("PRAGMA user_version").Scan(&version); e != nil {
		return fail(e)
	}
	if version > 1 {
		return fail(errors.New("unsupported schema"))
	}
	if version == 0 {
		tx, e := db.Begin()
		if e != nil {
			return fail(e)
		}
		if _, e = tx.Exec(schema); e != nil {
			tx.Rollback()
			return fail(e)
		}
		if e = tx.Commit(); e != nil {
			return fail(e)
		}
	}
	return db, nil
}
func id() string {
	b := make([]byte, 24)
	if _, e := rand.Read(b); e != nil {
		panic("random unavailable")
	}
	return base64.RawURLEncoding.EncodeToString(b)
}
func hash(s string) string { h := sha256.Sum256([]byte(s)); return hex.EncodeToString(h[:]) }
func marshal(v any) []byte {
	b, e := json.Marshal(v)
	if e != nil {
		panic("invalid internal data")
	}
	return b
}
func seal(key []byte, aad string, b []byte) ([]byte, error) {
	block, e := aes.NewCipher(key)
	if e != nil {
		return nil, e
	}
	g, e := cipher.NewGCM(block)
	if e != nil {
		return nil, e
	}
	nonce := make([]byte, g.NonceSize())
	if _, e = rand.Read(nonce); e != nil {
		return nil, e
	}
	return g.Seal(nonce, nonce, b, []byte(aad)), nil
}
func unseal(key []byte, aad string, b []byte) ([]byte, error) {
	block, e := aes.NewCipher(key)
	if e != nil {
		return nil, e
	}
	g, e := cipher.NewGCM(block)
	if e != nil {
		return nil, e
	}
	if len(b) < g.NonceSize() {
		return nil, errors.New("invalid state")
	}
	return g.Open(nil, b[:g.NonceSize()], b[g.NonceSize():], []byte(aad))
}
func transaction(ctx context.Context, db *sql.DB, fn func(*sql.Tx) error) error {
	tx, e := db.BeginTx(ctx, nil)
	if e != nil {
		return e
	}
	defer tx.Rollback()
	if e = fn(tx); e != nil {
		return e
	}
	return tx.Commit()
}
