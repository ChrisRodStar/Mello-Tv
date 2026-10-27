use std::time::SystemTime;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use stream_core::{
    accounts::{Preferences, Profile, ProfileInput, ProgressUpdate, Target, is_safe_identifier},
    catalog::{MediaType, Title, TitleIdentity},
};
use stream_storage::accounts::ListKind;

use crate::{error::AppError, routes::browse::normalize_artwork_url, state::AppState};

#[derive(Debug, Deserialize)]
pub struct CreateProfilePayload {
    pub name: String,
    pub color: String,
    pub preferences: Option<Preferences>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ResumeQuery {
    #[serde(rename = "type")]
    pub media_type: String,
    pub id: i64,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct HomeQuery {
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub topic: Option<String>,
    pub shelf: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecommendationsQuery {
    #[serde(rename = "type")]
    pub media_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GlobalProgressPayload {
    pub profile_id: Option<String>,
    #[serde(flatten)]
    pub update: ProgressUpdate,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        // Profiles CRUD
        .route("/api/profiles", get(list_profiles).post(create_profile))
        .route("/api/v1/profiles", get(list_profiles).post(create_profile))
        .route(
            "/api/profiles/{id}",
            get(get_profile).put(update_profile).delete(delete_profile),
        )
        .route(
            "/api/v1/profiles/{id}",
            get(get_profile).put(update_profile).delete(delete_profile),
        )
        // Preferences & Activity
        .route(
            "/api/profiles/{id}/preferences",
            put(update_profile_preferences),
        )
        .route(
            "/api/v1/profiles/{id}/preferences",
            put(update_profile_preferences),
        )
        .route("/api/profiles/{id}/activity", post(record_profile_activity))
        .route(
            "/api/v1/profiles/{id}/activity",
            post(record_profile_activity),
        )
        // Lists: Library, Favorites & Recommendation Choices
        .route("/api/profiles/{id}/library", get(list_library))
        .route("/api/v1/profiles/{id}/library", get(list_library))
        .route(
            "/api/profiles/{id}/library/{media_type}/{title_id}",
            put(put_library_title).delete(delete_library_title),
        )
        .route(
            "/api/v1/profiles/{id}/library/{media_type}/{title_id}",
            put(put_library_title).delete(delete_library_title),
        )
        .route("/api/profiles/{id}/favorites", get(list_favorites))
        .route("/api/v1/profiles/{id}/favorites", get(list_favorites))
        .route(
            "/api/profiles/{id}/favorites/{media_type}/{title_id}",
            put(put_favorites_title).delete(delete_favorites_title),
        )
        .route(
            "/api/v1/profiles/{id}/favorites/{media_type}/{title_id}",
            put(put_favorites_title).delete(delete_favorites_title),
        )
        .route(
            "/api/profiles/{id}/recommendation-hidden",
            get(list_recommendation_hidden),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-hidden",
            get(list_recommendation_hidden),
        )
        .route(
            "/api/profiles/{id}/recommendation-hidden/{media_type}/{title_id}",
            put(put_recommendation_hidden_title).delete(delete_recommendation_hidden_title),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-hidden/{media_type}/{title_id}",
            put(put_recommendation_hidden_title).delete(delete_recommendation_hidden_title),
        )
        .route(
            "/api/profiles/{id}/recommendation-watched",
            get(list_recommendation_watched),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-watched",
            get(list_recommendation_watched),
        )
        .route(
            "/api/profiles/{id}/recommendation-watched/{media_type}/{title_id}",
            put(put_recommendation_watched_title).delete(delete_recommendation_watched_title),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-watched/{media_type}/{title_id}",
            put(put_recommendation_watched_title).delete(delete_recommendation_watched_title),
        )
        .route(
            "/api/profiles/{id}/recommendation-ignored",
            get(list_recommendation_ignored),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-ignored",
            get(list_recommendation_ignored),
        )
        .route(
            "/api/profiles/{id}/recommendation-ignored/{media_type}/{title_id}",
            put(put_recommendation_ignored_title).delete(delete_recommendation_ignored_title),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-ignored/{media_type}/{title_id}",
            put(put_recommendation_ignored_title).delete(delete_recommendation_ignored_title),
        )
        // Feedback & Topics
        .route(
            "/api/profiles/{id}/recommendation-feedback",
            get(get_recommendation_feedback),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-feedback",
            get(get_recommendation_feedback),
        )
        .route(
            "/api/profiles/{id}/recommendation-topics",
            get(get_recommendation_topics),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-topics",
            get(get_recommendation_topics),
        )
        .route(
            "/api/profiles/{id}/recommendation-topics/{topic_id}",
            put(put_recommendation_topic).delete(delete_recommendation_topic),
        )
        .route(
            "/api/v1/profiles/{id}/recommendation-topics/{topic_id}",
            put(put_recommendation_topic).delete(delete_recommendation_topic),
        )
        // Home & Recommendations
        .route("/api/profiles/{id}/home", get(get_home))
        .route("/api/v1/profiles/{id}/home", get(get_home))
        .route(
            "/api/profiles/{id}/recommendations",
            get(get_recommendations),
        )
        .route(
            "/api/v1/profiles/{id}/recommendations",
            get(get_recommendations),
        )
        // Progress, Resume & History
        .route("/api/profiles/{id}/history", get(get_history))
        .route("/api/v1/profiles/{id}/history", get(get_history))
        .route("/api/profiles/{id}/resume", get(get_resume))
        .route("/api/v1/profiles/{id}/resume", get(get_resume))
        .route("/api/profiles/{id}/progress", put(update_progress))
        .route("/api/v1/profiles/{id}/progress", put(update_progress))
        .route("/api/progress", put(update_global_progress))
        .route("/api/v1/progress", put(update_global_progress))
}

fn generate_profile_id() -> Result<String, AppError> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes)
        .map_err(|e| AppError::Internal(format!("failed to generate random identifier: {e}")))?;
    let mut s = String::with_capacity(16);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    Ok(s)
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

async fn list_profiles(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let profiles = state
        .accounts
        .list_profiles(&state.household_account_id)
        .await?;
    Ok(Json(json!({ "profiles": profiles })))
}

async fn create_profile(
    State(state): State<AppState>,
    Json(payload): Json<CreateProfilePayload>,
) -> Result<(StatusCode, Json<Profile>), AppError> {
    let input = ProfileInput {
        name: payload.name,
        color: payload.color,
    };
    input.validate()?;

    let profile_id = generate_profile_id()?;
    let preferences = payload.preferences.unwrap_or_default();
    preferences.validate()?;

    let profile = Profile {
        id: profile_id,
        name: input.name,
        color: input.color,
        preferences,
    };

    let created = state
        .accounts
        .create_profile(&state.household_account_id, profile)
        .await?;

    Ok((StatusCode::CREATED, Json(created)))
}

async fn get_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Profile>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let profile = state
        .accounts
        .get_profile(&state.household_account_id, &id)
        .await?;
    Ok(Json(profile))
}

async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProfileInput>,
) -> Result<Json<Profile>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    input.validate()?;
    let updated = state
        .accounts
        .update_profile(&state.household_account_id, &id, input)
        .await?;
    Ok(Json(updated))
}

async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    state
        .accounts
        .delete_profile(&state.household_account_id, &id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_profile_preferences(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(preferences): Json<Preferences>,
) -> Result<Json<Profile>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    preferences.validate()?;
    let updated = state
        .accounts
        .update_preferences(&state.household_account_id, &id, preferences)
        .await?;
    Ok(Json(updated))
}

async fn record_profile_activity(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let now: DateTime<Utc> = SystemTime::now().into();
    state
        .accounts
        .record_activity(&state.household_account_id, &id, now)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_library(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let titles = state
        .accounts
        .list_titles(&state.household_account_id, &id, ListKind::Library)
        .await?;
    let mut payload = json!({ "titles": titles });
    normalize_artwork_url(&mut payload);
    Ok(Json(payload))
}

async fn put_library_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;
    let title = load_or_create_title(&state, identity).await;

    state
        .accounts
        .put_title(&state.household_account_id, &id, ListKind::Library, title)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_library_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;

    state
        .accounts
        .remove_title(
            &state.household_account_id,
            &id,
            ListKind::Library,
            identity,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_favorites(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let titles = state
        .accounts
        .list_titles(&state.household_account_id, &id, ListKind::Favorites)
        .await?;
    let mut payload = json!({ "titles": titles });
    normalize_artwork_url(&mut payload);
    Ok(Json(payload))
}

async fn put_favorites_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;
    let title = load_or_create_title(&state, identity).await;

    state
        .accounts
        .put_title(&state.household_account_id, &id, ListKind::Favorites, title)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_favorites_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;

    state
        .accounts
        .remove_title(
            &state.household_account_id,
            &id,
            ListKind::Favorites,
            identity,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// Recommendation Hidden
async fn list_recommendation_hidden(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let titles = state
        .accounts
        .list_titles(
            &state.household_account_id,
            &id,
            ListKind::RecommendationHidden,
        )
        .await?;
    let mut payload = json!({ "titles": titles });
    normalize_artwork_url(&mut payload);
    Ok(Json(payload))
}

async fn put_recommendation_hidden_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;
    let title = load_or_create_title(&state, identity).await;

    state
        .accounts
        .put_title(
            &state.household_account_id,
            &id,
            ListKind::RecommendationHidden,
            title,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_recommendation_hidden_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;

    state
        .accounts
        .remove_title(
            &state.household_account_id,
            &id,
            ListKind::RecommendationHidden,
            identity,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// Recommendation Watched
async fn list_recommendation_watched(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let titles = state
        .accounts
        .list_titles(
            &state.household_account_id,
            &id,
            ListKind::RecommendationWatched,
        )
        .await?;
    let mut payload = json!({ "titles": titles });
    normalize_artwork_url(&mut payload);
    Ok(Json(payload))
}

async fn put_recommendation_watched_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;
    let title = load_or_create_title(&state, identity).await;

    state
        .accounts
        .put_title(
            &state.household_account_id,
            &id,
            ListKind::RecommendationWatched,
            title,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_recommendation_watched_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;

    state
        .accounts
        .remove_title(
            &state.household_account_id,
            &id,
            ListKind::RecommendationWatched,
            identity,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// Recommendation Ignored
async fn list_recommendation_ignored(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let titles = state
        .accounts
        .list_titles(
            &state.household_account_id,
            &id,
            ListKind::RecommendationIgnored,
        )
        .await?;
    let mut payload = json!({ "titles": titles });
    normalize_artwork_url(&mut payload);
    Ok(Json(payload))
}

async fn put_recommendation_ignored_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;
    let title = load_or_create_title(&state, identity).await;

    state
        .accounts
        .put_title(
            &state.household_account_id,
            &id,
            ListKind::RecommendationIgnored,
            title,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_recommendation_ignored_title(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&media_type)?;
    let identity = TitleIdentity::new(mt, title_id)?;

    state
        .accounts
        .remove_title(
            &state.household_account_id,
            &id,
            ListKind::RecommendationIgnored,
            identity,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// Feedback & Topics
async fn get_recommendation_feedback(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let feedback = state
        .accounts
        .recommendation_feedback(&state.household_account_id, &id)
        .await?;
    let mut payload =
        serde_json::to_value(feedback).map_err(|e| AppError::Internal(e.to_string()))?;
    normalize_artwork_url(&mut payload);
    Ok(Json(payload))
}

async fn get_recommendation_topics(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let topics = state
        .accounts
        .list_recommendation_topics(&state.household_account_id, &id)
        .await?;
    Ok(Json(json!({ "topics": topics })))
}

async fn put_recommendation_topic(
    State(state): State<AppState>,
    Path((id, topic_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) || !is_safe_identifier(&topic_id) {
        return Err(AppError::BadRequest("invalid identifier".into()));
    }
    state
        .accounts
        .put_recommendation_topic(&state.household_account_id, &id, &topic_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_recommendation_topic(
    State(state): State<AppState>,
    Path((id, topic_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    if !is_safe_identifier(&id) || !is_safe_identifier(&topic_id) {
        return Err(AppError::BadRequest("invalid identifier".into()));
    }
    state
        .accounts
        .delete_recommendation_topic(&state.household_account_id, &id, &topic_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// Home & Recommendations
async fn get_home(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<HomeQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let (excluded_titles, hidden_topics) = state
        .accounts
        .get_recommendation_exclusions(&state.household_account_id, &id)
        .await?;
    let now: DateTime<Utc> = SystemTime::now().into();
    let records = state.catalog.list("shelf", 1000, now).await?;
    let media_filter = params.media_type.as_deref().unwrap_or("all");
    let shelf_filter = params.shelf.as_deref();

    let mut sections = Vec::new();
    let mut featured = Vec::new();

    for record in records {
        let mut shelf_data = record.data;
        normalize_artwork_url(&mut shelf_data);
        let shelf_id = shelf_data
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if hidden_topics.contains(shelf_id) {
            continue;
        }
        if shelf_filter.is_none()
            && (shelf_id.starts_with("onboarding-") || shelf_id.starts_with("favorites-"))
        {
            continue;
        }
        if shelf_filter.is_some_and(|target_shelf| shelf_id != target_shelf) {
            continue;
        }
        let heading = shelf_data
            .get("heading")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let raw_titles = shelf_data
            .get("titles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut filtered_titles = Vec::new();
        for t in raw_titles {
            let t_type = t.get("type").and_then(|v| v.as_str()).unwrap_or_default();
            let t_id = t.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let t_key = format!("{t_type}:{t_id}");
            if (media_filter != "all" && t_type != media_filter) || excluded_titles.contains(&t_key)
            {
                continue;
            }
            if t.get("poster")
                .and_then(|v| v.as_str())
                .is_some_and(|p| p.is_empty())
            {
                continue;
            }
            filtered_titles.push(t);
        }

        if !filtered_titles.is_empty() {
            if featured.is_empty() && shelf_filter.is_none() {
                featured.extend(filtered_titles.iter().take(5).cloned());
            }
            sections.push(json!({
                "id": shelf_id,
                "heading": heading,
                "titles": filtered_titles,
            }));
        }
    }

    use std::fmt::Write;
    let mut hasher = Sha256::new();
    if let Ok(bytes) = serde_json::to_vec(&sections) {
        hasher.update(&bytes);
    }
    let hash = hasher.finalize();
    let mut digest = String::with_capacity(64);
    for byte in &hash {
        let _ = write!(digest, "{byte:02x}");
    }
    let page_version = if digest.len() >= 16 {
        &digest[..16]
    } else {
        &digest
    };

    Ok(Json(json!({
        "sections": sections,
        "featured": featured,
        "page_version": page_version,
    })))
}

async fn get_recommendations(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<RecommendationsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let (excluded_titles, hidden_topics) = state
        .accounts
        .get_recommendation_exclusions(&state.household_account_id, &id)
        .await?;
    let now: DateTime<Utc> = SystemTime::now().into();
    let records = state.catalog.list("shelf", 1000, now).await?;
    let media_filter = params.media_type.as_deref().unwrap_or("all");

    let mut sections = Vec::new();
    for record in records {
        let mut shelf_data = record.data;
        normalize_artwork_url(&mut shelf_data);
        let shelf_id = shelf_data
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if hidden_topics.contains(shelf_id)
            || shelf_id.starts_with("onboarding-")
            || shelf_id.starts_with("favorites-")
        {
            continue;
        }
        let heading = shelf_data
            .get("heading")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let raw_titles = shelf_data
            .get("titles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut filtered_titles = Vec::new();
        for t in raw_titles {
            let t_type = t.get("type").and_then(|v| v.as_str()).unwrap_or_default();
            let t_id = t.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let t_key = format!("{t_type}:{t_id}");
            if (media_filter != "all" && t_type != media_filter) || excluded_titles.contains(&t_key)
            {
                continue;
            }
            if t.get("poster")
                .and_then(|v| v.as_str())
                .is_some_and(|p| p.is_empty())
            {
                continue;
            }
            filtered_titles.push(t);
        }

        if !filtered_titles.is_empty() {
            sections.push(json!({
                "id": shelf_id,
                "heading": heading,
                "titles": filtered_titles,
            }));
        }
    }

    Ok(Json(json!({
        "data": {
            "sections": sections,
        }
    })))
}

async fn get_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let offset = params.offset.unwrap_or(0);
    if !(0..=20_000).contains(&offset) {
        return Err(AppError::BadRequest(
            "offset must be between 0 and 20000".into(),
        ));
    }
    let records = state
        .accounts
        .history(&state.household_account_id, &id, offset)
        .await?;
    Ok(Json(json!({
        "records": records,
        "offset": offset,
        "limit": 100,
    })))
}

async fn get_resume(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ResumeQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let mt = parse_media_type(&params.media_type)?;
    let identity = TitleIdentity::new(mt, params.id)?;
    let target = Target::new(identity, params.season, params.episode)?;

    match state
        .accounts
        .get_progress(&state.household_account_id, &id, target)
        .await?
    {
        Some(progress) => Ok(Json(
            serde_json::to_value(progress).map_err(|e| AppError::Internal(e.to_string()))?,
        )),
        None => Err(AppError::NotFound("no resume progress found".into())),
    }
}

async fn update_progress(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(update): Json<ProgressUpdate>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    update.validate()?;
    let now: DateTime<Utc> = SystemTime::now().into();
    let progress = state
        .accounts
        .update_progress_at(&state.household_account_id, &id, update, now)
        .await?;

    Ok(Json(
        serde_json::to_value(progress).map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn update_global_progress(
    State(state): State<AppState>,
    Json(payload): Json<GlobalProgressPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    payload.update.validate()?;
    let profile_id = match payload.profile_id {
        Some(pid) if is_safe_identifier(&pid) => pid,
        _ => {
            let profiles = state
                .accounts
                .list_profiles(&state.household_account_id)
                .await?;
            let first = profiles
                .into_iter()
                .next()
                .ok_or_else(|| AppError::NotFound("no profiles available".into()))?;
            first.id
        }
    };

    let now: DateTime<Utc> = SystemTime::now().into();
    let progress = state
        .accounts
        .update_progress_at(
            &state.household_account_id,
            &profile_id,
            payload.update,
            now,
        )
        .await?;

    Ok(Json(
        serde_json::to_value(progress).map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn load_or_create_title(state: &AppState, identity: TitleIdentity) -> Title {
    let now: DateTime<Utc> = SystemTime::now().into();
    if let Ok(record) = state.catalog.title(&identity.key(), now).await
        && let Ok(title) = serde_json::from_value::<Title>(record.data)
    {
        return title;
    }

    Title {
        identity,
        title: String::new(),
        overview: String::new(),
        date: String::new(),
        rating: 0.0,
        poster: String::new(),
        backdrop: String::new(),
        imdb_id: String::new(),
        mal_id: 0,
        runtime: 0,
        genres: Vec::new(),
        seasons: Vec::new(),
        details: false,
    }
}
