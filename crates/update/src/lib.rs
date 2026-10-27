#![allow(clippy::result_large_err)]

//! Authenticated update discovery and staging. TUF owns trust/expiry/rollback
//! verification; only fully verified artifacts become available for activation.
//! Installing/restarting an application is a separate supervised operation.

pub mod install;
pub mod transport;

use futures_util::StreamExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use stream_core::{
    packages::{ArtifactDigest, MAX_PROVIDER_ARTIFACT_BYTES, ProviderManifest},
    providers::ProviderId,
};
use stream_provider_runtime::{ProviderRuntime, RuntimeError, StagedPackage};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tough::{ExpirationEnforcement, Repository, RepositoryLoader, TargetName, Transport};
use url::Url;

const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_APPLICATION_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Error)]
pub enum UpdateError {
    #[error("invalid update configuration or release descriptor")]
    Invalid,
    #[error("another updater owns the metadata store")]
    Locked,
    #[error("update metadata or target verification failed")]
    Verification(#[from] tough::error::Error),
    #[error("update filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("update JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("update target is unavailable")]
    Missing,
    #[error("update target exceeds host policy")]
    Capacity,
    #[error("update artifact no longer matches its verified digest")]
    Changed,
    #[error("update operation was cancelled")]
    Cancelled,
    #[error("update operation timed out")]
    Timeout,
    #[error("provider update cannot be staged")]
    Runtime(#[from] RuntimeError),
    #[error("update state storage failed")]
    Storage(#[from] stream_storage::StorageError),
    #[error("update conflicts with installed state: {0}")]
    Conflict(&'static str),
    #[error("update state is inconsistent: {0}")]
    State(&'static str),
}
impl fmt::Debug for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// The caller supplies an out-of-band trusted root and an application-private
/// directory. There is deliberately no production root key or URL in this crate.
pub struct UpdateRepository {
    repository: Repository,
    artifacts: PathBuf,
    store: UpdateStore,
    downloads: tokio::sync::Semaphore,
}
impl fmt::Debug for UpdateRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateRepository").finish_non_exhaustive()
    }
}

/// Shared ownership of the private updater directory. The daemon opens this
/// once, restores installed modules offline, then refreshes TUF metadata through
/// the same handle; it never tries to reacquire its own process lock.
#[derive(Clone)]
pub struct UpdateStore {
    directory: PathBuf,
    process_lock: Arc<File>,
    operations: Arc<tokio::sync::Mutex<()>>,
    provenance: Arc<()>,
}
impl fmt::Debug for UpdateStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateStore").finish_non_exhaustive()
    }
}
impl UpdateStore {
    pub async fn open(directory: impl AsRef<Path>) -> Result<Self, UpdateError> {
        let directory = directory.as_ref().to_path_buf();
        if directory.as_os_str().is_empty() {
            return Err(UpdateError::Invalid);
        }
        let (directory, lock) = tokio::task::spawn_blocking(move || -> Result<_, UpdateError> {
            private_directory(&directory)?;
            let directory = std::fs::canonicalize(directory)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(directory.join("updater.lock"))?;
            lock.try_lock().map_err(|error| match error {
                std::fs::TryLockError::WouldBlock => UpdateError::Locked,
                std::fs::TryLockError::Error(error) => UpdateError::Io(error),
            })?;
            private_directory(&directory.join("metadata"))?;
            private_directory(&directory.join("artifacts"))?;
            Ok((directory, lock))
        })
        .await
        .map_err(|_| UpdateError::Invalid)??;
        Ok(Self {
            directory,
            process_lock: Arc::new(lock),
            operations: Arc::new(tokio::sync::Mutex::new(())),
            provenance: Arc::new(()),
        })
    }

    pub async fn installer(
        &self,
        runtime: ProviderRuntime,
    ) -> Result<install::ProviderInstaller, UpdateError> {
        install::ProviderInstaller::open(
            self.directory.join("updates.sqlite"),
            self.directory.join("artifacts"),
            runtime,
            self.process_lock.clone(),
            self.operations.clone(),
            self.provenance.clone(),
        )
        .await
    }
}

impl UpdateRepository {
    pub async fn open<T: Transport + 'static>(
        bootstrap_root: &[u8],
        metadata: Url,
        targets: Url,
        store: impl AsRef<Path>,
        transport: T,
    ) -> Result<Self, UpdateError> {
        Self::with_store(
            bootstrap_root,
            metadata,
            targets,
            UpdateStore::open(store).await?,
            transport,
        )
        .await
    }

    pub async fn with_store<T: Transport + 'static>(
        bootstrap_root: &[u8],
        metadata: Url,
        targets: Url,
        store: UpdateStore,
        transport: T,
    ) -> Result<Self, UpdateError> {
        if bootstrap_root.is_empty() || bootstrap_root.len() > MAX_METADATA_BYTES as usize {
            return Err(UpdateError::Invalid);
        }
        for url in [&metadata, &targets] {
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.path().ends_with('/')
            {
                return Err(UpdateError::Invalid);
            }
        }
        let operations = store.operations.clone();
        let guard = operations.lock().await;
        // This file is trusted local TUF state, protected like the persisted
        // rollback metadata. Never load a root from the downloaded targets tree.
        let root_path = store.directory.join("trusted-root.json");
        let root = match tokio::fs::read(&root_path).await {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => bootstrap_root.to_vec(),
            Err(error) => return Err(error.into()),
        };
        if root.len() > MAX_METADATA_BYTES as usize {
            return Err(UpdateError::Capacity);
        }
        let loader = RepositoryLoader::new(&root, metadata, targets)
            .transport(transport)
            .datastore(store.directory.join("metadata"))
            .expiration_enforcement(ExpirationEnforcement::Safe)
            .limits(tough::Limits {
                max_root_size: MAX_METADATA_BYTES,
                max_timestamp_size: MAX_METADATA_BYTES,
                max_snapshot_size: MAX_METADATA_BYTES,
                max_targets_size: MAX_METADATA_BYTES,
                max_root_updates: 128,
            })
            .load();
        let repository = tokio::time::timeout(std::time::Duration::from_secs(60), loader)
            .await
            .map_err(|_| UpdateError::Timeout)??;
        let trusted = serde_json::to_vec(repository.root())?;
        tokio::task::spawn_blocking(move || atomic_write(&root_path, &trusted))
            .await
            .map_err(|_| UpdateError::Invalid)??;
        drop(guard);
        Ok(Self {
            repository,
            artifacts: store.directory.join("artifacts"),
            store,
            downloads: tokio::sync::Semaphore::new(1),
        })
    }

    /// Target names are identifiers, never destination filesystem paths.
    pub async fn download(
        &self,
        name: &str,
        max_bytes: u64,
        cancel: &CancellationToken,
    ) -> Result<VerifiedArtifact, UpdateError> {
        let operation = async {
            let _download = self
                .downloads
                .acquire()
                .await
                .map_err(|_| UpdateError::Invalid)?;
            if max_bytes == 0 || max_bytes > MAX_APPLICATION_BYTES {
                return Err(UpdateError::Capacity);
            }
            let name = canonical_target(name)?;
            let metadata = self
                .repository
                .targets()
                .signed
                .find_target(&name, false)
                .map_err(|_| UpdateError::Missing)?;
            if metadata.length > max_bytes {
                return Err(UpdateError::Capacity);
            }
            let expected_length = metadata.length;
            let stream = self
                .repository
                .read_target(&name)
                .await?
                .ok_or(UpdateError::Missing)?;
            tokio::pin!(stream);
            let directory = self.artifacts.clone();
            let temporary =
                tokio::task::spawn_blocking(move || tempfile::NamedTempFile::new_in(directory))
                    .await
                    .map_err(|_| UpdateError::Invalid)??;
            let mut file = tokio::fs::File::from_std(temporary.reopen()?);
            let mut hash = Sha256::new();
            let mut length = 0u64;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                length = length
                    .checked_add(chunk.len() as u64)
                    .ok_or(UpdateError::Capacity)?;
                if length > max_bytes || length > expected_length {
                    return Err(UpdateError::Capacity);
                }
                hash.update(&chunk);
                file.write_all(&chunk).await?;
            }
            // read_target reports authentication failure at EOF. Reaching here
            // proves the whole stream, rather than just its first signed chunk.
            if length != expected_length {
                return Err(UpdateError::Changed);
            }
            file.sync_all().await?;
            drop(file);
            let digest = hex(&hash.finalize());
            let destination = self.artifacts.join(&digest);
            let persisted = destination.clone();
            tokio::task::spawn_blocking(move || -> Result<(), UpdateError> {
                match temporary.persist_noclobber(&persisted) {
                    Ok(_) => Ok(()),
                    Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
                    Err(error) => Err(error.error.into()),
                }
            })
            .await
            .map_err(|_| UpdateError::Invalid)??;
            let artifact = VerifiedArtifact {
                path: destination,
                digest,
                length,
                target: Arc::from(name.raw()),
            };
            artifact.verify().await?;
            Ok(artifact)
        };
        tokio::select! {biased;_=cancel.cancelled()=>Err(UpdateError::Cancelled),result=tokio::time::timeout(std::time::Duration::from_secs(300),operation)=>result.map_err(|_|UpdateError::Timeout)?,}
    }

    pub async fn provider(
        &self,
        manifest_target: &str,
        artifact_target: &str,
        expected: &ProviderId,
        host: &Version,
        cancel: &CancellationToken,
    ) -> Result<VerifiedProvider, UpdateError> {
        let descriptor = self.download(manifest_target, 64 * 1024, cancel).await?;
        let manifest: ProviderManifest =
            serde_json::from_slice(&descriptor.read(64 * 1024).await?)?;
        manifest
            .validate_for_host(host)
            .map_err(|_| UpdateError::Invalid)?;
        if &manifest.provider_id != expected {
            return Err(UpdateError::Invalid);
        }
        let artifact = self
            .download(artifact_target, MAX_PROVIDER_ARTIFACT_BYTES, cancel)
            .await?;
        if artifact.length != manifest.artifact_bytes
            || artifact.digest != manifest.artifact_sha256.as_str()
        {
            return Err(UpdateError::Changed);
        }
        Ok(VerifiedProvider {
            manifest,
            artifact,
            provenance: self.store.provenance.clone(),
        })
    }

    pub async fn application(
        &self,
        descriptor_target: &str,
        artifact_target: &str,
        expected_target: &str,
        schemas: InstalledSchemas,
        cancel: &CancellationToken,
    ) -> Result<VerifiedApplication, UpdateError> {
        let descriptor = self.download(descriptor_target, 64 * 1024, cancel).await?;
        let release: ApplicationRelease =
            serde_json::from_slice(&descriptor.read(64 * 1024).await?)?;
        release.validate_for(expected_target, schemas)?;
        let artifact = self
            .download(artifact_target, MAX_APPLICATION_BYTES, cancel)
            .await?;
        if artifact.length != release.artifact_bytes
            || artifact.digest != release.artifact_sha256.as_str()
        {
            return Err(UpdateError::Changed);
        }
        Ok(VerifiedApplication { release, artifact })
    }
}

/// Created only after a complete authenticated target stream is staged. The
/// digest is rechecked before use because filesystem content can later change.
pub struct VerifiedArtifact {
    path: PathBuf,
    digest: String,
    length: u64,
    target: Arc<str>,
}
impl fmt::Debug for VerifiedArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedArtifact")
            .field("bytes", &self.length)
            .finish_non_exhaustive()
    }
}
impl VerifiedArtifact {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn sha256(&self) -> &str {
        &self.digest
    }
    pub fn length(&self) -> u64 {
        self.length
    }
    pub async fn verify(&self) -> Result<(), UpdateError> {
        let mut file = tokio::fs::File::open(&self.path).await?;
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let size = file.read(&mut buffer).await?;
            if size == 0 {
                break;
            }
            total = total.checked_add(size as u64).ok_or(UpdateError::Changed)?;
            if total > self.length {
                return Err(UpdateError::Changed);
            }
            hash.update(&buffer[..size]);
        }
        if total != self.length || hex(&hash.finalize()) != self.digest {
            return Err(UpdateError::Changed);
        }
        Ok(())
    }
    pub async fn read(&self, max_bytes: u64) -> Result<Vec<u8>, UpdateError> {
        if self.length > max_bytes {
            return Err(UpdateError::Capacity);
        }
        let file = tokio::fs::File::open(&self.path).await?;
        let mut bytes = Vec::new();
        file.take(self.length.saturating_add(1))
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() as u64 != self.length || hex(&Sha256::digest(&bytes)) != self.digest {
            return Err(UpdateError::Changed);
        }
        Ok(bytes)
    }
}

pub struct VerifiedProvider {
    manifest: ProviderManifest,
    artifact: VerifiedArtifact,
    provenance: Arc<()>,
}
impl VerifiedProvider {
    pub fn manifest(&self) -> &ProviderManifest {
        &self.manifest
    }
    pub async fn stage(self, runtime: &ProviderRuntime) -> Result<StagedPackage, UpdateError> {
        let bytes = self.artifact.read(MAX_PROVIDER_ARTIFACT_BYTES).await?;
        Ok(runtime.stage(self.manifest, bytes).await?)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InstalledSchemas {
    pub api: i64,
    pub catalog: i64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaRange {
    pub minimum: i64,
    pub maximum: i64,
}
impl SchemaRange {
    fn contains(self, value: i64) -> bool {
        self.minimum >= 0
            && self.maximum >= self.minimum
            && (self.minimum..=self.maximum).contains(&value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationRelease {
    pub schema_version: u16,
    pub version: Version,
    /// Exact Rust target triple, including platform ABI.
    pub target: String,
    pub artifact_sha256: ArtifactDigest,
    pub artifact_bytes: u64,
    pub api_schema: SchemaRange,
    pub catalog_schema: SchemaRange,
}
impl ApplicationRelease {
    pub fn validate_for(&self, target: &str, schemas: InstalledSchemas) -> Result<(), UpdateError> {
        if self.schema_version != 1
            || self.target != target
            || self.target.is_empty()
            || self.target.len() > 80
            || !self.target.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
            || self.artifact_bytes == 0
            || self.artifact_bytes > MAX_APPLICATION_BYTES
            || !self.api_schema.contains(schemas.api)
            || !self.catalog_schema.contains(schemas.catalog)
        {
            return Err(UpdateError::Invalid);
        }
        Ok(())
    }
}
pub struct VerifiedApplication {
    release: ApplicationRelease,
    artifact: VerifiedArtifact,
}
impl VerifiedApplication {
    pub fn release(&self) -> &ApplicationRelease {
        &self.release
    }
    pub fn artifact(&self) -> &VerifiedArtifact {
        &self.artifact
    }
}

fn canonical_target(value: &str) -> Result<TargetName, UpdateError> {
    if value.is_empty()
        || value.len() > 256
        || value.contains(['\\', ':', '%'])
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(UpdateError::Invalid);
    }
    let name = TargetName::new(value)?;
    if name.raw() != name.resolved()
        || value.starts_with('/')
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(UpdateError::Invalid);
    }
    Ok(name)
}
fn private_directory(path: &Path) -> Result<(), std::io::Error> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    let directory = path.parent().ok_or(UpdateError::Invalid)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| UpdateError::Io(error.error))?;
    Ok(())
}
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(value, "{byte:02x}");
    }
    value
}
