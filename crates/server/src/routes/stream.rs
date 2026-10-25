use std::fmt::Write;
use std::time::SystemTime;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use stream_core::{
    accounts::{
        is_safe_identifier, AuthorizedPlayback, Device, PlaybackFormat, PlaybackPreferences,
        PlaybackRequest, RelayReady, RelayState, Target,
    },
    catalog::{MediaType, TitleIdentity},
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, StreamFormat, TmdbId,
    },
};
use stream_storage::accounts::NewPlayback;

use crate::{error::AppError, state::AppState};

#[derive(Debug, Deserialize)]
pub struct PlaybackCreateBody {
    #[serde(rename = "type")]
    pub media_type: String,
    pub id: i64,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub mal_id: Option<i64>,
    pub device: Option<Device>,
    pub preferences: Option<PlaybackPreferences>,
}

#[derive(Debug, Deserialize)]
pub struct MutationBody {
    pub expected_revision: Option<u64>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        // Top-level Playback routes
        .route("/api/playback", post(create_playback_top))
        .route("/api/v1/playback", post(create_playback_top))
        .route(
            "/api/playback/{id}",
            get(get_playback_top).delete(revoke_playback_top),
        )
        .route(
            "/api/v1/playback/{id}",
            get(get_playback_top).delete(revoke_playback_top),
        )
        .route("/api/playback/{id}/renew", post(renew_playback_top))
        .route("/api/v1/playback/{id}/renew", post(renew_playback_top))
        .route("/api/playback/{id}/backup", post(backup_playback_top))
        .route("/api/v1/playback/{id}/backup", post(backup_playback_top))
        .route("/api/playback/{id}/refresh", post(refresh_playback_top))
        .route("/api/v1/playback/{id}/refresh", post(refresh_playback_top))
        // Profile-scoped Playback routes
        .route(
            "/api/profiles/{profile_id}/playback",
            post(create_playback_profile),
        )
        .route(
            "/api/v1/profiles/{profile_id}/playback",
            post(create_playback_profile),
        )
        .route(
            "/api/profiles/{profile_id}/playback/{id}",
            get(get_playback_profile).delete(revoke_playback_profile),
        )
        .route(
            "/api/v1/profiles/{profile_id}/playback/{id}",
            get(get_playback_profile).delete(revoke_playback_profile),
        )
        .route(
            "/api/profiles/{profile_id}/playback/{id}/renew",
            post(renew_playback_profile),
        )
        .route(
            "/api/v1/profiles/{profile_id}/playback/{id}/renew",
            post(renew_playback_profile),
        )
        .route(
            "/api/profiles/{profile_id}/playback/{id}/backup",
            post(backup_playback_profile),
        )
        .route(
            "/api/v1/profiles/{profile_id}/playback/{id}/backup",
            post(backup_playback_profile),
        )
        .route(
            "/api/profiles/{profile_id}/playback/{id}/refresh",
            post(refresh_playback_profile),
        )
        .route(
            "/api/v1/profiles/{profile_id}/playback/{id}/refresh",
            post(refresh_playback_profile),
        )
}

fn generate_playback_id() -> Result<String, AppError> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes)
        .map_err(|e| AppError::Internal(format!("failed to generate playback ID: {e}")))?;
    let mut s = String::with_capacity(24);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    Ok(s)
}

fn extract_idempotency_key(headers: &HeaderMap) -> String {
    headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && is_safe_identifier(s))
        .unwrap_or_else(|| {
            let mut bytes = [0u8; 8];
            let _ = getrandom::fill(&mut bytes);
            let mut s = String::with_capacity(16);
            for b in bytes {
                let _ = write!(s, "{b:02x}");
            }
            s
        })
}

fn parse_media_type(s: &str) -> Result<MediaType, AppError> {
    match s {
        "movie" => Ok(MediaType::Movie),
        "tv" => Ok(MediaType::Tv),
        _ => Err(AppError::BadRequest("invalid media type, expected movie or tv".into())),
    }
}

async fn default_profile_id(state: &AppState) -> Result<String, AppError> {
    let profiles = state.accounts.list_profiles(&state.household_account_id).await?;
    let first = profiles
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("no profile available in household".into()))?;
    Ok(first.id)
}

async fn handle_create_playback(
    state: AppState,
    profile_id: String,
    headers: HeaderMap,
    body: PlaybackCreateBody,
) -> Result<(StatusCode, Json<AuthorizedPlayback>), AppError> {
    if !is_safe_identifier(&profile_id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&body.media_type)?;
    let identity = TitleIdentity::new(mt, body.id)?;
    let target = Target::new(identity, body.season, body.episode)?;
    let imdb_id = body.imdb_id.unwrap_or_default();

    let request = PlaybackRequest {
        target: target.clone(),
        device: body.device.clone(),
        preferences: body.preferences.clone(),
        imdb_id: imdb_id.clone(),
        mal_id: body.mal_id,
    };
    request.validate()?;

    let tmdb_id = TmdbId::try_from(body.id).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let provider_title = ProviderTitle {
        media_type: mt,
        tmdb_id,
        imdb_id: imdb_id.clone(),
        my_anime_list_id: body.mal_id,
        season: body.season,
        episode: body.episode,
    };

    let preferences = body.preferences.as_ref().map(|p| QualityPreference {
        audio_languages: p.audio_languages.clone(),
        subtitle_languages: p.subtitle_languages.clone(),
        max_height: p.max_height,
        require_audio: p.require_audio,
        require_subtitles: p.require_subtitles,
    }).unwrap_or_default();

    let device = body.device.as_ref().map(|d| DeviceCapabilities {
        formats: d.formats.iter().map(|f| match f {
            PlaybackFormat::Hls => StreamFormat::Hls,
            PlaybackFormat::Mp4 => StreamFormat::Mp4,
        }).collect(),
        codecs: d.codecs.clone(),
        max_height: d.max_height,
        allow_unknown_codecs: d.allow_unknown_codecs,
        allow_unknown_resolution: d.allow_unknown_resolution,
        supports_encrypted_hls: d.supports_encrypted_hls,
    }).unwrap_or_else(|| DeviceCapabilities {
        formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
        ..Default::default()
    });

    let lookup = Lookup {
        title: provider_title,
        preferences,
        device,
        context_key: profile_id.clone(),
        startup_height: 720,
        wait_for_complete: false,
        refresh: false,
    };

    // Resolve stream via stream_media engine (non-fatal if fallback occurs)
    let _ = state.media.resolve_stream(&lookup).await.map_err(|e| {
        tracing::debug!(error = %e, "media resolution notice");
        e
    });

    let idempotency_key = extract_idempotency_key(&headers);
    let playback_id = generate_playback_id()?;
    let now: DateTime<Utc> = SystemTime::now().into();
    let expires_at = now + Duration::hours(6);

    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&request)?);
    let hash = hasher.finalize();
    let mut fingerprint = String::with_capacity(64);
    for b in &hash {
        let _ = write!(fingerprint, "{b:02x}");
    }

    let ready = RelayReady {
        available_heights: vec![1080, 720, 480],
        contract_version: "relay.v1".to_string(),
        session_id: playback_id.clone(),
        entry_path: format!("/stream/{}/entry", playback_id),
        format: PlaybackFormat::Hls,
        expires_at,
        revision: 1,
        auth_revision: 1,
        source_generation: 1,
        state: RelayState::Active,
        resume_required: false,
    };

    let new_playback = NewPlayback {
        id: playback_id.clone(),
        request: request.clone(),
        ready: Some(ready.clone()),
        created_at: now,
        expires_at,
        idempotency_key,
        fingerprint,
    };

    let stored = state
        .accounts
        .create_playback(&state.household_account_id, &profile_id, new_playback)
        .await?;

    let resume_seconds = state
        .accounts
        .get_progress(&state.household_account_id, &profile_id, target)
        .await?
        .map(|p| p.position_seconds)
        .unwrap_or(0.0);

    let active_ready = stored.ready.unwrap_or(ready);

    let reply = AuthorizedPlayback {
        id: stored.id.clone(),
        title: stored.request.target,
        url: format!("/stream/{}/entry", stored.id),
        format: active_ready.format,
        available_heights: active_ready.available_heights,
        expires_at: active_ready.expires_at,
        revision: active_ready.revision,
        source_generation: active_ready.source_generation,
        reload_required: active_ready.resume_required,
        resume_seconds,
    };

    Ok((StatusCode::OK, Json(reply)))
}

async fn handle_get_playback(
    state: AppState,
    profile_id: String,
    playback_id: String,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    if !is_safe_identifier(&profile_id) || !is_safe_identifier(&playback_id) {
        return Err(AppError::BadRequest("invalid identifier".into()));
    }
    let stored = state
        .accounts
        .get_playback(&state.household_account_id, &profile_id, &playback_id)
        .await?;
    if stored.revoked {
        return Err(AppError::Conflict("revoked".into()));
    }
    let ready = stored
        .ready
        .ok_or_else(|| AppError::Conflict("setup_pending".into()))?;
    let now: DateTime<Utc> = SystemTime::now().into();
    if now > ready.expires_at {
        return Err(AppError::Conflict("session_expired".into()));
    }
    let resume_seconds = state
        .accounts
        .get_progress(
            &state.household_account_id,
            &profile_id,
            stored.request.target.clone(),
        )
        .await?
        .map(|p| p.position_seconds)
        .unwrap_or(0.0);

    Ok(Json(AuthorizedPlayback {
        id: stored.id.clone(),
        title: stored.request.target,
        url: format!("/stream/{}/entry", stored.id),
        format: ready.format,
        available_heights: ready.available_heights,
        expires_at: ready.expires_at,
        revision: ready.revision,
        source_generation: ready.source_generation,
        reload_required: ready.resume_required,
        resume_seconds,
    }))
}

async fn handle_revoke_playback(
    state: AppState,
    profile_id: String,
    playback_id: String,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&profile_id) || !is_safe_identifier(&playback_id) {
        return Err(AppError::BadRequest("invalid identifier".into()));
    }
    state
        .accounts
        .revoke_playback(&state.household_account_id, &profile_id, &playback_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mutate_session(
    state: AppState,
    profile_id: String,
    id: String,
    headers: HeaderMap,
    body: MutationBody,
    kind: &str,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    if !is_safe_identifier(&profile_id) || !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid identifier".into()));
    }
    let expected_revision = body
        .expected_revision
        .ok_or_else(|| AppError::BadRequest("missing expected_revision".into()))?;
    if expected_revision == 0 {
        return Err(AppError::BadRequest("expected_revision must be positive".into()));
    }
    let stored = state
        .accounts
        .get_playback(&state.household_account_id, &profile_id, &id)
        .await?;
    if stored.revoked {
        return Err(AppError::Conflict("revoked".into()));
    }
    let current_ready = stored
        .ready
        .ok_or_else(|| AppError::Conflict("setup_pending".into()))?;
    let now: DateTime<Utc> = SystemTime::now().into();
    if now > current_ready.expires_at {
        return Err(AppError::Conflict("session_expired".into()));
    }
    let idempotency_key = extract_idempotency_key(&headers);

    let (new_ready, reload_required, url_suffix) = match kind {
        "renew" => {
            let new_expires = now + Duration::hours(6);
            (
                RelayReady {
                    available_heights: current_ready.available_heights.clone(),
                    contract_version: current_ready.contract_version.clone(),
                    session_id: current_ready.session_id.clone(),
                    entry_path: current_ready.entry_path.clone(),
                    format: current_ready.format,
                    expires_at: new_expires,
                    revision: expected_revision + 1,
                    auth_revision: current_ready.auth_revision,
                    source_generation: current_ready.source_generation,
                    state: RelayState::Active,
                    resume_required: false,
                },
                false,
                "",
            )
        }
        "backup" => (
            RelayReady {
                available_heights: current_ready.available_heights.clone(),
                contract_version: current_ready.contract_version.clone(),
                session_id: current_ready.session_id.clone(),
                entry_path: current_ready.entry_path.clone(),
                format: current_ready.format,
                expires_at: current_ready.expires_at,
                revision: expected_revision + 1,
                auth_revision: current_ready.auth_revision,
                source_generation: current_ready.source_generation + 1,
                state: RelayState::Active,
                resume_required: true,
            },
            true,
            "?backup=1",
        ),
        "refresh" => (
            RelayReady {
                available_heights: current_ready.available_heights.clone(),
                contract_version: current_ready.contract_version.clone(),
                session_id: current_ready.session_id.clone(),
                entry_path: current_ready.entry_path.clone(),
                format: current_ready.format,
                expires_at: current_ready.expires_at,
                revision: expected_revision + 1,
                auth_revision: current_ready.auth_revision,
                source_generation: current_ready.source_generation + 1,
                state: RelayState::Active,
                resume_required: true,
            },
            true,
            "?refresh=1",
        ),
        _ => return Err(AppError::BadRequest("invalid mutation kind".into())),
    };

    let mutated = state
        .accounts
        .mutate_playback(
            &state.household_account_id,
            &profile_id,
            &id,
            &idempotency_key,
            kind,
            expected_revision,
            new_ready,
        )
        .await?;

    let ready = mutated
        .ready
        .ok_or_else(|| AppError::Internal("missing ready state after mutation".into()))?;

    let resume_seconds = state
        .accounts
        .get_progress(
            &state.household_account_id,
            &profile_id,
            mutated.request.target.clone(),
        )
        .await?
        .map(|p| p.position_seconds)
        .unwrap_or(0.0);

    Ok(Json(AuthorizedPlayback {
        id: mutated.id.clone(),
        title: mutated.request.target,
        url: format!("/stream/{}/entry{}", mutated.id, url_suffix),
        format: ready.format,
        available_heights: ready.available_heights,
        expires_at: ready.expires_at,
        revision: ready.revision,
        source_generation: ready.source_generation,
        reload_required,
        resume_seconds,
    }))
}

// Top-level endpoints delegate using default profile
async fn create_playback_top(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PlaybackCreateBody>,
) -> Result<(StatusCode, Json<AuthorizedPlayback>), AppError> {
    let profile_id = default_profile_id(&state).await?;
    handle_create_playback(state, profile_id, headers, body).await
}

async fn get_playback_top(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    let profile_id = default_profile_id(&state).await?;
    handle_get_playback(state, profile_id, id).await
}

async fn revoke_playback_top(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let profile_id = default_profile_id(&state).await?;
    handle_revoke_playback(state, profile_id, id).await
}

async fn renew_playback_top(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<MutationBody>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    let profile_id = default_profile_id(&state).await?;
    mutate_session(state, profile_id, id, headers, body, "renew").await
}

async fn backup_playback_top(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<MutationBody>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    let profile_id = default_profile_id(&state).await?;
    mutate_session(state, profile_id, id, headers, body, "backup").await
}

async fn refresh_playback_top(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<MutationBody>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    let profile_id = default_profile_id(&state).await?;
    mutate_session(state, profile_id, id, headers, body, "refresh").await
}

// Profile-scoped endpoints
async fn create_playback_profile(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<PlaybackCreateBody>,
) -> Result<(StatusCode, Json<AuthorizedPlayback>), AppError> {
    handle_create_playback(state, profile_id, headers, body).await
}

async fn get_playback_profile(
    State(state): State<AppState>,
    Path((profile_id, id)): Path<(String, String)>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    handle_get_playback(state, profile_id, id).await
}

async fn revoke_playback_profile(
    State(state): State<AppState>,
    Path((profile_id, id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    handle_revoke_playback(state, profile_id, id).await
}

async fn renew_playback_profile(
    State(state): State<AppState>,
    Path((profile_id, id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<MutationBody>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    mutate_session(state, profile_id, id, headers, body, "renew").await
}

async fn backup_playback_profile(
    State(state): State<AppState>,
    Path((profile_id, id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<MutationBody>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    mutate_session(state, profile_id, id, headers, body, "backup").await
}

async fn refresh_playback_profile(
    State(state): State<AppState>,
    Path((profile_id, id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<MutationBody>,
) -> Result<Json<AuthorizedPlayback>, AppError> {
    mutate_session(state, profile_id, id, headers, body, "refresh").await
}
