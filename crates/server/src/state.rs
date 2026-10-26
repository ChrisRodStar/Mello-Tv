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
            catalog,
            accounts,
            media,
            relay: Arc::new(MediaRelay::new()),
            household_account_id,
            http_port: 8080,
            started_at: std::time::Instant::now(),
            playback_mutation_locks: Arc::new(PlaybackMutationLocks::default()),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
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
