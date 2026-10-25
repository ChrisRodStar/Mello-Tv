use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
    thread,
    time::Duration,
};

use semver::Version;
use sha2::{Digest, Sha256};
use stream_core::{
    packages::{ArtifactDigest, PackageError, ProviderManifest},
    providers::{AdapterResult, Lookup, ProviderId},
};
use thiserror::Error;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, timeout_at},
};
use tokio_util::sync::CancellationToken;
use wasmtime::{
    Config, Engine, Store, StoreLimitsBuilder,
    component::{Component, Linker},
};

use crate::{
    bindings::{Provider, ProviderPre},
    http::{HostState, HttpPolicy, HttpTransport},
};

#[derive(Error)]
pub enum RuntimeError {
    #[error("provider manifest is invalid or incompatible")]
    Manifest(#[from] PackageError),
    #[error("provider artifact length or digest does not match its manifest")]
    ArtifactMismatch,
    #[error("provider component could not be compiled, linked or executed")]
    Component(#[source] wasmtime::Error),
    #[error("provider JSON contract is invalid")]
    Contract,
    #[error("provider component imports an unsupported host capability")]
    UnsupportedImport,
    #[error("provider execution failed")]
    ProviderFailure,
    #[error("provider runtime capacity is exhausted")]
    Capacity,
    #[error("provider runtime is unavailable")]
    Closed,
    #[error("provider lookup timed out")]
    Timeout,
    #[error("provider lookup was cancelled")]
    Cancelled,
    #[error("provider version must be newer than all previously activated versions")]
    VersionConflict,
    #[error("provider package belongs to a different runtime")]
    ForeignPackage,
    #[error("provider is not installed")]
    NotFound,
    #[error("provider has no retained previous version")]
    NoPreviousVersion,
    #[error("provider is already installed in this runtime")]
    AlreadyInstalled,
    #[error("trusted installed provider state is inconsistent")]
    InvalidInstalledState,
    #[error("provider runtime configuration is invalid")]
    InvalidConfig,
    #[error("provider runtime thread could not start")]
    Thread(#[source] std::io::Error),
}

impl fmt::Debug for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

/// Host-wide bounds apply in addition to each provider's requested limits.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub max_concurrent_lookups: usize,
    pub max_concurrent_compilations: usize,
    pub max_staged_packages: usize,
    pub max_active_providers: usize,
    pub max_cache_entries: usize,
    pub max_compiled_bytes: usize,
    pub max_guest_memory_bytes: usize,
    pub max_request_bytes: usize,
    pub max_output_bytes: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_concurrent_lookups: 8,
            max_concurrent_compilations: 1,
            max_staged_packages: 16,
            max_active_providers: 32,
            max_cache_entries: 16,
            max_compiled_bytes: 128 * 1024 * 1024,
            max_guest_memory_bytes: 256 * 1024 * 1024,
            max_request_bytes: 1024 * 1024,
            max_output_bytes: 1024 * 1024,
        }
    }
}

impl RuntimeConfig {
    fn validate(&self) -> Result<(), RuntimeError> {
        if !(1..=64).contains(&self.max_concurrent_lookups)
            || !(1..=4).contains(&self.max_concurrent_compilations)
            || !(1..=128).contains(&self.max_staged_packages)
            || !(1..=64).contains(&self.max_active_providers)
            || !(1..=128).contains(&self.max_cache_entries)
            || !(1024 * 1024..=1024 * 1024 * 1024).contains(&self.max_compiled_bytes)
            || !(65_536..=1024 * 1024 * 1024).contains(&self.max_guest_memory_bytes)
            || !(1024..=8 * 1024 * 1024).contains(&self.max_request_bytes)
            || !(1024..=8 * 1024 * 1024).contains(&self.max_output_bytes)
        {
            return Err(RuntimeError::InvalidConfig);
        }
        Ok(())
    }
}

struct CompiledComponent {
    component: Component,
    _budget: OwnedSemaphorePermit,
}

struct LoadedProvider {
    manifest: ProviderManifest,
    pre: ProviderPre<HostState>,
    _compiled: Arc<CompiledComponent>,
}

/// Staging validates bytes and capabilities but does NOT authenticate signatures.
/// Only an update layer which has authenticated signed metadata should activate
/// downloaded packages. Fixture/development packages use this same explicit API.
pub struct StagedPackage {
    loaded: Arc<LoadedProvider>,
    runtime: Weak<Inner>,
    _slot: OwnedSemaphorePermit,
}

impl StagedPackage {
    pub fn manifest(&self) -> &ProviderManifest {
        &self.loaded.manifest
    }
}

impl fmt::Debug for StagedPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StagedPackage")
            .field("provider", &self.loaded.manifest.provider_id)
            .field("version", &self.loaded.manifest.version)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct ProviderHandle {
    loaded: Arc<LoadedProvider>,
    runtime: Weak<Inner>,
    generation: u64,
}

impl ProviderHandle {
    pub fn manifest(&self) -> &ProviderManifest {
        &self.loaded.manifest
    }
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

impl fmt::Debug for ProviderHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderHandle")
            .field("provider", &self.loaded.manifest.provider_id)
            .field("version", &self.loaded.manifest.version)
            .field("generation", &self.generation)
            .finish()
    }
}

struct ActiveProvider {
    current: Arc<LoadedProvider>,
    previous: Option<Arc<LoadedProvider>>,
    highest_version: Version,
    generation: u64,
}

#[derive(Default)]
struct Registry {
    active: BTreeMap<ProviderId, ActiveProvider>,
    cache: HashMap<ArtifactDigest, Arc<CompiledComponent>>,
    cache_order: Vec<ArtifactDigest>,
}

struct Inner {
    engine: Engine,
    transport: Arc<dyn HttpTransport>,
    host_version: Version,
    config: RuntimeConfig,
    registry: Mutex<Registry>,
    lookups: Arc<Semaphore>,
    compilations: Arc<Semaphore>,
    stages: Arc<Semaphore>,
    compiled_budget: Arc<Semaphore>,
    memory_budget: Arc<Semaphore>,
    stop: Arc<AtomicBool>,
    epoch_thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::Release);
        if let Some(handle) = self.epoch_thread.take() {
            handle.thread().unpark();
            let _ = handle.join();
        }
    }
}

#[derive(Clone)]
pub struct ProviderRuntime {
    inner: Arc<Inner>,
}

impl fmt::Debug for ProviderRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderRuntime")
            .field("host_version", &self.inner.host_version)
            .finish_non_exhaustive()
    }
}

impl ProviderRuntime {
    pub fn new(
        host_version: Version,
        transport: Arc<dyn HttpTransport>,
        config: RuntimeConfig,
    ) -> Result<Self, RuntimeError> {
        config.validate()?;
        let mut engine_config = Config::new();
        engine_config
            .wasm_component_model(true)
            .epoch_interruption(true);
        let engine = Engine::new(&engine_config).map_err(RuntimeError::Component)?;
        let stop = Arc::new(AtomicBool::new(false));
        let epoch_engine = engine.clone();
        let epoch_stop = stop.clone();
        let epoch_thread = thread::Builder::new()
            .name("stream-provider-epochs".into())
            .spawn(move || {
                while !epoch_stop.load(AtomicOrdering::Acquire) {
                    thread::park_timeout(Duration::from_millis(5));
                    epoch_engine.increment_epoch();
                }
            })
            .map_err(RuntimeError::Thread)?;
        Ok(Self {
            inner: Arc::new(Inner {
                engine,
                transport,
                host_version,
                lookups: Arc::new(Semaphore::new(config.max_concurrent_lookups)),
                compilations: Arc::new(Semaphore::new(config.max_concurrent_compilations)),
                stages: Arc::new(Semaphore::new(config.max_staged_packages)),
                compiled_budget: Arc::new(Semaphore::new(config.max_compiled_bytes / 1024)),
                memory_budget: Arc::new(Semaphore::new(config.max_guest_memory_bytes / 65_536)),
                config,
                registry: Mutex::new(Registry::default()),
                stop,
                epoch_thread: Some(epoch_thread),
            }),
        })
    }

    /// Compile trusted portable Wasm off the async executor. Native serialized
    /// artifacts are never accepted/deserialized. Compilation is never done by
    /// resolve(); holding a staged package reserves a bounded staging slot.
    pub async fn stage(
        &self,
        manifest: ProviderManifest,
        artifact: Vec<u8>,
    ) -> Result<StagedPackage, RuntimeError> {
        manifest.validate_for_host(&self.inner.host_version)?;
        if artifact.len() as u64 != manifest.artifact_bytes
            || digest(&artifact) != manifest.artifact_sha256.as_str()
        {
            return Err(RuntimeError::ArtifactMismatch);
        }
        let slot = self
            .inner
            .stages
            .clone()
            .try_acquire_owned()
            .map_err(|_| RuntimeError::Capacity)?;
        let cached = {
            let registry = self
                .inner
                .registry
                .lock()
                .map_err(|_| RuntimeError::Closed)?;
            registry.cache.get(&manifest.artifact_sha256).cloned()
        };
        let compiled = if let Some(cached) = cached {
            cached
        } else {
            let permit = self
                .inner
                .compilations
                .clone()
                .try_acquire_owned()
                .map_err(|_| RuntimeError::Capacity)?;
            let engine = self.inner.engine.clone();
            let (component, compiled_bytes) = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let component =
                    Component::new(&engine, &artifact).map_err(RuntimeError::Component)?;
                // Measured serialized generated-code size bounds retained cache
                // weight. It is generated locally and is never deserialized.
                let compiled_bytes = component
                    .serialize()
                    .map_err(RuntimeError::Component)?
                    .len();
                Ok::<_, RuntimeError>((component, compiled_bytes))
            })
            .await
            .map_err(|_| RuntimeError::Closed)??;
            let units = u32::try_from(compiled_bytes.div_ceil(1024).max(1))
                .map_err(|_| RuntimeError::Capacity)?;
            // An impossible-to-fit package must not evict useful cached code.
            // This is retained-code accounting, not a bound on compiler scratch
            // memory; compilation itself is separately concurrency-limited.
            if u64::from(units) > (self.inner.config.max_compiled_bytes / 1024) as u64 {
                return Err(RuntimeError::Capacity);
            }
            let mut registry = self
                .inner
                .registry
                .lock()
                .map_err(|_| RuntimeError::Closed)?;
            if let Some(cached) = registry.cache.get(&manifest.artifact_sha256) {
                cached.clone()
            } else {
                let reservation = loop {
                    match self
                        .inner
                        .compiled_budget
                        .clone()
                        .try_acquire_many_owned(units)
                    {
                        Ok(reservation) => break reservation,
                        Err(_) => {
                            if registry.cache_order.is_empty() {
                                return Err(RuntimeError::Capacity);
                            }
                            let oldest = registry.cache_order.remove(0);
                            registry.cache.remove(&oldest);
                        }
                    }
                };
                let compiled = Arc::new(CompiledComponent {
                    component,
                    _budget: reservation,
                });
                while registry.cache.len() >= self.inner.config.max_cache_entries {
                    if registry.cache_order.is_empty() {
                        return Err(RuntimeError::Capacity);
                    }
                    let oldest = registry.cache_order.remove(0);
                    registry.cache.remove(&oldest);
                }
                registry.cache_order.push(manifest.artifact_sha256.clone());
                registry
                    .cache
                    .insert(manifest.artifact_sha256.clone(), compiled.clone());
                compiled
            }
        };
        if compiled
            .component
            .component_type()
            .imports(&self.inner.engine)
            .any(|(name, _)| name != "stream-tv:provider/http@1.0.0")
        {
            return Err(RuntimeError::UnsupportedImport);
        }
        let resources = compiled
            .component
            .resources_required()
            .ok_or(RuntimeError::UnsupportedImport)?;
        if resources.num_memories > 1
            || resources.num_tables > 2
            || resources
                .max_initial_table_size
                .is_some_and(|size| size > 4096)
            || resources.max_initial_memory_size.is_some_and(|pages| {
                pages
                    .checked_mul(65_536)
                    .is_none_or(|bytes| bytes > manifest.limits.memory_bytes)
            })
        {
            return Err(RuntimeError::Capacity);
        }
        let mut linker = Linker::new(&self.inner.engine);
        Provider::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)
            .map_err(RuntimeError::Component)?;
        let pre = ProviderPre::new(
            linker
                .instantiate_pre(&compiled.component)
                .map_err(RuntimeError::Component)?,
        )
        .map_err(RuntimeError::Component)?;
        Ok(StagedPackage {
            loaded: Arc::new(LoadedProvider {
                manifest,
                pre,
                _compiled: compiled,
            }),
            runtime: Arc::downgrade(&self.inner),
            _slot: slot,
        })
    }

    /// Exercise a staged component against caller-owned fixture inputs before
    /// making it active. Compilation/linking alone do not establish protocol
    /// health. The current provider generation is unchanged on any failure.
    pub async fn resolve_staged(
        &self,
        package: &StagedPackage,
        lookup: &Lookup,
        cancellation: CancellationToken,
    ) -> Result<AdapterResult, RuntimeError> {
        if !Weak::ptr_eq(&package.runtime, &Arc::downgrade(&self.inner)) {
            return Err(RuntimeError::ForeignPackage);
        }
        let handle = ProviderHandle {
            loaded: package.loaded.clone(),
            runtime: package.runtime.clone(),
            generation: 0,
        };
        self.resolve_handle(&handle, lookup, cancellation).await
    }

    /// Activate a compatible staged package after the caller has authenticated
    /// its metadata and applied its fixture/health policy. Staging alone does
    /// not prove a changing upstream protocol still works.
    pub fn activate(&self, package: StagedPackage) -> Result<ProviderHandle, RuntimeError> {
        if !Weak::ptr_eq(&package.runtime, &Arc::downgrade(&self.inner)) {
            return Err(RuntimeError::ForeignPackage);
        }
        let mut registry = self
            .inner
            .registry
            .lock()
            .map_err(|_| RuntimeError::Closed)?;
        let id = package.loaded.manifest.provider_id.clone();
        let version = package.loaded.manifest.version.clone();
        let generation = if let Some(active) = registry.active.get_mut(&id) {
            if version.cmp_precedence(&active.highest_version) != Ordering::Greater {
                return Err(RuntimeError::VersionConflict);
            }
            let generation = active
                .generation
                .checked_add(1)
                .ok_or(RuntimeError::Capacity)?;
            active.previous = Some(active.current.clone());
            active.current = package.loaded.clone();
            active.highest_version = version;
            active.generation = generation;
            generation
        } else {
            if registry.active.len() >= self.inner.config.max_active_providers {
                return Err(RuntimeError::Capacity);
            }
            registry.active.insert(
                id,
                ActiveProvider {
                    current: package.loaded.clone(),
                    previous: None,
                    highest_version: version,
                    generation: 1,
                },
            );
            1
        };
        Ok(ProviderHandle {
            loaded: package.loaded,
            runtime: Arc::downgrade(&self.inner),
            generation,
        })
    }

    /// Restore an installer's trusted local journal into a fresh registry entry.
    /// This is not a download-policy or signature-verification entrypoint. The
    /// installer must authenticate cached packages before staging them and must
    /// preserve its durable version watermark. Deliberate local recovery may
    /// place a lower version in `active` while retaining a higher watermark.
    pub fn restore_installed(
        &self,
        active: StagedPackage,
        previous: Option<StagedPackage>,
        highest_version: Version,
    ) -> Result<ProviderHandle, RuntimeError> {
        let runtime = Arc::downgrade(&self.inner);
        if !Weak::ptr_eq(&active.runtime, &runtime)
            || previous
                .as_ref()
                .is_some_and(|package| !Weak::ptr_eq(&package.runtime, &runtime))
        {
            return Err(RuntimeError::ForeignPackage);
        }
        let id = active.loaded.manifest.provider_id.clone();
        if highest_version.cmp_precedence(&active.loaded.manifest.version) == Ordering::Less
            || previous.as_ref().is_some_and(|package| {
                package.loaded.manifest.provider_id != id
                    || highest_version.cmp_precedence(&package.loaded.manifest.version)
                        == Ordering::Less
            })
        {
            return Err(RuntimeError::InvalidInstalledState);
        }
        let mut registry = self
            .inner
            .registry
            .lock()
            .map_err(|_| RuntimeError::Closed)?;
        if registry.active.contains_key(&id) {
            return Err(RuntimeError::AlreadyInstalled);
        }
        if registry.active.len() >= self.inner.config.max_active_providers {
            return Err(RuntimeError::Capacity);
        }
        registry.active.insert(
            id,
            ActiveProvider {
                current: active.loaded.clone(),
                previous: previous.as_ref().map(|package| package.loaded.clone()),
                highest_version,
                generation: 1,
            },
        );
        Ok(ProviderHandle {
            loaded: active.loaded,
            runtime,
            generation: 1,
        })
    }

    pub fn snapshot(&self, provider: &ProviderId) -> Result<ProviderHandle, RuntimeError> {
        let registry = self
            .inner
            .registry
            .lock()
            .map_err(|_| RuntimeError::Closed)?;
        let active = registry
            .active
            .get(provider)
            .ok_or(RuntimeError::NotFound)?;
        Ok(ProviderHandle {
            loaded: active.current.clone(),
            runtime: Arc::downgrade(&self.inner),
            generation: active.generation,
        })
    }

    /// Capture one coherent generation of every installed provider. Callers
    /// retain these handles for a whole round, even if an update activates.
    pub fn snapshots(&self) -> Result<Vec<ProviderHandle>, RuntimeError> {
        let registry = self
            .inner
            .registry
            .lock()
            .map_err(|_| RuntimeError::Closed)?;
        Ok(registry
            .active
            .values()
            .map(|active| ProviderHandle {
                loaded: active.current.clone(),
                runtime: Arc::downgrade(&self.inner),
                generation: active.generation,
            })
            .collect())
    }

    /// Explicit local recovery. Upgrade anti-downgrade watermark is retained.
    /// Persisted signed metadata/version policy belongs to the update service.
    pub fn recover_previous(&self, provider: &ProviderId) -> Result<ProviderHandle, RuntimeError> {
        let mut registry = self
            .inner
            .registry
            .lock()
            .map_err(|_| RuntimeError::Closed)?;
        let active = registry
            .active
            .get_mut(provider)
            .ok_or(RuntimeError::NotFound)?;
        let generation = active
            .generation
            .checked_add(1)
            .ok_or(RuntimeError::Capacity)?;
        let previous = active
            .previous
            .take()
            .ok_or(RuntimeError::NoPreviousVersion)?;
        active.previous = Some(active.current.clone());
        active.current = previous;
        active.generation = generation;
        Ok(ProviderHandle {
            loaded: active.current.clone(),
            runtime: Arc::downgrade(&self.inner),
            generation,
        })
    }

    pub async fn resolve(
        &self,
        provider: &ProviderId,
        lookup: &Lookup,
        cancellation: CancellationToken,
    ) -> Result<AdapterResult, RuntimeError> {
        let handle = self.snapshot(provider)?;
        self.resolve_handle(&handle, lookup, cancellation).await
    }

    /// A pinned handle keeps old component code and manifest policy alive while
    /// another task activates a newer generation for subsequent lookups.
    pub async fn resolve_handle(
        &self,
        handle: &ProviderHandle,
        lookup: &Lookup,
        cancellation: CancellationToken,
    ) -> Result<AdapterResult, RuntimeError> {
        if !Weak::ptr_eq(&handle.runtime, &Arc::downgrade(&self.inner)) {
            return Err(RuntimeError::ForeignPackage);
        }
        lookup.validate().map_err(|_| RuntimeError::Contract)?;
        lookup
            .validate_context()
            .map_err(|_| RuntimeError::Contract)?;
        let lookup_json = serde_json::to_string(lookup).map_err(|_| RuntimeError::Contract)?;
        if lookup_json.len() > 32 * 1024 {
            return Err(RuntimeError::Contract);
        }
        let manifest = &handle.loaded.manifest;
        let deadline =
            Instant::now() + Duration::from_millis(u64::from(manifest.limits.timeout_ms));
        let cancellation = cancellation.child_token();
        let _guard = cancellation.clone().drop_guard();
        let memory_pages = u32::try_from(manifest.limits.memory_bytes.div_ceil(65_536))
            .map_err(|_| RuntimeError::Capacity)?;
        if manifest.limits.memory_bytes > self.inner.config.max_guest_memory_bytes as u64 {
            return Err(RuntimeError::Capacity);
        }
        let operation = async {
            let _lookup = self
                .inner
                .lookups
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| RuntimeError::Closed)?;
            let _memory = self
                .inner
                .memory_budget
                .clone()
                .acquire_many_owned(memory_pages)
                .await
                .map_err(|_| RuntimeError::Closed)?;
            let limits = StoreLimitsBuilder::new()
                .memory_size(
                    usize::try_from(manifest.limits.memory_bytes)
                        .map_err(|_| RuntimeError::Capacity)?,
                )
                .memories(1)
                .tables(2)
                .table_elements(4096)
                .instances(8)
                .trap_on_grow_failure(true)
                .build();
            let state = HostState {
                limits,
                transport: self.inner.transport.clone(),
                policy: HttpPolicy {
                    allowed_origins: manifest.allowed_origins.clone().into(),
                    context_key: lookup.context_key.clone().into(),
                    deadline,
                    max_response_bytes: manifest.limits.max_response_bytes as usize,
                },
                cancellation: cancellation.clone(),
                max_requests: manifest.limits.max_http_requests,
                requests: 0,
                request_bytes: 0,
                response_bytes: 0,
                response_metadata_bytes: 0,
                max_request_bytes: self.inner.config.max_request_bytes,
                clock_calls: 0,
                random_calls: 0,
                random_bytes: 0,
            };
            let mut store = Store::new(&self.inner.engine, state);
            store.limiter(|state| &mut state.limits);
            store.set_epoch_deadline(1);
            store.epoch_deadline_async_yield_and_update(1);
            let instance = handle
                .loaded
                .pre
                .instantiate_async(&mut store)
                .await
                .map_err(RuntimeError::Component)?;
            let output = instance
                .call_resolve(&mut store, &lookup_json)
                .await
                .map_err(RuntimeError::Component)?
                .map_err(|_| RuntimeError::ProviderFailure)?;
            if cancellation.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(RuntimeError::Timeout);
            }
            if output.len() > self.inner.config.max_output_bytes {
                return Err(RuntimeError::Capacity);
            }
            let result: AdapterResult =
                serde_json::from_str(&output).map_err(|_| RuntimeError::Contract)?;
            result
                .validate_for(&manifest.provider_id, &lookup.title, 32)
                .map_err(|_| RuntimeError::Contract)?;
            Ok(result)
        };
        tokio::select! {
            biased;
            _=cancellation.cancelled()=>Err(RuntimeError::Cancelled),
            result=timeout_at(deadline,operation)=>result.map_err(|_|RuntimeError::Timeout)?,
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    use std::fmt::Write;
    let mut result = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}
