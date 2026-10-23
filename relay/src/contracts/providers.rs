//! Go providers.v1 wire types. Sensitive structs deliberately do not implement Debug.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
pub struct Title {
    pub r#type: String,
    pub tmdb_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub my_anime_list_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct FetchScope {
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookies: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_path: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct PublicResources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct FetchContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<FetchScope>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_resources: Option<PublicResources>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Track {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub default: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Variant {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codecs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_group: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_data: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<Track>>,
    pub tracks_known: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<Variant>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Source {
    pub id: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    pub title: Title,
    pub content_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_variant_url: Option<String>,
    pub url: String,
    pub format: String,
    pub fetch_context: FetchContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub resolver_metadata: Metadata,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Observation {
    pub at: DateTime<Utc>,
    pub accessible: bool,
    pub format: String,
    pub playlist_metadata: Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<Variant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_byte: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_bps: Option<f64>,
    pub samples_completed: i64,
    pub sample_bytes: i64,
    pub encrypted: bool,
    pub protection_known: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reliability: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Probed {
    pub source: Source,
    pub observation: Observation,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Ranked {
    pub candidate: Probed,
    pub score: f64,
    #[serde(deserialize_with = "required_nullable")]
    pub reasons: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub duration: i64,
    pub candidate_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Selection {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<Ranked>,
    #[serde(deserialize_with = "required_nullable")]
    pub backups: Option<Vec<Ranked>>,
    #[serde(deserialize_with = "required_nullable")]
    pub rejected: Option<BTreeMap<String, String>>,
    #[serde(deserialize_with = "required_nullable")]
    pub providers: Option<Vec<ProviderStatus>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
