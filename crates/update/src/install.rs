//! Provider package installation, verification, and state tracking.

use std::{fs::File, path::PathBuf, sync::Arc};
use rusqlite::Connection;
use stream_core::providers::ProviderId;
use stream_provider_runtime::{ProviderHandle, ProviderRuntime};
use tokio::sync::Mutex;
use crate::{UpdateError, VerifiedProvider};

pub struct ProviderInstaller {
    db_path: PathBuf,
    artifacts_dir: PathBuf,
    runtime: ProviderRuntime,
    _process_lock: Arc<File>,
    operations: Arc<Mutex<()>>,
    provenance: Arc<()>,
}

impl std::fmt::Debug for ProviderInstaller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderInstaller")
            .field("db_path", &self.db_path)
            .field("artifacts_dir", &self.artifacts_dir)
            .finish_non_exhaustive()
    }
}

impl ProviderInstaller {
    pub async fn open(
        db_path: PathBuf,
        artifacts_dir: PathBuf,
        runtime: ProviderRuntime,
        process_lock: Arc<File>,
        operations: Arc<Mutex<()>>,
        provenance: Arc<()>,
    ) -> Result<Self, UpdateError> {
        {
            let _guard = operations.lock().await;
            let db_path_clone = db_path.clone();
            tokio::task::spawn_blocking(move || -> Result<(), UpdateError> {
                let conn = Connection::open(&db_path_clone).map_err(|e| UpdateError::Storage(e.into()))?;
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS installed_providers (
                        provider_id TEXT PRIMARY KEY,
                        version TEXT NOT NULL,
                        manifest TEXT NOT NULL,
                        artifact_digest TEXT NOT NULL,
                        installed_at INTEGER NOT NULL
                    );",
                ).map_err(|e| UpdateError::Storage(e.into()))?;
                Ok(())
            })
            .await
            .map_err(|_| UpdateError::Invalid)??;
        }

        Ok(Self {
            db_path,
            artifacts_dir,
            runtime,
            _process_lock: process_lock,
            operations,
            provenance,
        })
    }

    pub fn runtime(&self) -> &ProviderRuntime {
        &self.runtime
    }

    pub async fn verify_package(&self, provider: &VerifiedProvider) -> Result<(), UpdateError> {
        if !Arc::ptr_eq(&self.provenance, &provider.provenance) {
            return Err(UpdateError::Invalid);
        }
        provider.artifact.verify().await?;
        if provider.artifact.length() != provider.manifest.artifact_bytes
            || provider.artifact.sha256() != provider.manifest.artifact_sha256.as_str()
        {
            return Err(UpdateError::Changed);
        }
        Ok(())
    }

    pub async fn install(&self, provider: VerifiedProvider) -> Result<ProviderHandle, UpdateError> {
        let _guard = self.operations.lock().await;
        self.verify_package(&provider).await?;
        let manifest = provider.manifest().clone();
        let provider_id = manifest.provider_id.clone();
        let version_str = manifest.version.to_string();
        let digest_str = manifest.artifact_sha256.as_str().to_owned();
        let manifest_json = serde_json::to_string(&manifest)?;

        let staged = provider.stage(&self.runtime).await?;
        let handle = self.runtime.activate(staged)?;

        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<(), UpdateError> {
            let conn = Connection::open(&db_path).map_err(|e| UpdateError::Storage(e.into()))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            conn.execute(
                "INSERT OR REPLACE INTO installed_providers (provider_id, version, manifest, artifact_digest, installed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![provider_id.as_str(), version_str, manifest_json, digest_str, now],
            ).map_err(|e| UpdateError::Storage(e.into()))?;
            Ok(())
        })
        .await
        .map_err(|_| UpdateError::Invalid)??;

        Ok(handle)
    }

    pub async fn rollback(&self, provider_id: &ProviderId) -> Result<ProviderHandle, UpdateError> {
        let _guard = self.operations.lock().await;
        let handle = self.runtime.recover_previous(provider_id)?;
        Ok(handle)
    }

    pub async fn uninstall(&self, provider_id: &ProviderId) -> Result<(), UpdateError> {
        let _guard = self.operations.lock().await;
        let db_path = self.db_path.clone();
        let pid = provider_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), UpdateError> {
            let conn = Connection::open(&db_path).map_err(|e| UpdateError::Storage(e.into()))?;
            conn.execute(
                "DELETE FROM installed_providers WHERE provider_id = ?1",
                rusqlite::params![pid],
            ).map_err(|e| UpdateError::Storage(e.into()))?;
            Ok(())
        })
        .await
        .map_err(|_| UpdateError::Invalid)??;
        Ok(())
    }
}
