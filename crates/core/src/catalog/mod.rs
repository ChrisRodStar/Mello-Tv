//! Catalog and native-client wire contracts preserved from the Go services.
//!
//! [`TitleIdentity`] represents a catalog item, not an episode or a provider
//! lookup. Optional external IDs and episode coordinates retain their existing
//! wire names so stored snapshots and native clients can migrate independently.

use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

pub use crate::domain::MediaType;

/// Existing catalog snapshot version; this is distinct from provider contracts.
pub const SNAPSHOT_VERSION: &str = "1";

/// Stable genre slugs and display labels used by onboarding and discovery.
pub const CANONICAL_GENRES: [(&str, &str); 9] = [
    ("action", "Action & adventure"),
    ("comedy", "Comedy"),
    ("drama", "Drama"),
    ("crime", "Crime & mystery"),
    ("scifi", "Sci-fi & fantasy"),
    ("animation", "Anime"),
    ("documentary", "Documentaries"),
    ("horror", "Horror"),
    ("family", "Family"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CatalogValidationError {
    #[error("TMDB identifiers must be positive")]
    InvalidTitleId,
    #[error("invalid catalog field: {0}")]
    InvalidField(&'static str),
    #[error("catalog input limit exceeded: {0}")]
    LimitExceeded(&'static str),
    #[error("duplicate catalog input: {0}")]
    Duplicate(&'static str),
    #[error("unsupported catalog snapshot version")]
    UnsupportedSnapshotVersion,
}

/// A validated `(type, positive TMDB ID)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TitleIdentity {
    #[serde(rename = "type")]
    media_type: MediaType,
    id: i64,
}

impl TitleIdentity {
    pub fn new(media_type: MediaType, id: i64) -> Result<Self, CatalogValidationError> {
        if id <= 0 {
            return Err(CatalogValidationError::InvalidTitleId);
        }
        Ok(Self { media_type, id })
    }

    pub const fn media_type(self) -> MediaType {
        self.media_type
    }

    pub const fn id(self) -> i64 {
        self.id
    }

    /// Canonical storage key, such as `movie:27205` or `tv:60625`.
    pub fn key(self) -> String {
        self.to_string()
    }
}

impl fmt::Display for TitleIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let media_type = match self.media_type {
            MediaType::Movie => "movie",
            MediaType::Tv => "tv",
        };
        write!(formatter, "{media_type}:{}", self.id)
    }
}

impl<'de> Deserialize<'de> for TitleIdentity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct WireIdentity {
            #[serde(rename = "type")]
            media_type: MediaType,
            id: i64,
        }
        let wire = WireIdentity::deserialize(deserializer)?;
        Self::new(wire.media_type, wire.id).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonSummary {
    pub season: i32,
    pub name: String,
    pub episode_count: i32,
}

impl SeasonSummary {
    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        if self.season < 0 || self.episode_count < 0 {
            return Err(CatalogValidationError::InvalidField("season summary"));
        }
        Ok(())
    }
}

/// Catalog/API title payload. Artwork fields contain URLs, not local paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Title {
    #[serde(flatten)]
    pub identity: TitleIdentity,
    pub title: String,
    pub overview: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub rating: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub poster: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub backdrop: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub imdb_id: String,
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub mal_id: i64,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub runtime: i32,
    #[serde(
        default,
        deserialize_with = "nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub genres: Vec<String>,
    #[serde(
        default,
        deserialize_with = "nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub seasons: Vec<SeasonSummary>,
    pub details: bool,
}

impl Title {
    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        finite_nonnegative(self.rating, "rating")?;
        if self.mal_id < 0 || self.runtime < 0 {
            return Err(CatalogValidationError::InvalidField("title metadata"));
        }
        let mut seasons = HashSet::with_capacity(self.seasons.len());
        for season in &self.seasons {
            season.validate()?;
            if !seasons.insert(season.season) {
                return Err(CatalogValidationError::Duplicate("season"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub tmdb_episode_id: i64,
    pub season: i32,
    pub episode: i32,
    pub name: String,
    pub overview: String,
    pub runtime: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub still: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub air_date: String,
}

impl Episode {
    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        if self.tmdb_episode_id <= 0 || self.season < 0 || self.episode <= 0 || self.runtime < 0 {
            return Err(CatalogValidationError::InvalidField("episode"));
        }
        Ok(())
    }
}

/// Single-episode endpoint includes its parent show ID alongside episode fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeDetail {
    pub show_id: i64,
    #[serde(flatten)]
    pub episode: Episode,
}

impl EpisodeDetail {
    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        if self.show_id <= 0 {
            return Err(CatalogValidationError::InvalidField("episode show ID"));
        }
        self.episode.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Season {
    pub show_id: i64,
    pub season: i32,
    pub name: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub episodes: Vec<Episode>,
}

impl Season {
    pub fn key(&self) -> Result<String, CatalogValidationError> {
        season_key(self.show_id, self.season)
    }

    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        season_key(self.show_id, self.season)?;
        let mut numbers = HashSet::with_capacity(self.episodes.len());
        for episode in &self.episodes {
            episode.validate()?;
            if episode.season != self.season {
                return Err(CatalogValidationError::InvalidField("episode season"));
            }
            if !numbers.insert(episode.episode) {
                return Err(CatalogValidationError::Duplicate("episode"));
            }
        }
        Ok(())
    }
}

pub fn season_key(show_id: i64, season: i32) -> Result<String, CatalogValidationError> {
    if show_id <= 0 || season < 0 {
        return Err(CatalogValidationError::InvalidField("season coordinates"));
    }
    Ok(format!("tv:{show_id}:{season}"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shelf {
    pub id: String,
    pub heading: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub titles: Vec<Title>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recommendations {
    pub fingerprint: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub pool: Vec<Title>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub sections: Vec<Shelf>,
}

/// Persisted catalog envelope. Unknown record payloads remain representable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogRecord<T = serde_json::Value> {
    pub kind: String,
    pub key: String,
    pub data: T,
    pub updated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revision: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub state: String,
}

impl<T> CatalogRecord<T> {
    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        if self.kind.is_empty() || self.key.is_empty() || self.revision < 0 {
            return Err(CatalogValidationError::InvalidField("record envelope"));
        }
        // The Go store permits zero/negative TTLs. Expiry may precede updated_at
        // and stale records remain useful, so do not impose timestamp ordering.
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    pub version: String,
    pub revision: i64,
    pub exported_at: DateTime<Utc>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub records: Vec<CatalogRecord>,
}

impl CatalogSnapshot {
    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        if self.version != SNAPSHOT_VERSION {
            return Err(CatalogValidationError::UnsupportedSnapshotVersion);
        }
        if self.revision < 0 {
            return Err(CatalogValidationError::InvalidField("snapshot revision"));
        }
        let mut keys = HashSet::with_capacity(self.records.len());
        for record in &self.records {
            record.validate()?;
            if record.revision > self.revision {
                return Err(CatalogValidationError::InvalidField("record revision"));
            }
            if !keys.insert((record.kind.as_str(), record.key.as_str())) {
                return Err(CatalogValidationError::Duplicate("record"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossIds {
    #[serde(default)]
    pub imdb_id: String,
    #[serde(default)]
    pub mal_id: i64,
}

impl CrossIds {
    pub fn validate(&self) -> Result<(), CatalogValidationError> {
        let imdb_digits = self.imdb_id.strip_prefix("tt");
        let valid_imdb = self.imdb_id.is_empty()
            || imdb_digits.is_some_and(|digits| {
                !digits.is_empty()
                    && digits.len() <= 18
                    && digits.bytes().all(|b| b.is_ascii_digit())
            });
        if !valid_imdb || self.mal_id < 0 {
            return Err(CatalogValidationError::InvalidField("cross IDs"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentPreference {
    Both,
    Movie,
    Tv,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favorite {
    #[serde(flatten)]
    pub identity: TitleIdentity,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewingSignal {
    #[serde(flatten)]
    pub identity: TitleIdentity,
    pub title: String,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub watched_seconds: f64,
    #[serde(serialize_with = "serialize_finite_nonnegative")]
    pub duration_seconds: f64,
    pub completed: bool,
    pub meaningful_episodes: i32,
    pub updated_at: DateTime<Utc>,
}

/// Taste inputs sent by the API to the catalog recommendation engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecommendationInputs {
    pub completed: bool,
    pub format: ContentPreference,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub genres: Vec<String>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub favorites: Vec<Favorite>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub signals: Vec<ViewingSignal>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub activity_days: Vec<String>,
}

impl RecommendationInputs {
    /// Pure validation: the caller supplies time to avoid hidden clock/I/O state.
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), CatalogValidationError> {
        check_limit(self.genres.len(), 9, "genres")?;
        check_limit(self.favorites.len(), 50, "favorites")?;
        check_limit(self.signals.len(), 100, "signals")?;
        check_limit(self.activity_days.len(), 30, "activity days")?;

        let mut genres = HashSet::with_capacity(self.genres.len());
        for genre in &self.genres {
            if !valid_genre(genre) {
                return Err(CatalogValidationError::InvalidField("genre"));
            }
            if !genres.insert(genre.as_str()) {
                return Err(CatalogValidationError::Duplicate("genre"));
            }
        }

        let mut favorites = HashSet::with_capacity(self.favorites.len());
        for favorite in &self.favorites {
            check_limit(favorite.title.len(), 300, "favorite title")?;
            if !favorites.insert(favorite.identity.key()) {
                return Err(CatalogValidationError::Duplicate("favorite"));
            }
        }

        let mut signals = HashSet::with_capacity(self.signals.len());
        for signal in &self.signals {
            check_limit(signal.title.len(), 300, "signal title")?;
            finite_nonnegative(signal.watched_seconds, "watched seconds")?;
            finite_nonnegative(signal.duration_seconds, "duration seconds")?;
            if !(0..=100_000).contains(&signal.meaningful_episodes)
                || signal.updated_at > now
                || (signal.updated_at.timestamp() == -62_135_596_800
                    && signal.updated_at.timestamp_subsec_nanos() == 0)
            {
                return Err(CatalogValidationError::InvalidField("viewing signal"));
            }
            if !signals.insert(signal.identity.key()) {
                return Err(CatalogValidationError::Duplicate("viewing signal"));
            }
        }

        let mut days = HashSet::with_capacity(self.activity_days.len());
        for day in &self.activity_days {
            if day.len() != 10 {
                return Err(CatalogValidationError::InvalidField("activity day"));
            }
            let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
                .map_err(|_| CatalogValidationError::InvalidField("activity day"))?;
            // Require the exact Go YYYY-MM-DD format, including zero padding.
            if date.format("%Y-%m-%d").to_string() != *day {
                return Err(CatalogValidationError::InvalidField("activity day"));
            }
            let midnight = date
                .and_hms_opt(0, 0, 0)
                .ok_or(CatalogValidationError::InvalidField("activity day"))?
                .and_utc();
            if midnight > now || now.signed_duration_since(midnight) > Duration::days(31) {
                return Err(CatalogValidationError::InvalidField("activity day"));
            }
            if !days.insert(day.as_str()) {
                return Err(CatalogValidationError::Duplicate("activity day"));
            }
        }
        Ok(())
    }
}

pub fn valid_genre(genre: &str) -> bool {
    CANONICAL_GENRES.iter().any(|(slug, _)| *slug == genre)
}

fn check_limit(
    value: usize,
    limit: usize,
    field: &'static str,
) -> Result<(), CatalogValidationError> {
    if value > limit {
        return Err(CatalogValidationError::LimitExceeded(field));
    }
    Ok(())
}

fn finite_nonnegative(value: f64, field: &'static str) -> Result<(), CatalogValidationError> {
    if !value.is_finite() || value < 0.0 {
        return Err(CatalogValidationError::InvalidField(field));
    }
    Ok(())
}

fn serialize_finite_nonnegative<S: Serializer>(
    value: &f64,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    finite_nonnegative(*value, "number").map_err(serde::ser::Error::custom)?;
    serializer.serialize_f64(*value)
}

fn nullable_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

fn is_zero_i64(value: &i64) -> bool {
    *value == 0
}

fn is_zero_i32(value: &i32) -> bool {
    *value == 0
}
