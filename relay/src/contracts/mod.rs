pub mod providers;
use chrono::{DateTime, Utc};
use providers::Selection;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSession {
    pub contract_version: String,
    pub owner_id: String,
    pub expires_at: DateTime<Utc>,
    pub selection: Selection,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MutationBase {
    pub owner_id: String,
    pub expected_revision: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenewSession {
    pub owner_id: String,
    pub expected_revision: u64,
    pub expires_at: DateTime<Utc>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSource {
    pub owner_id: String,
    pub expected_revision: u64,
    pub selection: Selection,
}
// Backup enrichment preserves the active generation, grants and control revision.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBackups {
    pub owner_id: String,
    pub expected_generation: u64,
    pub selection: Selection,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Draining,
    Expired,
    Revoked,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackFormat {
    Hls,
    Mp4,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReady {
    pub contract_version: String,
    pub session_id: String,
    pub entry_path: String,
    pub format: PlaybackFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_heights: Option<Vec<i64>>,
    pub expires_at: DateTime<Utc>,
    pub revision: u64,
    pub auth_revision: u64,
    pub source_generation: u64,
    pub state: SessionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_required: Option<bool>,
}
