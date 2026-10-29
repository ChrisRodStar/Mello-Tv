//! Bounded provider racing with scoped coalescing and progressive measured
//! startup. Provider generations are captured once; cached URLs are re-probed.

mod health;

use crate::{
    prober::{ProbeOptions, ProbeProgress, StreamProber},
    selection,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream::FuturesUnordered};
use semver::Version;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use stream_core::providers::{
    AdapterResult, ErrorCategory, Lookup, ProviderId, ProviderStatus, Selection,
};
use stream_provider_runtime::{ProviderHandle, ProviderRuntime, RuntimeError};
use stream_storage::cache::{ByteBoundCache, CacheNamespace, NamespacedKey, source_cache_deadline};
use thiserror::Error;
use tokio::{
    sync::{Semaphore, mpsc, watch},
    task::JoinSet,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("invalid provider lookup")]
    InvalidLookup,
    #[error("provider lookup capacity exhausted")]
    Capacity,
    #[error("provider coordinator is closed")]
    Closed,
    #[error("provider lookup cancelled")]
    Cancelled,
    #[error("provider lookup timed out")]
    Timeout,
    #[error("no eligible measured media source")]
    Unavailable,
    #[error("provider response violated its contract")]
    Protocol,
}

#[async_trait]
pub trait DiscoveryProvider: Send + Sync {
    fn id(&self) -> &ProviderId;
    fn version(&self) -> &Version;
    fn generation(&self) -> u64;
    async fn resolve(
        &self,
        lookup: &Lookup,
        cancel: CancellationToken,
    ) -> Result<AdapterResult, ErrorCategory>;
}

pub trait ProviderCatalog: Send + Sync {
    fn snapshot(&self) -> Result<Vec<Arc<dyn DiscoveryProvider>>, DiscoveryError>;
}

#[derive(Clone)]
pub struct ComponentProviders {
    runtime: ProviderRuntime,
}
impl ComponentProviders {
    pub fn new(runtime: ProviderRuntime) -> Self {
        Self { runtime }
    }
}
struct ComponentProvider {
    runtime: ProviderRuntime,
    handle: ProviderHandle,
}
#[async_trait]
impl DiscoveryProvider for ComponentProvider {
    fn id(&self) -> &ProviderId {
        &self.handle.manifest().provider_id
    }
    fn version(&self) -> &Version {
        &self.handle.manifest().version
    }
    fn generation(&self) -> u64 {
        self.handle.generation()
    }
    async fn resolve(
        &self,
        lookup: &Lookup,
        cancel: CancellationToken,
    ) -> Result<AdapterResult, ErrorCategory> {
        self.runtime
            .resolve_handle(&self.handle, lookup, cancel)
            .await
            .map_err(|error| match error {
                RuntimeError::Cancelled => ErrorCategory::Cancelled,
                RuntimeError::Timeout => ErrorCategory::Timeout,
                RuntimeError::Contract
                | RuntimeError::Component(_)
                | RuntimeError::ProviderFailure => ErrorCategory::ProtocolChange,
                _ => ErrorCategory::Unavailable,
            })
    }
}
impl ProviderCatalog for ComponentProviders {
    fn snapshot(&self) -> Result<Vec<Arc<dyn DiscoveryProvider>>, DiscoveryError> {
        self.runtime
            .snapshots()
            .map_err(|_| DiscoveryError::Closed)
            .map(|handles| {
                handles
                    .into_iter()
                    .map(|handle| {
                        Arc::new(ComponentProvider {
                            runtime: self.runtime.clone(),
                            handle,
                        }) as Arc<dyn DiscoveryProvider>
                    })
                    .collect()
            })
    }
}

#[derive(Clone, Debug)]
pub struct DiscoveryLimits {
    pub max_flights: usize,
    pub concurrent_providers: usize,
    pub round_timeout: Duration,
    pub provider_timeout: Duration,
}
impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            max_flights: 32,
            concurrent_providers: 10,
            round_timeout: Duration::from_secs(25),
            provider_timeout: Duration::from_secs(18),
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LookupKey {
    context: String,
    payload: String,
    generations: Vec<(ProviderId, Version, u64)>,
}
type CachedKey = NamespacedKey<String>;
struct Flight {
    publication: Mutex<()>,
    cancellation: CancellationToken,
    waiters: AtomicUsize,
    published: AtomicBool,
    complete_requested: AtomicBool,
    initial: watch::Sender<Option<Arc<Selection>>>,
    finished: watch::Sender<Option<Result<Arc<Selection>, DiscoveryError>>>,
}
impl Flight {
    fn new(complete: bool) -> Self {
        Self {
            publication: Mutex::new(()),
            cancellation: CancellationToken::new(),
            waiters: AtomicUsize::new(0),
            published: AtomicBool::new(false),
            complete_requested: AtomicBool::new(complete),
            initial: watch::channel(None).0,
            finished: watch::channel(None).0,
        }
    }
}
struct Waiter(Arc<Flight>);
impl Drop for Waiter {
    fn drop(&mut self) {
        if self.0.waiters.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _publication = self.0.publication.lock();
            if self.0.waiters.load(Ordering::Acquire) == 0
                && (!self.0.published.load(Ordering::Acquire)
                    || self.0.complete_requested.load(Ordering::Acquire))
            {
                self.0.cancellation.cancel();
            }
        }
    }
}
struct ProviderObservation {
    version: Version,
    generation: u64,
    observed_at: i64,
    failure: Option<ErrorCategory>,
    candidate_count: i64,
}

struct Inner {
    observations: Mutex<BTreeMap<ProviderId, ProviderObservation>>,
    health_results: tokio::sync::Mutex<Option<health::CachedHealth>>,
    providers: Arc<dyn ProviderCatalog>,
    prober: StreamProber,
    limits: DiscoveryLimits,
    flights: Mutex<BTreeMap<LookupKey, Arc<Flight>>>,
    tasks: tokio::sync::Mutex<JoinSet<()>>,
    slots: Semaphore,
    closed: AtomicBool,
    cache: ByteBoundCache<CachedKey, AdapterResult>,
}
#[derive(Clone)]
pub struct DiscoveryCoordinator {
    inner: Arc<Inner>,
}
struct Owner {
    inner: Arc<Inner>,
    key: LookupKey,
    flight: Arc<Flight>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        {
            let _publication = self.flight.publication.lock();
            self.flight.cancellation.cancel();
        }
        if self.flight.finished.borrow().is_none() {
            self.flight
                .finished
                .send_replace(Some(Err(DiscoveryError::Closed)));
        }
        if let Ok(mut flights) = self.inner.flights.lock()
            && flights
                .get(&self.key)
                .is_some_and(|current| Arc::ptr_eq(current, &self.flight))
        {
            flights.remove(&self.key);
        }
    }
}

impl DiscoveryCoordinator {
    pub fn new(
        providers: Arc<dyn ProviderCatalog>,
        prober: StreamProber,
        limits: DiscoveryLimits,
    ) -> Result<Self, DiscoveryError> {
        if !(1..=64).contains(&limits.max_flights)
            || !(1..=32).contains(&limits.concurrent_providers)
            || limits.round_timeout.is_zero()
            || limits.round_timeout > Duration::from_secs(25)
            || limits.provider_timeout.is_zero()
            || limits.provider_timeout > limits.round_timeout
        {
            return Err(DiscoveryError::InvalidLookup);
        }
        let cache = ByteBoundCache::new(256, 8 * 1024 * 1024, Duration::from_secs(1800))
            .map_err(|_| DiscoveryError::Capacity)?;
        Ok(Self {
            inner: Arc::new(Inner {
                observations: Mutex::new(BTreeMap::new()),
                health_results: tokio::sync::Mutex::new(None),
                providers,
                prober,
                slots: Semaphore::new(limits.concurrent_providers),
                limits,
                flights: Mutex::new(BTreeMap::new()),
                tasks: tokio::sync::Mutex::new(JoinSet::new()),
                closed: AtomicBool::new(false),
                cache,
            }),
        })
    }

    pub async fn resolve(
        &self,
        lookup: Lookup,
        cancel: &CancellationToken,
    ) -> Result<Arc<Selection>, DiscoveryError> {
        let deadline = Instant::now() + self.inner.limits.round_timeout;
        lookup
            .validate()
            .and_then(|()| lookup.validate_context())
            .map_err(|_| DiscoveryError::InvalidLookup)?;
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(DiscoveryError::Closed);
        }
        if cancel.is_cancelled() {
            return Err(DiscoveryError::Cancelled);
        }
        let providers = self.inner.providers.snapshot()?;
        if providers.is_empty() {
            return Err(DiscoveryError::Unavailable);
        }
        if providers.len() > 32 {
            return Err(DiscoveryError::Capacity);
        }
        let mut generations: Vec<_> = providers
            .iter()
            .map(|provider| {
                (
                    provider.id().clone(),
                    provider.version().clone(),
                    provider.generation(),
                )
            })
            .collect();
        generations.sort();
        if generations.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(DiscoveryError::Protocol);
        }
        let mut keyed = lookup.clone();
        keyed.wait_for_complete = false;
        let key = LookupKey {
            context: lookup.context_key.clone(),
            payload: serde_json::to_string(&keyed).map_err(|_| DiscoveryError::InvalidLookup)?,
            generations,
        };
        let flight = {
            // Serialize spawn admission with shutdown and reap completed owners.
            let mut tasks = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
                guard = tokio::time::timeout_at(deadline, self.inner.tasks.lock()) => {
                    guard.map_err(|_| DiscoveryError::Timeout)?
                },
            };
            while tasks.try_join_next().is_some() {}
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(DiscoveryError::Closed);
            }
            let mut flights = self
                .inner
                .flights
                .lock()
                .map_err(|_| DiscoveryError::Closed)?;
            let flight = if let Some(flight) = flights.get(&key) {
                flight.clone()
            } else {
                if flights.len() >= self.inner.limits.max_flights {
                    return Err(DiscoveryError::Capacity);
                }
                let flight = Arc::new(Flight::new(lookup.wait_for_complete));
                flights.insert(key.clone(), flight.clone());
                let owner = Owner {
                    inner: self.inner.clone(),
                    key: key.clone(),
                    flight: flight.clone(),
                };
                let lookup = lookup.clone();
                let task = async move {
                    let round = owner.inner.run_round(&lookup, providers, &owner.flight);
                    let result = tokio::select! {
                        biased;
                        _ = owner.flight.cancellation.cancelled() => Err(DiscoveryError::Cancelled),
                        result = tokio::time::timeout(owner.inner.limits.round_timeout, round) => {
                            result.unwrap_or(Err(DiscoveryError::Timeout)).map(Arc::new)
                        },
                    };
                    match owner.flight.publication.lock() {
                        Ok(_publication) => {
                            let result = if owner.flight.cancellation.is_cancelled() {
                                Err(DiscoveryError::Cancelled)
                            } else {
                                result
                            };
                            owner.flight.finished.send_replace(Some(result));
                        }
                        Err(_) => {
                            owner
                                .flight
                                .finished
                                .send_replace(Some(Err(DiscoveryError::Closed)));
                        }
                    };
                };
                tasks.spawn(task);
                flight
            };
            {
                let _publication = flight
                    .publication
                    .lock()
                    .map_err(|_| DiscoveryError::Closed)?;
                if flight.cancellation.is_cancelled() && flight.finished.borrow().is_none() {
                    return Err(DiscoveryError::Cancelled);
                }
                flight.waiters.fetch_add(1, Ordering::AcqRel);
                if lookup.wait_for_complete {
                    flight.complete_requested.store(true, Ordering::Release);
                }
            }
            flight
        };
        let _waiter = Waiter(flight.clone());
        let mut initial = flight.initial.subscribe();
        let mut finished = flight.finished.subscribe();
        let wait = async {
            loop {
                if cancel.is_cancelled() {
                    return Err(DiscoveryError::Cancelled);
                }
                if let Some(result) = finished.borrow().clone() {
                    return result;
                }
                if !lookup.wait_for_complete
                    && let Some(result) = initial.borrow().clone()
                    && selection::startup_ready(&lookup, &result, SystemTime::now().into())
                {
                    return Ok(result);
                }
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
                    result = finished.changed() => {
                        result.map_err(|_| DiscoveryError::Closed)?;
                    },
                    result = initial.changed(), if !lookup.wait_for_complete => {
                        result.map_err(|_| DiscoveryError::Closed)?;
                    },
                }
            }
        };
        tokio::time::timeout_at(deadline, wait)
            .await
            .map_err(|_| DiscoveryError::Timeout)?
    }

    /// Playback close/revocation can cancel its trusted partition, including
    /// background completion after a provisional result was already returned.
    pub fn cancel_context(&self, context: &str) -> Result<(), DiscoveryError> {
        let flights = self
            .inner
            .flights
            .lock()
            .map_err(|_| DiscoveryError::Closed)?;
        for (key, flight) in flights.iter() {
            if key.context == context {
                let _publication = flight.publication.lock();
                flight.cancellation.cancel();
            }
        }
        drop(flights);
        for provider in self.inner.providers.snapshot()? {
            let namespace =
                CacheNamespace::new(provider.id().clone(), provider.version().clone(), context)
                    .map_err(|_| DiscoveryError::InvalidLookup)?;
            self.inner
                .cache
                .invalidate_namespace(&namespace)
                .map_err(|_| DiscoveryError::Closed)?;
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), DiscoveryError> {
        self.inner.closed.store(true, Ordering::Release);
        // Keep owned handles in shared state across the await. If this caller
        // disconnects, the next shutdown caller can still join every owner.
        let mut tasks = self.inner.tasks.lock().await;
        {
            let flights = self
                .inner
                .flights
                .lock()
                .map_err(|_| DiscoveryError::Closed)?;
            for flight in flights.values() {
                let _publication = flight.publication.lock();
                flight.cancellation.cancel();
            }
        }
        while tasks.join_next().await.is_some() {}
        self.inner
            .cache
            .clear()
            .map_err(|_| DiscoveryError::Closed)?;
        Ok(())
    }
}

impl Inner {
    fn cache_key(
        provider: &dyn DiscoveryProvider,
        lookup: &Lookup,
    ) -> Result<CachedKey, DiscoveryError> {
        let namespace = CacheNamespace::new(
            provider.id().clone(),
            provider.version().clone(),
            lookup.context_key.clone(),
        )
        .map_err(|_| DiscoveryError::InvalidLookup)?;
        let mut lookup = lookup.clone();
        lookup.wait_for_complete = false;
        lookup.refresh = false;
        let payload = serde_json::to_string(&lookup).map_err(|_| DiscoveryError::InvalidLookup)?;
        Ok(NamespacedKey::new(
            namespace,
            format!("{}:{payload}", provider.generation()),
        ))
    }

    async fn provider(
        &self,
        provider: &dyn DiscoveryProvider,
        lookup: &Lookup,
        flight: &Flight,
    ) -> (ProviderStatus, Vec<stream_core::providers::CandidateStream>) {
        let cancel = flight.cancellation.child_token();
        let _cancel_on_drop = cancel.clone().drop_guard();
        let start = Instant::now();
        let key = Self::cache_key(provider, lookup);
        let operation = async {
            if !lookup.refresh
                && let Ok(key) = &key
                && let Ok(Some(value)) = self.cache.get(key)
            {
                return Ok(value);
            }
            let _slot = self
                .slots
                .acquire()
                .await
                .map_err(|_| ErrorCategory::Unavailable)?;
            let value = provider.resolve(lookup, cancel.clone()).await?;
            value
                .validate_for(provider.id(), &lookup.title, 32)
                .map_err(|_| ErrorCategory::ProtocolChange)?;
            let value = Arc::new(value);
            // Never publish late results after the last waiter cancelled.
            if let Ok(key) = key {
                let wall: DateTime<Utc> = SystemTime::now().into();
                let expiries = value
                    .candidates
                    .iter()
                    .flatten()
                    .map(|candidate| candidate.expires_at);
                if let Ok(Some(deadline)) = source_cache_deadline(
                    std::time::Instant::now(),
                    wall,
                    Duration::from_secs(1800),
                    expiries,
                ) && let Ok(encoded) = serde_json::to_vec(value.as_ref())
                {
                    let weight = encoded.len().saturating_add(32 * 1024);
                    let _publication = flight
                        .publication
                        .lock()
                        .map_err(|_| ErrorCategory::Unavailable)?;
                    if !cancel.is_cancelled() {
                        let _ = self
                            .cache
                            .insert(key, value.clone(), weight, Some(deadline));
                    }
                }
            }
            Ok::<_, ErrorCategory>(value)
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ErrorCategory::Cancelled),
            result = tokio::time::timeout(self.limits.provider_timeout, operation) => {
                result.unwrap_or(Err(ErrorCategory::Timeout))
            },
        };
        let duration = i64::try_from(start.elapsed().as_nanos()).unwrap_or(i64::MAX);
        // Cancellation and title-specific lack of support do not indicate an outage.
        if !matches!(
            &result,
            Err(ErrorCategory::Cancelled | ErrorCategory::Unsupported)
        ) && let Ok(mut observations) = self.observations.lock()
            && (observations.len() < 32 || observations.contains_key(provider.id()))
        {
            let (failure, candidate_count) = match &result {
                Ok(value) => (
                    None,
                    value
                        .candidates
                        .as_ref()
                        .map_or(0, |items| items.len() as i64),
                ),
                Err(category) => (Some(*category), 0),
            };
            let observed_at: DateTime<Utc> = SystemTime::now().into();
            observations.insert(
                provider.id().clone(),
                ProviderObservation {
                    version: provider.version().clone(),
                    generation: provider.generation(),
                    observed_at: observed_at.timestamp(),
                    failure,
                    candidate_count,
                },
            );
        }
        match result {
            Ok(value) => {
                let candidates = value.candidates.clone().unwrap_or_default();
                (
                    ProviderStatus {
                        provider: provider.id().clone(),
                        failure: None,
                        duration,
                        candidate_count: candidates.len() as i64,
                    },
                    candidates,
                )
            }
            Err(category) => (
                ProviderStatus {
                    provider: provider.id().clone(),
                    failure: Some(category),
                    duration,
                    candidate_count: 0,
                },
                Vec::new(),
            ),
        }
    }

    async fn run_round(
        self: &Arc<Self>,
        lookup: &Lookup,
        providers: Vec<Arc<dyn DiscoveryProvider>>,
        flight: &Arc<Flight>,
    ) -> Result<Selection, DiscoveryError> {
        let cancel = flight.cancellation.child_token();
        let _cancel_on_drop = cancel.clone().drop_guard();
        // Every adapter returns at most 32 candidates. The aggregate bound lets
        // collection keep polling all adapters while probing applies backpressure.
        let (sender, receiver) = mpsc::channel(providers.len() * 32);
        let callback: ProbeProgress = {
            let lookup = lookup.clone();
            let flight = flight.clone();
            Arc::new(move |items| {
                let Ok(_publication) = flight.publication.lock() else {
                    return;
                };
                if flight.published.load(Ordering::Acquire) || flight.cancellation.is_cancelled() {
                    return;
                }
                if let Ok(mut result) =
                    selection::select(&lookup, items.to_vec(), SystemTime::now().into())
                    && selection::startup_ready(&lookup, &result, SystemTime::now().into())
                {
                    result.pending = true;
                    flight.published.store(true, Ordering::Release);
                    flight.initial.send_replace(Some(Arc::new(result)));
                }
            })
        };
        let height = [lookup.device.max_height, lookup.preferences.max_height]
            .into_iter()
            .filter(|height| *height > 0)
            .min();
        let probing = self.prober.probe_stream(
            receiver,
            ProbeOptions {
                max_height: height,
                startup_height: (lookup.startup_height > 0).then_some(lookup.startup_height),
            },
            &cancel,
            Some(callback),
        );
        let collecting = async {
            let mut outcomes = providers
                .iter()
                .map(|provider| self.provider(provider.as_ref(), lookup, flight))
                .collect::<FuturesUnordered<_>>();
            let mut statuses = Vec::with_capacity(providers.len());
            while let Some((mut status, candidates)) = outcomes.next().await {
                for mut candidate in candidates.into_iter().take(6) {
                    if candidate.title.imdb_id.is_empty() {
                        candidate.title.imdb_id = lookup.title.imdb_id.clone();
                    }
                    if candidate.title.my_anime_list_id.is_none() {
                        candidate.title.my_anime_list_id = lookup.title.my_anime_list_id;
                    }
                    candidate.id = format!("{}:{}", status.provider, candidate.id);
                    if candidate.id.len() > 256 {
                        status.failure = Some(ErrorCategory::ProtocolChange);
                        continue;
                    }
                    if sender.send(candidate).await.is_err() {
                        break;
                    }
                }
                statuses.push(status);
            }
            drop(sender);
            statuses
        };
        let (mut statuses, probed) = tokio::join!(collecting, probing);
        let probed = probed.map_err(|error| match error {
            crate::prober::ProbeRoundError::Cancelled => DiscoveryError::Cancelled,
            _ => DiscoveryError::Capacity,
        })?;
        if cancel.is_cancelled() {
            return Err(DiscoveryError::Cancelled);
        }
        for provider in &providers {
            let observations: Vec<_> = probed
                .iter()
                .filter(|item| &item.source.provider == provider.id())
                .collect();
            if !observations.iter().any(|item| item.observation.accessible)
                && observations.iter().any(|item| {
                    matches!(
                        item.observation.failure,
                        Some(
                            ErrorCategory::UpstreamFailure
                                | ErrorCategory::ProtocolChange
                                | ErrorCategory::Timeout
                        )
                    )
                })
                && let Ok(key) = Self::cache_key(provider.as_ref(), lookup)
            {
                let _ = self.cache.remove(&key);
            }
        }
        let mut result = selection::select(lookup, probed, SystemTime::now().into())
            .map_err(|_| DiscoveryError::Protocol)?;
        statuses.sort_by(|a, b| a.provider.cmp(&b.provider));
        result.providers = Some(statuses);
        if result.selected.is_none() {
            return Err(DiscoveryError::Unavailable);
        }
        result.validate().map_err(|_| DiscoveryError::Protocol)?;
        Ok(result)
    }
}

#[async_trait]
impl stream_core::ports::MediaEngine for DiscoveryCoordinator {
    fn diagnostics(&self) -> stream_core::ports::media::MediaDiagnostics {
        use stream_core::ports::media::{MediaDiagnostics, ProviderDiagnostic};
        let Ok(providers) = self.inner.providers.snapshot() else {
            return MediaDiagnostics::default();
        };
        let observations = self.inner.observations.lock().ok();
        MediaDiagnostics {
            configured: !self.inner.closed.load(Ordering::Acquire) && !providers.is_empty(),
            providers: providers
                .into_iter()
                .map(|provider| {
                    let observation = observations
                        .as_ref()
                        .and_then(|items| items.get(provider.id()))
                        .filter(|item| {
                            item.version == *provider.version()
                                && item.generation == provider.generation()
                        });
                    ProviderDiagnostic {
                        id: provider.id().to_string(),
                        version: provider.version().to_string(),
                        last_observed_at: observation.map(|item| item.observed_at),
                        last_failure: observation.and_then(|item| item.failure),
                        candidate_count: observation.map(|item| item.candidate_count),
                    }
                })
                .collect(),
        }
    }
    async fn check_provider_health(
        &self,
    ) -> Result<Vec<stream_core::ports::media::ProviderHealth>, stream_core::error::DomainError>
    {
        self.run_provider_health_checks()
            .await
            .map_err(|error| stream_core::error::DomainError::Infrastructure(Box::new(error)))
    }

    async fn resolve_stream(
        &self,
        lookup: &Lookup,
    ) -> Result<Selection, stream_core::error::DomainError> {
        let cancel = CancellationToken::new();
        self.resolve(lookup.clone(), &cancel)
            .await
            .map(|arc| (*arc).clone())
            .map_err(|err| match err {
                DiscoveryError::InvalidLookup => {
                    stream_core::error::DomainError::Validation(err.to_string())
                }
                DiscoveryError::Unavailable => {
                    stream_core::error::DomainError::NotFound(err.to_string())
                }
                _ => stream_core::error::DomainError::Infrastructure(Box::new(err)),
            })
    }
}
