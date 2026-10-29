use crate::error::DomainError;
use crate::providers::{Lookup, Selection};
use async_trait::async_trait;
use serde::Serialize;

/// Operational metadata only: no title coordinates, credentials, or source URLs.
#[derive(Clone, Debug, Default, Serialize)]
pub struct MediaDiagnostics {
    pub configured: bool,
    pub providers: Vec<ProviderDiagnostic>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderDiagnostic {
    pub id: String,
    pub version: String,
    pub last_observed_at: Option<i64>,
    pub last_failure: Option<crate::providers::ErrorCategory>,
    pub candidate_count: Option<i64>,
}

/// Independent resolver and media-access evidence. Never contains source URLs
/// or credentials, and does not assert device playback or every-title coverage.
#[derive(Clone, Debug, Serialize)]
pub struct ProviderHealth {
    pub id: String,
    pub version: String,
    pub working: bool,
    pub checked_at: i64,
    pub elapsed_ms: u64,
    pub samples_checked: u8,
    pub detail: String,
}

#[async_trait]
pub trait MediaEngine: Send + Sync {
    fn diagnostics(&self) -> MediaDiagnostics {
        MediaDiagnostics::default()
    }

    async fn check_provider_health(&self) -> Result<Vec<ProviderHealth>, DomainError> {
        Err(DomainError::Validation(
            "Provider checks are unavailable".into(),
        ))
    }

    /// Resolve using title coordinates, device/preferences and an ingress-owned
    /// credential partition. Pending startup selections retain their full evidence.
    async fn resolve_stream(&self, lookup: &Lookup) -> Result<Selection, DomainError>;
}
