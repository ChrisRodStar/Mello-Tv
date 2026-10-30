//! Ephemeral, client-reported playback activity. This is independent of relay traffic.
use crate::{AppError, state::AppState};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(35);
const LIMIT: usize = 128;

#[derive(Clone, Deserialize, Serialize)]
pub struct Activity {
    pub session_id: String,
    pub profile_id: String,
    pub title: String,
    pub device: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub state: PlaybackState,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackState {
    Playing,
    Paused,
    Buffering,
}

type Entries = Arc<Mutex<HashMap<String, (Instant, Activity)>>>;

pub fn routes() -> Router<AppState> {
    let entries: Entries = Arc::new(Mutex::new(HashMap::new()));
    let reader = entries.clone();
    let writer = entries.clone();
    Router::new()
        .route(
            "/api/v1/playback-activity",
            get(move || list(reader.clone())),
        )
        .route(
            "/api/v1/playback-activity/{id}",
            put(move |state, path, body| update(writer.clone(), state, path, body))
                .delete(move |path| remove(entries.clone(), path)),
        )
}

async fn list(entries: Entries) -> Json<Vec<Activity>> {
    let mut entries = entries.lock().unwrap_or_else(|e| e.into_inner());
    entries.retain(|_, (at, _)| at.elapsed() < TTL);
    let mut rows: Vec<_> = entries.values().map(|(_, value)| value.clone()).collect();
    rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    Json(rows)
}

async fn update(
    entries: Entries,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(activity): Json<Activity>,
) -> Result<StatusCode, AppError> {
    if id != activity.session_id
        || id.len() != 36
        || !id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')
        || activity.profile_id.is_empty()
        || activity.profile_id.len() > 128
        || activity.title.trim().is_empty()
        || activity.title.len() > 512
        || activity
            .device
            .as_deref()
            .is_some_and(|device| !["iPhone", "iPad", "iOS"].contains(&device))
        || !activity.position_seconds.is_finite()
        || activity.position_seconds < 0.0
        || activity
            .duration_seconds
            .is_some_and(|d| !d.is_finite() || d <= 0.0)
    {
        return Ok(StatusCode::BAD_REQUEST);
    }
    state
        .accounts
        .get_profile(&state.household_account_id, &activity.profile_id)
        .await?;
    let mut entries = entries.lock().unwrap_or_else(|e| e.into_inner());
    entries.retain(|_, (at, _)| at.elapsed() < TTL);
    if !entries.contains_key(&id) && entries.len() >= LIMIT {
        return Ok(StatusCode::SERVICE_UNAVAILABLE);
    }
    entries.insert(id, (Instant::now(), activity));
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(entries: Entries, Path(id): Path<String>) -> StatusCode {
    entries
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    StatusCode::NO_CONTENT
}
