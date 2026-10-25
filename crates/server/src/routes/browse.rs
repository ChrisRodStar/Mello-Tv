use std::time::SystemTime;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use stream_storage::StorageError;

use crate::{error::AppError, state::AppState};

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
        .route("/api/titles/{media_type}/{id}/cross-ids", put(put_cross_ids))
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

async fn status() -> Json<serde_json::Value> {
    Json(json!({ "version": "api.v1", "status": "ok" }))
}

async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
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
    let records = state.catalog.search(q, offset, now).await?;
    let mut records_val =
        serde_json::to_value(&records).map_err(|e| AppError::Internal(e.to_string()))?;
    normalize_artwork_url(&mut records_val);

    Ok(Json(json!({
        "scope": "cached_titles",
        "records": records_val,
        "offset": offset,
        "limit": params.limit.unwrap_or(50),
    })))
}

async fn search_results(
    State(state): State<AppState>,
    Query(params): Query<SearchResultQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
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
            let mut val =
                serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
            normalize_artwork_url(&mut val);
            Ok(Json(val))
        }
        Err(StorageError::NotFound) => Err(AppError::NotFound("search results not found".into())),
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
    let mut val =
        serde_json::to_value(&records).map_err(|e| AppError::Internal(e.to_string()))?;
    normalize_artwork_url(&mut val);
    Ok(Json(json!({ "records": val })))
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
) -> Result<Json<serde_json::Value>, AppError> {
    if id <= 0 || (media_type != "movie" && media_type != "tv") {
        return Err(AppError::BadRequest("invalid title coordinates".into()));
    }

    let key = format!("{media_type}:{id}");
    let now: DateTime<Utc> = SystemTime::now().into();
    match state.catalog.title(&key, now).await {
        Ok(record) => {
            let mut val =
                serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
            normalize_artwork_url(&mut val);
            Ok(Json(val))
        }
        Err(StorageError::NotFound) => Err(AppError::NotFound(format!("title {key} not found"))),
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
) -> Result<Json<serde_json::Value>, AppError> {
    if id <= 0 || !(0..=10_000).contains(&season_num) {
        return Err(AppError::BadRequest("invalid season coordinates".into()));
    }

    let key = format!("tv:{id}:{season_num}");
    let now: DateTime<Utc> = SystemTime::now().into();
    match state.catalog.get("season", &key, now).await {
        Ok(record) => {
            let mut val =
                serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?;
            normalize_artwork_url(&mut val);
            Ok(Json(val))
        }
        Err(StorageError::NotFound) => Err(AppError::NotFound(format!("season {key} not found"))),
        Err(e) => Err(AppError::Storage(e)),
    }
}

async fn episode(
    State(state): State<AppState>,
    Path((id, season_num, episode_num)): Path<(i64, i64, i64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    if id <= 0 || !(0..=10_000).contains(&season_num) || !(1..=100_000).contains(&episode_num) {
        return Err(AppError::BadRequest("invalid episode coordinates".into()));
    }

    let key = format!("tv:{id}:{season_num}");
    let now: DateTime<Utc> = SystemTime::now().into();
    let record = match state.catalog.get("season", &key, now).await {
        Ok(rec) => rec,
        Err(StorageError::NotFound) => {
            return Err(AppError::NotFound(format!("season {key} not found")));
        }
        Err(e) => return Err(AppError::Storage(e)),
    };

    let episodes = record
        .data
        .get("episodes")
        .and_then(|val| val.as_array())
        .ok_or_else(|| AppError::NotFound("no episodes found in season".into()))?;

    let ep = episodes
        .iter()
        .find(|item| {
            item.get("episode")
                .or_else(|| item.get("number"))
                .and_then(|n| n.as_i64())
                .is_some_and(|n| n == episode_num)
        })
        .ok_or_else(|| {
            AppError::NotFound(format!("episode {episode_num} not found in season {key}"))
        })?;

    let mut ep_val = ep.clone();
    normalize_artwork_url(&mut ep_val);

    Ok(Json(json!({
        "show_id": id,
        "data": ep_val,
        "state": record.state,
        "updated_at": record.updated_at,
        "expires_at": record.expires_at,
    })))
}

async fn internal_episode(
    State(state): State<AppState>,
    Path((id, season_num, episode_num)): Path<(i64, i64, i64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    if id <= 0 || !(0..=10_000).contains(&season_num) || !(1..=100_000).contains(&episode_num) {
        return Err(AppError::BadRequest("invalid episode coordinates".into()));
    }

    let key = format!("tv:{id}:{season_num}");
    let now: DateTime<Utc> = SystemTime::now().into();
    let mut record = match state.catalog.get("season", &key, now).await {
        Ok(rec) => rec,
        Err(StorageError::NotFound) => {
            return Err(AppError::NotFound(format!("season {key} not found")));
        }
        Err(e) => return Err(AppError::Storage(e)),
    };

    let episodes = record
        .data
        .get("episodes")
        .and_then(|val| val.as_array())
        .ok_or_else(|| AppError::NotFound("no episodes found in season".into()))?;

    let ep = episodes
        .iter()
        .find(|item| {
            item.get("episode")
                .or_else(|| item.get("number"))
                .and_then(|n| n.as_i64())
                .is_some_and(|n| n == episode_num)
        })
        .ok_or_else(|| {
            AppError::NotFound(format!("episode {episode_num} not found in season {key}"))
        })?;

    let mut ep_val = ep.clone();
    if let serde_json::Value::Object(ref mut map) = ep_val {
        map.insert("show_id".to_string(), json!(id));
    }
    record.data = ep_val;
    Ok(Json(
        serde_json::to_value(record).map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn snapshot(State(state): State<AppState>) -> Result<Response, AppError> {
    let now: DateTime<Utc> = SystemTime::now().into();
    let snapshot = state.catalog.export_raw(now).await?;
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
