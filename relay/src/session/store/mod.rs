//! One bounded, joined blocking owner for SQLite. Transactions precede publication.
use super::{
    AuthorizationRecord, AuthorizationStore, Resource,
    crypto::{Crypto, random_id},
};
use crate::{
    config::Config,
    contracts::{
        providers::{FetchContext, Selection},
        *,
    },
    error::{Error, INVALID, STORE},
    validation,
};
use axum::http::StatusCode;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{File, OpenOptions},
    future::Future,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const UNKNOWN: Error = Error(StatusCode::NOT_FOUND, "unknown_resource");
const CONFLICT: Error = Error(StatusCode::CONFLICT, "revision_conflict");
const EXPIRED: Error = Error(StatusCode::GONE, "session_expired");
const REVOKED: Error = Error(StatusCode::FORBIDDEN, "revoked_grant");
const LIMIT: Error = Error(StatusCode::TOO_MANY_REQUESTS, "state_limit");

type Job = Box<dyn FnOnce(&mut Engine) + Send>;
#[derive(Clone)]
pub struct Store {
    sender: mpsc::SyncSender<Job>,
    closing: Arc<AtomicBool>,
    config: Arc<Config>,
}
/// Must be polled and joined by the service owner. Dropping Store handles cannot
/// detach work; the receiver and engine belong exclusively to this worker.
pub struct Worker {
    receiver: mpsc::Receiver<Job>,
    engine: Engine,
    closing: Arc<AtomicBool>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Session {
    sid: String,
    owner: String,
    expires: i64,
    revision: u64,
    auth_revision: u64,
    generation: u64,
    revoked: bool,
    secret: String,
    created: i64,
    format: String,
    title: crate::contracts::providers::Title,
}
#[derive(Serialize, Deserialize)]
struct Generation {
    selection: Selection,
    root: String,
    valid_until: Option<i64>,
}
#[derive(Serialize, Deserialize)]
struct Idempotency {
    fingerprint: Vec<u8>,
    ready: SessionReady,
    expires: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Manifest,
    Segment,
    Fragment,
    Init,
    Key,
    Subtitle,
    Mp4,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByteRange {
    pub offset: u64,
    pub length: u64,
}
/// Sensitive: intentionally no Debug. Only the guarded transport may use a URI.
#[derive(Clone, Serialize, Deserialize)]
pub struct ManifestContext {
    pub ancestors: Vec<String>,
    pub selected_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adaptive_variants: Vec<crate::contracts::providers::Variant>,
    /// Verified variant and rendition children cannot introduce new unprobed masters.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub media_only: bool,
    pub subtitles: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Registration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ManifestContext>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub encrypted: bool,
    pub uri: String,
    pub kind: ResourceKind,
    pub context: FetchContext,
    pub range: Option<ByteRange>,
    pub expires_at: Option<i64>,
}
/// Immutable recovery input; no store lock or transaction survives this read.
pub(crate) struct RecoverySnapshot {
    pub selection: Selection,
    pub expires_at: i64,
    pub cancelled: CancellationToken,
    pub _reader: Arc<()>,
}

struct GenerationLease {
    cancelled: CancellationToken,
    readers: Arc<()>,
}
type CacheKey = (String, u64, String);
type CacheValue = (zeroize::Zeroizing<Vec<u8>>, usize);
struct Engine {
    db: Connection,
    _lock: File,
    crypto: Crypto,
    config: Arc<Config>,
    leases: BTreeMap<(String, u64), GenerationLease>,
    cache: BTreeMap<CacheKey, CacheValue>,
    order: VecDeque<CacheKey>,
    cache_bytes: usize,
    failed: bool,
    cleanup_cursor: (String, i64),
}
impl Store {
    pub(crate) fn root(selection: &Selection) -> Result<Registration, Error> {
        Engine::root(selection)
    }
    /// Startup is blocking and bounded by configured record/disk limits. The
    /// daemon calls this before admitting requests, on a blocking worker.
    pub fn open(config: Arc<Config>) -> Result<(Self, Worker), Error> {
        config.validate()?;
        let crypto = Crypto::load(Path::new(&config.state.encryption_key_file))?;
        let path = Path::new(&config.state.database_path);
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or(STORE)?;
        // A private relay-owned directory also protects SQLite's rollback journal.
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|_| STORE)?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| STORE)?;
        }
        let canonical_parent = std::fs::canonicalize(parent).map_err(|_| STORE)?;
        let canonical_key =
            std::fs::canonicalize(&config.state.encryption_key_file).map_err(|_| STORE)?;
        if canonical_key.starts_with(&canonical_parent) {
            return Err(STORE);
        }
        let meta = std::fs::symlink_metadata(parent).map_err(|_| STORE)?;
        if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
            return Err(STORE);
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path.with_extension("lock"))
            .map_err(|_| STORE)?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| STORE)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| STORE)?;
        let meta = file.metadata().map_err(|_| STORE)?;
        if !meta.is_file()
            || meta.permissions().mode() & 0o077 != 0
            || meta.len() > config.state.max_database_bytes
        {
            return Err(STORE);
        }
        let db = Connection::open(path).map_err(|_| STORE)?;
        db.busy_timeout(Duration::from_millis(250))
            .map_err(|_| STORE)?;
        db.set_limit(
            rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
            (config.state.max_snapshot_bytes * 8).min(i32::MAX as u64) as i32,
        )
        .map_err(|_| STORE)?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY; PRAGMA secure_delete=ON; PRAGMA cache_size=-2048;").map_err(|_|STORE)?;
        let page_size: i64 = db
            .pragma_query_value(None, "page_size", |r| r.get(0))
            .map_err(|_| STORE)?;
        db.pragma_update(
            None,
            "max_page_count",
            config.state.max_database_bytes as i64 / page_size,
        )
        .map_err(|_| STORE)?;
        let mut engine = Engine {
            db,
            _lock: lock,
            crypto,
            config,
            leases: BTreeMap::new(),
            cache: BTreeMap::new(),
            order: VecDeque::new(),
            cache_bytes: 0,
            failed: false,
            cleanup_cursor: (String::new(), 0),
        };
        engine.migrate()?;
        engine.restore()?;
        let (sender, receiver) =
            mpsc::sync_channel(engine.config.limits.max_store_queue_count as usize);
        let closing = Arc::new(AtomicBool::new(false));
        Ok((
            Self {
                sender,
                closing: closing.clone(),
                config: engine.config.clone(),
            },
            Worker {
                receiver,
                engine,
                closing,
            },
        ))
    }
    fn bound<T: Serialize>(&self, value: &T) -> Result<(), Error> {
        if serde_json::to_vec(value).map_err(|_| INVALID)?.len()
            > self.config.state.max_snapshot_bytes as usize
        {
            return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "import_too_large"));
        }
        Ok(())
    }
    async fn call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Engine) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let _time = crate::health::Timer::start(crate::health::Stage::Store);
        if self.closing.load(Ordering::Acquire) {
            return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "draining"));
        }
        let (send, receive) = oneshot::channel();
        self.sender
            .try_send(Box::new(move |engine| {
                if send.is_closed() {
                    return;
                }
                let result = if engine.failed {
                    Err(STORE)
                } else {
                    operation(engine)
                };
                // An I/O/authentication failure poisons this owner. Previously admitted
                // readers are cancelled; no cache can hide later corruption.
                if result.as_ref().is_err_and(|e| e.1 == STORE.1) {
                    engine.fail();
                }
                let _ = send.send(result);
            }))
            .map_err(|_| STORE)?;
        receive.await.map_err(|_| STORE)?
    }
    pub async fn create(&self, request: CreateSession, key: String) -> Result<SessionReady, Error> {
        self.bound(&request)?;
        self.call(move |e| e.create(request, key)).await
    }
    pub async fn read(&self, sid: String) -> Result<SessionReady, Error> {
        self.call(move |e| {
            let s = e.session(&sid)?;
            e.ready(&s, false)
        })
        .await
    }
    pub async fn renew(&self, sid: String, request: RenewSession) -> Result<SessionReady, Error> {
        self.call(move |e| e.renew(sid, request)).await
    }
    pub async fn backups(&self, sid: String, request: UpdateBackups) -> Result<(), Error> {
        self.bound(&request)?;
        self.call(move |engine| engine.backups(sid, request)).await
    }
    pub async fn source(&self, sid: String, request: UpdateSource) -> Result<SessionReady, Error> {
        self.bound(&request)?;
        self.call(move |e| e.source(sid, request)).await
    }
    pub(crate) async fn check_mutation(
        &self,
        sid: String,
        owner: String,
        revision: u64,
    ) -> Result<(), Error> {
        self.call(move |e| e.mutation(&sid, &owner, revision).map(|_| ()))
            .await
    }
    pub(crate) async fn recovery(
        &self,
        sid: String,
        owner: String,
        revision: u64,
        replacement: Option<Selection>,
    ) -> Result<RecoverySnapshot, Error> {
        if let Some(selection) = &replacement {
            self.bound(selection)?;
        }
        self.call(move |e| {
            let s = e.mutation(&sid, &owner, revision)?;
            let selection = match replacement {
                Some(selection) => {
                    e.validate_replacement(&s, &selection)?;
                    selection
                }
                None => e.generation(&sid, s.generation)?.selection,
            };
            let lease = e.leases.entry((sid, 0)).or_insert_with(|| GenerationLease {
                cancelled: CancellationToken::new(),
                readers: Arc::new(()),
            });
            Ok(RecoverySnapshot {
                selection,
                expires_at: s.expires,
                cancelled: lease.cancelled.clone(),
                _reader: lease.readers.clone(),
            })
        })
        .await
    }
    pub(crate) async fn activate(
        &self,
        sid: String,
        request: UpdateSource,
        batch: Vec<Registration>,
        cancel: CancellationToken,
        epoch: CancellationToken,
        deadline: tokio::time::Instant,
    ) -> Result<SessionReady, Error> {
        self.bound(&request)?;
        self.bound(&batch)?;
        self.call(move |e| e.source_prepared(sid, request, batch, Some((cancel, epoch, deadline))))
            .await
    }
    /// A bounded round trip through the actual store owner; does not fetch media.
    pub async fn health(&self) -> Result<(), Error> {
        self.call(|e| {
            let _: i64 =
                e.db.query_row("SELECT 1", [], |r| r.get(0))
                    .map_err(|_| STORE)?;
            e.writable()
        })
        .await
    }
    pub async fn revoke(&self, sid: String) -> Result<(), Error> {
        self.call(move |e| e.revoke(sid)).await
    }
    /// Registrations are all-or-nothing. Callers must await this result before
    /// serializing any resource URL into a manifest.
    pub async fn register(
        &self,
        sid: String,
        generation: u64,
        batch: Vec<Registration>,
    ) -> Result<Vec<String>, Error> {
        if batch.len() as u64 > self.config.limits.max_manifest_registrations_count {
            return Err(LIMIT);
        }
        self.bound(&batch)?;
        self.call(move |e| e.register(sid, generation, batch)).await
    }
    pub async fn resolve(
        &self,
        sid: String,
        generation: u64,
        rid: String,
    ) -> Result<Registration, Error> {
        self.call(move |e| {
            let s = e.session(&sid)?;
            e.active(&s)?;
            let g = e.generation(&sid, generation)?;
            e.valid_generation(&g)?;
            e.resource(&s, generation, &rid)
        })
        .await
    }
    pub async fn cleanup(&self) -> Result<(), Error> {
        self.call(|e| e.cleanup()).await
    }
}
impl Worker {
    pub async fn run(mut self, shutdown: CancellationToken) -> Result<(), Error> {
        tokio::task::spawn_blocking(move || {
            let mut last = Instant::now();
            loop {
                if shutdown.is_cancelled() {
                    break;
                }
                match self.receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(job) => job(&mut self.engine),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                self.engine.expire_readers();
                if last.elapsed() >= Duration::from_secs(self.engine.config.state.cleanup_period_s)
                {
                    if self.engine.cleanup().is_err() {
                        self.engine.fail();
                    }
                    last = Instant::now();
                }
            }
            self.closing.store(true, Ordering::Release);
            // Pending calls fail closed on drain. The in-flight transaction above
            // has already settled; queued work must not extend shutdown.
            for job in self.receiver.try_iter() {
                drop(job);
            }
            self.engine.fail();
            self.engine.db.close().map_err(|_| STORE)?;
            Ok(())
        })
        .await
        .map_err(|_| STORE)?
    }
}
impl AuthorizationStore for Store {
    fn lookup<'a>(
        &'a self,
        sid: &'a str,
        resource: &'a Resource,
    ) -> Pin<Box<dyn Future<Output = Result<AuthorizationRecord, Error>> + Send + 'a>> {
        let sid = sid.to_owned();
        let resource = resource.clone();
        Box::pin(async move { self.call(move |e| e.lookup(sid, resource)).await })
    }
}
impl Engine {
    fn fail(&mut self) {
        self.failed = true;
        for lease in self.leases.values() {
            lease.cancelled.cancel();
        }
        self.clear_cache();
    }
    fn clear_cache(&mut self) {
        self.cache.clear();
        self.order.clear();
        self.cache_bytes = 0;
    }
}

fn sql_u64(n: u64) -> Result<i64, Error> {
    n.try_into().map_err(|_| INVALID)
}
fn read_u64(row: &rusqlite::Row<'_>, index: usize) -> Result<u64, Error> {
    row.get::<_, i64>(index)
        .map_err(|_| STORE)?
        .try_into()
        .map_err(|_| STORE)
}

mod db;
mod lifecycle;
mod resources;
#[cfg(test)]
mod tests;
