//! Legacy-compatible API state storage. Network playback orchestration stays in
//! the application layer; all account/profile mutations here are transactional.

use std::{collections::HashSet, fmt, path::Path, sync::Arc, time::SystemTime};

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use stream_core::{
    accounts::{
        PlaybackRequest, Preferences, Profile, ProfileInput, Progress, ProgressUpdate, RelayReady,
        Target, is_safe_identifier,
    },
    catalog::{Title, TitleIdentity},
};
use zeroize::Zeroizing;

use crate::{StorageError, db::Database};

const QUEUE_CAPACITY: usize = 64;
const SCHEMA_VERSION: i64 = 1;
const SENTINEL_AAD: &str = "api-key-check";
const SENTINEL_PLAIN: &[u8] = b"family-stream-api.v1";

// Kept compatible with api/internal/api/store.go, including tables not yet used
// by Rust. Do not repurpose user_version or share this file with catalog state.
const SCHEMA: &str = r#"
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
PRAGMA user_version=1;
"#;

const TABLES: &[(&str, &[&str])] = &[
    ("settings", &["key", "value"]),
    ("accounts", &["id", "username", "password", "salt", "admin"]),
    ("sessions", &["hash", "account", "expires"]),
    (
        "profiles",
        &["id", "account", "data", "version", "synced", "last_sync"],
    ),
    ("lists", &["profile", "kind", "target", "data"]),
    (
        "progress",
        &["profile", "target", "type", "title_id", "data"],
    ),
    ("activity", &["profile", "day"]),
    (
        "playbacks",
        &[
            "id",
            "profile",
            "request",
            "ready",
            "revoked",
            "revoked_done",
            "expires",
            "total",
            "sequence",
            "created",
            "idem",
            "fingerprint",
            "stage",
        ],
    ),
    (
        "mutations",
        &[
            "playback", "idem", "kind", "revision", "payload", "base", "done", "result",
        ],
    ),
    (
        "mutation_history",
        &["playback", "idem", "kind", "revision", "result"],
    ),
    ("deletions", &["handle"]),
    ("refresh_jobs", &["key", "data"]),
    ("snapshot", &["id", "revision", "data"]),
];

/// Profile list namespaces accepted by the existing HTTP API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Library,
    Favorites,
    RecommendationHidden,
    RecommendationWatched,
    RecommendationIgnored,
}

impl ListKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Favorites => "favorites",
            Self::RecommendationHidden => "recommendation-hidden",
            Self::RecommendationWatched => "recommendation-watched",
            Self::RecommendationIgnored => "recommendation-ignored",
        }
    }

    pub fn from_path(s: &str) -> Option<Self> {
        match s {
            "library" => Some(Self::Library),
            "favorites" => Some(Self::Favorites),
            "recommendation-hidden" => Some(Self::RecommendationHidden),
            "recommendation-watched" => Some(Self::RecommendationWatched),
            "recommendation-ignored" => Some(Self::RecommendationIgnored),
            _ => None,
        }
    }

    const fn limit(self) -> i64 {
        match self {
            Self::Favorites => 50,
            _ => 1_000,
        }
    }
}

/// Full recommendation feedback payload matching iOS and Go contracts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecommendationFeedback {
    pub hidden: Vec<Title>,
    pub watched: Vec<Title>,
    pub ignored: Vec<Title>,
    pub favorites: Vec<Title>,
    pub topics: Vec<String>,
}

/// A durable playback allocation. The caller supplies a stable request
/// fingerprint and idempotency key; no provider or relay network call occurs.
#[derive(Debug, Clone)]
pub struct NewPlayback {
    pub id: String,
    pub request: PlaybackRequest,
    pub ready: Option<RelayReady>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub idempotency_key: String,
    pub fingerprint: String,
}

/// The durable, non-secret state needed to resume playback orchestration.
#[derive(Debug, Clone)]
pub struct StoredPlayback {
    pub id: String,
    pub request: PlaybackRequest,
    pub ready: Option<RelayReady>,
    pub revoked: bool,
}

struct ProgressSession {
    request: Vec<u8>,
    total: f64,
    sequence: i64,
    created: i64,
    expires: i64,
    revoked: bool,
    ready: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct AccountStore {
    database: Database,
    key: Arc<Zeroizing<[u8; 32]>>,
}

impl fmt::Debug for AccountStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccountStore")
            .field("key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl AccountStore {
    pub async fn open(path: impl AsRef<Path>, key: [u8; 32]) -> Result<Self, StorageError> {
        let database = Database::open(path, QUEUE_CAPACITY).await?;
        match Self::from_database(database.clone(), key).await {
            Ok(store) => Ok(store),
            Err(error) => {
                let _ = database.shutdown().await;
                Err(error)
            }
        }
    }

    pub async fn from_database(database: Database, key: [u8; 32]) -> Result<Self, StorageError> {
        let key = Arc::new(Zeroizing::new(key));
        let validation_key = key.clone();
        database
            .call(move |connection| initialize(connection, validation_key.as_ref()))
            .await?;
        Ok(Self { database, key })
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub async fn shutdown(&self) -> Result<(), StorageError> {
        self.database.shutdown().await
    }

    /// Preserve the configured or existing household owner. Multiple legacy
    /// accounts require an explicit selection rather than combining their data.
    pub async fn select_household(&self, configured: Option<&str>) -> Result<String, StorageError> {
        if configured.is_some_and(|value| !is_safe_identifier(value)) {
            return Err(StorageError::InvalidInput("household_account_id"));
        }
        let configured = configured.map(str::to_owned);
        self.database.call(move |connection| {
            let transaction = connection.transaction()?;
            let selected = match configured {
                Some(value) => value,
                None => {
                    let prior: Option<String> = transaction.query_row(
                        "SELECT value FROM settings WHERE key='household-account-id'", [],
                        |row| row.get(0),
                    ).optional()?;
                    if let Some(prior) = prior {
                        prior
                    } else {
                        let owners = {
                            let mut statement = transaction.prepare("SELECT id FROM accounts ORDER BY id LIMIT 2")?;
                            statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?
                        };
                        match owners.as_slice() {
                            [] => {
                                let owner = random_identifier()?;
                                transaction.execute("INSERT INTO accounts VALUES(?1,'household',?2,?3,1)", params![owner, Vec::<u8>::new(), Vec::<u8>::new()])?;
                                owner
                            }
                            [owner] => owner.clone(),
                            _ => return Err(StorageError::Conflict("multiple_household_owners")),
                        }
                    }
                }
            };
            if !is_safe_identifier(&selected) {
                return Err(StorageError::Corrupt("household account identifier"));
            }
            let exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)", [&selected], |row| row.get(0))?;
            if !exists {
                return Err(StorageError::NotFound);
            }
            transaction.execute("INSERT INTO settings(key,value) VALUES('household-account-id',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [&selected])?;
            transaction.commit()?;
            Ok(selected)
        }).await
    }

    pub async fn list_profiles(&self, account: &str) -> Result<Vec<Profile>, StorageError> {
        check_identifier(account)?;
        let account = account.to_owned();
        self.database
            .call(move |connection| {
                let mut statement = connection
                    .prepare_cached("SELECT id,data FROM profiles WHERE account=?1 ORDER BY id")?;
                let records = statement.query_map([account], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?;
                records
                    .map(|record| {
                        let (id, data) = record?;
                        decode_profile(&id, &data)
                    })
                    .collect()
            })
            .await
    }

    pub async fn get_profile(
        &self,
        account: &str,
        profile_id: &str,
    ) -> Result<Profile, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| owned_profile(connection, &account, &profile_id))
            .await
    }

    pub async fn create_profile(
        &self,
        account: &str,
        profile: Profile,
    ) -> Result<Profile, StorageError> {
        check_identifier(account)?;
        profile
            .validate()
            .map_err(|_| StorageError::InvalidInput("profile"))?;
        let account = account.to_owned();
        let data = serde_json::to_vec(&profile)?;
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
                    [&account],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(StorageError::NotFound);
                }
                let count: i64 =
                    transaction.query_row("SELECT count(*) FROM profiles", [], |row| row.get(0))?;
                if count >= 128 {
                    return Err(StorageError::Conflict("profile_limit"));
                }
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM profiles WHERE id=?1)",
                    [&profile.id],
                    |row| row.get(0),
                )?;
                if exists {
                    return Err(StorageError::Conflict("profile_exists"));
                }
                transaction.execute(
                    "INSERT INTO profiles(id,account,data) VALUES(?1,?2,?3)",
                    params![profile.id, account, data],
                )?;
                transaction.commit()?;
                Ok(profile)
            })
            .await
    }

    pub async fn update_profile(
        &self,
        account: &str,
        profile_id: &str,
        input: ProfileInput,
    ) -> Result<Profile, StorageError> {
        input
            .validate()
            .map_err(|_| StorageError::InvalidInput("profile"))?;
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                let mut profile = owned_profile(&transaction, &account, &profile_id)?;
                profile.name = input.name;
                profile.color = input.color;
                transaction.execute(
                    "UPDATE profiles SET data=?1,version=version+1 WHERE id=?2",
                    params![serde_json::to_vec(&profile)?, profile_id],
                )?;
                transaction.commit()?;
                Ok(profile)
            })
            .await
    }

    pub async fn update_preferences(
        &self,
        account: &str,
        profile_id: &str,
        preferences: Preferences,
    ) -> Result<Profile, StorageError> {
        preferences
            .validate()
            .map_err(|_| StorageError::InvalidInput("preferences"))?;
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                let mut profile = owned_profile(&transaction, &account, &profile_id)?;
                profile.preferences = preferences;
                transaction.execute(
                    "UPDATE profiles SET data=?1,version=version+1 WHERE id=?2",
                    params![serde_json::to_vec(&profile)?, profile_id],
                )?;
                transaction.commit()?;
                Ok(profile)
            })
            .await
    }

    pub async fn delete_profile(
        &self,
        account: &str,
        profile_id: &str,
    ) -> Result<(), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                transaction.execute("INSERT OR IGNORE INTO deletions VALUES(?1)", [&profile_id])?;
                transaction.execute(
                    "UPDATE playbacks SET revoked=1 WHERE profile=?1",
                    [&profile_id],
                )?;
                transaction.execute(
                    "DELETE FROM profiles WHERE id=?1 AND account=?2",
                    params![profile_id, account],
                )?;
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn list_titles(
        &self,
        account: &str,
        profile_id: &str,
        kind: ListKind,
    ) -> Result<Vec<Title>, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database.call(move |connection| {
            owned_profile(connection, &account, &profile_id)?;
            let mut statement = connection.prepare_cached("SELECT data FROM lists WHERE profile=?1 AND kind=?2 ORDER BY target LIMIT 1000")?;
            let rows = statement.query_map(params![profile_id, kind.as_str()], |row| row.get::<_, Vec<u8>>(0))?;
            rows.map(|data| {
                let title: Title = serde_json::from_slice(&data?)?;
                title.validate().map_err(|_| StorageError::Corrupt("list title"))?;
                Ok(title)
            }).collect()
        }).await
    }

    pub async fn put_title(
        &self,
        account: &str,
        profile_id: &str,
        kind: ListKind,
        title: Title,
    ) -> Result<(), StorageError> {
        title
            .validate()
            .map_err(|_| StorageError::InvalidInput("title"))?;
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database.call(move |connection| {
            let transaction = connection.transaction()?;
            owned_profile(&transaction, &account, &profile_id)?;
            let key = title.identity.key();
            let count: i64 = transaction.query_row("SELECT count(*) FROM lists WHERE profile=?1 AND kind=?2 AND target<>?3", params![profile_id, kind.as_str(), key], |row| row.get(0))?;
            if count >= kind.limit() { return Err(StorageError::Conflict("list_limit")); }
            transaction.execute("INSERT INTO lists VALUES(?1,?2,?3,?4) ON CONFLICT(profile,kind,target) DO UPDATE SET data=excluded.data", params![profile_id, kind.as_str(), key, serde_json::to_vec(&title)?])?;
            increment_version(&transaction, &profile_id)?;
            transaction.commit()?;
            Ok(())
        }).await
    }

    pub async fn remove_title(
        &self,
        account: &str,
        profile_id: &str,
        kind: ListKind,
        title: TitleIdentity,
    ) -> Result<(), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                transaction.execute(
                    "DELETE FROM lists WHERE profile=?1 AND kind=?2 AND target=?3",
                    params![profile_id, kind.as_str(), title.key()],
                )?;
                increment_version(&transaction, &profile_id)?;
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    /// Insert or recover the same allocation by `(profile, idempotency_key)`.
    /// Conflicting fingerprints do not replace or mutate an existing playback.
    pub async fn create_playback(
        &self,
        account: &str,
        profile_id: &str,
        playback: NewPlayback,
    ) -> Result<StoredPlayback, StorageError> {
        validate_new_playback(&playback)?;
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database.call(move |connection| {
            let transaction = connection.transaction()?;
            owned_profile(&transaction, &account, &profile_id)?;
            let previous: Option<(String, String)> = transaction.query_row("SELECT id,fingerprint FROM playbacks WHERE profile=?1 AND idem=?2", params![profile_id, playback.idempotency_key], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
            if let Some((id, fingerprint)) = previous {
                if fingerprint != playback.fingerprint { return Err(StorageError::Conflict("idempotency_conflict")); }
                return load_playback(&transaction, &profile_id, &id);
            }
            let count: i64 = transaction.query_row("SELECT count(*) FROM playbacks", [], |row| row.get(0))?;
            if count >= 4_096 { return Err(StorageError::Conflict("playback_limit")); }
            let exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM playbacks WHERE id=?1)", [&playback.id], |row| row.get(0))?;
            if exists { return Err(StorageError::Conflict("playback_exists")); }
            let request = serde_json::to_vec(&playback.request)?;
            let ready = playback.ready.as_ref().map(serde_json::to_vec).transpose()?;
            transaction.execute("INSERT INTO playbacks(id,profile,request,ready,expires,created,idem,fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![playback.id, profile_id, request, ready, playback.expires_at.timestamp(), playback.created_at.timestamp(), playback.idempotency_key, playback.fingerprint])?;
            let result = StoredPlayback { id: playback.id, request: playback.request, ready: playback.ready, revoked: false };
            transaction.commit()?;
            Ok(result)
        }).await
    }

    pub async fn get_playback(
        &self,
        account: &str,
        profile_id: &str,
        playback_id: &str,
    ) -> Result<StoredPlayback, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(playback_id)?;
        let playback_id = playback_id.to_owned();
        self.database
            .call(move |connection| {
                owned_profile(connection, &account, &profile_id)?;
                load_playback(connection, &profile_id, &playback_id)
            })
            .await
    }

    /// Finish initial relay registration. Later renew/failover mutations need
    /// their own revision-checked transaction rather than overwriting readiness.
    pub async fn save_ready(
        &self,
        account: &str,
        profile_id: &str,
        playback_id: &str,
        ready: RelayReady,
    ) -> Result<(), StorageError> {
        ready
            .validate()
            .map_err(|_| StorageError::InvalidInput("relay_ready"))?;
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(playback_id)?;
        let playback_id = playback_id.to_owned();
        self.database.call(move |connection| {
            let transaction = connection.transaction()?;
            owned_profile(&transaction, &account, &profile_id)?;
            let playback = load_playback(&transaction, &profile_id, &playback_id)?;
            if playback.revoked { return Err(StorageError::Conflict("revoked")); }
            if let Some(previous) = playback.ready {
                return if previous == ready { Ok(()) } else { Err(StorageError::Conflict("ready_already_registered")) };
            }
            transaction.execute("UPDATE playbacks SET ready=?1,expires=?2,stage=NULL WHERE id=?3 AND profile=?4", params![serde_json::to_vec(&ready)?, ready.expires_at.timestamp(), playback_id, profile_id])?;
            transaction.commit()?;
            Ok(())
        }).await
    }

    pub async fn revoke_playback(
        &self,
        account: &str,
        profile_id: &str,
        playback_id: &str,
    ) -> Result<(), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(playback_id)?;
        let playback_id = playback_id.to_owned();
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                if transaction.execute(
                    "UPDATE playbacks SET revoked=1 WHERE id=?1 AND profile=?2",
                    params![playback_id, profile_id],
                )? == 0
                {
                    return Err(StorageError::NotFound);
                }
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    /// AAD equals the playback ID, exactly matching Go's encrypted stage blob.
    pub async fn save_stage(
        &self,
        account: &str,
        profile_id: &str,
        playback_id: &str,
        plaintext: Zeroizing<Vec<u8>>,
    ) -> Result<(), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(playback_id)?;
        if plaintext.len() > 8 * 1_024 * 1_024 {
            return Err(StorageError::Capacity);
        }
        let playback_id = playback_id.to_owned();
        let key = self.key.clone();
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                let playback = load_playback(&transaction, &profile_id, &playback_id)?;
                if playback.revoked {
                    return Err(StorageError::Conflict("revoked"));
                }
                if playback.ready.is_some() {
                    return Err(StorageError::Conflict("ready_already_registered"));
                }
                let encrypted = seal(key.as_ref(), &playback_id, &plaintext)?;
                transaction.execute(
                    "UPDATE playbacks SET stage=?1 WHERE id=?2 AND profile=?3",
                    params![encrypted, playback_id, profile_id],
                )?;
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn load_stage(
        &self,
        account: &str,
        profile_id: &str,
        playback_id: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(playback_id)?;
        let playback_id = playback_id.to_owned();
        let key = self.key.clone();
        self.database
            .call(move |connection| {
                owned_profile(connection, &account, &profile_id)?;
                let encrypted: Option<Vec<u8>> = connection
                    .query_row(
                        "SELECT stage FROM playbacks WHERE id=?1 AND profile=?2",
                        params![playback_id, profile_id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or(StorageError::NotFound)?;
                encrypted
                    .filter(|value| !value.is_empty())
                    .map(|encrypted| unseal(key.as_ref(), &playback_id, &encrypted))
                    .transpose()
            })
            .await
    }

    pub async fn update_progress(
        &self,
        account: &str,
        profile_id: &str,
        update: ProgressUpdate,
    ) -> Result<Progress, StorageError> {
        self.update_progress_at(account, profile_id, update, SystemTime::now().into())
            .await
    }

    /// `now` is a trusted application clock, never a client-supplied timestamp.
    pub async fn update_progress_at(
        &self,
        account: &str,
        profile_id: &str,
        update: ProgressUpdate,
        now: DateTime<Utc>,
    ) -> Result<Progress, StorageError> {
        update
            .validate()
            .map_err(|_| StorageError::InvalidInput("progress"))?;
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database.call(move |connection| {
            let transaction = connection.transaction()?;
            owned_profile(&transaction, &account, &profile_id)?;
            let row: Option<ProgressSession> = transaction.query_row(
                "SELECT request,total,sequence,created,expires,revoked,ready FROM playbacks WHERE id=?1 AND profile=?2", params![update.playback_id, profile_id],
                |row| Ok(ProgressSession { request:row.get(0)?,total:row.get(1)?,sequence:row.get(2)?,created:row.get(3)?,expires:row.get(4)?,revoked:row.get(5)?,ready:row.get(6)? }),
            ).optional()?;
            let ProgressSession { request, total, sequence, created, expires, revoked, ready } = row.ok_or(StorageError::NotFound)?;
            if ready.as_ref().is_none_or(Vec::is_empty) { return Err(StorageError::Conflict("setup_pending")); }
            if revoked || now.timestamp() > expires.saturating_add(300) { return Err(StorageError::Conflict("session_expired")); }
            if !total.is_finite() || total < 0.0 || sequence < 0 { return Err(StorageError::Corrupt("playback progress counters")); }
            let request: PlaybackRequest = serde_json::from_slice(&request)?;
            request.validate().map_err(|_| StorageError::Corrupt("playback request"))?;
            let mut progress = read_progress(&transaction, &profile_id, &request.target)?.unwrap_or_else(|| Progress {
                target: request.target.clone(), position_seconds:0.0, duration_seconds:0.0, watched_seconds:0.0, completed:false, updated_at: now,
            });
            if update.sequence <= sequence { return Ok(progress); }
            if update.watched_seconds < total { return Err(StorageError::Conflict("counter_regression")); }
            let elapsed = now.timestamp().saturating_sub(created).max(0) as f64;
            if update.watched_seconds > elapsed * 2.0 + 30.0 { return Err(StorageError::InvalidInput("watched_seconds exceeds elapsed playback")); }
            progress.watched_seconds += update.watched_seconds - total;
            progress.position_seconds = if update.duration_seconds > 0.0 { update.position_seconds.min(update.duration_seconds) } else { update.position_seconds };
            progress.duration_seconds = update.duration_seconds;
            progress.completed = progress.completed || (update.duration_seconds > 0.0 && progress.position_seconds >= update.duration_seconds * 0.9 && progress.watched_seconds >= update.duration_seconds * 0.6);
            progress.updated_at = now;
            progress.validate().map_err(|_| StorageError::Corrupt("progress accumulation"))?;
            let media_type = match request.target.identity().media_type() { stream_core::domain::MediaType::Movie => "movie", stream_core::domain::MediaType::Tv => "tv" };
            transaction.execute("INSERT INTO progress VALUES(?1,?2,?3,?4,?5) ON CONFLICT(profile,target) DO UPDATE SET data=excluded.data", params![profile_id, request.target.key(), media_type, request.target.identity().id(), serde_json::to_vec(&progress)?])?;
            transaction.execute("UPDATE playbacks SET total=?1,sequence=?2 WHERE id=?3 AND profile=?4", params![update.watched_seconds, update.sequence, update.playback_id, profile_id])?;
            touch(&transaction, &profile_id, now)?;
            transaction.commit()?;
            Ok(progress)
        }).await
    }

    pub async fn get_progress(
        &self,
        account: &str,
        profile_id: &str,
        target: Target,
    ) -> Result<Option<Progress>, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                owned_profile(connection, &account, &profile_id)?;
                read_progress(connection, &profile_id, &target)
            })
            .await
    }

    pub async fn history(
        &self,
        account: &str,
        profile_id: &str,
        offset: i64,
    ) -> Result<Vec<Progress>, StorageError> {
        if offset < 0 {
            return Err(StorageError::InvalidInput("offset"));
        }
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database.call(move |connection| {
            owned_profile(connection, &account, &profile_id)?;
            let mut statement = connection.prepare_cached("SELECT target,data FROM progress WHERE profile=?1 ORDER BY json_extract(data,'$.updated_at') DESC,target LIMIT 100 OFFSET ?2")?;
            let rows = statement.query_map(params![profile_id,offset], |row| Ok((row.get::<_,String>(0)?,row.get::<_,Vec<u8>>(1)?)))?;
            rows.map(|row| { let (target,data)=row?; decode_progress(&target,&data) }).collect()
        }).await
    }

    pub async fn record_activity(
        &self,
        account: &str,
        profile_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                touch(&transaction, &profile_id, now)?;
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    /// Retrieve grouped feedback lists and hidden topics for the recommendation engine.
    pub async fn recommendation_feedback(
        &self,
        account: &str,
        profile_id: &str,
    ) -> Result<RecommendationFeedback, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                owned_profile(connection, &account, &profile_id)?;
                let mut statement = connection.prepare_cached(
                    "SELECT kind,target,data FROM lists WHERE profile=?1 AND kind IN ('recommendation-hidden','recommendation-watched','recommendation-ignored','favorites','recommendation-topics') ORDER BY kind,target",
                )?;
                let rows = statement.query_map([&profile_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                })?;
                let mut hidden = Vec::new();
                let mut watched = Vec::new();
                let mut ignored = Vec::new();
                let mut favorites = Vec::new();
                let mut topics = Vec::new();
                for row in rows {
                    let (kind, target, data) = row?;
                    if kind == "recommendation-topics" {
                        topics.push(target);
                        continue;
                    }
                    let title: Title = serde_json::from_slice(&data)?;
                    match kind.as_str() {
                        "recommendation-hidden" => hidden.push(title),
                        "recommendation-watched" => watched.push(title),
                        "recommendation-ignored" => ignored.push(title),
                        "favorites" => favorites.push(title),
                        _ => {}
                    }
                }
                Ok(RecommendationFeedback {
                    hidden,
                    watched,
                    ignored,
                    favorites,
                    topics,
                })
            })
            .await
    }

    pub async fn list_recommendation_topics(
        &self,
        account: &str,
        profile_id: &str,
    ) -> Result<Vec<String>, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                owned_profile(connection, &account, &profile_id)?;
                let mut statement = connection.prepare_cached(
                    "SELECT target FROM lists WHERE profile=?1 AND kind='recommendation-topics' ORDER BY target",
                )?;
                let rows = statement.query_map([&profile_id], |row| row.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
            })
            .await
    }

    pub async fn put_recommendation_topic(
        &self,
        account: &str,
        profile_id: &str,
        topic_id: &str,
    ) -> Result<(), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(topic_id)?;
        if !topic_id.starts_with("topic-") && !topic_id.starts_with("taste-") {
            return Err(StorageError::InvalidInput("topic_id"));
        }
        let topic_id = topic_id.to_owned();
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                let data = serde_json::to_vec(&serde_json::json!({ "id": topic_id }))?;
                transaction.execute(
                    "INSERT INTO lists VALUES(?1, 'recommendation-topics', ?2, ?3) ON CONFLICT(profile,kind,target) DO NOTHING",
                    params![profile_id, topic_id, data],
                )?;
                increment_version(&transaction, &profile_id)?;
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn delete_recommendation_topic(
        &self,
        account: &str,
        profile_id: &str,
        topic_id: &str,
    ) -> Result<(), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(topic_id)?;
        let topic_id = topic_id.to_owned();
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                transaction.execute(
                    "DELETE FROM lists WHERE profile=?1 AND kind='recommendation-topics' AND target=?2",
                    params![profile_id, topic_id],
                )?;
                increment_version(&transaction, &profile_id)?;
                transaction.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn get_recommendation_exclusions(
        &self,
        account: &str,
        profile_id: &str,
    ) -> Result<(HashSet<String>, HashSet<String>), StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        self.database
            .call(move |connection| {
                owned_profile(connection, &account, &profile_id)?;
                let mut statement = connection.prepare_cached(
                    "SELECT kind,target FROM lists WHERE profile=?1 AND kind IN ('recommendation-hidden','recommendation-watched','recommendation-topics')",
                )?;
                let rows = statement.query_map([&profile_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                let mut excluded_titles = HashSet::new();
                let mut hidden_topics = HashSet::new();
                for row in rows {
                    let (kind, target) = row?;
                    if kind == "recommendation-topics" {
                        hidden_topics.insert(target);
                    } else {
                        excluded_titles.insert(target);
                    }
                }
                let mut movie_stmt = connection.prepare_cached(
                    "SELECT target, data FROM progress WHERE profile=?1 AND type='movie'",
                )?;
                let movie_rows = movie_stmt.query_map([&profile_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?;
                for row in movie_rows {
                    let (target, data) = row?;
                    if decode_progress(&target, &data).is_ok_and(|p| p.completed) {
                        excluded_titles.insert(target);
                    }
                }
                Ok((excluded_titles, hidden_topics))
            })
            .await
    }

    /// Mutate an active playback session lease (renew, backup, refresh).
    #[allow(clippy::too_many_arguments)]
    pub async fn mutate_playback(
        &self,
        account: &str,
        profile_id: &str,
        playback_id: &str,
        idempotency_key: &str,
        kind: &str,
        expected_revision: u64,
        new_ready: RelayReady,
    ) -> Result<StoredPlayback, StorageError> {
        let (account, profile_id) = identifiers(account, profile_id)?;
        check_identifier(playback_id)?;
        check_identifier(idempotency_key)?;
        new_ready
            .validate()
            .map_err(|_| StorageError::InvalidInput("relay_ready"))?;
        let playback_id = playback_id.to_owned();
        let idempotency_key = idempotency_key.to_owned();
        let kind = kind.to_owned();
        self.database
            .call(move |connection| {
                let transaction = connection.transaction()?;
                owned_profile(&transaction, &account, &profile_id)?;
                let playback = load_playback(&transaction, &profile_id, &playback_id)?;
                if playback.revoked {
                    return Err(StorageError::Conflict("revoked"));
                }
                let current_ready = playback
                    .ready
                    .ok_or(StorageError::Conflict("setup_pending"))?;

                let prior: Option<(String, i64, Vec<u8>)> = {
                    let mut stmt = transaction.prepare_cached(
                        "SELECT kind,revision,result FROM mutation_history WHERE playback=?1 AND idem=?2",
                    )?;
                    stmt.query_row(
                        params![playback_id, idempotency_key],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Vec<u8>>(2)?)),
                    )
                    .optional()?
                };
                if let Some((prior_kind, prior_revision, prior_result)) = prior {
                    if prior_kind != kind || prior_revision as u64 != expected_revision {
                        return Err(StorageError::Conflict("idempotency_conflict"));
                    }
                    let ready: RelayReady = serde_json::from_slice(&prior_result)?;
                    if ready.revision != current_ready.revision {
                        return Err(StorageError::Conflict("operation_superseded"));
                    }
                    return Ok(StoredPlayback {
                        id: playback_id,
                        request: playback.request,
                        ready: Some(ready),
                        revoked: playback.revoked,
                    });
                }

                if expected_revision != current_ready.revision {
                    return Err(StorageError::Conflict("revision_conflict"));
                }

                if new_ready.session_id != current_ready.session_id
                    || new_ready.revision != expected_revision + 1
                    || new_ready.auth_revision != current_ready.auth_revision
                {
                    return Err(StorageError::InvalidInput("relay_invalid_transition"));
                }

                if kind == "renew"
                    && (new_ready.source_generation != current_ready.source_generation
                        || new_ready.expires_at <= current_ready.expires_at)
                {
                    return Err(StorageError::InvalidInput("relay_invalid_transition"));
                }
                if kind != "renew" && new_ready.source_generation <= current_ready.source_generation {
                    return Err(StorageError::InvalidInput("relay_invalid_transition"));
                }

                let result_bytes = serde_json::to_vec(&new_ready)?;
                transaction.execute(
                    "INSERT INTO mutation_history(playback,idem,kind,revision,result) VALUES(?1,?2,?3,?4,?5)",
                    params![playback_id, idempotency_key, kind, expected_revision as i64, result_bytes],
                )?;
                transaction.execute(
                    "UPDATE playbacks SET ready=?1,expires=?2 WHERE id=?3 AND profile=?4",
                    params![
                        result_bytes,
                        new_ready.expires_at.timestamp(),
                        playback_id,
                        profile_id
                    ],
                )?;
                let _ = transaction
                    .execute("DELETE FROM mutations WHERE playback=?1", params![playback_id]);
                transaction.commit()?;
                Ok(StoredPlayback {
                    id: playback_id,
                    request: playback.request,
                    ready: Some(new_ready),
                    revoked: false,
                })
            })
            .await
    }
}

fn check_identifier(value: &str) -> Result<(), StorageError> {
    if is_safe_identifier(value) {
        Ok(())
    } else {
        Err(StorageError::InvalidInput("identifier"))
    }
}

fn identifiers(account: &str, profile_id: &str) -> Result<(String, String), StorageError> {
    check_identifier(account)?;
    check_identifier(profile_id)?;
    Ok((account.to_owned(), profile_id.to_owned()))
}

fn initialize(connection: &mut Connection, key: &[u8; 32]) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    if version == 0 {
        let objects: i64 = transaction.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        if objects != 0 {
            return Err(StorageError::Corrupt("database is not empty API state"));
        }
        transaction.execute_batch(SCHEMA)?;
    } else if version == SCHEMA_VERSION {
        validate_schema(&transaction)?;
    } else {
        return Err(StorageError::UnsupportedSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    let sentinel: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT value FROM settings WHERE key='encryption-check'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match sentinel {
        Some(envelope) => {
            let plain = unseal(key, SENTINEL_AAD, &envelope)?;
            if plain.as_slice() != SENTINEL_PLAIN {
                return Err(StorageError::Crypto);
            }
        }
        None => {
            let encrypted_rows: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM playbacks WHERE stage IS NOT NULL AND length(stage)>0) OR EXISTS(SELECT 1 FROM mutations WHERE payload IS NOT NULL AND length(payload)>0)", [], |row| row.get(0))?;
            if encrypted_rows {
                return Err(StorageError::Crypto);
            }
            transaction.execute(
                "INSERT INTO settings VALUES('encryption-check',?1)",
                [seal(key, SENTINEL_AAD, SENTINEL_PLAIN)?],
            )?;
        }
    }
    transaction.commit()?;
    // Persistent configuration is applied only after schema and key acceptance.
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    Ok(())
}

fn validate_schema(connection: &Connection) -> Result<(), StorageError> {
    let count: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type IN ('table','view','trigger') AND name NOT LIKE 'sqlite_%'", [], |row|row.get(0))?;
    if count != TABLES.len() as i64 {
        return Err(StorageError::Corrupt("API table set"));
    }
    for (table, expected) in TABLES {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::Corrupt("API table missing"));
        }
        let columns = {
            let mut statement =
                connection.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?;
            statement
                .query_map([table], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        if !columns
            .iter()
            .map(String::as_str)
            .eq(expected.iter().copied())
        {
            return Err(StorageError::Corrupt("API table columns"));
        }
    }
    // Names alone do not establish ownership: preserve PK/FK constraints,
    // column affinities/defaults and indexes before writing the key sentinel.
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(SCHEMA)?;
    if schema_definitions(connection)? != schema_definitions(&reference)? {
        return Err(StorageError::Corrupt("API schema constraints"));
    }
    Ok(())
}

fn schema_definitions(
    connection: &Connection,
) -> Result<Vec<(String, String, String)>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
    )?;
    statement
        .query_map([], |row| {
            let sql: String = row.get(2)?;
            let normalized = sql
                .chars()
                .filter(|value| !value.is_ascii_whitespace())
                .flat_map(char::to_lowercase)
                .collect();
            Ok((row.get(0)?, row.get(1)?, normalized))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn decode_profile(id: &str, data: &[u8]) -> Result<Profile, StorageError> {
    let profile: Profile = serde_json::from_slice(data)?;
    profile
        .validate()
        .map_err(|_| StorageError::Corrupt("profile data"))?;
    if profile.id != id {
        return Err(StorageError::Corrupt("profile identity"));
    }
    Ok(profile)
}

fn owned_profile(
    connection: &Connection,
    account: &str,
    profile_id: &str,
) -> Result<Profile, StorageError> {
    let data: Option<Vec<u8>> = connection
        .query_row(
            "SELECT data FROM profiles WHERE id=?1 AND account=?2",
            params![profile_id, account],
            |row| row.get(0),
        )
        .optional()?;
    decode_profile(profile_id, &data.ok_or(StorageError::NotFound)?)
}

fn increment_version(transaction: &Transaction<'_>, profile_id: &str) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE profiles SET version=version+1 WHERE id=?1",
        [profile_id],
    )?;
    Ok(())
}

fn touch(
    transaction: &Transaction<'_>,
    profile_id: &str,
    now: DateTime<Utc>,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT OR IGNORE INTO activity VALUES(?1,?2)",
        params![profile_id, now.format("%Y-%m-%d").to_string()],
    )?;
    increment_version(transaction, profile_id)
}

fn validate_new_playback(playback: &NewPlayback) -> Result<(), StorageError> {
    check_identifier(&playback.id)?;
    check_identifier(&playback.idempotency_key)?;
    playback
        .request
        .validate()
        .map_err(|_| StorageError::InvalidInput("playback_request"))?;
    if playback.fingerprint.len() != 64
        || !playback
            .fingerprint
            .bytes()
            .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
    {
        return Err(StorageError::InvalidInput("fingerprint"));
    }
    if playback.expires_at < playback.created_at {
        return Err(StorageError::InvalidInput("playback expiry"));
    }
    if let Some(ready) = &playback.ready {
        ready
            .validate()
            .map_err(|_| StorageError::InvalidInput("relay_ready"))?;
        if ready.expires_at.timestamp() != playback.expires_at.timestamp() {
            return Err(StorageError::InvalidInput("relay expiry mismatch"));
        }
    }
    Ok(())
}

fn load_playback(
    connection: &Connection,
    profile_id: &str,
    playback_id: &str,
) -> Result<StoredPlayback, StorageError> {
    let row: Option<(Vec<u8>, Option<Vec<u8>>, bool)> = connection
        .query_row(
            "SELECT request,ready,revoked FROM playbacks WHERE id=?1 AND profile=?2",
            params![playback_id, profile_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let (request, ready, revoked) = row.ok_or(StorageError::NotFound)?;
    let request: PlaybackRequest = serde_json::from_slice(&request)?;
    request
        .validate()
        .map_err(|_| StorageError::Corrupt("playback request"))?;
    let ready: Option<RelayReady> = ready
        .filter(|value| !value.is_empty())
        .map(|data| serde_json::from_slice(&data))
        .transpose()?;
    if let Some(ready) = &ready {
        ready
            .validate()
            .map_err(|_| StorageError::Corrupt("relay ready"))?;
    }
    Ok(StoredPlayback {
        id: playback_id.to_owned(),
        request,
        ready,
        revoked,
    })
}

fn decode_progress(target_key: &str, data: &[u8]) -> Result<Progress, StorageError> {
    let progress: Progress = serde_json::from_slice(data)?;
    progress
        .validate()
        .map_err(|_| StorageError::Corrupt("progress data"))?;
    if progress.target.key() != target_key {
        return Err(StorageError::Corrupt("progress identity"));
    }
    Ok(progress)
}

fn read_progress(
    connection: &Connection,
    profile_id: &str,
    target: &Target,
) -> Result<Option<Progress>, StorageError> {
    let target_key = target.key();
    let data: Option<Vec<u8>> = connection
        .query_row(
            "SELECT data FROM progress WHERE profile=?1 AND target=?2",
            params![profile_id, target_key],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| decode_progress(&target_key, &data))
        .transpose()
}

fn random_identifier() -> Result<String, StorageError> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|_| StorageError::Crypto)?;
    use std::fmt::Write;
    let mut result = String::with_capacity(48);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").map_err(|_| StorageError::Crypto)?;
    }
    Ok(result)
}

fn seal(key: &[u8; 32], aad: &str, plain: &[u8]) -> Result<Vec<u8>, StorageError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| StorageError::Crypto)?;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|_| StorageError::Crypto)?;
    let ciphertext = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plain,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| StorageError::Crypto)?;
    let mut envelope = Vec::with_capacity(nonce.len() + ciphertext.len());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

fn unseal(key: &[u8; 32], aad: &str, envelope: &[u8]) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    let (nonce, ciphertext) = envelope.split_at_checked(12).ok_or(StorageError::Crypto)?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| StorageError::Crypto)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| StorageError::Crypto)?;
    cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| StorageError::Crypto)
}
