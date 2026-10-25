//! Untrusted provider package metadata. Validation does not authenticate a package;
//! the update service must verify signed metadata and artifact bytes before loading.

use std::{collections::BTreeSet, fmt};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::providers::ProviderId;

pub const PROVIDER_INTERFACE_MAJOR: u16 = 1;
pub const PROVIDER_INTERFACE_MINOR: u16 = 0;
pub const MAX_PROVIDER_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PROVIDER_MEMORY_BYTES: u64 = 128 * 1024 * 1024;

/// Canonical lowercase SHA-256 digest from authenticated target metadata.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ArtifactDigest(String);

impl ArtifactDigest {
    pub fn new(value: impl Into<String>) -> Result<Self, PackageError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(PackageError::InvalidDigest);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ArtifactDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ArtifactDigest")
            .field(&self.0)
            .finish()
    }
}

impl TryFrom<String> for ArtifactDigest {
    type Error = PackageError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ArtifactDigest> for String {
    fn from(value: ArtifactDigest) -> Self {
        value.0
    }
}

/// Exact origin capability; the runtime must still validate DNS and every redirect.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AllowedOrigin(String);

impl AllowedOrigin {
    pub fn new(value: impl Into<String>) -> Result<Self, PackageError> {
        let value = value.into();
        if value.len() > 2048
            || value.trim() != value
            || value.contains('\\')
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(PackageError::InvalidOrigin);
        }
        let parsed = Url::parse(&value).map_err(|_| PackageError::InvalidOrigin)?;
        if !matches!(parsed.scheme(), "https" | "http")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(PackageError::InvalidOrigin);
        }
        Ok(Self(parsed.origin().ascii_serialization()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AllowedOrigin {
    type Error = PackageError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<AllowedOrigin> for String {
    fn from(value: AllowedOrigin) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub memory_bytes: u64,
    pub max_http_requests: u16,
    pub max_response_bytes: u32,
    pub timeout_ms: u32,
}

impl ResourceLimits {
    pub fn validate(&self) -> Result<(), PackageError> {
        if self.memory_bytes == 0
            || self.memory_bytes > MAX_PROVIDER_MEMORY_BYTES
            || self.max_http_requests == 0
            || self.max_http_requests > 128
            || self.max_response_bytes == 0
            || self.max_response_bytes > 8 * 1024 * 1024
            || self.timeout_ms == 0
            || self.timeout_ms > 60_000
        {
            return Err(PackageError::ResourceLimit);
        }
        Ok(())
    }
}

/// Package versions evolve independently from the application and interface.
/// This manifest grants no authority until the host authenticates and approves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderManifest {
    pub schema_version: u16,
    pub provider_id: ProviderId,
    pub version: Version,
    pub host_version: VersionReq,
    pub interface: InterfaceVersion,
    pub artifact_sha256: ArtifactDigest,
    pub artifact_bytes: u64,
    pub allowed_origins: Vec<AllowedOrigin>,
    pub limits: ResourceLimits,
}

impl ProviderManifest {
    pub fn validate_for_host(&self, host_version: &Version) -> Result<(), PackageError> {
        if self.schema_version != 1 {
            return Err(PackageError::UnsupportedSchema(self.schema_version));
        }
        if self.interface.major != PROVIDER_INTERFACE_MAJOR
            || self.interface.minor > PROVIDER_INTERFACE_MINOR
        {
            return Err(PackageError::IncompatibleInterface);
        }
        if !self.host_version.matches(host_version) {
            return Err(PackageError::IncompatibleHost);
        }
        if self.artifact_bytes == 0 || self.artifact_bytes > MAX_PROVIDER_ARTIFACT_BYTES {
            return Err(PackageError::ArtifactSize);
        }
        if self.allowed_origins.is_empty() || self.allowed_origins.len() > 32 {
            return Err(PackageError::OriginCount);
        }
        let unique: BTreeSet<_> = self.allowed_origins.iter().collect();
        if unique.len() != self.allowed_origins.len() {
            return Err(PackageError::DuplicateOrigin);
        }
        self.limits.validate()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PackageError {
    #[error("artifact digest must contain 64 lowercase hexadecimal characters")]
    InvalidDigest,
    #[error("capability must be an HTTP(S) origin without credentials, path, query or fragment")]
    InvalidOrigin,
    #[error("unsupported provider manifest schema {0}")]
    UnsupportedSchema(u16),
    #[error("provider interface is incompatible with this host")]
    IncompatibleInterface,
    #[error("provider package does not support this host version")]
    IncompatibleHost,
    #[error("provider artifact size is outside host limits")]
    ArtifactSize,
    #[error("provider must request between one and 32 network origins")]
    OriginCount,
    #[error("provider contains duplicate normalized origins")]
    DuplicateOrigin,
    #[error("provider resource request is outside host limits")]
    ResourceLimit,
}
