//! HTTP surface:
//!
//! - `GET /play/movie/{tmdb}` and `GET /play/tv/{tmdb}/{season}/{episode}`
//!   (play key): resolve a source and redirect to the relay. These are the
//!   URLs inside the `.strm` files.
//! - `GET /stream/...` (per-session token): the relayed playlist and media.
//! - `/api/...` (API key): library management, search and diagnostics.
//! - `GET /health`: unauthenticated liveness.

// Handlers return early with a ready-made `Response`; boxing it buys nothing.
#![allow(clippy::result_large_err)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use stream_core::{
    domain::MediaType,
    ports::MediaEngine,
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, Selection, StreamFormat,
        TmdbId,
    },
};
use stream_media::{MediaRelay, RelayError, RelayResponse, delivery_format};

use crate::{
    auth::{Keys, random_hex},
    library::{Kind, Library},
    tmdb::{Tmdb, TmdbError},
};

/// A resolved session is reused for repeat opens of the same title (Jellyfin
/// probes a file, then plays it) for this long.
const SESSION_REUSE: Duration = Duration::from_secs(20 * 60);
/// After a failed resolution, answer immediately instead of re-running all
/// providers for every retry.
const FAILURE_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AppState {
    pub keys: Keys,
    pub media: Arc<dyn MediaEngine>,
    pub relay: Arc<MediaRelay>,
    pub tmdb: Tmdb,
    pub library: Arc<Library>,
    pub public_url: url::Url,
    pub public_url_configured: bool,
    pub max_height: i64,
    pub audio_languages: Vec<String>,
    play_cache: Arc<Mutex<HashMap<String, CachedPlay>>>,
    title_locks: Arc<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>>,
}

#[derive(Clone)]
enum CachedPlay {
    Ready {
        session_id: String,
        token: String,
        format: StreamFormat,
        height: Option<i64>,
        provider: String,
        created: Instant,
        source_expires: Option<chrono::DateTime<Utc>>,
    },
    Failed {
        at: Instant,
        reason: String,
    },
}

pub struct AppStateInit {
    pub keys: Keys,
    pub media: Arc<dyn MediaEngine>,
    pub relay: Arc<MediaRelay>,
    pub tmdb: Tmdb,
    pub library: Arc<Library>,
    pub public_url: url::Url,
    pub public_url_configured: bool,
    pub max_height: i64,
    pub audio_languages: Vec<String>,
}

impl AppState {
    pub fn new(init: AppStateInit) -> Self {
        Self {
            keys: init.keys,
            media: init.media,
            relay: init.relay,
            tmdb: init.tmdb,
            library: init.library,
            public_url: init.public_url,
            public_url_configured: init.public_url_configured,
            max_height: init.max_height,
            audio_languages: init.audio_languages,
            play_cache: Arc::default(),
            title_locks: Arc::default(),
        }
    }

    fn title_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .title_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(key.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/play/movie/{id}", get(play_movie).options(preflight))
        .route(
            "/play/tv/{id}/{season}/{episode}",
            get(play_episode).options(preflight),
        )
        .route(
            "/stream/{session}/{entry}",
            get(stream_entry).options(preflight),
        )
        .route(
            "/stream/{session}/r/{file}",
            get(stream_resource).options(preflight),
        )
        .route("/api/status", get(api_status))
        .route("/api/search", get(api_search))
        .route("/api/library", get(api_library))
        .route("/api/library/sync", post(api_sync))
        .route(
            "/api/library/{kind}/{id}",
            post(api_library_add).delete(api_library_remove),
        )
        .route("/api/resolve/movie/{id}", get(api_resolve_movie))
        .route(
            "/api/resolve/tv/{id}/{season}/{episode}",
            get(api_resolve_episode),
        )
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

async fn security_headers(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    // Paths only: query strings carry keys and tokens and must not be logged.
    let status = response.status();
    if path.starts_with("/stream/") && status.is_success() {
        tracing::trace!(%method, %path, %status, latency = ?started.elapsed(), "request");
    } else if status.is_server_error() {
        tracing::warn!(%method, %path, %status, latency = ?started.elapsed(), "request");
    } else {
        tracing::debug!(%method, %path, %status, latency = ?started.elapsed(), "request");
    }
    response
}

fn cors(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("Content-Length, Content-Range, Accept-Ranges"),
    );
}

async fn preflight() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    cors(&mut response);
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Range"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    response
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

async fn health() -> &'static str {
    "ok"
}

// ---------------------------------------------------------------- playback

#[derive(Deserialize)]
struct PlayQuery {
    key: Option<String>,
    imdb: Option<String>,
}

#[derive(Clone, Copy)]
struct Target {
    media_type: MediaType,
    id: i64,
    season: Option<i64>,
    episode: Option<i64>,
}

impl Target {
    fn cache_key(&self) -> String {
        match self.media_type {
            MediaType::Movie => format!("movie:{}", self.id),
            MediaType::Tv => format!(
                "tv:{}:{}:{}",
                self.id,
                self.season.unwrap_or(0),
                self.episode.unwrap_or(0)
            ),
        }
    }

    fn validate(&self) -> Result<(), Response> {
        let in_range = |v: Option<i64>, min: i64| v.is_none_or(|v| (min..=10_000).contains(&v));
        if self.id <= 0
            || self.id > 100_000_000
            || !in_range(self.season, 0)
            || !in_range(self.episode, 1)
        {
            return Err(error(StatusCode::BAD_REQUEST, "invalid title"));
        }
        Ok(())
    }
}

async fn play_movie(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PlayQuery>,
) -> Response {
    let target = Target {
        media_type: MediaType::Movie,
        id,
        season: None,
        episode: None,
    };
    play(state, target, query).await
}

async fn play_episode(
    State(state): State<AppState>,
    Path((id, season, episode)): Path<(i64, i64, i64)>,
    Query(query): Query<PlayQuery>,
) -> Response {
    let target = Target {
        media_type: MediaType::Tv,
        id,
        season: Some(season),
        episode: Some(episode),
    };
    play(state, target, query).await
}

async fn play(state: AppState, target: Target, query: PlayQuery) -> Response {
    let authorized = query
        .key
        .as_deref()
        .is_some_and(|key| state.keys.is_play_key(key) || state.keys.is_api_key(key));
    if !authorized {
        return error(StatusCode::UNAUTHORIZED, "missing or invalid key");
    }
    if let Err(response) = target.validate() {
        return response;
    }
    let mut response = match ensure_session(&state, target, query.imdb).await {
        Ok(CachedPlay::Ready {
            session_id,
            token,
            format,
            ..
        }) => {
            let location = entry_path(&session_id, &token, format);
            let mut response = StatusCode::FOUND.into_response();
            if let Ok(value) = HeaderValue::from_str(&location) {
                response.headers_mut().insert(header::LOCATION, value);
            }
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Ok(CachedPlay::Failed { reason, .. }) | Err(reason) => error(
            StatusCode::NOT_FOUND,
            format!("no playable source: {reason}"),
        ),
    };
    cors(&mut response);
    response
}

fn entry_path(session_id: &str, token: &str, format: StreamFormat) -> String {
    let file = if format == StreamFormat::Mp4 {
        "video.mp4"
    } else {
        "master.m3u8"
    };
    format!("/stream/{session_id}/{file}?st={token}")
}

async fn ensure_session(
    state: &AppState,
    target: Target,
    imdb: Option<String>,
) -> Result<CachedPlay, String> {
    let key = target.cache_key();
    let lock = state.title_lock(&key);
    let _guard = lock.lock().await;

    if let Some(cached) = cached_play(state, &key) {
        return Ok(cached);
    }

    let result = resolve(state, target, imdb).await;
    let entry = match result {
        Ok(entry) => entry,
        Err(reason) => {
            tracing::warn!(title = %key, %reason, "no playable source");
            CachedPlay::Failed {
                at: Instant::now(),
                reason,
            }
        }
    };
    if let Ok(mut cache) = state.play_cache.lock() {
        cache.retain(|_, entry| match entry {
            CachedPlay::Ready { created, .. } => created.elapsed() < SESSION_REUSE,
            CachedPlay::Failed { at, .. } => at.elapsed() < FAILURE_BACKOFF,
        });
        cache.insert(key, entry.clone());
    }
    Ok(entry)
}

fn cached_play(state: &AppState, key: &str) -> Option<CachedPlay> {
    let cache = state.play_cache.lock().ok()?;
    let entry = cache.get(key)?;
    match entry {
        CachedPlay::Ready {
            session_id,
            created,
            source_expires,
            ..
        } => {
            let fresh_source = source_expires
                .is_none_or(|expires| expires > Utc::now() + chrono::Duration::minutes(10));
            (created.elapsed() < SESSION_REUSE
                && fresh_source
                && state.relay.has_session(session_id))
            .then(|| entry.clone())
        }
        CachedPlay::Failed { at, .. } => (at.elapsed() < FAILURE_BACKOFF).then(|| entry.clone()),
    }
}

async fn resolve(
    state: &AppState,
    target: Target,
    imdb: Option<String>,
) -> Result<CachedPlay, String> {
    let imdb = match imdb.filter(|id| id.starts_with("tt") && id.len() <= 12) {
        Some(id) => id,
        None => lookup_imdb(&state.tmdb, target).await,
    };
    let tmdb_id = TmdbId::try_from(target.id).map_err(|e| e.to_string())?;
    let lookup = Lookup {
        title: ProviderTitle {
            media_type: target.media_type,
            tmdb_id,
            imdb_id: imdb,
            my_anime_list_id: None,
            season: target.season,
            episode: target.episode,
        },
        preferences: QualityPreference {
            audio_languages: state.audio_languages.clone(),
            ..QualityPreference::default()
        },
        device: DeviceCapabilities {
            formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
            codecs: ["avc1", "mp4a", "h264", "aac"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            max_height: state.max_height,
            allow_unknown_codecs: true,
            allow_unknown_resolution: true,
            supports_encrypted_hls: true,
        },
        context_key: "jellyfin".to_owned(),
        // Jellyfin cannot switch sources mid-play, so ask for the target
        // quality up front; the engine still returns its best within ~25s.
        startup_height: state.max_height,
        wait_for_complete: false,
        refresh: false,
    };
    let started = Instant::now();
    let selection = state
        .media
        .resolve_stream(&lookup)
        .await
        .map_err(|e| e.to_string())?;
    install(state, selection, started)
}

fn install(state: &AppState, selection: Selection, started: Instant) -> Result<CachedPlay, String> {
    let selected = selection
        .selected
        .as_ref()
        .ok_or_else(|| "no source selected".to_owned())?;
    let height = selected
        .candidate
        .observation
        .variant
        .as_ref()
        .and_then(|variant| variant.height);
    let provider = selected.candidate.source.provider.to_string();
    let source_expires = selected.candidate.source.expires_at;
    let format = delivery_format(&selected.candidate);
    let session_id = random_hex(12).map_err(|e| e.to_string())?;
    let token = MediaRelay::generate_token().ok_or_else(|| "no randomness".to_owned())?;
    state
        .relay
        .register_session(session_id.clone(), token.clone(), selection)
        .ok_or_else(|| "relay unavailable".to_owned())?;
    tracing::info!(
        %provider,
        ?height,
        ?format,
        elapsed = ?started.elapsed(),
        "source resolved"
    );
    Ok(CachedPlay::Ready {
        session_id,
        token,
        format,
        height,
        provider,
        created: Instant::now(),
        source_expires,
    })
}

async fn lookup_imdb(tmdb: &Tmdb, target: Target) -> String {
    if !tmdb.configured() {
        return String::new();
    }
    let imdb = match target.media_type {
        MediaType::Movie => tmdb.movie(target.id).await.ok().and_then(|m| m.imdb_id),
        MediaType::Tv => tmdb
            .show(target.id, false)
            .await
            .ok()
            .and_then(|s| s.external_ids.imdb_id),
    };
    imdb.filter(|id| id.starts_with("tt") && id.len() <= 12)
        .unwrap_or_default()
}

// ------------------------------------------------------------------ relay

#[derive(Deserialize)]
struct StreamQuery {
    st: Option<String>,
}

async fn stream_entry(
    State(state): State<AppState>,
    Path((session, _entry)): Path<(String, String)>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = query.st.filter(|s| !s.is_empty()) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    relay_response(state.relay.fetch_entry(&session, &token, range).await)
}

async fn stream_resource(
    State(state): State<AppState>,
    Path((session, file)): Path<(String, String)>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = query.st.filter(|s| !s.is_empty()) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    relay_response(
        state
            .relay
            .fetch_resource(&session, &file, &token, range)
            .await,
    )
}

fn relay_response(result: Result<RelayResponse, RelayError>) -> Response {
    let mut response = match result {
        Ok(RelayResponse::Manifest(manifest)) => {
            let mut response = (StatusCode::OK, manifest).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.apple.mpegurl"),
            );
            response
        }
        Ok(RelayResponse::Binary {
            status,
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream,
        }) => {
            let mut response = Response::new(Body::from_stream(stream));
            *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            let headers = response.headers_mut();
            if let Ok(value) = HeaderValue::from_str(&content_type) {
                headers.insert(header::CONTENT_TYPE, value);
            }
            if let Some(len) = content_length {
                headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
            }
            for (name, value) in [
                (header::CONTENT_RANGE, content_range),
                (header::ACCEPT_RANGES, accept_ranges),
            ] {
                if let Some(value) = value.and_then(|v| HeaderValue::from_str(&v).ok()) {
                    headers.insert(name, value);
                }
            }
            response
        }
        Err(RelayError::InvalidToken) => StatusCode::FORBIDDEN.into_response(),
        Err(RelayError::SessionNotFound | RelayError::ResourceNotFound) => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "stream relay delivery error");
            StatusCode::BAD_GATEWAY.into_response()
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    cors(&mut response);
    response
}

// -------------------------------------------------------------------- API

#[derive(Deserialize)]
struct ApiQuery {
    key: Option<String>,
    q: Option<String>,
}

fn require_api(state: &AppState, headers: &HeaderMap, query: &ApiQuery) -> Result<(), Response> {
    if state.keys.authorize_api(headers, query.key.as_deref()) {
        Ok(())
    } else {
        Err(error(
            StatusCode::UNAUTHORIZED,
            "missing or invalid API key",
        ))
    }
}

fn tmdb_error(error_value: TmdbError) -> Response {
    let status = match error_value {
        TmdbError::NotFound => StatusCode::NOT_FOUND,
        TmdbError::NotConfigured => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    };
    error(status, error_value.to_string())
}

async fn api_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    if let Err(response) = require_api(&state, &headers, &query) {
        return response;
    }
    let entries = state.library.entries();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "public_url": state.public_url.as_str(),
        "public_url_configured": state.public_url_configured,
        "tmdb_configured": state.tmdb.configured(),
        "max_height": state.max_height,
        "active_streams": state.relay.active_streams(),
        "providers": state.media.diagnostics(),
        "library": {
            "movies": entries.movies.len(),
            "shows": entries.shows.len(),
            "last_sync": state.library.last_report(),
        },
    }))
    .into_response()
}

async fn api_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    if let Err(response) = require_api(&state, &headers, &query) {
        return response;
    }
    let Some(q) = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty() && q.len() <= 200)
    else {
        return error(StatusCode::BAD_REQUEST, "missing ?q= search text");
    };
    match state.tmdb.search(q).await {
        Ok(results) => Json(json!({ "results": results })).into_response(),
        Err(e) => tmdb_error(e),
    }
}

async fn api_library(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    if let Err(response) = require_api(&state, &headers, &query) {
        return response;
    }
    Json(state.library.entries()).into_response()
}

async fn api_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    if let Err(response) = require_api(&state, &headers, &query) {
        return response;
    }
    Json(state.library.sync().await).into_response()
}

async fn api_library_add(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, i64)>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    if let Err(response) = require_api(&state, &headers, &query) {
        return response;
    }
    let Some(kind) = Kind::parse(&kind).filter(|_| id > 0) else {
        return error(
            StatusCode::BAD_REQUEST,
            "use /api/library/movie/{id} or /api/library/tv/{id}",
        );
    };
    match state.library.add(kind, id).await {
        Ok(entry) => (StatusCode::CREATED, Json(json!({ "added": entry }))).into_response(),
        Err(e) => tmdb_error(e),
    }
}

async fn api_library_remove(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, i64)>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    if let Err(response) = require_api(&state, &headers, &query) {
        return response;
    }
    let Some(kind) = Kind::parse(&kind) else {
        return error(StatusCode::BAD_REQUEST, "kind must be movie or tv");
    };
    match state.library.remove(kind, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "not in library"),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn api_resolve_movie(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    let target = Target {
        media_type: MediaType::Movie,
        id,
        season: None,
        episode: None,
    };
    api_resolve(state, target, headers, query).await
}

async fn api_resolve_episode(
    State(state): State<AppState>,
    Path((id, season, episode)): Path<(i64, i64, i64)>,
    headers: HeaderMap,
    Query(query): Query<ApiQuery>,
) -> Response {
    let target = Target {
        media_type: MediaType::Tv,
        id,
        season: Some(season),
        episode: Some(episode),
    };
    api_resolve(state, target, headers, query).await
}

/// Debug helper: resolves like `/play` but reports what was picked.
async fn api_resolve(
    state: AppState,
    target: Target,
    headers: HeaderMap,
    query: ApiQuery,
) -> Response {
    if let Err(response) = require_api(&state, &headers, &query) {
        return response;
    }
    if let Err(response) = target.validate() {
        return response;
    }
    let started = Instant::now();
    match ensure_session(&state, target, None).await {
        Ok(CachedPlay::Ready {
            session_id,
            token,
            format,
            height,
            provider,
            ..
        }) => {
            let base = state.public_url.as_str().trim_end_matches('/').to_owned();
            Json(json!({
                "provider": provider,
                "format": format,
                "height": height,
                "elapsed_ms": started.elapsed().as_millis(),
                "stream_url": format!("{base}{}", entry_path(&session_id, &token, format)),
            }))
            .into_response()
        }
        Ok(CachedPlay::Failed { reason, .. }) | Err(reason) => error(
            StatusCode::NOT_FOUND,
            format!("no playable source: {reason}"),
        ),
    }
}
