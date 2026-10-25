//! Provider discovery contracts, independent of networking and persistence.
//!
//! These DTOs retain the `providers.v1` JSON names. Call `validate` at every
//! untrusted boundary before using a DTO. URL validation here is syntactic;
//! destination/DNS approval belongs to the transport at connection time.
//! Durations on the legacy wire are signed nanoseconds, never milliseconds.

mod identity;
mod lookup;
mod result;
mod source;

pub use identity::{ProviderId, ProviderTitle, TmdbId};
pub use lookup::{DeviceCapabilities, Lookup, QualityPreference};
pub use result::{
    AdapterResult, ErrorCategory, ProviderFailure, ProviderStatus, RankedSource, Selection,
};
pub use source::{
    CandidateStream, FetchContext, FetchScope, Metadata, ProbeObservation, ProbedStream,
    PublicResourceContext, StreamFormat, Track, Variant,
};

use thiserror::Error;

/// Current private provider contract, retained during the incremental migration.
pub const CONTRACT_VERSION: &str = "providers.v1";

/// Stable field-oriented validation failure without the rejected secret value.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("invalid provider field {field}: {reason}")]
pub struct ValidationError {
    pub field: &'static str,
    pub reason: &'static str,
}

impl ValidationError {
    pub(crate) const fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }
}

pub(crate) fn bounded(
    value: &str,
    min: usize,
    max: usize,
    field: &'static str,
) -> Result<(), ValidationError> {
    if value.len() < min || value.len() > max || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ValidationError::new(
            field,
            "length or control character is invalid",
        ));
    }
    Ok(())
}

pub(crate) fn strings(
    values: &[String],
    max_count: usize,
    field: &'static str,
) -> Result<(), ValidationError> {
    if values.len() > max_count {
        return Err(ValidationError::new(field, "too many entries"));
    }
    values
        .iter()
        .try_for_each(|value| bounded(value, 1, 64, field))
}

pub(crate) fn nonnegative(value: i64, field: &'static str) -> Result<(), ValidationError> {
    if value < 0 {
        return Err(ValidationError::new(field, "must be nonnegative"));
    }
    Ok(())
}

pub(crate) fn serialize_finite<S: serde::Serializer>(
    value: &f64,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if !value.is_finite() {
        return Err(serde::ser::Error::custom("nonfinite provider measurement"));
    }
    serializer.serialize_f64(*value)
}

pub(crate) fn serialize_optional_finite<S: serde::Serializer>(
    value: &Option<f64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serialize_finite(value, serializer),
        None => serializer.serialize_none(),
    }
}

pub(crate) fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer)
}
