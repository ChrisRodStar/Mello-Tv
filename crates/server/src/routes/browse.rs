use std::time::SystemTime;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use stream_storage::StorageError;

use crate::{
    error::AppError,
    state::{AppState, CATALOG_CONFIG_GUIDANCE},
};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub query: Option<String>,
    pub offset: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResultQuery {
    pub q: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CategoriesQuery {
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CrossIdsPayload {
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub mal_id: Option<i64>,
}

fn pending_catalog_record(
    kind: &str,
    key: &str,
    data: serde_json::Value,
    status: &str,
    reason: Option<String>,
) -> serde_json::Value {
    let updated_at: DateTime<Utc> = SystemTime::now().into();
    let expires_at = updated_at + chrono::Duration::hours(1);
    let mut value = json!({"kind": kind, "key": key, "data": data, "updated_at": updated_at, "expires_at": expires_at, "revision": 0, "state": status, "status": status});
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    value
}

fn request_status(state: &AppState, key: &str) -> Option<(&'static str, Option<String>)> {
    state.catalog_request_status(key).and_then(|item| {
        use crate::state::CatalogGenerationStatus;
        // Cache presence is checked by the caller. A completed job must not
        // suppress a refresh after its cached record expires or is removed.
        if matches!(item.status, CatalogGenerationStatus::Ready)
            || (matches!(item.status, CatalogGenerationStatus::Pending)
                && !state.is_catalog_job_running(key))
            || (matches!(
                item.status,
                CatalogGenerationStatus::Failed | CatalogGenerationStatus::Unavailable
            ) && item.updated_at.is_none_or(|updated| {
                let now: DateTime<Utc> = SystemTime::now().into();
                now.signed_duration_since(updated) >= chrono::Duration::seconds(30)
            }))
        {
            return None;
        }
        let status = match item.status {
            crate::state::CatalogGenerationStatus::Pending => "pending",
            crate::state::CatalogGenerationStatus::Ready => "ready",
            crate::state::CatalogGenerationStatus::Unavailable => "unavailable",
            crate::state::CatalogGenerationStatus::Failed => "failed",
        };
        Some((status, item.reason))
    })
}

fn queue_title_refresh(
    state: &AppState,
    job_key: String,
    identity: stream_core::catalog::TitleIdentity,
) -> bool {
    let Some(worker) = state
        .catalog_worker
        .clone()
        .filter(|worker| worker.available())
    else {
        return false;
    };
    if !state.ensure_catalog_job(&job_key) {
        return true;
    }
    let job_state = state.clone();
    let lock_key = job_key;
    tokio::spawn(async move {
        let result = worker.title(identity).await;
        let (status, reason) = match result {
            Ok(lookup)
                if lookup
                    .value
                    .as_ref()
                    .is_some_and(|record| record.data.details) =>
            {
                (crate::state::CatalogGenerationStatus::Ready, None)
            }
            Ok(lookup)
                if lookup.availability == crate::catalog_service::Availability::Unavailable =>
            {
                (
                    crate::state::CatalogGenerationStatus::Unavailable,
                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                )
            }
            Ok(_) => (
                crate::state::CatalogGenerationStatus::Failed,
                Some("Title details are not available from the catalog.".to_owned()),
            ),
            Err(_) => (
                crate::state::CatalogGenerationStatus::Failed,
                Some("Title lookup failed. Try again later.".to_owned()),
            ),
        };
        job_state.set_catalog_request_status(&lock_key, status, reason);
        job_state.finish_catalog_job(&lock_key);
    });
    true
}

fn queue_season_refresh(
    state: &AppState,
    job_key: String,
    show_id: i64,
    season: i32,
    force: bool,
) -> bool {
    let Some(worker) = state
        .catalog_worker
        .clone()
        .filter(|worker| worker.available())
    else {
        return false;
    };
    if !state.ensure_catalog_job(&job_key) {
        return true;
    }
    let job_state = state.clone();
    let lock_key = job_key;
    tokio::spawn(async move {
        let result = if force {
            worker.refresh_season(show_id, season).await
        } else {
            worker.season(show_id, season).await
        };
        let (status, reason) = match result {
            Ok(lookup) if lookup.value.is_some() => {
                (crate::state::CatalogGenerationStatus::Ready, None)
            }
            Ok(lookup)
                if lookup.availability == crate::catalog_service::Availability::Unavailable =>
            {
                (
                    crate::state::CatalogGenerationStatus::Unavailable,
                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                )
            }
            Ok(_) => (
                crate::state::CatalogGenerationStatus::Failed,
                Some("Season is not available from the catalog.".to_owned()),
            ),
            Err(_) => (
                crate::state::CatalogGenerationStatus::Failed,
                Some("Season lookup failed. Try again later.".to_owned()),
            ),
        };
        job_state.set_catalog_request_status(&lock_key, status, reason);
        job_state.finish_catalog_job(&lock_key);
    });
    true
}

pub(crate) fn normalize_artwork_url(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if (k == "poster" || k == "backdrop" || k == "still") && v.is_string() {
                    if let Some(s) = v.as_str() {
                        if s.starts_with("/internal/v1/artwork/") {
                            *v = serde_json::Value::String(s.replacen(
                                "/internal/v1/artwork/",
                                "https://image.tmdb.org/t/p/",
                                1,
                            ));
                        } else if s.starts_with("/api/artwork/") {
                            *v = serde_json::Value::String(s.replacen(
                                "/api/artwork/",
                                "https://image.tmdb.org/t/p/",
                                1,
                            ));
                        } else if s.starts_with("/api/v1/artwork/") {
                            *v = serde_json::Value::String(s.replacen(
                                "/api/v1/artwork/",
                                "https://image.tmdb.org/t/p/",
                                1,
                            ));
                        }
                    }
                } else {
                    normalize_artwork_url(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                normalize_artwork_url(v);
            }
        }
        _ => {}
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        // Health
        .route("/health/livez", get(livez))
        .route("/health/readyz", get(readyz))
        .route("/internal/v1/health/live", get(livez))
        .route("/internal/v1/health/ready", get(readyz))
        .route("/api/status", get(status))
        .route("/api/v1/status", get(status))
        // Search
        .route("/internal/v1/search", get(search))
        .route("/api/catalog/search", get(search))
        .route("/api/search", get(search))
        .route("/api/v1/search", get(search))
        .route("/internal/v1/search-results", get(search_results))
        .route("/api/catalog/search-results", get(search_results))
        .route("/api/search-results", get(search_results))
        .route("/api/v1/search-results", get(search_results))
        // Categories & Browse & Shelves
        .route("/internal/v1/shelves", get(categories))
        .route("/internal/v1/shelves/{id}", get(shelf_by_id))
        .route("/api/catalog/categories", get(categories))
        .route("/api/categories", get(categories))
        .route("/api/catalog/browse", get(categories))
        .route("/api/browse", get(categories))
        .route("/api/v1/browse", get(categories))
        .route("/api/shelves", get(categories))
        .route("/api/v1/shelves", get(categories))
        .route("/api/catalog/shelves", get(categories))
        .route("/api/shelves/{id}", get(shelf_by_id))
        .route("/api/v1/shelves/{id}", get(shelf_by_id))
        .route("/api/catalog/shelves/{id}", get(shelf_by_id))
        // Titles & Cross-IDs
        .route(
            "/internal/v1/titles/{media_type}/{id}",
            get(title).put(put_cross_ids),
        )
        .route(
            "/internal/v1/titles/{media_type}/{id}/cross-ids",
            put(put_cross_ids),
        )
        .route("/api/catalog/titles/{media_type}/{id}", get(title))
        .route("/api/titles/{media_type}/{id}", get(title))
        .route("/api/v1/titles/{media_type}/{id}", get(title))
        .route(
            "/api/catalog/titles/{media_type}/{id}/cross-ids",
            put(put_cross_ids),
        )
        .route(
            "/api/titles/{media_type}/{id}/cross-ids",
            put(put_cross_ids),
        )
        .route(
            "/api/v1/titles/{media_type}/{id}/cross-ids",
            put(put_cross_ids),
        )
        // TV Seasons & Episodes
        .route("/internal/v1/tv/{id}/seasons/{season}", get(season))
        .route("/api/catalog/tv/{id}/seasons/{season}", get(season))
        .route("/api/tv/{id}/seasons/{season}", get(season))
        .route("/api/v1/tv/{id}/seasons/{season}", get(season))
        .route(
            "/internal/v1/tv/{id}/seasons/{season}/episodes/{episode}",
            get(internal_episode),
        )
        .route(
            "/api/catalog/tv/{id}/seasons/{season}/episodes/{episode}",
            get(episode),
        )
        .route(
            "/api/tv/{id}/seasons/{season}/episodes/{episode}",
            get(episode),
        )
        .route(
            "/api/v1/tv/{id}/seasons/{season}/episodes/{episode}",
            get(episode),
        )
        // Artwork Redirects
        .route("/internal/v1/artwork/{size}/{filename}", get(artwork))
        .route("/api/artwork/{size}/{filename}", get(artwork))
        .route("/api/v1/artwork/{size}/{filename}", get(artwork))
        .route("/api/catalog/artwork/{size}/{filename}", get(artwork))
        // Snapshots
        .route("/internal/v1/snapshot", get(snapshot))
        .route("/api/v1/snapshot", get(snapshot))
        .route("/api/catalog/snapshot", get(snapshot))
}

async fn livez() -> Json<serde_json::Value> {
    Json(json!({ "version": "api.v1", "status": "alive" }))
}

async fn readyz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state.catalog.health().await?;
    Ok(Json(json!({ "version": "api.v1", "status": "ready" })))
}

async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let now: DateTime<Utc> = SystemTime::now().into();
    let has_snapshot = state
        .catalog
        .list("shelf", 1, now)
        .await
        .map(|shelves| !shelves.is_empty())
        .unwrap_or(false);
    let announcement =
        crate::discovery::build_announcement(&state.household_account_id, state.http_port);
    let catalog_db_available = state.catalog.health().await.is_ok();
    let catalog_configured = state
        .catalog_worker
        .as_ref()
        .is_some_and(|worker| worker.available());
    let media = state.media.diagnostics();

    Json(json!({
        "version": "api.v1",
        "status": "ok",
        "catalog_snapshot": if has_snapshot { "retained" } else { "missing" },
        "catalog_status": if !catalog_db_available { "unavailable" } else if !catalog_configured { "unconfigured" } else if has_snapshot { "ready" } else { "pending" },
        "catalog_configured": catalog_configured,
        "uptime_seconds": state.started_at.elapsed().as_secs(),
        "media": media,
        "provider_updates": { "mode": "bundled", "available": null },
        "service": announcement.service,
        "app_version": announcement.app_version,
        "server_id": announcement.server_id,
        "server_name": announcement.server_name,
        "port": announcement.port,
        "endpoints": announcement.endpoints,
    }))
}

async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let q = params
        .q
        .or(params.query)
        .ok_or_else(|| AppError::BadRequest("missing search query".to_string()))?;
    let q = q.trim();
    if q.is_empty() || q.len() > 200 {
        return Err(AppError::BadRequest("invalid search query".to_string()));
    }
    let offset = params.offset.unwrap_or(0);
    let now: DateTime<Utc> = SystemTime::now().into();
    let worker_configured = state
        .catalog_worker
        .as_ref()
        .is_some_and(|worker| worker.available());
    let limit = if worker_configured { 20 } else { 50 };
    let cache_key = crate::catalog_service::search_cache_key(q, offset);
    let request_key = format!("search:{cache_key}");
    let upstream_record = state.catalog.get("search", &cache_key, now).await.ok();
    let cached_titles = upstream_record
        .as_ref()
        .and_then(|record| record.data.as_array().map(|values| (record, values)));
    let cache_fresh = cached_titles
        .as_ref()
        .is_some_and(|(record, _)| record.expires_at > now);
    let mut records = Vec::<serde_json::Value>::new();
    if let Some((record, titles)) = cached_titles.filter(|_| cache_fresh) {
        for value in titles {
            if let Ok(title) = serde_json::from_value::<stream_core::catalog::Title>(value.clone())
            {
                records.push(json!({"kind":"title","key":title.identity.key(),"data":title,"updated_at":record.updated_at,"expires_at":record.expires_at,"revision":record.revision,"state":record.state}));
            }
        }
    }
    let mut status = if cache_fresh { "ready" } else { "pending" };
    let mut reason = None;
    if !cache_fresh {
        if let Some((previous, previous_reason)) = request_status(&state, &request_key) {
            status = previous;
            reason = previous_reason;
        } else {
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            if profiles.is_empty() {
                status = "unavailable";
                reason = Some("Create a profile to start catalog generation.".to_owned());
            } else if let Some(worker) = state
                .catalog_worker
                .clone()
                .filter(|worker| worker.available())
            {
                if state.ensure_catalog_job(&request_key) {
                    let job_state = state.clone();
                    let key = request_key.clone();
                    let query = q.to_owned();
                    tokio::spawn(async move {
                        let result = worker.search(&query, offset).await;
                        let (status, reason) = match result {
                            Ok(lookup) if lookup.value.is_some() => {
                                (crate::state::CatalogGenerationStatus::Ready, None)
                            }
                            Ok(lookup)
                                if lookup.availability
                                    == crate::catalog_service::Availability::Unavailable =>
                            {
                                (
                                    crate::state::CatalogGenerationStatus::Unavailable,
                                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                                )
                            }
                            Ok(_) => (crate::state::CatalogGenerationStatus::Ready, None),
                            Err(_) => (
                                crate::state::CatalogGenerationStatus::Failed,
                                Some("Catalog search failed. Try again later.".to_owned()),
                            ),
                        };
                        job_state.set_catalog_request_status(&key, status, reason);
                        job_state.finish_catalog_job(&key);
                    });
                }
            } else {
                status = "unavailable";
                reason = Some(CATALOG_CONFIG_GUIDANCE.to_owned());
            }
        }
        if let Some((record, titles)) = cached_titles {
            if records.is_empty() {
                for value in titles {
                    if let Ok(title) =
                        serde_json::from_value::<stream_core::catalog::Title>(value.clone())
                    {
                        records.push(json!({"kind":"title","key":title.identity.key(),"data":title,"updated_at":record.updated_at,"expires_at":record.expires_at,"revision":record.revision,"state":"stale"}));
                    }
                }
            }
        } else {
            // A stale per-title cache can still provide useful results while a full query refresh runs.
            let local = state.catalog.search(q, offset, now).await?;
            if records.is_empty() {
                records = serde_json::to_value(local)
                    .map_err(|e| AppError::Internal(e.to_string()))?
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
            }
        }
    }
    let stale = !cache_fresh && cached_titles.is_some();
    if stale {
        status = "ready";
    }
    let mut records_val = json!(records);
    normalize_artwork_url(&mut records_val);
    let http_status = if status == "pending" {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((
        http_status,
        Json(json!({
            "scope":"cached_titles", "status":status, "reason":reason,
            "records":records_val, "offset":offset, "limit":limit, "stale":stale,
        })),
    ))
}

async fn search_results(
    State(state): State<AppState>,
    Query(params): Query<SearchResultQuery>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let q = params
        .q
        .or(params.query)
        .ok_or_else(|| AppError::BadRequest("missing search query".to_string()))?;

    let q = q.trim();
    if q.is_empty() || q.len() > 200 {
        return Err(AppError::BadRequest("invalid search query".to_string()));
    }

    let now: DateTime<Utc> = SystemTime::now().into();
    match state.catalog.get("search", q, now).await {
        Ok(record) => {
            let stale = record.state == "stale" || record.expires_at <= now;
            let mut val =
                serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
            if stale {
                let profiles = state
                    .accounts
                    .list_profiles(&state.household_account_id)
                    .await?;
                let key = format!("search:{}", crate::catalog_service::search_cache_key(q, 0));
                if !profiles.is_empty()
                    && let Some(worker) = state
                        .catalog_worker
                        .clone()
                        .filter(|worker| worker.available())
                {
                    if state.ensure_catalog_job(&key) {
                        let job_state = state.clone();
                        let lock_key = key.clone();
                        let query = q.to_owned();
                        tokio::spawn(async move {
                            let result = worker.search(&query, 0).await;
                            let (status, reason) = match result {
                                Ok(lookup) if lookup.value.is_some() => {
                                    (crate::state::CatalogGenerationStatus::Ready, None)
                                }
                                Ok(lookup)
                                    if lookup.availability
                                        == crate::catalog_service::Availability::Unavailable =>
                                {
                                    (
                                        crate::state::CatalogGenerationStatus::Unavailable,
                                        Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                                    )
                                }
                                Ok(_) => (crate::state::CatalogGenerationStatus::Ready, None),
                                Err(_) => (
                                    crate::state::CatalogGenerationStatus::Failed,
                                    Some("Catalog search failed. Try again later.".to_owned()),
                                ),
                            };
                            job_state.set_catalog_request_status(&lock_key, status, reason);
                            job_state.finish_catalog_job(&lock_key);
                        });
                    }
                    val["status"] = json!("ready");
                    val["stale"] = json!(true);
                } else if !profiles.is_empty() {
                    val["status"] = json!("ready");
                    val["stale"] = json!(true);
                    val["reason"] = json!(CATALOG_CONFIG_GUIDANCE);
                }
            }
            normalize_artwork_url(&mut val);
            Ok((StatusCode::OK, Json(val)))
        }
        Err(StorageError::NotFound) => {
            let key = format!("search:{}", crate::catalog_service::search_cache_key(q, 0));
            if let Some((status, reason)) = request_status(&state, &key) {
                return Ok((
                    if status == "pending" {
                        StatusCode::ACCEPTED
                    } else {
                        StatusCode::OK
                    },
                    Json(pending_catalog_record(
                        "search",
                        q,
                        json!([]),
                        status,
                        reason,
                    )),
                ));
            }
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            if profiles.is_empty() {
                return Ok((
                    StatusCode::OK,
                    Json(pending_catalog_record(
                        "search",
                        q,
                        json!([]),
                        "unavailable",
                        Some("Create a profile to start catalog generation.".to_owned()),
                    )),
                ));
            }
            if let Some(worker) = state
                .catalog_worker
                .clone()
                .filter(|worker| worker.available())
            {
                if state.try_start_catalog_job(&key) {
                    let job_state = state.clone();
                    let job_key = key.clone();
                    let query = q.to_owned();
                    tokio::spawn(async move {
                        let result = worker.search(&query, 0).await;
                        match result {
                            Ok(lookup) if lookup.value.is_some() => job_state
                                .set_catalog_request_status(
                                    &job_key,
                                    crate::state::CatalogGenerationStatus::Ready,
                                    None,
                                ),
                            Ok(lookup)
                                if lookup.availability
                                    == crate::catalog_service::Availability::Unavailable =>
                            {
                                job_state.set_catalog_request_status(
                                    &job_key,
                                    crate::state::CatalogGenerationStatus::Unavailable,
                                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                                )
                            }
                            Ok(_) => job_state.set_catalog_request_status(
                                &job_key,
                                crate::state::CatalogGenerationStatus::Ready,
                                None,
                            ),
                            Err(_) => job_state.set_catalog_request_status(
                                &job_key,
                                crate::state::CatalogGenerationStatus::Failed,
                                Some("Catalog search failed. Try again later.".to_owned()),
                            ),
                        };
                        job_state.finish_catalog_job(&job_key);
                    });
                }
                Ok((
                    StatusCode::ACCEPTED,
                    Json(pending_catalog_record(
                        "search",
                        q,
                        json!([]),
                        "pending",
                        None,
                    )),
                ))
            } else {
                Ok((
                    StatusCode::OK,
                    Json(pending_catalog_record(
                        "search",
                        q,
                        json!([]),
                        "unavailable",
                        Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                    )),
                ))
            }
        }
        Err(e) => Err(AppError::Storage(e)),
    }
}

async fn categories(
    State(state): State<AppState>,
    Query(params): Query<CategoriesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let now: DateTime<Utc> = SystemTime::now().into();
    let limit = params.limit.unwrap_or(1000).clamp(1, 20_000);
    let records = state.catalog.list("shelf", limit, now).await?;
    let mut status = if records.is_empty() {
        "pending"
    } else {
        "ready"
    };
    let mut reason = None;
    if records.is_empty() {
        if let Some((previous, previous_reason)) = request_status(&state, "browse") {
            status = previous;
            reason = previous_reason;
        } else {
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            if profiles.is_empty() {
                status = "unavailable";
                reason = Some("Create a profile to start catalog generation.".to_owned());
            } else if let Some(worker) = state
                .catalog_worker
                .clone()
                .filter(|worker| worker.available())
            {
                if state.try_start_catalog_job("browse") {
                    let job_state = state.clone();
                    let profile = profiles[0].clone();
                    tokio::spawn(async move {
                        let result = worker.warm_profile(profile).await;
                        match result {
                            Ok(crate::catalog_service::GenerationStatus::Ready) => job_state
                                .set_catalog_request_status(
                                    "browse",
                                    crate::state::CatalogGenerationStatus::Ready,
                                    None,
                                ),
                            Ok(crate::catalog_service::GenerationStatus::Unavailable) => job_state
                                .set_catalog_request_status(
                                    "browse",
                                    crate::state::CatalogGenerationStatus::Unavailable,
                                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                                ),
                            Err(_) => job_state.set_catalog_request_status(
                                "browse",
                                crate::state::CatalogGenerationStatus::Failed,
                                Some(
                                    "Catalog browse generation failed. Try again later.".to_owned(),
                                ),
                            ),
                        };
                        job_state.finish_catalog_job("browse");
                    });
                }
            } else {
                status = "unavailable";
                reason = Some(CATALOG_CONFIG_GUIDANCE.to_owned());
            }
        }
    }
    let mut val = serde_json::to_value(&records).map_err(|e| AppError::Internal(e.to_string()))?;
    normalize_artwork_url(&mut val);
    Ok(Json(
        json!({ "records": val, "status": status, "reason": reason }),
    ))
}

async fn shelf_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let now: DateTime<Utc> = SystemTime::now().into();
    match state.catalog.get("shelf", &id, now).await {
        Ok(record) => {
            let mut val =
                serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
            normalize_artwork_url(&mut val);
            Ok(Json(val))
        }
        Err(StorageError::NotFound) => Err(AppError::NotFound(format!("shelf {id} not found"))),
        Err(e) => Err(AppError::Storage(e)),
    }
}

async fn title(
    State(state): State<AppState>,
    Path((media_type, id)): Path<(String, i64)>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if id <= 0 || (media_type != "movie" && media_type != "tv") {
        return Err(AppError::BadRequest("invalid title coordinates".into()));
    }
    let media = if media_type == "movie" {
        stream_core::catalog::MediaType::Movie
    } else {
        stream_core::catalog::MediaType::Tv
    };
    let identity = stream_core::catalog::TitleIdentity::new(media, id)
        .map_err(|_| AppError::BadRequest("invalid title coordinates".into()))?;
    let key = format!("{media_type}:{id}");
    let now: DateTime<Utc> = SystemTime::now().into();
    match state.catalog.title(&key, now).await {
        Ok(record) => {
            let long_legacy_ttl = record.expires_at.signed_duration_since(record.updated_at)
                > chrono::Duration::days(30);
            let stale = record.state == "stale" || record.expires_at <= now || long_legacy_ttl;
            let needs_details = !record
                .data
                .get("details")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if stale || needs_details {
                let has_profiles = !state
                    .accounts
                    .list_profiles(&state.household_account_id)
                    .await?
                    .is_empty();
                let job_key = format!("title:{key}");
                if has_profiles
                    && state
                        .catalog_worker
                        .as_ref()
                        .is_some_and(|worker| worker.available())
                {
                    let _ = queue_title_refresh(&state, job_key.clone(), identity);
                    if needs_details {
                        let mut val = serde_json::to_value(record)
                            .map_err(|e| AppError::Internal(e.to_string()))?;
                        val["status"] = json!("pending");
                        return Ok((StatusCode::ACCEPTED, Json(val)));
                    }
                    let mut val = serde_json::to_value(record)
                        .map_err(|e| AppError::Internal(e.to_string()))?;
                    val["status"] = json!("pending");
                    return Ok((StatusCode::OK, Json(val)));
                }
                let mut val =
                    serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
                if needs_details {
                    val["status"] = json!("unavailable");
                    val["reason"] = json!(CATALOG_CONFIG_GUIDANCE);
                }
                normalize_artwork_url(&mut val);
                return Ok((StatusCode::OK, Json(val)));
            }
            let mut val =
                serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
            normalize_artwork_url(&mut val);
            Ok((StatusCode::OK, Json(val)))
        }
        Err(StorageError::NotFound) => {
            let job_key = format!("title:{key}");
            if let Some((status, reason)) = request_status(&state, &job_key) {
                let code = if status == "pending" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                };
                let mut payload = json!({"status":status,"key":key});
                if let Some(reason) = reason {
                    payload["reason"] = json!(reason);
                }
                return Ok((code, Json(payload)));
            }
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            if profiles.is_empty() {
                return Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"unavailable","reason":"Create a profile to start catalog generation.","key":key}),
                    ),
                ));
            }
            if queue_title_refresh(&state, job_key, identity) {
                Ok((
                    StatusCode::ACCEPTED,
                    Json(json!({"status":"pending","key":key})),
                ))
            } else {
                Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"unavailable","reason":CATALOG_CONFIG_GUIDANCE,"key":key}),
                    ),
                ))
            }
        }
        Err(e) => Err(AppError::Storage(e)),
    }
}

async fn put_cross_ids(
    State(state): State<AppState>,
    Path((media_type, id)): Path<(String, i64)>,
    Json(body): Json<CrossIdsPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    if id <= 0 || (media_type != "movie" && media_type != "tv") {
        return Err(AppError::BadRequest("invalid identity".into()));
    }
    let imdb_id = body.imdb_id.unwrap_or_default();
    let mal_id = body.mal_id.unwrap_or(0);

    if !imdb_id.is_empty() {
        let valid_imdb = imdb_id.starts_with("tt")
            && imdb_id.len() >= 3
            && imdb_id.len() <= 20
            && imdb_id[2..].chars().all(|c| c.is_ascii_digit());
        if !valid_imdb {
            return Err(AppError::BadRequest("invalid_cross_ids".into()));
        }
    }
    if mal_id < 0 {
        return Err(AppError::BadRequest("invalid_cross_ids".into()));
    }

    let key = format!("{media_type}:{id}");
    let now: DateTime<Utc> = SystemTime::now().into();
    let write = stream_storage::catalog::CatalogWrite {
        kind: "cross_ids".to_string(),
        key,
        data: json!({
            "imdb_id": imdb_id,
            "mal_id": mal_id,
        }),
        ttl: chrono::Duration::days(36500),
    };
    state.catalog.put(write, now).await?;
    Ok(Json(json!({ "status": "saved" })))
}

async fn season(
    State(state): State<AppState>,
    Path((id, season_num)): Path<(i64, i64)>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if id <= 0 || !(0..=10_000).contains(&season_num) {
        return Err(AppError::BadRequest("invalid season coordinates".into()));
    }
    let key = format!("tv:{id}:{season_num}");
    let job_key = format!("season:{key}");
    let now: DateTime<Utc> = SystemTime::now().into();
    match state.catalog.get("season", &key, now).await {
        Ok(record) => {
            let legacy_ttl = record.expires_at.signed_duration_since(record.updated_at)
                > chrono::Duration::days(30);
            if record.state == "stale" || record.expires_at <= now || legacy_ttl {
                let profiles = state
                    .accounts
                    .list_profiles(&state.household_account_id)
                    .await?;
                if !profiles.is_empty()
                    && state
                        .catalog_worker
                        .as_ref()
                        .is_some_and(|worker| worker.available())
                {
                    queue_season_refresh(&state, job_key, id, season_num as i32, true);
                }
            }
            let mut val =
                serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
            normalize_artwork_url(&mut val);
            Ok((StatusCode::OK, Json(val)))
        }
        Err(StorageError::NotFound) => {
            if let Some((status, reason)) = request_status(&state, &job_key) {
                let code = if status == "pending" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                };
                let mut payload = json!({"status":status,"key":key});
                if let Some(reason) = reason {
                    payload["reason"] = json!(reason);
                }
                return Ok((code, Json(payload)));
            }
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            if profiles.is_empty() {
                return Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"unavailable","reason":"Create a profile to start catalog generation.","key":key}),
                    ),
                ));
            }
            if queue_season_refresh(&state, job_key, id, season_num as i32, false) {
                Ok((
                    StatusCode::ACCEPTED,
                    Json(json!({"status":"pending","key":key})),
                ))
            } else {
                Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"unavailable","reason":CATALOG_CONFIG_GUIDANCE,"key":key}),
                    ),
                ))
            }
        }
        Err(e) => Err(AppError::Storage(e)),
    }
}

async fn episode(
    State(state): State<AppState>,
    Path((id, season_num, episode_num)): Path<(i64, i64, i64)>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if id <= 0 || !(0..=10_000).contains(&season_num) || !(1..=100_000).contains(&episode_num) {
        return Err(AppError::BadRequest("invalid episode coordinates".into()));
    }

    let key = format!("tv:{id}:{season_num}");
    let now: DateTime<Utc> = SystemTime::now().into();
    let record = match state.catalog.get("season", &key, now).await {
        Ok(rec) => rec,
        Err(StorageError::NotFound) => {
            let job_key = format!("season:{key}");
            if let Some((status, reason)) = request_status(&state, &job_key) {
                let code = if status == "pending" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                };
                let mut payload = json!({"status":status,"key":key});
                if let Some(reason) = reason {
                    payload["reason"] = json!(reason);
                }
                return Ok((code, Json(payload)));
            }
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            if profiles.is_empty() {
                return Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"unavailable","reason":"Create a profile to start catalog generation.","key":key}),
                    ),
                ));
            }
            if queue_season_refresh(&state, job_key, id, season_num as i32, false) {
                return Ok((
                    StatusCode::ACCEPTED,
                    Json(json!({"status":"pending","key":key})),
                ));
            }
            return Ok((
                StatusCode::OK,
                Json(json!({"status":"unavailable","reason":CATALOG_CONFIG_GUIDANCE,"key":key})),
            ));
        }
        Err(e) => return Err(AppError::Storage(e)),
    };

    let episodes = record
        .data
        .get("episodes")
        .and_then(|val| val.as_array())
        .ok_or_else(|| AppError::NotFound("no episodes found in season".into()))?;

    let Some(ep) = episodes.iter().find(|item| {
        item.get("episode")
            .or_else(|| item.get("number"))
            .and_then(|n| n.as_i64())
            .is_some_and(|n| n == episode_num)
    }) else {
        let job_key = format!("season:{key}");
        if let Some((status, reason)) = request_status(&state, &job_key) {
            if status == "ready" {
                return Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"failed","reason":format!("Episode {episode_num} is not present in the refreshed season."),"key":key}),
                    ),
                ));
            }
            let code = if status == "pending" {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            };
            let mut payload = json!({"status":status,"key":key});
            if let Some(reason) = reason {
                payload["reason"] = json!(reason);
            }
            return Ok((code, Json(payload)));
        }
        let profiles = state
            .accounts
            .list_profiles(&state.household_account_id)
            .await?;
        if profiles.is_empty() {
            return Ok((
                StatusCode::OK,
                Json(
                    json!({"status":"unavailable","reason":"Create a profile to start catalog generation.","key":key}),
                ),
            ));
        }
        if queue_season_refresh(&state, job_key, id, season_num as i32, true) {
            return Ok((
                StatusCode::ACCEPTED,
                Json(json!({"status":"pending","key":key,"episode":episode_num})),
            ));
        }
        return Ok((
            StatusCode::OK,
            Json(json!({"status":"unavailable","reason":CATALOG_CONFIG_GUIDANCE,"key":key})),
        ));
    };

    let mut ep_val = ep.clone();
    normalize_artwork_url(&mut ep_val);

    Ok((
        StatusCode::OK,
        Json(json!({
            "show_id": id,
            "data": ep_val,
            "state": record.state,
            "updated_at": record.updated_at,
            "expires_at": record.expires_at,
        })),
    ))
}

async fn internal_episode(
    State(state): State<AppState>,
    Path((id, season_num, episode_num)): Path<(i64, i64, i64)>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if id <= 0 || !(0..=10_000).contains(&season_num) || !(1..=100_000).contains(&episode_num) {
        return Err(AppError::BadRequest("invalid episode coordinates".into()));
    }

    let key = format!("tv:{id}:{season_num}");
    let now: DateTime<Utc> = SystemTime::now().into();
    let mut record = match state.catalog.get("season", &key, now).await {
        Ok(rec) => rec,
        Err(StorageError::NotFound) => {
            let job_key = format!("season:{key}");
            if let Some((status, reason)) = request_status(&state, &job_key) {
                let code = if status == "pending" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                };
                let mut payload = json!({"status":status,"key":key});
                if let Some(reason) = reason {
                    payload["reason"] = json!(reason);
                }
                return Ok((code, Json(payload)));
            }
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            if profiles.is_empty() {
                return Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"unavailable","reason":"Create a profile to start catalog generation.","key":key}),
                    ),
                ));
            }
            if queue_season_refresh(&state, job_key, id, season_num as i32, false) {
                return Ok((
                    StatusCode::ACCEPTED,
                    Json(json!({"status":"pending","key":key})),
                ));
            }
            return Ok((
                StatusCode::OK,
                Json(json!({"status":"unavailable","reason":CATALOG_CONFIG_GUIDANCE,"key":key})),
            ));
        }
        Err(e) => return Err(AppError::Storage(e)),
    };

    let episodes = record
        .data
        .get("episodes")
        .and_then(|val| val.as_array())
        .ok_or_else(|| AppError::NotFound("no episodes found in season".into()))?;

    let Some(ep) = episodes.iter().find(|item| {
        item.get("episode")
            .or_else(|| item.get("number"))
            .and_then(|n| n.as_i64())
            .is_some_and(|n| n == episode_num)
    }) else {
        let job_key = format!("season:{key}");
        if let Some((status, reason)) = request_status(&state, &job_key) {
            if status == "ready" {
                return Ok((
                    StatusCode::OK,
                    Json(
                        json!({"status":"failed","reason":format!("Episode {episode_num} is not present in the refreshed season."),"key":key}),
                    ),
                ));
            }
            let code = if status == "pending" {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            };
            let mut payload = json!({"status":status,"key":key});
            if let Some(reason) = reason {
                payload["reason"] = json!(reason);
            }
            return Ok((code, Json(payload)));
        }
        let profiles = state
            .accounts
            .list_profiles(&state.household_account_id)
            .await?;
        if profiles.is_empty() {
            return Ok((
                StatusCode::OK,
                Json(
                    json!({"status":"unavailable","reason":"Create a profile to start catalog generation.","key":key}),
                ),
            ));
        }
        if queue_season_refresh(&state, job_key, id, season_num as i32, true) {
            return Ok((
                StatusCode::ACCEPTED,
                Json(json!({"status":"pending","key":key,"episode":episode_num})),
            ));
        }
        return Ok((
            StatusCode::OK,
            Json(json!({"status":"unavailable","reason":CATALOG_CONFIG_GUIDANCE,"key":key})),
        ));
    };

    let mut ep_val = ep.clone();
    if let serde_json::Value::Object(ref mut map) = ep_val {
        map.insert("show_id".to_string(), json!(id));
    }
    record.data = ep_val;
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?),
    ))
}

async fn snapshot(State(state): State<AppState>) -> Result<Response, AppError> {
    let now: DateTime<Utc> = SystemTime::now().into();
    let mut snapshot = state.catalog.export_raw(now).await?;
    snapshot.records.retain(|record| {
        matches!(
            record.kind.as_str(),
            "title" | "season" | "shelf" | "cross_ids" | "search"
        )
    });
    let etag = format!("\"catalog-v1-{}\"", snapshot.revision);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::HeaderName::from_static("x-catalog-revision"),
        HeaderValue::from_str(&etag).map_err(|e| AppError::Internal(e.to_string()))?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

    Ok((headers, Json(snapshot)).into_response())
}

async fn artwork(Path((size, filename)): Path<(String, String)>) -> Response {
    if (size != "w300" && size != "w500" && size != "w1280")
        || filename.is_empty()
        || filename.contains('/')
        || filename.contains("..")
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_artwork_request" })),
        )
            .into_response();
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    let target_url = format!("https://image.tmdb.org/t/p/{size}/{filename}");
    if let Ok(val) = HeaderValue::from_str(&target_url) {
        headers.insert(header::LOCATION, val);
    }

    (StatusCode::FOUND, headers).into_response()
}
