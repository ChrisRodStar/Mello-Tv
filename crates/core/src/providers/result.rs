use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    CONTRACT_VERSION, CandidateStream, ProbedStream, ProviderId, ProviderTitle, ValidationError,
    bounded, nonnegative, required_nullable,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Unavailable,
    Unsupported,
    Timeout,
    #[serde(rename = "cancellation")]
    Cancelled,
    UpstreamFailure,
    ProtocolChange,
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancellation",
            Self::UpstreamFailure => "upstream_failure",
            Self::ProtocolChange => "protocol_change",
        })
    }
}

/// A sanitized wire failure. Internal source errors belong in typed host errors.
#[derive(Clone, Debug, Error, Eq, PartialEq, Serialize, Deserialize)]
#[error("{category}: {stage}: {detail}")]
pub struct ProviderFailure {
    pub category: ErrorCategory,
    pub stage: String,
    pub detail: String,
}

impl ProviderFailure {
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded(&self.stage, 1, 128, "failure.stage")?;
        bounded(&self.detail, 0, 2048, "failure.detail")
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ProviderFailure>,
    #[serde(deserialize_with = "required_nullable")]
    pub candidates: Option<Vec<CandidateStream>>,
    /// Legacy Go `time.Duration`, in nanoseconds.
    pub resolver_duration: i64,
}

impl AdapterResult {
    pub fn validate_for(
        &self,
        provider: &ProviderId,
        title: &ProviderTitle,
        candidate_limit: usize,
    ) -> Result<(), ValidationError> {
        nonnegative(self.resolver_duration, "resolver_duration")?;
        if self.issues.len() > 128
            || self
                .candidates
                .as_ref()
                .is_some_and(|items| items.len() > candidate_limit)
        {
            return Err(ValidationError::new(
                "adapter_result",
                "entry limit exceeded",
            ));
        }
        self.issues.iter().try_for_each(ProviderFailure::validate)?;
        let mut ids = BTreeSet::new();
        for candidate in self.candidates.iter().flatten() {
            if !ids.insert(candidate.id.as_str()) {
                return Err(ValidationError::new(
                    "candidate.id",
                    "duplicate candidate identity",
                ));
            }
            candidate.validate_for(provider, title)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ErrorCategory>,
    /// Legacy Go `time.Duration`, in nanoseconds.
    pub duration: i64,
    pub candidate_count: i64,
}

impl ProviderStatus {
    pub fn validate(&self) -> Result<(), ValidationError> {
        nonnegative(self.duration, "provider.duration")?;
        nonnegative(self.candidate_count, "provider.candidate_count")
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedSource {
    pub candidate: ProbedStream,
    #[serde(deserialize_with = "required_nullable")]
    pub reasons: Option<Vec<String>>,
    #[serde(serialize_with = "super::serialize_finite")]
    pub score: f64,
}

impl RankedSource {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.candidate.validate()?;
        if !self.score.is_finite() {
            return Err(ValidationError::new("score", "must be finite"));
        }
        if let Some(reasons) = &self.reasons {
            if reasons.len() > 64 {
                return Err(ValidationError::new("reasons", "too many reasons"));
            }
            for reason in reasons {
                bounded(reason, 0, 1024, "reason")?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    #[serde(deserialize_with = "required_nullable")]
    pub rejected: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<RankedSource>,
    pub version: String,
    #[serde(deserialize_with = "required_nullable")]
    pub backups: Option<Vec<RankedSource>>,
    #[serde(deserialize_with = "required_nullable")]
    pub providers: Option<Vec<ProviderStatus>>,
    #[serde(default, skip_serializing_if = "super::lookup::is_false")]
    pub pending: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            rejected: None,
            selected: None,
            version: CONTRACT_VERSION.to_owned(),
            backups: None,
            providers: None,
            pending: false,
        }
    }
}

impl Selection {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.version != CONTRACT_VERSION {
            return Err(ValidationError::new(
                "version",
                "unsupported provider contract",
            ));
        }
        if self.backups.as_ref().is_some_and(|items| items.len() > 128)
            || self
                .providers
                .as_ref()
                .is_some_and(|items| items.len() > 64)
            || self
                .rejected
                .as_ref()
                .is_some_and(|items| items.len() > 1024)
        {
            return Err(ValidationError::new("selection", "entry limit exceeded"));
        }
        if let Some(selected) = &self.selected {
            selected.validate()?;
        }
        let mut source_ids = BTreeSet::new();
        if let Some(selected) = &self.selected {
            source_ids.insert(selected.candidate.source.id.as_str());
        }
        for source in self.backups.iter().flatten() {
            source.validate()?;
            if !source_ids.insert(source.candidate.source.id.as_str()) {
                return Err(ValidationError::new(
                    "backups",
                    "duplicate selected/backup source identity",
                ));
            }
        }
        let mut provider_ids = BTreeSet::new();
        for status in self.providers.iter().flatten() {
            status.validate()?;
            if !provider_ids.insert(&status.provider) {
                return Err(ValidationError::new(
                    "providers",
                    "duplicate provider status",
                ));
            }
        }
        for (id, reason) in self.rejected.iter().flatten() {
            bounded(id, 1, 256, "rejected.id")?;
            bounded(reason, 0, 2048, "rejected.reason")?;
        }
        Ok(())
    }
}
