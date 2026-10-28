//! Catalog persistence compatible with the Go catalog's SQLite schema version 2.
//!
//! All SQL runs on the database worker. Public snapshots require recommendation
//! enrichment in the application layer; [`CatalogStore::export_raw`] provides a
//! coherent storage snapshot without pretending to perform that work.

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, types::ValueRef};
use serde::Deserialize;
use serde_json::Value;
use stream_core::catalog::{CatalogRecord, CatalogSnapshot, CrossIds, SNAPSHOT_VERSION, Title};

use crate::{StorageError, db::Database};

const SCHEMA_VERSION: i64 = 2;
const SELECT_RECORD: &str = "SELECT kind,key,data,updated,expires,revision FROM records";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogLimits {
    pub max_bytes: u64,
    pub max_records: u32,
}

impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            max_bytes: 128 * 1024 * 1024,
            max_records: 20_000,
        }
    }
}

/// One atomic write. Negative TTLs are supported for immediately stale data,
/// matching the Go store rather than discarding useful last-known-good records.
#[derive(Debug, Clone)]
pub struct CatalogWrite {
    pub kind: String,
    pub key: String,
    pub data: Value,
    pub ttl: Duration,
}

#[derive(Clone)]
pub struct CatalogStore {
    database: Database,
    limits: CatalogLimits,
}

impl CatalogStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let database = Database::open(path, 64).await?;
        match Self::from_database(database.clone()).await {
            Ok(store) => Ok(store),
            Err(error) => {
                let _ = database.shutdown().await;
                Err(error)
            }
        }
    }

    pub async fn from_database(database: Database) -> Result<Self, StorageError> {
        database.call(migrate).await?;
        Ok(Self {
            database,
            limits: CatalogLimits::default(),
        })
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn with_limits(mut self, limits: CatalogLimits) -> Result<Self, StorageError> {
        if limits.max_bytes == 0 || limits.max_records == 0 {
            return Err(StorageError::InvalidInput(
                "catalog limits must be positive",
            ));
        }
        self.limits = limits;
        Ok(self)
    }

    pub async fn close(&self) -> Result<(), StorageError> {
        self.database.shutdown().await
    }

    pub async fn shutdown(&self) -> Result<(), StorageError> {
        self.close().await
    }

    pub async fn health(&self) -> Result<(), StorageError> {
        self.database
            .call(|connection| validate_schema(connection))
            .await
    }

    /// Fetches a raw record and labels expiration using caller-supplied time.
    pub async fn get(
        &self,
        kind: &str,
        key: &str,
        now: DateTime<Utc>,
    ) -> Result<CatalogRecord, StorageError> {
        let kind = kind.to_owned();
        let key = key.to_owned();
        self.database
            .call(move |connection| get_record(connection, &kind, &key, now))
            .await
    }

    pub async fn list(
        &self,
        kind: &str,
        limit: u32,
        now: DateTime<Utc>,
    ) -> Result<Vec<CatalogRecord>, StorageError> {
        if limit > 20_000 {
            return Err(StorageError::InvalidInput(
                "catalog list limit exceeds 20000",
            ));
        }
        let kind = kind.to_owned();
        self.database
            .call(move |connection| {
                let mut statement = connection.prepare_cached(&format!(
                    "{SELECT_RECORD} WHERE kind=? ORDER BY key LIMIT ?"
                ))?;
                let mut rows = statement.query(params![kind, limit])?;
                let mut records = Vec::new();
                while let Some(row) = rows.next()? {
                    records.push(read_record(row, now)?);
                }
                Ok(records)
            })
            .await
    }

    /// Literal substring search, preserving SQLite's existing ASCII lower-case
    /// semantics. `%` and `_` are ordinary characters, never LIKE wildcards.
    pub async fn search(
        &self,
        query: &str,
        offset: u32,
        now: DateTime<Utc>,
    ) -> Result<Vec<CatalogRecord>, StorageError> {
        if query.len() > 200 {
            return Err(StorageError::InvalidInput(
                "catalog search query exceeds 200 bytes",
            ));
        }
        let query = query.to_owned();
        self.database.call(move |connection| {
            let mut statement = connection.prepare_cached(
                "SELECT kind,key,data,updated,expires,revision FROM records INDEXED BY records_title_search WHERE kind='title' AND instr(lower(json_extract(data,'$.title')),lower(?))>0 ORDER BY key LIMIT 50 OFFSET ?"
            )?;
            let mut rows = statement.query(params![query, offset])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                records.push(read_record(row, now)?);
            }
            Ok(records)
        }).await
    }

    /// Read a title with external cross-ID overrides from the same transaction.
    pub async fn title(
        &self,
        key: &str,
        now: DateTime<Utc>,
    ) -> Result<CatalogRecord, StorageError> {
        let key = key.to_owned();
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                let mut record = get_record(&transaction, "title", &key, now)?;
                match get_record(&transaction, "cross_ids", &key, now) {
                    Ok(cross) => {
                        let title = Title::deserialize(&record.data).ok();
                        let ids = serde_json::from_value::<CrossIds>(cross.data).ok();
                        if let (Some(mut title), Some(ids)) =
                            (title, ids.filter(|ids| ids.validate().is_ok()))
                        {
                            if !ids.imdb_id.is_empty() {
                                title.imdb_id = ids.imdb_id;
                            }
                            if ids.mal_id > 0 {
                                title.mal_id = ids.mal_id;
                            }
                            record.data = serde_json::to_value(title)?;
                            record.revision = record.revision.max(cross.revision);
                        }
                    }
                    Err(StorageError::NotFound) => {}
                    Err(error) => return Err(error),
                }
                transaction.commit()?;
                Ok(record)
            })
            .await
    }

    pub async fn delete(&self, kind: &str, key: &str) -> Result<(), StorageError> {
        if kind.is_empty() || kind.len() > 64 || key.is_empty() || key.len() > 200 {
            return Err(StorageError::InvalidInput(
                "invalid catalog record kind/key",
            ));
        }
        let kind = kind.to_owned();
        let key = key.to_owned();
        self.database
            .call(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let removed = transaction.execute(
                    "DELETE FROM records WHERE kind=?1 AND key=?2",
                    params![kind, key],
                )?;
                if removed > 0 {
                    let next = revision(&transaction)?
                        .checked_add(1)
                        .ok_or(StorageError::Capacity)?;
                    transaction.execute("UPDATE revision SET value=?1", [next])?;
                }
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn put(&self, write: CatalogWrite, now: DateTime<Utc>) -> Result<i64, StorageError> {
        self.put_batch(vec![write], now).await
    }

    /// Commits the whole batch and revision together, or rolls both back. Even
    /// an empty batch advances the revision, matching the existing Go behavior.
    pub async fn put_batch(
        &self,
        writes: Vec<CatalogWrite>,
        now: DateTime<Utc>,
    ) -> Result<i64, StorageError> {
        let limits = self.limits;
        if writes.len() > 20_000 {
            return Err(StorageError::InvalidInput(
                "catalog batch exceeds 20000 writes",
            ));
        }
        let mut encoded = Vec::with_capacity(writes.len());
        let mut encoded_bytes = 0_u64;
        for write in writes {
            if write.kind.is_empty()
                || write.kind.len() > 64
                || write.key.is_empty()
                || write.key.len() > 200
            {
                return Err(StorageError::InvalidInput(
                    "invalid catalog record kind/key",
                ));
            }
            if write.kind == "artwork" {
                return Err(StorageError::InvalidInput("artwork storage is disabled"));
            }
            let expires = now
                .checked_add_signed(write.ttl)
                .ok_or(StorageError::InvalidInput(
                    "catalog TTL overflows timestamp",
                ))?;
            if write.kind == "title" {
                let title = Title::deserialize(&write.data)?;
                title
                    .validate()
                    .map_err(|_| StorageError::InvalidInput("invalid catalog title"))?;
                if title.identity.key() != write.key {
                    return Err(StorageError::InvalidInput(
                        "catalog title key does not match identity",
                    ));
                }
            } else if write.kind == "cross_ids" {
                let cross = CrossIds::deserialize(&write.data)?;
                cross
                    .validate()
                    .map_err(|_| StorageError::InvalidInput("invalid catalog cross_ids"))?;
            }
            let data = serde_json::to_vec(&write.data)?;
            encoded_bytes = encoded_bytes
                .checked_add(data.len() as u64)
                .ok_or(StorageError::Capacity)?;
            if encoded_bytes > limits.max_bytes {
                return Err(StorageError::Capacity);
            }
            encoded.push((write.kind, write.key, data, expires.timestamp_millis()));
        }
        self.database.call(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let prior_revision = revision(&transaction)?;
            let next_revision = prior_revision.checked_add(1)
                .ok_or(StorageError::Capacity)?;
            transaction.execute("UPDATE revision SET value=?", [next_revision])?;
            for (kind, key, mut data, expires) in encoded {
                if kind == "title" {
                    let prior = transaction.query_row(
                        "SELECT data FROM records WHERE kind='title' AND key=?", [&key],
                        |row| Ok(read_bytes(row, 0)?.to_vec())
                    ).optional()?;
                    if let Some(prior) = prior {
                        let previous: Title = serde_json::from_slice(&prior)?;
                        let mut next: Title = serde_json::from_slice(&data)?;
                        if previous.details && !next.details { continue; }
                        if next.imdb_id.is_empty() { next.imdb_id = previous.imdb_id; }
                        if next.mal_id == 0 { next.mal_id = previous.mal_id; }
                        data = serde_json::to_vec(&next)?;
                    }
                }
                transaction.execute(
                    "INSERT INTO records(kind,key,data,updated,expires,revision) VALUES(?,?,?,?,?,?) ON CONFLICT(kind,key) DO UPDATE SET data=excluded.data,updated=excluded.updated,expires=excluded.expires,revision=excluded.revision",
                    params![kind, key, data, now.timestamp_millis(), expires, next_revision]
                )?;
            }
            let (count, bytes): (i64, i64) = transaction.query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(data)),0) FROM records", [],
                |row| Ok((row.get(0)?, row.get(1)?))
            )?;
            if count < 0 || bytes < 0 || count as u64 > u64::from(limits.max_records) || bytes as u64 > limits.max_bytes {
                return Err(StorageError::Capacity);
            }
            transaction.commit()?;
            Ok(next_revision)
        }).await
    }

    /// Raw consistent export. Includes input records and does not annotate
    /// recommendation cadence or merge cross-IDs; do not serve as public API yet.
    pub async fn export_raw(&self, now: DateTime<Utc>) -> Result<CatalogSnapshot, StorageError> {
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                let revision = revision(&transaction)?;
                let records = {
                    let mut statement =
                        transaction.prepare(&format!("{SELECT_RECORD} ORDER BY kind,key"))?;
                    let mut rows = statement.query([])?;
                    let mut records = Vec::new();
                    while let Some(row) = rows.next()? {
                        records.push(read_record(row, now)?);
                    }
                    records
                };
                transaction.commit()?;
                Ok(CatalogSnapshot {
                    version: SNAPSHOT_VERSION.to_owned(),
                    revision,
                    exported_at: now,
                    records,
                })
            })
            .await
    }

    /// Backup creates a point-in-time SQLite database copy using VACUUM INTO.
    pub async fn backup(&self, path: impl AsRef<Path>) -> Result<(), StorageError> {
        let mut path = path.as_ref().to_path_buf();
        if path.exists() || std::fs::symlink_metadata(&path).is_ok() {
            return Err(StorageError::InvalidInput(
                "backup destination must not exist",
            ));
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
            let parent_canonical = std::fs::canonicalize(parent)?;
            if let Some(name) = path.file_name() {
                path = parent_canonical.join(name);
            }
        }
        let path_str = path
            .to_str()
            .ok_or(StorageError::InvalidInput("invalid backup path"))?
            .to_owned();
        self.database
            .call(move |connection| {
                connection.execute("VACUUM INTO ?", [&path_str])?;
                Ok(())
            })
            .await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let user_version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if user_version != 0 {
        return Err(StorageError::InvalidInput(
            "database is not a catalog database",
        ));
    }
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='schema_migrations')", [], |row| row.get(0)
    )?;
    let version: i64 = if exists {
        transaction.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?
    } else {
        0
    };
    if version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    if version < 0 {
        return Err(StorageError::Corrupt("negative catalog schema version"));
    }
    if version == 0 {
        let foreign_tables: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name<>'schema_migrations'", [], |row| row.get(0)
        )?;
        if foreign_tables != 0 {
            return Err(StorageError::InvalidInput(
                "database contains unowned tables",
            ));
        }
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE records(kind TEXT NOT NULL,key TEXT NOT NULL,data BLOB NOT NULL,updated INTEGER NOT NULL,expires INTEGER NOT NULL,revision INTEGER NOT NULL,PRIMARY KEY(kind,key));
             CREATE INDEX records_expiry ON records(expires,kind);
             CREATE INDEX records_title_search ON records(kind,lower(json_extract(data,'$.title')),key);
             CREATE TABLE revision(value INTEGER NOT NULL);
             INSERT INTO revision VALUES(0);
             CREATE TABLE jobs(key TEXT PRIMARY KEY,spec BLOB NOT NULL,due INTEGER NOT NULL DEFAULT 0,last_success INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0);
             CREATE INDEX jobs_due ON jobs(due);
             INSERT INTO schema_migrations VALUES(2);"
        )?;
    } else if version == 1 {
        validate_tables(&transaction)?;
        validate_index(&transaction, "records_expiry", "onrecords(expires,kind)")?;
        validate_index(&transaction, "jobs_due", "onjobs(due)")?;
        transaction.execute_batch(
            "CREATE INDEX IF NOT EXISTS records_title_search ON records(kind,lower(json_extract(data,'$.title')),key);
             INSERT INTO schema_migrations VALUES(2);"
        )?;
    }
    validate_schema(&transaction)?;
    transaction.commit()?;
    // Do not change persistent pragmas before ownership/version checks succeed.
    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if journal_mode != "wal" && journal_mode != "memory" {
        return Err(StorageError::Corrupt("catalog requires WAL journaling"));
    }
    connection.execute_batch("PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_size_limit=16777216; PRAGMA mmap_size=268435456; PRAGMA cache_size=-64000;")?;
    Ok(())
}

fn validate_schema(connection: &Connection) -> Result<(), StorageError> {
    let version: i64 = connection.query_row(
        "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if version != SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    validate_tables(connection)?;
    validate_index(connection, "records_expiry", "onrecords(expires,kind)")?;
    validate_index(
        connection,
        "records_title_search",
        "onrecords(kind,lower(json_extract(data,'$.title')),key)",
    )?;
    validate_index(connection, "jobs_due", "onjobs(due)")?;
    revision(connection)?;
    Ok(())
}

fn validate_tables(connection: &Connection) -> Result<(), StorageError> {
    // Column names/order/types and PK positions are the persisted Go contract.
    // Do not silently attach this repository to an unrelated SQLite database.
    for (table, expected) in [
        ("schema_migrations", vec![("version", "INTEGER", false, 1)]),
        (
            "records",
            vec![
                ("kind", "TEXT", true, 1),
                ("key", "TEXT", true, 2),
                ("data", "BLOB", true, 0),
                ("updated", "INTEGER", true, 0),
                ("expires", "INTEGER", true, 0),
                ("revision", "INTEGER", true, 0),
            ],
        ),
        ("revision", vec![("value", "INTEGER", true, 0)]),
        (
            "jobs",
            vec![
                ("key", "TEXT", false, 1),
                ("spec", "BLOB", true, 0),
                ("due", "INTEGER", true, 0),
                ("last_success", "INTEGER", true, 0),
                ("failed", "INTEGER", true, 0),
            ],
        ),
    ] {
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let actual = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?.to_ascii_uppercase(),
                    row.get::<_, bool>(3)?,
                    row.get::<_, i32>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if actual.len() != expected.len()
            || actual.iter().zip(expected).any(|(actual, expected)| {
                actual.0 != expected.0
                    || actual.1 != expected.1
                    || actual.2 != expected.2
                    || actual.3 != expected.3
            })
        {
            return Err(StorageError::Corrupt(
                "catalog table schema does not match supported version",
            ));
        }
    }
    Ok(())
}

fn validate_index(
    connection: &Connection,
    name: &str,
    expected_body: &str,
) -> Result<(), StorageError> {
    let sql: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type='index' AND name=?",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    let Some(sql) = sql else {
        return Err(StorageError::Corrupt("catalog index missing"));
    };
    let normalized: String = sql
        .to_ascii_lowercase()
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    if !normalized.ends_with(expected_body) {
        return Err(StorageError::Corrupt(
            "catalog index does not match supported version",
        ));
    }
    Ok(())
}

fn revision(connection: &Connection) -> Result<i64, StorageError> {
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM revision", [], |row| row.get(0))?;
    if count != 1 {
        return Err(StorageError::Corrupt("catalog requires one revision row"));
    }
    let value: i64 = connection.query_row("SELECT value FROM revision", [], |row| row.get(0))?;
    if value < 0 {
        return Err(StorageError::Corrupt("negative catalog revision"));
    }
    Ok(value)
}

fn get_record(
    connection: &Connection,
    kind: &str,
    key: &str,
    now: DateTime<Utc>,
) -> Result<CatalogRecord, StorageError> {
    let mut statement =
        connection.prepare_cached(&format!("{SELECT_RECORD} WHERE kind=? AND key=?"))?;
    let mut rows = statement.query(params![kind, key])?;
    let row = rows.next()?.ok_or(StorageError::NotFound)?;
    read_record(row, now)
}

fn read_bytes<'row>(row: &'row Row<'_>, column: usize) -> Result<&'row [u8], rusqlite::Error> {
    match row.get_ref(column)? {
        ValueRef::Blob(bytes) | ValueRef::Text(bytes) => Ok(bytes),
        value => Err(rusqlite::Error::InvalidColumnType(
            column,
            "data".to_owned(),
            value.data_type(),
        )),
    }
}

fn read_record(row: &Row<'_>, now: DateTime<Utc>) -> Result<CatalogRecord, StorageError> {
    let updated: i64 = row.get(3)?;
    let expires: i64 = row.get(4)?;
    let updated_at = DateTime::from_timestamp_millis(updated).ok_or(StorageError::Corrupt(
        "catalog updated timestamp out of range",
    ))?;
    let expires_at = DateTime::from_timestamp_millis(expires).ok_or(StorageError::Corrupt(
        "catalog expiry timestamp out of range",
    ))?;
    let revision: i64 = row.get(5)?;
    if revision < 0 {
        return Err(StorageError::Corrupt("negative catalog record revision"));
    }
    let data_bytes = read_bytes(row, 2)?;
    if data_bytes.len() > 16 * 1024 * 1024 {
        return Err(StorageError::Corrupt(
            "catalog record data exceeds size limit",
        ));
    }
    Ok(CatalogRecord {
        kind: row.get(0)?,
        key: row.get(1)?,
        data: serde_json::from_slice(data_bytes)?,
        updated_at,
        expires_at,
        revision,
        state: if now < expires_at { "fresh" } else { "stale" }.to_owned(),
    })
}
