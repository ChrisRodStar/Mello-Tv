//! Small, strictly bounded caches for provider results and probe observations.
//!
//! Values are shared with `Arc`, without serialization or deep cloning. Capacity
//! is enforced before publishing each entry; `accounted_bytes` is supplied by the
//! caller and must include retained key/value allocations. It is an accounting
//! budget, not a measurement of process RSS or external `Arc` owners.
//!
//! A mutex protects all state. Expiry and LRU eviction scan the configured entry
//! bound, which is appropriate for the existing small provider caches. No work is
//! spawned, and eviction is complete before an operation returns. Callbacks and
//! network requests never run while holding the lock.

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use semver::Version;
use stream_core::providers::ProviderId;
use thiserror::Error;

pub const SOURCE_EXPIRY_MARGIN: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CacheError {
    #[error("cache entry and byte limits must be positive")]
    InvalidLimits,
    #[error("cache entry accounting must include at least one byte")]
    InvalidWeight,
    #[error("cache entry exceeds its byte budget ({bytes} > {limit})")]
    EntryTooLarge { bytes: usize, limit: usize },
    #[error("cache deadline is outside the monotonic clock range")]
    DeadlineOverflow,
    #[error("cache access sequence exhausted")]
    SequenceOverflow,
    #[error("cache state lock is poisoned")]
    Poisoned,
    #[error("credential cache namespace must contain 1 to 128 bytes without controls")]
    InvalidNamespace,
}

/// Discovery/probe isolation includes the exact provider package generation and
/// a trusted credential partition. Never use a raw authorization token as a key.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CacheNamespace {
    provider: ProviderId,
    version: Version,
    context_key: String,
}

impl CacheNamespace {
    pub fn new(
        provider: ProviderId,
        version: Version,
        context_key: impl Into<String>,
    ) -> Result<Self, CacheError> {
        let context_key = context_key.into();
        if context_key.is_empty()
            || context_key.len() > 128
            || context_key.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(CacheError::InvalidNamespace);
        }
        Ok(Self {
            provider,
            version,
            context_key,
        })
    }

    pub fn provider(&self) -> &ProviderId {
        &self.provider
    }
    pub fn version(&self) -> &Version {
        &self.version
    }
}

impl fmt::Debug for CacheNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CacheNamespace")
            .field("provider", &self.provider)
            .field("version", &self.version)
            .field("context_key", &"[REDACTED]")
            .finish()
    }
}

/// The remaining key must include content coordinates and every relevant device
/// and language preference. A title ID alone is not a resolution cache key.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NamespacedKey<K> {
    namespace: CacheNamespace,
    key: K,
}

impl<K> NamespacedKey<K> {
    pub fn new(namespace: CacheNamespace, key: K) -> Self {
        Self { namespace, key }
    }
    pub fn namespace(&self) -> &CacheNamespace {
        &self.namespace
    }
    pub fn key(&self) -> &K {
        &self.key
    }
}

impl<K> fmt::Debug for NamespacedKey<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NamespacedKey")
            .field("namespace", &self.namespace)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheStats {
    pub entries: usize,
    pub accounted_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InsertOutcome {
    Inserted,
    Replaced,
    /// A zero TTL or elapsed caller deadline also invalidates the same old key.
    NotRetained,
}

struct Entry<V> {
    value: Arc<V>,
    bytes: usize,
    expires_at: Instant,
    last_access: u64,
}

struct State<K, V> {
    entries: BTreeMap<K, Entry<V>>,
    bytes: usize,
    sequence: u64,
}

impl<K: Ord + Clone, V> State<K, V> {
    fn next_sequence(&mut self) -> Result<u64, CacheError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(CacheError::SequenceOverflow)?;
        Ok(self.sequence)
    }

    fn remove(&mut self, key: &K) -> Option<Arc<V>> {
        let entry = self.entries.remove(key)?;
        self.bytes -= entry.bytes;
        Some(entry.value)
    }

    fn prune(&mut self, now: Instant, retired: &mut Vec<Arc<V>>) {
        let expired: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| now >= entry.expires_at)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            if let Some(value) = self.remove(&key) {
                retired.push(value);
            }
        }
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            accounted_bytes: self.bytes,
        }
    }
}

pub struct ByteBoundCache<K, V> {
    state: Mutex<State<K, V>>,
    max_entries: usize,
    max_bytes: usize,
    ttl: Duration,
}

impl<K: Ord + Clone, V> ByteBoundCache<K, V> {
    pub fn new(max_entries: usize, max_bytes: usize, ttl: Duration) -> Result<Self, CacheError> {
        if max_entries == 0 || max_bytes == 0 {
            return Err(CacheError::InvalidLimits);
        }
        Ok(Self {
            state: Mutex::new(State {
                entries: BTreeMap::new(),
                bytes: 0,
                sequence: 0,
            }),
            max_entries,
            max_bytes,
            ttl,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, State<K, V>>, CacheError> {
        self.state.lock().map_err(|_| CacheError::Poisoned)
    }

    pub fn get(&self, key: &K) -> Result<Option<Arc<V>>, CacheError> {
        self.get_at(key, Instant::now())
    }

    /// `now` permits deterministic tests; production callers should use `get`.
    pub fn get_at(&self, key: &K, now: Instant) -> Result<Option<Arc<V>>, CacheError> {
        // Declare retired values first so the lock is released before their
        // destructors run, including on early returns or error propagation.
        let mut retired = Vec::new();
        let mut state = self.lock()?;
        state.prune(now, &mut retired);
        if !state.entries.contains_key(key) {
            return Ok(None);
        }
        let sequence = state.next_sequence()?;
        let Some(entry) = state.entries.get_mut(key) else {
            return Ok(None);
        };
        entry.last_access = sequence;
        Ok(Some(Arc::clone(&entry.value)))
    }

    pub fn insert(
        &self,
        key: K,
        value: Arc<V>,
        accounted_bytes: usize,
        deadline: Option<Instant>,
    ) -> Result<InsertOutcome, CacheError> {
        self.insert_at(key, value, accounted_bytes, Instant::now(), deadline)
    }

    /// Retention ends at the earlier of configured TTL and caller deadline.
    /// An oversized/invalid replacement leaves the existing entry untouched.
    /// The caller-supplied `now` must be a monotonic sample, not wall-clock time.
    pub fn insert_at(
        &self,
        key: K,
        value: Arc<V>,
        accounted_bytes: usize,
        now: Instant,
        deadline: Option<Instant>,
    ) -> Result<InsertOutcome, CacheError> {
        let mut retired = Vec::new();
        if self.ttl.is_zero() || deadline.is_some_and(|deadline| deadline <= now) {
            let mut state = self.lock()?;
            state.prune(now, &mut retired);
            if let Some(value) = state.remove(&key) {
                retired.push(value);
            }
            return Ok(InsertOutcome::NotRetained);
        }
        if accounted_bytes == 0 {
            return Err(CacheError::InvalidWeight);
        }
        if accounted_bytes > self.max_bytes {
            return Err(CacheError::EntryTooLarge {
                bytes: accounted_bytes,
                limit: self.max_bytes,
            });
        }
        // Clamp the duration first: a finite caller deadline can make an
        // otherwise overflowing configured TTL representable.
        let ttl = deadline.map_or(self.ttl, |deadline| {
            self.ttl.min(deadline.duration_since(now))
        });
        let expires_at = now.checked_add(ttl).ok_or(CacheError::DeadlineOverflow)?;
        let mut state = self.lock()?;
        state.prune(now, &mut retired);
        let sequence = state.next_sequence()?;
        let replaced = if let Some(value) = state.remove(&key) {
            retired.push(value);
            true
        } else {
            false
        };
        while state.entries.len() >= self.max_entries
            || state.bytes > self.max_bytes - accounted_bytes
        {
            let oldest = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            if let Some(value) = state.remove(&oldest) {
                retired.push(value);
            }
        }
        state.bytes += accounted_bytes;
        state.entries.insert(
            key,
            Entry {
                value,
                bytes: accounted_bytes,
                expires_at,
                last_access: sequence,
            },
        );
        Ok(if replaced {
            InsertOutcome::Replaced
        } else {
            InsertOutcome::Inserted
        })
    }

    pub fn remove(&self, key: &K) -> Result<Option<Arc<V>>, CacheError> {
        Ok(self.lock()?.remove(key))
    }

    pub fn stats(&self) -> Result<CacheStats, CacheError> {
        self.stats_at(Instant::now())
    }

    pub fn stats_at(&self, now: Instant) -> Result<CacheStats, CacheError> {
        let mut retired = Vec::new();
        let mut state = self.lock()?;
        state.prune(now, &mut retired);
        Ok(state.stats())
    }

    pub fn clear(&self) -> Result<(), CacheError> {
        let mut state = self.lock()?;
        let retired = std::mem::take(&mut state.entries);
        state.bytes = 0;
        drop(state);
        drop(retired);
        Ok(())
    }
}

impl<K: Ord + Clone, V> ByteBoundCache<NamespacedKey<K>, V> {
    /// Invalidate exactly one package/credential generation after provider update
    /// or credential revocation, without evicting another user's results.
    pub fn invalidate_namespace(&self, namespace: &CacheNamespace) -> Result<usize, CacheError> {
        let mut retired = Vec::new();
        let mut state = self.lock()?;
        let keys: Vec<_> = state
            .entries
            .keys()
            .filter(|key| &key.namespace == namespace)
            .cloned()
            .collect();
        let count = keys.len();
        for key in keys {
            if let Some(value) = state.remove(&key) {
                retired.push(value);
            }
        }
        Ok(count)
    }
}

/// Convert source wall-clock expiry to a monotonic cache deadline once, using
/// the legacy five-second safety margin. Existing source expiries only shorten
/// the TTL; already expired/near-expiry candidates make the whole result uncached.
/// Never keep recomputing retained cache deadlines from a changing wall clock.
pub fn source_cache_deadline(
    now: Instant,
    wall_now: DateTime<Utc>,
    ttl: Duration,
    expiries: impl IntoIterator<Item = Option<DateTime<Utc>>>,
) -> Result<Option<Instant>, CacheError> {
    if ttl.is_zero() {
        return Ok(None);
    }
    let mut effective_ttl = ttl;
    for expiry in expiries.into_iter().flatten() {
        // A non-null pointer to Go's zero time is treated as unknown expiry.
        if expiry.timestamp() == -62_135_596_800 && expiry.timestamp_subsec_nanos() == 0 {
            continue;
        }
        let Some(remaining) = expiry
            .signed_duration_since(wall_now)
            .to_std()
            .ok()
            .and_then(|remaining| remaining.checked_sub(SOURCE_EXPIRY_MARGIN))
        else {
            return Ok(None);
        };
        if remaining.is_zero() {
            return Ok(None);
        }
        effective_ttl = effective_ttl.min(remaining);
    }
    now.checked_add(effective_ttl)
        .map(Some)
        .ok_or(CacheError::DeadlineOverflow)
}
