use crate::catalog_service::CatalogWorker;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use stream_core::ports::{CatalogRepository, MediaEngine};
use stream_media::MediaRelay;
use stream_storage::{AccountStore, CatalogStore};

#[derive(Clone)]
pub struct AppState {
    pub catalog: Arc<CatalogStore>,
    pub accounts: Arc<AccountStore>,
    pub media: Arc<dyn MediaEngine>,
    pub relay: Arc<MediaRelay>,
    pub catalog_worker: Option<Arc<CatalogWorker>>,
    pub jellyfin: Option<Arc<crate::jellyfin::JellyfinBridge>>,
    catalog_generation: Arc<Mutex<HashMap<String, CatalogGeneration>>>,
    catalog_jobs: Arc<Mutex<HashMap<String, bool>>>,
    catalog_request_status: Arc<Mutex<HashMap<String, CatalogGeneration>>>,
    pub household_account_id: String,
    pub http_port: u16,
    pub started_at: std::time::Instant,
    playback_mutation_locks: Arc<PlaybackMutationLocks>,
}

#[derive(Default)]
struct PlaybackMutationLocks {
    locks: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
}

impl AppState {
    pub fn new(
        catalog: Arc<CatalogStore>,
        accounts: Arc<AccountStore>,
        media: Arc<dyn MediaEngine>,
        household_account_id: String,
    ) -> Self {
        Self {
            catalog: catalog.clone(),
            accounts,
            media,
            relay: Arc::new(MediaRelay::new()),
            catalog_worker: None,
            jellyfin: None,
            catalog_generation: Arc::new(Mutex::new(HashMap::new())),
            catalog_jobs: Arc::new(Mutex::new(HashMap::new())),
            catalog_request_status: Arc::new(Mutex::new(HashMap::new())),
            household_account_id,
            http_port: 8080,
            started_at: std::time::Instant::now(),
            playback_mutation_locks: Arc::new(PlaybackMutationLocks::default()),
        }
    }

    pub fn with_catalog_worker(mut self, worker: Option<Arc<CatalogWorker>>) -> Self {
        self.catalog_worker = worker;
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
    }

    pub fn with_jellyfin(mut self, bridge: Option<Arc<crate::jellyfin::JellyfinBridge>>) -> Self {
        self.jellyfin = bridge;
        self
    }

    pub fn set_catalog_generation(
        &self,
        profile_id: &str,
        status: CatalogGenerationStatus,
        reason: Option<String>,
    ) {
        let generation = CatalogGeneration {
            status,
            updated_at: Some(std::time::SystemTime::now().into()),
            reason,
            stale: false,
        };
        let mut entries = self
            .catalog_generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.len() >= 256
            && !entries.contains_key(profile_id)
            && let Some(oldest) = entries.keys().next().cloned()
        {
            entries.remove(&oldest);
        }
        entries.insert(profile_id.to_owned(), generation);
    }

    pub fn try_start_catalog_job(&self, key: &str) -> bool {
        let mut jobs = self
            .catalog_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(dirty) = jobs.get_mut(key) {
            *dirty = true;
            return false;
        }
        if jobs.len() >= 64 {
            return false;
        }
        jobs.insert(key.to_owned(), false);
        self.set_catalog_request_status(key, CatalogGenerationStatus::Pending, None);
        true
    }

    /// Returns whether a duplicate update arrived during the active job.
    /// Start work if absent without marking an in-flight job dirty. Use for status polling.
    pub fn ensure_catalog_job(&self, key: &str) -> bool {
        let mut jobs = self
            .catalog_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if jobs.contains_key(key) || jobs.len() >= 64 {
            return false;
        }
        jobs.insert(key.to_owned(), false);
        self.set_catalog_request_status(key, CatalogGenerationStatus::Pending, None);
        true
    }

    pub fn is_catalog_job_running(&self, key: &str) -> bool {
        self.catalog_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(key)
    }

    pub fn finish_catalog_job(&self, key: &str) -> bool {
        let mut jobs = self
            .catalog_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        jobs.remove(key).unwrap_or(false)
    }

    pub fn set_catalog_request_status(
        &self,
        key: &str,
        status: CatalogGenerationStatus,
        reason: Option<String>,
    ) {
        let mut states = self
            .catalog_request_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if states.len() >= 256
            && !states.contains_key(key)
            && let Some(oldest) = states.keys().next().cloned()
        {
            states.remove(&oldest);
        }
        states.insert(
            key.to_owned(),
            CatalogGeneration {
                status,
                updated_at: Some(std::time::SystemTime::now().into()),
                reason,
                stale: false,
            },
        );
    }

    pub fn catalog_request_status(&self, key: &str) -> Option<CatalogGeneration> {
        self.catalog_request_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
    }

    pub fn clear_profile_catalog_state(&self, profile_id: &str) {
        self.catalog_generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(profile_id);
        let prefix = format!("profile:{profile_id}:");
        self.catalog_request_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| !key.starts_with(&prefix));
    }

    pub fn set_catalog_generation_stale(&self, profile_id: &str, reason: Option<String>) {
        let generation = CatalogGeneration {
            status: CatalogGenerationStatus::Ready,
            updated_at: Some(std::time::SystemTime::now().into()),
            reason,
            stale: true,
        };
        let mut entries = self
            .catalog_generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.insert(profile_id.to_owned(), generation);
    }

    pub fn catalog_generation(&self, profile_id: &str) -> CatalogGeneration {
        let entries = self
            .catalog_generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.get(profile_id).cloned().unwrap_or_else(|| {
            if self
                .catalog_worker
                .as_ref()
                .is_some_and(|worker| worker.available())
            {
                CatalogGeneration {
                    status: CatalogGenerationStatus::Pending,
                    updated_at: None,
                    reason: None,
                    stale: false,
                }
            } else {
                CatalogGeneration {
                    status: CatalogGenerationStatus::Unavailable,
                    updated_at: None,
                    reason: Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                    stale: false,
                }
            }
        })
    }

    pub fn catalog_repo(&self) -> Arc<dyn CatalogRepository> {
        self.catalog.clone()
    }

    pub fn playback_mutation_lock(&self, playback_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = match self.playback_mutation_locks.locks.lock() {
            Ok(locks) => locks,
            Err(poisoned) => poisoned.into_inner(),
        };
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(playback_id).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(playback_id.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

/// Fallback / default media engine when external engine is not connected.
#[derive(Clone, Default)]
pub struct NoopMediaEngine;

#[async_trait::async_trait]
impl MediaEngine for NoopMediaEngine {
    async fn resolve_stream(
        &self,
        _lookup: &stream_core::providers::Lookup,
    ) -> Result<stream_core::providers::Selection, stream_core::error::DomainError> {
        Err(stream_core::error::DomainError::Infrastructure(
            "media engine not configured".into(),
        ))
    }
}

pub const CATALOG_CONFIG_GUIDANCE: &str =
    "Catalog access is unavailable. Try again or configure a personal TMDB key on your server.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogGenerationStatus {
    Pending,
    Ready,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CatalogGeneration {
    pub status: CatalogGenerationStatus,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reason: Option<String>,
    pub stale: bool,
}
