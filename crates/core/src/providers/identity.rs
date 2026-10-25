use std::fmt;

use serde::{Deserialize, Serialize};

use crate::domain::MediaType;

use super::{ValidationError, bounded};

/// Positive TMDB identity that retains the legacy signed 64-bit numeric wire type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct TmdbId(i64);

impl TmdbId {
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for TmdbId {
    type Error = ValidationError;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value <= 0 {
            return Err(ValidationError::new("tmdb_id", "must be positive"));
        }
        Ok(Self(value))
    }
}

impl From<TmdbId> for i64 {
    fn from(value: TmdbId) -> Self {
        value.0
    }
}

/// Stable provider package ID, extensible without changing an enum or the daemon.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderId(String);

impl ProviderId {
    pub const BUILT_IN: [&'static str; 5] = ["vidcore", "vidlink", "vidsrc", "vidfast", "videasy"];
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        value.into().try_into()
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProviderId {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
            || !value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase())
        {
            return Err(ValidationError::new(
                "provider",
                "expected a lowercase package identifier",
            ));
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for ProviderId {
    type Error = ValidationError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.to_owned().try_into()
    }
}

impl From<ProviderId> for String {
    fn from(value: ProviderId) -> Self {
        value.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lookup coordinates; intentionally separate from a catalog title's metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTitle {
    #[serde(rename = "type")]
    pub media_type: MediaType,
    pub tmdb_id: TmdbId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub imdb_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub my_anime_list_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<i64>,
}

impl ProviderTitle {
    pub fn movie(tmdb_id: TmdbId) -> Self {
        Self {
            media_type: MediaType::Movie,
            tmdb_id,
            imdb_id: String::new(),
            my_anime_list_id: None,
            season: None,
            episode: None,
        }
    }

    pub fn episode(tmdb_id: TmdbId, season: i64, episode: i64) -> Result<Self, ValidationError> {
        let title = Self {
            media_type: MediaType::Tv,
            season: Some(season),
            episode: Some(episode),
            ..Self::movie(tmdb_id)
        };
        title.validate()?;
        Ok(title)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        match self.media_type {
            MediaType::Movie if self.season.is_none() && self.episode.is_none() => {}
            MediaType::Tv
                if self.season.is_some_and(|value| value >= 0)
                    && self.episode.is_some_and(|value| value > 0) => {}
            _ => {
                return Err(ValidationError::new(
                    "title",
                    "movie/episode coordinates are inconsistent",
                ));
            }
        }
        if self.my_anime_list_id.is_some_and(|value| value <= 0) {
            return Err(ValidationError::new("my_anime_list_id", "must be positive"));
        }
        bounded(&self.imdb_id, 0, 32, "imdb_id")?;
        if self.imdb_id.trim() != self.imdb_id {
            return Err(ValidationError::new(
                "imdb_id",
                "surrounding whitespace is forbidden",
            ));
        }
        Ok(())
    }

    /// Exact content coordinates; cross-IDs do not define identity in the Go contract.
    pub fn same_content(&self, other: &Self) -> bool {
        self.media_type == other.media_type
            && self.tmdb_id == other.tmdb_id
            && match self.media_type {
                MediaType::Movie => true,
                MediaType::Tv => {
                    self.season.is_some()
                        && self.episode.is_some()
                        && self.season == other.season
                        && self.episode == other.episode
                }
            }
    }

    /// Stronger boundary check also rejects contradictory supplied cross-identifiers.
    pub fn compatible_with(&self, other: &Self) -> bool {
        self.same_content(other)
            && (self.imdb_id.is_empty()
                || other.imdb_id.is_empty()
                || self.imdb_id == other.imdb_id)
            && !matches!((self.my_anime_list_id, other.my_anime_list_id), (Some(a), Some(b)) if a != b)
    }
}
