use std::fmt::Write;
use std::time::SystemTime;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
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
    error::DomainError,
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, StreamFormat, TmdbId,
    },
};
use stream_media::{MediaRelay, RelayError, RelayResponse};
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

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub st: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        // Stream delivery routes
        .route("/stream/{id}/entry", get(stream_entry))
        .route("/stream/{id}/r/{rid}", get(stream_resource))
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
        _ => Err(AppError::BadRequest(
            "invalid media type, expected movie or tv".into(),
        )),
    }
}

async fn default_profile_id(state: &AppState) -> Result<String, AppError> {
    let profiles = state
        .accounts
        .list_profiles(&state.household_account_id)
        .await?;
    let first = profiles
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("no profile available in household".into()))?;
    Ok(first.id)
}

async fn enrich_from_catalog(
    state: &AppState,
    media_type: MediaType,
    tmdb_id: i64,
    imdb_id: &mut String,
    mal_id: &mut Option<i64>,
) {
    if !imdb_id.is_empty() && mal_id.is_some() {
        return;
    }
    let mt_str = match media_type {
        MediaType::Movie => "movie",
        MediaType::Tv => "tv",
    };
    let key = format!("{mt_str}:{tmdb_id}");
    let now: DateTime<Utc> = SystemTime::now().into();
    if let Ok(record) = state.catalog.get("title", &key, now).await {
        if imdb_id.is_empty()
            && let Some(cached_imdb) = record.data.get("imdb_id").and_then(|v| v.as_str())
            && !cached_imdb.is_empty()
        {
            *imdb_id = cached_imdb.to_owned();
        }
        if mal_id.is_none()
            && let Some(cached_mal) = record.data.get("mal_id").and_then(|v| v.as_i64())
            && cached_mal > 0
        {
            *mal_id = Some(cached_mal);
        }
    }
}

fn build_lookup_from_request(
    profile_id: &str,
    request: &PlaybackRequest,
    refresh: bool,
) -> Result<Lookup, AppError> {
    let identity = request.target.identity();
    let tmdb_id =
        TmdbId::try_from(identity.id()).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let provider_title = ProviderTitle {
        media_type: identity.media_type(),
        tmdb_id,
        imdb_id: request.imdb_id.clone(),
        my_anime_list_id: request.mal_id,
        season: request.target.season(),
        episode: request.target.episode(),
    };

    let preferences = request
        .preferences
        .as_ref()
        .map(|p| QualityPreference {
            audio_languages: p.audio_languages.clone(),
            subtitle_languages: p.subtitle_languages.clone(),
            max_height: p.max_height,
            require_audio: p.require_audio,
            require_subtitles: p.require_subtitles,
        })
        .unwrap_or_default();

    let device = request
        .device
        .as_ref()
        .map(|d| {
            let mut codecs = d.codecs.clone();
            for default_codec in ["avc1", "mp4a", "h264", "aac"] {
                if !codecs.iter().any(|c| c.eq_ignore_ascii_case(default_codec)) {
                    codecs.push(default_codec.to_owned());
                }
            }
            DeviceCapabilities {
                formats: d
                    .formats
                    .iter()
                    .map(|f| match f {
                        PlaybackFormat::Hls => StreamFormat::Hls,
                        PlaybackFormat::Mp4 => StreamFormat::Mp4,
                    })
                    .collect(),
                codecs,
                max_height: d.max_height,
                allow_unknown_codecs: true,
                allow_unknown_resolution: true,
                supports_encrypted_hls: d.supports_encrypted_hls,
            }
        })
        .unwrap_or_else(|| DeviceCapabilities {
            formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
            codecs: vec!["avc1".into(), "mp4a".into(), "h264".into(), "aac".into()],
            max_height: 1080,
            allow_unknown_codecs: true,
            allow_unknown_resolution: true,
            supports_encrypted_hls: true,
        });

    Ok(Lookup {
        title: provider_title,
        preferences,
        device,
        context_key: profile_id.to_owned(),
        startup_height: 720,
        wait_for_complete: false,
        refresh,
    })
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
    let mut imdb_id = body.imdb_id.unwrap_or_default();
    let mut mal_id = body.mal_id;

    enrich_from_catalog(&state, mt, body.id, &mut imdb_id, &mut mal_id).await;

    let request = PlaybackRequest {
        target: target.clone(),
        device: body.device.clone(),
        preferences: body.preferences.clone(),
        imdb_id: imdb_id.clone(),
        mal_id,
    };
    request.validate()?;

    let lookup = build_lookup_from_request(&profile_id, &request, false)?;

    let resolved_selection = match state.media.resolve_stream(&lookup).await {
        Ok(selection) => Some(selection),
        Err(DomainError::NotFound(msg)) => {
            tracing::warn!(%profile_id, id = body.id, %msg, "no playable source found across providers");
            return Err(AppError::Unprocessable("no_source".into()));
        }
        Err(e) => {
            tracing::debug!(error = %e, "media resolution notice (using fallback session)");
            None
        }
    };

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

    let token = MediaRelay::generate_token();
    if let Some(selection) = &resolved_selection {
        state
            .relay
            .register_session(playback_id.clone(), token.clone(), selection.clone());
    }

    let (format, available_heights) = state
        .relay
        .active_info(&playback_id)
        .map(|(fmt, heights)| {
            let pf = match fmt {
                StreamFormat::Mp4 => PlaybackFormat::Mp4,
                _ => PlaybackFormat::Hls,
            };
            (pf, heights)
        })
        .unwrap_or_else(|| (PlaybackFormat::Hls, vec![1080, 720, 480]));

    let ready = RelayReady {
        available_heights,
        contract_version: "relay.v1".to_string(),
        session_id: playback_id.clone(),
        entry_path: format!("/stream/{}/entry", playback_id),
        format,
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

    if stored.id != playback_id {
        state.relay.revoke_session(&playback_id);
        if !state.relay.has_session(&stored.id)
            && let Some(selection) = resolved_selection
        {
            state
                .relay
                .register_session(stored.id.clone(), token, selection);
        }
    }

    let resume_seconds = state
        .accounts
        .get_progress(&state.household_account_id, &profile_id, target)
        .await?
        .map(|p| p.position_seconds)
        .unwrap_or(0.0);

    let active_ready = stored.ready.unwrap_or(ready);
    let session_token = state.relay.token_for(&stored.id);

    let reply = AuthorizedPlayback {
        id: stored.id.clone(),
        title: stored.request.target,
        url: format!("/stream/{}/entry?st={}", stored.id, session_token),
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

    let session_token = state.relay.token_for(&stored.id);

    Ok(Json(AuthorizedPlayback {
        id: stored.id.clone(),
        title: stored.request.target,
        url: format!("/stream/{}/entry?st={}", stored.id, session_token),
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
    state.relay.revoke_session(&playback_id);
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
        return Err(AppError::BadRequest(
            "expected_revision must be positive".into(),
        ));
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
    if current_ready.revision == expected_revision {
        match kind {
            "backup" if state.relay.has_session(&id) => {
                if let Err(RelayError::NoBackupAvailable) = state.relay.activate_backup(&id) {
                    return Err(AppError::Unprocessable("source_refresh_required".into()));
                }
            }
            "refresh" => {
                let lookup = build_lookup_from_request(&profile_id, &stored.request, true)?;
                match state.media.resolve_stream(&lookup).await {
                    Ok(selection) => {
                        let _ = state.relay.replace_selection(&id, selection);
                    }
                    Err(DomainError::NotFound(_)) => {
                        return Err(AppError::Unprocessable("no_source".into()));
                    }
                    Err(_) => {}
                }
            }
            _ => {}
        }
    }

    let idempotency_key = extract_idempotency_key(&headers);
    let (format, available_heights) = state
        .relay
        .active_info(&id)
        .map(|(fmt, heights)| {
            let pf = match fmt {
                StreamFormat::Mp4 => PlaybackFormat::Mp4,
                _ => PlaybackFormat::Hls,
            };
            (pf, heights)
        })
        .unwrap_or_else(|| (current_ready.format, current_ready.available_heights.clone()));

    let (new_ready, reload_required) = match kind {
        "renew" => {
            let new_expires = now + Duration::hours(6);
            (
                RelayReady {
                    available_heights,
                    contract_version: current_ready.contract_version.clone(),
                    session_id: current_ready.session_id.clone(),
                    entry_path: current_ready.entry_path.clone(),
                    format,
                    expires_at: new_expires,
                    revision: expected_revision + 1,
                    auth_revision: current_ready.auth_revision,
                    source_generation: current_ready.source_generation,
                    state: RelayState::Active,
                    resume_required: false,
                },
                false,
            )
        }
        "backup" | "refresh" => (
            RelayReady {
                available_heights,
                contract_version: current_ready.contract_version.clone(),
                session_id: current_ready.session_id.clone(),
                entry_path: current_ready.entry_path.clone(),
                format,
                expires_at: current_ready.expires_at,
                revision: expected_revision + 1,
                auth_revision: current_ready.auth_revision,
                source_generation: current_ready.source_generation + 1,
                state: RelayState::Active,
                resume_required: true,
            },
            true,
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

    let session_token = state.relay.token_for(&mutated.id);

    Ok(Json(AuthorizedPlayback {
        id: mutated.id.clone(),
        title: mutated.request.target,
        url: format!("/stream/{}/entry?st={}", mutated.id, session_token),
        format: ready.format,
        available_heights: ready.available_heights,
        expires_at: ready.expires_at,
        revision: ready.revision,
        source_generation: ready.source_generation,
        reload_required,
        resume_seconds,
    }))
}

async fn ensure_session_loaded(state: &AppState, session_id: &str, token: &str) {
    if state.relay.has_session(session_id) {
        return;
    }
    let Ok(profiles) = state
        .accounts
        .list_profiles(&state.household_account_id)
        .await
    else {
        return;
    };
    for p in profiles {
        if let Ok(stored) = state
            .accounts
            .get_playback(&state.household_account_id, &p.id, session_id)
            .await
        {
            if stored.revoked {
                return;
            }
            if let Ok(lookup) = build_lookup_from_request(&p.id, &stored.request, false)
                && let Ok(selection) = state.media.resolve_stream(&lookup).await
            {
                state
                    .relay
                    .register_session(session_id.to_owned(), token.to_owned(), selection);
            }
            return;
        }
    }
}

fn relay_result_to_response(result: Result<RelayResponse, RelayError>) -> Response {
    match result {
        Ok(RelayResponse::Manifest(manifest)) => {
            let mut res = (StatusCode::OK, manifest).into_response();
            let headers = res.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.apple.mpegurl"),
            );
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            headers.insert(
                "referrer-policy",
                HeaderValue::from_static("no-referrer"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            res
        }
        Ok(RelayResponse::Binary {
            status,
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream,
        }) => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            let mut res = Response::new(Body::from_stream(stream));
            *res.status_mut() = status_code;
            let headers = res.headers_mut();
            if let Ok(ct) = HeaderValue::from_str(&content_type) {
                headers.insert(header::CONTENT_TYPE, ct);
            }
            if let Some(len) = content_length
                && let Ok(val) = HeaderValue::from_str(&len.to_string())
            {
                headers.insert(header::CONTENT_LENGTH, val);
            }
            if let Some(cr) = content_range
                && let Ok(val) = HeaderValue::from_str(&cr)
            {
                headers.insert(header::CONTENT_RANGE, val);
            }
            if let Some(ar) = accept_ranges
                && let Ok(val) = HeaderValue::from_str(&ar)
            {
                headers.insert(header::ACCEPT_RANGES, val);
            }
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            headers.insert(
                "referrer-policy",
                HeaderValue::from_static("no-referrer"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            res
        }
        Err(RelayError::InvalidToken) => StatusCode::FORBIDDEN.into_response(),
        Err(RelayError::SessionNotFound | RelayError::ResourceNotFound) => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "stream relay delivery error");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

async fn stream_entry(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = query.st.filter(|s| !s.is_empty()) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    ensure_session_loaded(&state, &id, &token).await;
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let result = state.relay.fetch_entry(&id, &token, range).await;
    relay_result_to_response(result)
}

async fn stream_resource(
    State(state): State<AppState>,
    Path((id, rid)): Path<(String, String)>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = query.st.filter(|s| !s.is_empty()) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let result = state.relay.fetch_resource(&id, &rid, &token, range).await;
    relay_result_to_response(result)
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
