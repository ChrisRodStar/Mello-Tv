//! Public API contracts for profiles, playback and durable viewing progress.
//!
//! These wire DTOs mirror the Go API, rather than the iOS presentation models.
//! Call `validate` on incoming DTOs before mutating state. Session ownership,
//! monotonic counters, expiry and idempotency require storage/application checks.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::catalog::{Title, TitleIdentity};
use crate::domain::MediaType;

/// A bounded, non-secret explanation suitable for invalid request diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {field}: {reason}")]
pub struct AccountValidationError {
    pub field: &'static str,
    pub reason: &'static str,
}

fn invalid(field: &'static str, reason: &'static str) -> AccountValidationError {
    AccountValidationError { field, reason }
}

/// IDs shared by profile, playback, session and idempotency-key routes.
pub fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Content-type preference used by onboarding/recommendations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreferredFormat {
    Both,
    Movie,
    Tv,
}

/// Preferences persisted by the API. Liked titles are a separate list endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub completed: bool,
    pub format: PreferredFormat,
    #[serde(deserialize_with = "nullable_vec")]
    pub genres: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            completed: false,
            format: PreferredFormat::Both,
            genres: Vec::new(),
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        const GENRES: [&str; 9] = [
            "action",
            "comedy",
            "drama",
            "crime",
            "scifi",
            "animation",
            "documentary",
            "horror",
            "family",
        ];
        if self.genres.len() > GENRES.len() {
            return Err(invalid("genres", "at most nine genres are supported"));
        }
        let mut seen = HashSet::with_capacity(self.genres.len());
        for genre in &self.genres {
            if !GENRES.contains(&genre.as_str()) || !seen.insert(genre) {
                return Err(invalid("genres", "unknown or duplicate genre"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub color: String,
    pub preferences: Preferences,
}

impl Profile {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        if !is_safe_identifier(&self.id) {
            return Err(invalid("id", "expected 1–128 ASCII identifier characters"));
        }
        validate_profile_fields(&self.name, &self.color)?;
        self.preferences.validate()
    }
}

/// Request for both profile creation and profile name/color edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub name: String,
    pub color: String,
}

impl ProfileInput {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        validate_profile_fields(&self.name, &self.color)
    }
}

fn validate_profile_fields(name: &str, color: &str) -> Result<(), AccountValidationError> {
    // Go's len(string) counts UTF-8 bytes. Preserve its 80/32-byte limits.
    if name.trim().is_empty() || name.len() > 80 {
        return Err(invalid(
            "name",
            "expected a nonblank name of at most 80 bytes",
        ));
    }
    if color.is_empty() || color.len() > 32 {
        return Err(invalid("color", "expected 1–32 bytes"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilesResponse {
    pub profiles: Vec<Profile>,
}

/// Validated public content coordinates. Season zero identifies TV specials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TargetWire", into = "TargetWire")]
pub struct Target {
    identity: TitleIdentity,
    season: Option<i64>,
    episode: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TargetWire {
    #[serde(flatten)]
    identity: TitleIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    season: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    episode: Option<i64>,
}

impl Target {
    pub fn new(
        identity: TitleIdentity,
        season: Option<i64>,
        episode: Option<i64>,
    ) -> Result<Self, AccountValidationError> {
        match identity.media_type() {
            MediaType::Movie if season.is_none() && episode.is_none() => {}
            MediaType::Tv
                if season.is_some_and(|value| (0..=10_000).contains(&value))
                    && episode.is_some_and(|value| (1..=100_000).contains(&value)) => {}
            MediaType::Movie => {
                return Err(invalid("target", "movies cannot have episode coordinates"));
            }
            MediaType::Tv => {
                return Err(invalid(
                    "target",
                    "TV requires season 0–10000 and episode 1–100000",
                ));
            }
        }
        Ok(Self {
            identity,
            season,
            episode,
        })
    }

    pub fn identity(&self) -> &TitleIdentity {
        &self.identity
    }

    pub fn season(&self) -> Option<i64> {
        self.season
    }

    pub fn episode(&self) -> Option<i64> {
        self.episode
    }

    pub fn key(&self) -> String {
        match (self.season, self.episode) {
            (Some(season), Some(episode)) => {
                format!("{}:{season}:{episode}", self.identity.key())
            }
            _ => self.identity.key(),
        }
    }
}

impl TryFrom<TargetWire> for Target {
    type Error = AccountValidationError;

    fn try_from(value: TargetWire) -> Result<Self, Self::Error> {
        Self::new(value.identity, value.season, value.episode)
    }
}

impl From<Target> for TargetWire {
    fn from(value: Target) -> Self {
        Self {
            identity: value.identity,
            season: value.season,
            episode: value.episode,
        }
    }
}

/// Only formats accepted by the public playback API. Provider probing may
/// understand additional formats that are not playable by this client contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackFormat {
    Hls,
    Mp4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub formats: Vec<PlaybackFormat>,
    #[serde(
        default,
        deserialize_with = "nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub codecs: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_height: i64,
    #[serde(default)]
    pub allow_unknown_codecs: bool,
    #[serde(default)]
    pub allow_unknown_resolution: bool,
    #[serde(default)]
    pub supports_encrypted_hls: bool,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

impl Device {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        if self.formats.is_empty() || self.formats.len() > 2 {
            return Err(invalid("formats", "expected one or two supported formats"));
        }
        if self.codecs.len() > 16 || self.codecs.iter().any(|value| value.len() > 32) {
            return Err(invalid(
                "codecs",
                "expected at most 16 values of at most 32 bytes",
            ));
        }
        if !(0..=16_384).contains(&self.max_height) {
            return Err(invalid("max_height", "expected 0–16384"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackPreferences {
    #[serde(
        default,
        deserialize_with = "nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub audio_languages: Vec<String>,
    #[serde(
        default,
        deserialize_with = "nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub subtitle_languages: Vec<String>,
    #[serde(default)]
    pub require_audio: bool,
    #[serde(default)]
    pub require_subtitles: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_height: i64,
}

impl PlaybackPreferences {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        if self.max_height < 0 {
            return Err(invalid("max_height", "must be nonnegative"));
        }
        for values in [&self.audio_languages, &self.subtitle_languages] {
            if values.len() > 16 || values.iter().any(|value| value.len() > 32) {
                return Err(invalid(
                    "languages",
                    "expected at most 16 values of at most 32 bytes",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackRequest {
    #[serde(flatten)]
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferences: Option<PlaybackPreferences>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub imdb_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mal_id: Option<i64>,
}

impl PlaybackRequest {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        if let Some(device) = &self.device {
            device.validate()?;
        }
        if let Some(preferences) = &self.preferences {
            preferences.validate()?;
        }
        let imdb_digits = self.imdb_id.strip_prefix("tt");
        if !self.imdb_id.is_empty()
            && !imdb_digits.is_some_and(|digits| {
                !digits.is_empty()
                    && digits.len() <= 20
                    && digits.bytes().all(|value| value.is_ascii_digit())
            })
        {
            return Err(invalid(
                "imdb_id",
                "expected tt followed by 1–20 ASCII digits",
            ));
        }
        if self.mal_id.is_some_and(|value| value <= 0) {
            return Err(invalid("mal_id", "must be positive when present"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    #[serde(flatten)]
    pub target: Target,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub position_seconds: f64,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub duration_seconds: f64,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub watched_seconds: f64,
    pub completed: bool,
    pub updated_at: DateTime<Utc>,
}

impl Progress {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        validate_finite_nonnegative(self.position_seconds, "position_seconds")?;
        validate_finite_nonnegative(self.duration_seconds, "duration_seconds")?;
        // Stored watched_seconds accumulates across playback IDs and may exceed
        // the per-update one-week cap. Do not reject legitimate stored history.
        validate_finite_nonnegative(self.watched_seconds, "watched_seconds")?;
        if self.updated_at.timestamp() < 0
            || (self.updated_at.timestamp() == -62_135_596_800
                && self.updated_at.timestamp_subsec_nanos() == 0)
        {
            return Err(invalid("updated_at", "must not be before Unix epoch"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressUpdate {
    pub playback_id: String,
    pub sequence: i64,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub position_seconds: f64,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub duration_seconds: f64,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub watched_seconds: f64,
}

impl ProgressUpdate {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        if !is_safe_identifier(&self.playback_id) {
            return Err(invalid(
                "playback_id",
                "expected 1–128 ASCII identifier characters",
            ));
        }
        if self.sequence <= 0 {
            return Err(invalid("sequence", "must be positive"));
        }
        validate_finite_nonnegative(self.position_seconds, "position_seconds")?;
        validate_finite_nonnegative(self.duration_seconds, "duration_seconds")?;
        validate_finite_nonnegative(self.watched_seconds, "watched_seconds")?;
        if self.position_seconds > 86_400.0 || self.duration_seconds > 86_400.0 {
            return Err(invalid("position/duration", "must not exceed one day"));
        }
        if self.watched_seconds > 604_800.0 {
            return Err(invalid(
                "watched_seconds",
                "must not exceed one week per playback",
            ));
        }
        Ok(())
    }
}

fn validate_finite_nonnegative(
    value: f64,
    field: &'static str,
) -> Result<(), AccountValidationError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(invalid(field, "must be finite and nonnegative"))
    }
}

fn serialize_finite_nonnegative<S: Serializer>(
    value: &f64,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if !value.is_finite() || *value < 0.0 {
        return Err(serde::ser::Error::custom(
            "expected a finite nonnegative number",
        ));
    }
    serializer.serialize_f64(*value)
}

fn nullable_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub records: Vec<Progress>,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TitlesResponse {
    pub titles: Vec<Title>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelayState {
    Active,
    Draining,
    Expired,
    Revoked,
}

/// Private relay control response. Control revision and authorization revision
/// are independent and must not be substituted for each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayReady {
    #[serde(
        default,
        deserialize_with = "nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub available_heights: Vec<i64>,
    pub contract_version: String,
    pub session_id: String,
    pub entry_path: String,
    pub format: PlaybackFormat,
    pub expires_at: DateTime<Utc>,
    pub revision: u64,
    pub auth_revision: u64,
    pub source_generation: u64,
    pub state: RelayState,
    pub resume_required: bool,
}

impl RelayReady {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        validate_heights(&self.available_heights)?;
        if !self.available_heights.is_empty() && self.format != PlaybackFormat::Hls {
            return Err(invalid(
                "available_heights",
                "renditions require HLS format",
            ));
        }
        if self.contract_version != "relay.v1" {
            return Err(invalid("contract_version", "unsupported relay contract"));
        }
        if !is_safe_identifier(&self.session_id)
            || self.entry_path != format!("/stream/{}/entry", self.session_id)
        {
            return Err(invalid(
                "entry_path",
                "must identify this session's local relay entry",
            ));
        }
        if self.revision == 0 || self.auth_revision == 0 || self.source_generation == 0 {
            return Err(invalid(
                "revision",
                "relay revisions/generation must be positive",
            ));
        }
        if self.expires_at.timestamp() == -62_135_596_800
            && self.expires_at.timestamp_subsec_nanos() == 0
        {
            return Err(invalid(
                "expires_at",
                "Go zero timestamp is not a valid expiry",
            ));
        }
        Ok(())
    }
}

fn validate_heights(heights: &[i64]) -> Result<(), AccountValidationError> {
    if heights.len() > 32 {
        return Err(invalid(
            "available_heights",
            "at most 32 renditions are supported",
        ));
    }
    let mut seen = HashSet::with_capacity(heights.len());
    if heights
        .iter()
        .any(|height| !(1..=16_384).contains(height) || !seen.insert(*height))
    {
        return Err(invalid(
            "available_heights",
            "expected distinct heights in 1–16384",
        ));
    }
    Ok(())
}

/// Public signed playback URL response. Debug output deliberately omits the URL
/// because its query contains the playback grant.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorizedPlayback {
    pub id: String,
    pub title: Target,
    pub url: String,
    pub format: PlaybackFormat,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub available_heights: Vec<i64>,
    pub expires_at: DateTime<Utc>,
    pub revision: u64,
    pub source_generation: u64,
    pub reload_required: bool,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub resume_seconds: f64,
}

impl std::fmt::Debug for AuthorizedPlayback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthorizedPlayback")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("url", &"[REDACTED]")
            .field("format", &self.format)
            .field("revision", &self.revision)
            .field("source_generation", &self.source_generation)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackMutation {
    pub expected_revision: u64,
}

impl PlaybackMutation {
    pub fn validate(&self) -> Result<(), AccountValidationError> {
        if self.expected_revision == 0 {
            return Err(invalid("expected_revision", "must be positive"));
        }
        Ok(())
    }
}

/// Claims only: signature verification and trusted key selection remain in the
/// relay's authentication implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackGrantClaims {
    #[serde(rename = "iss")]
    pub issuer: String,
    #[serde(rename = "aud")]
    pub audience: String,
    #[serde(rename = "sub")]
    pub owner_id: String,
    #[serde(rename = "sid")]
    pub session_id: String,
    #[serde(rename = "arv")]
    pub auth_revision: u64,
    #[serde(rename = "iat")]
    pub issued_at: i64,
    #[serde(rename = "nbf")]
    pub not_before: i64,
    #[serde(rename = "exp")]
    pub expires_at: i64,
    #[serde(rename = "jti")]
    pub grant_id: String,
}
