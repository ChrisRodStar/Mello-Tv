CREATE TABLE metadata (id INTEGER PRIMARY KEY CHECK(id=1), blob BLOB NOT NULL);
CREATE TABLE sessions (sid TEXT PRIMARY KEY, expires INTEGER NOT NULL, blob BLOB NOT NULL);
CREATE INDEX session_expiry ON sessions(expires);
CREATE TABLE generations (sid TEXT NOT NULL REFERENCES sessions(sid) ON DELETE CASCADE, generation INTEGER NOT NULL, blob BLOB NOT NULL, PRIMARY KEY(sid,generation));
CREATE TABLE resources (sid TEXT NOT NULL, generation INTEGER NOT NULL, rid TEXT NOT NULL, blob BLOB NOT NULL, PRIMARY KEY(sid,rid), FOREIGN KEY(sid,generation) REFERENCES generations(sid,generation) ON DELETE CASCADE);
CREATE INDEX resource_generation ON resources(sid,generation);
CREATE TABLE idempotency (id TEXT PRIMARY KEY, expires INTEGER NOT NULL, blob BLOB NOT NULL);
CREATE INDEX idempotency_expiry ON idempotency(expires);
PRAGMA user_version=1;
