use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::{
    ErrorCategory, ProviderId, ProviderTitle, ValidationError, bounded, nonnegative, strings,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamFormat {
    #[default]
    Unknown,
    Hls,
    Dash,
    Mp4,
}

/// Credentials remain scoped to an origin and an unambiguous path boundary.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FetchScope {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    pub origin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookies: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookie_path: String,
}

impl fmt::Debug for FetchScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FetchScope")
            .field("credentials", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublicResourceContext {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub referer: String,
}

impl fmt::Debug for PublicResourceContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PublicResourceContext { [REDACTED] }")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FetchContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<FetchScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_resources: Option<PublicResourceContext>,
}

/// Syntax validation only; this does not authorize a network destination.
pub fn validate_source_url(value: &str) -> Result<Url, ValidationError> {
    bounded(value, 1, 8192, "url")?;
    if value.contains('\\') || value.trim() != value {
        return Err(ValidationError::new("url", "ambiguous URL syntax"));
    }
    let parsed = Url::parse(value)
        .map_err(|_| ValidationError::new("url", "expected an absolute HTTP URL"))?;
    if !matches!(parsed.scheme(), "https" | "http")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ValidationError::new(
            "url",
            "HTTP host required; credentials and fragments forbidden",
        ));
    }
    Ok(parsed)
}

fn root_origin(value: &str) -> Result<(), ValidationError> {
    let url = validate_source_url(value)?;
    if url.path() != "/" || url.query().is_some() {
        return Err(ValidationError::new("origin", "expected a root origin"));
    }
    Ok(())
}

fn scope_path(value: &str) -> Result<(), ValidationError> {
    if !value.starts_with('/')
        || value.contains(['?', '#', '\\', '%'])
        || value.contains("//")
        || value
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
    {
        return Err(ValidationError::new(
            "scope_path",
            "expected an unambiguous absolute path",
        ));
    }
    bounded(value, 1, 2048, "scope_path")
}

fn header_value(value: &str) -> Result<(), ValidationError> {
    if value.len() > 8192
        || value
            .bytes()
            .any(|byte| (byte < 32 && byte != b'\t') || byte == 127)
    {
        return Err(ValidationError::new("header", "invalid header value"));
    }
    Ok(())
}

impl FetchContext {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.scopes.len() > 16 {
            return Err(ValidationError::new(
                "fetch_context",
                "too many credential scopes",
            ));
        }
        if let Some(public) = &self.public_resources {
            for value in [&public.origin, &public.referer] {
                if !value.is_empty() {
                    root_origin(value)?;
                }
            }
        }
        let mut total_bytes = 0usize;
        for scope in &self.scopes {
            root_origin(&scope.origin)?;
            for value in [&scope.path_prefix, &scope.cookie_path] {
                if !value.is_empty() {
                    scope_path(value)?;
                }
            }
            if !scope.cookies.is_empty() && scope.cookie_path.is_empty() {
                return Err(ValidationError::new(
                    "cookie_path",
                    "cookie credentials require an explicit path",
                ));
            }
            header_value(&scope.cookies)?;
            total_bytes += scope.cookies.len();
            if scope.headers.len() > 64 {
                return Err(ValidationError::new("headers", "too many headers"));
            }
            let mut names = BTreeSet::new();
            for (name, value) in &scope.headers {
                let canonical = name.to_ascii_lowercase();
                if !names.insert(canonical.clone())
                    || !matches!(
                        canonical.as_str(),
                        "user-agent"
                            | "accept"
                            | "accept-language"
                            | "referer"
                            | "origin"
                            | "authorization"
                            | "x-playback-environment"
                            | "x-csrf-token"
                    )
                {
                    return Err(ValidationError::new(
                        "headers",
                        "duplicate or unsupported header",
                    ));
                }
                header_value(value)?;
                if matches!(canonical.as_str(), "origin" | "referer") {
                    validate_source_url(value)?;
                }
                total_bytes += name.len() + value.len();
            }
        }
        if total_bytes > 16384 {
            return Err(ValidationError::new(
                "fetch_context",
                "credential byte budget exceeded",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default)]
    pub default: bool,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub audio_group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtitle_group: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codecs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
}

impl Variant {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_source_url(&self.url)?;
        strings(&self.codecs, 32, "variant.codecs")?;
        bounded(&self.audio_group, 0, 256, "audio_group")?;
        bounded(&self.subtitle_group, 0, 256, "subtitle_group")?;
        for value in [self.bitrate, self.width, self.height]
            .into_iter()
            .flatten()
        {
            nonnegative(value, "variant dimensions/bitrate")?;
        }
        Ok(())
    }
}

/// Resolver claims remain distinct from measured playlist facts.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_data: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<Track>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Variant>,
    #[serde(default)]
    pub tracks_known: bool,
}

impl Metadata {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.provider_data.len() > 128 || self.tracks.len() > 128 || self.variants.len() > 128 {
            return Err(ValidationError::new("metadata", "entry limit exceeded"));
        }
        for key in self.provider_data.keys() {
            bounded(key, 1, 128, "provider_data key")?;
        }
        for track in &self.tracks {
            if !matches!(track.kind.as_str(), "audio" | "subtitles") {
                return Err(ValidationError::new(
                    "track.kind",
                    "expected audio or subtitles",
                ));
            }
            bounded(&track.language, 0, 64, "track.language")?;
            bounded(&track.name, 0, 256, "track.name")?;
            bounded(&track.group, 0, 256, "track.group")?;
            if !track.url.is_empty() {
                validate_source_url(&track.url)?;
            }
        }
        self.variants.iter().try_for_each(Variant::validate)
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateStream {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub server: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub selected_variant_url: String,
    pub url: String,
    pub provider: ProviderId,
    pub format: StreamFormat,
    pub title: ProviderTitle,
    pub fetch_context: FetchContext,
    pub resolver_metadata: Metadata,
    #[serde(default)]
    pub content_verified: bool,
}

impl fmt::Debug for CandidateStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CandidateStream")
            .field("provider", &self.provider)
            .field("format", &self.format)
            .field("title", &self.title)
            .field("upstream", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl CandidateStream {
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded(&self.id, 1, 256, "candidate.id")?;
        bounded(&self.server, 0, 256, "candidate.server")?;
        validate_source_url(&self.url)?;
        if !self.selected_variant_url.is_empty() {
            validate_source_url(&self.selected_variant_url)?;
        }
        self.title.validate()?;
        self.fetch_context.validate()?;
        self.resolver_metadata.validate()
    }

    /// Validate adapter ownership and exact requested content before probing.
    pub fn validate_for(
        &self,
        provider: &ProviderId,
        title: &ProviderTitle,
    ) -> Result<(), ValidationError> {
        self.validate()?;
        if &self.provider != provider || !self.title.compatible_with(title) {
            return Err(ValidationError::new(
                "candidate",
                "provider or title ownership mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeObservation {
    pub at: DateTime<Utc>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "super::serialize_optional_finite"
    )]
    pub delivery_bps: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "super::serialize_optional_finite"
    )]
    pub reliability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<Variant>,
    /// Legacy Go `time.Duration`, in nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_byte: Option<i64>,
    pub sample_bytes: i64,
    pub format: StreamFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ErrorCategory>,
    pub playlist_metadata: Metadata,
    pub samples_completed: i64,
    pub accessible: bool,
    pub encrypted: bool,
    pub protection_known: bool,
}

impl ProbeObservation {
    /// Structural measurement validity, not playback eligibility or freshness.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self
            .delivery_bps
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err(ValidationError::new(
                "delivery_bps",
                "must be finite and nonnegative",
            ));
        }
        if self
            .reliability
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(ValidationError::new(
                "reliability",
                "must be finite and in 0..=1",
            ));
        }
        if let Some(value) = self.first_byte {
            nonnegative(value, "first_byte")?;
        }
        nonnegative(self.sample_bytes, "sample_bytes")?;
        nonnegative(self.samples_completed, "samples_completed")?;
        if let Some(variant) = &self.variant {
            variant.validate()?;
        }
        self.playlist_metadata.validate()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbedStream {
    pub source: CandidateStream,
    pub observation: ProbeObservation,
}

impl ProbedStream {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.source.validate()?;
        self.observation.validate()
    }
}
