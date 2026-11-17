use std::{collections::HashSet, time::SystemTime};

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
    accounts::{
        Preferences, Profile, ProfileInput, ProfilePlaybackSettings, ProgressUpdate, Target,
        is_safe_identifier,
    },
    catalog::{
        ContentPreference, Favorite, MediaType, RecommendationInputs, Shelf, Title, TitleIdentity,
        ViewingSignal,
    },
};
use stream_storage::accounts::ListKind;

use crate::{
    catalog_service::{rotation_day, rotation_key},
    error::AppError,
    routes::browse::normalize_artwork_url,
    state::{AppState, CATALOG_CONFIG_GUIDANCE, CatalogGenerationStatus},
};

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
struct WatchedTarget {
    season: Option<i64>,
    episode: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct HomeQuery {
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub topic: Option<String>,
    pub shelf: Option<String>,
    #[serde(default)]
    pub expand: bool,
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
        .route("/api/profiles/{id}/catalog-status", get(get_catalog_status))
        .route(
            "/api/v1/profiles/{id}/catalog-status",
            get(get_catalog_status),
        )
        .route(
            "/api/v1/profiles/{id}/playback-settings",
            get(get_playback_settings).put(put_playback_settings),
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
        .route(
            "/api/v1/profiles/{id}/recommendations/refresh",
            get(recommendation_refresh_status).post(refresh_profile_recommendations),
        )
        // Progress, Resume & History
        .route("/api/profiles/{id}/history", get(get_history))
        .route("/api/v1/profiles/{id}/history", get(get_history))
        .route(
            "/api/v1/profiles/{id}/continue-watching/{media_type}/{title_id}",
            axum::routing::delete(remove_from_continue_watching),
        )
        .route(
            "/api/v1/profiles/{id}/watched/{media_type}/{title_id}",
            put(mark_watched),
        )
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

    queue_profile_warmup(state.clone(), created.clone(), false);
    if created.preferences.completed {
        queue_profile_recommendations(state.clone(), created.id.clone(), true, false).await;
    }

    Ok((StatusCode::CREATED, Json(created)))
}

fn queue_profile_warmup(state: AppState, profile: Profile, preserve_stale: bool) {
    let profile_id = profile.id.clone();
    let worker = match state.catalog_worker.clone() {
        Some(worker) if worker.available() => worker,
        _ => {
            if preserve_stale {
                state.set_catalog_generation_stale(
                    &profile_id,
                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                );
            } else {
                state.set_catalog_generation(
                    &profile_id,
                    CatalogGenerationStatus::Unavailable,
                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                );
            }
            return;
        }
    };
    let key = format!("profile:{profile_id}:warm");
    if !state.ensure_catalog_job(&key) {
        return;
    }
    if !preserve_stale {
        state.set_catalog_generation(&profile_id, CatalogGenerationStatus::Pending, None);
    }
    let state_for_job = state;
    tokio::spawn(async move {
        let result = worker.warm_profile(profile.clone()).await;
        let changed_during_run = state_for_job.finish_catalog_job(&key);
        let latest = state_for_job
            .accounts
            .get_profile(&state_for_job.household_account_id, &profile_id)
            .await
            .ok();
        let Some(latest) = latest else {
            let _ = state_for_job
                .catalog
                .delete("profile_generation", &profile_id)
                .await;
            let _ = state_for_job
                .catalog
                .delete("recommendations", &profile_id)
                .await;
            state_for_job.clear_profile_catalog_state(&profile_id);
            return;
        };
        let old_fingerprint = crate::catalog_service::profile_generation_fingerprint(&profile).ok();
        let latest_fingerprint =
            crate::catalog_service::profile_generation_fingerprint(&latest).ok();
        if changed_during_run || old_fingerprint != latest_fingerprint {
            queue_profile_warmup(state_for_job.clone(), latest, false);
            return;
        }
        match result {
            Ok(crate::catalog_service::GenerationStatus::Ready) => state_for_job.set_catalog_generation(&profile_id, CatalogGenerationStatus::Ready, None),
            Ok(crate::catalog_service::GenerationStatus::Unavailable) if preserve_stale => state_for_job.set_catalog_generation_stale(&profile_id, Some(CATALOG_CONFIG_GUIDANCE.to_owned())),
            Ok(crate::catalog_service::GenerationStatus::Unavailable) => state_for_job.set_catalog_generation(&profile_id, CatalogGenerationStatus::Unavailable, Some(CATALOG_CONFIG_GUIDANCE.to_owned())),
            Err(_) if preserve_stale => state_for_job.set_catalog_generation_stale(&profile_id, Some("Catalog refresh failed; showing the last saved catalog.".to_owned())),
            Err(_) => state_for_job.set_catalog_generation(&profile_id, CatalogGenerationStatus::Failed, Some("Catalog generation failed. Check the TMDB key in Mello TV Settings > Server and try again.".to_owned())),
        }
    });
}

async fn get_catalog_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let profile = state
        .accounts
        .get_profile(&state.household_account_id, &id)
        .await?;
    let now: DateTime<Utc> = SystemTime::now().into();
    if profile.preferences.completed {
        let inputs = recommendation_inputs(&state, &id).await;
        let cached = state.catalog.get("recommendations", &id, now).await.ok();
        let matching = match (cached, inputs.as_ref()) {
            (Some(record), Some(inputs)) => {
                crate::catalog_service::recommendation_fingerprint(&id, inputs)
                    .is_ok_and(|fingerprint| {
                        record
                            .data
                            .get("fingerprint")
                            .and_then(|value| value.as_str())
                            == Some(fingerprint.as_str())
                    })
                    .then_some(record)
            }
            _ => None,
        };
        if let Some(record) = matching {
            if record.expires_at > now {
                state.set_catalog_generation(&id, CatalogGenerationStatus::Ready, None);
            } else {
                let configured = state
                    .catalog_worker
                    .as_ref()
                    .is_some_and(|worker| worker.available());
                let reason = if configured {
                    None
                } else {
                    Some(CATALOG_CONFIG_GUIDANCE.to_owned())
                };
                state.set_catalog_generation_stale(&id, reason);
                if configured {
                    queue_profile_recommendations(state.clone(), id.clone(), false, true).await;
                }
            }
        } else {
            let current = state.catalog_generation(&id);
            let cooldown_active = matches!(
                current.status,
                CatalogGenerationStatus::Failed | CatalogGenerationStatus::Unavailable
            ) && current.updated_at.is_some_and(|updated| {
                SystemTime::now()
                    .duration_since(updated.into())
                    .is_ok_and(|age| age < std::time::Duration::from_secs(30))
            });
            if !state.is_catalog_job_running(&format!("profile:{id}:recommendations"))
                && !cooldown_active
            {
                if state
                    .catalog_worker
                    .as_ref()
                    .is_some_and(|worker| worker.available())
                {
                    queue_profile_recommendations(state.clone(), id.clone(), false, false).await;
                } else {
                    state.set_catalog_generation(
                        &id,
                        CatalogGenerationStatus::Unavailable,
                        Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                    );
                }
            }
        }
    } else {
        let marker = state.catalog.get("profile_generation", &id, now).await.ok();
        let matching = marker.as_ref().is_some_and(|record| {
            crate::catalog_service::profile_generation_fingerprint(&profile).is_ok_and(
                |fingerprint| {
                    record
                        .data
                        .get("fingerprint")
                        .and_then(|value| value.as_str())
                        == Some(fingerprint.as_str())
                },
            )
        });
        if let Some(record) = marker.filter(|_| matching) {
            if record.expires_at > now {
                state.set_catalog_generation(&id, CatalogGenerationStatus::Ready, None);
            } else {
                let configured = state
                    .catalog_worker
                    .as_ref()
                    .is_some_and(|worker| worker.available());
                state.set_catalog_generation_stale(
                    &id,
                    if configured {
                        None
                    } else {
                        Some(CATALOG_CONFIG_GUIDANCE.to_owned())
                    },
                );
                if configured {
                    queue_profile_warmup(state.clone(), profile, true);
                }
            }
        } else {
            let current = state.catalog_generation(&id);
            let cooldown_active = matches!(
                current.status,
                CatalogGenerationStatus::Failed | CatalogGenerationStatus::Unavailable
            ) && current.updated_at.is_some_and(|updated| {
                SystemTime::now()
                    .duration_since(updated.into())
                    .is_ok_and(|age| age < std::time::Duration::from_secs(30))
            });
            if !state.is_catalog_job_running(&format!("profile:{id}:warm")) && !cooldown_active {
                if state
                    .catalog_worker
                    .as_ref()
                    .is_some_and(|worker| worker.available())
                {
                    queue_profile_warmup(state.clone(), profile, false);
                } else {
                    state.set_catalog_generation(
                        &id,
                        CatalogGenerationStatus::Unavailable,
                        Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                    );
                }
            }
        }
    }
    Ok(Json(
        serde_json::to_value(state.catalog_generation(&id))
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn recommendation_inputs(state: &AppState, profile_id: &str) -> Option<RecommendationInputs> {
    let profile = state
        .accounts
        .get_profile(&state.household_account_id, profile_id)
        .await
        .ok()?;
    if !profile.preferences.completed {
        return None;
    }
    let ignored: std::collections::HashSet<String> = state
        .accounts
        .list_titles(
            &state.household_account_id,
            profile_id,
            ListKind::RecommendationIgnored,
        )
        .await
        .ok()?
        .into_iter()
        .map(|title| title.identity.key())
        .collect();
    let titles = state
        .accounts
        .list_titles(&state.household_account_id, profile_id, ListKind::Favorites)
        .await
        .ok()?;
    let favorites = titles
        .into_iter()
        .filter(|title| !ignored.contains(&title.identity.key()))
        .map(|title| Favorite {
            identity: title.identity,
            title: title.title,
        })
        .collect();
    let mut history = state
        .accounts
        .history(&state.household_account_id, profile_id, 0)
        .await
        .unwrap_or_default();
    history.retain(|progress| !ignored.contains(&progress.target.identity().key()));
    let mut grouped_signals = std::collections::HashMap::<String, ViewingSignal>::new();
    for progress in history.iter().take(100) {
        let identity = *progress.target.identity();
        let key = identity.key();
        let signal = grouped_signals.entry(key).or_insert_with(|| ViewingSignal {
            identity,
            title: String::new(),
            watched_seconds: 0.0,
            duration_seconds: 0.0,
            completed: false,
            meaningful_episodes: 0,
            updated_at: progress.updated_at,
        });
        signal.watched_seconds += progress.watched_seconds;
        signal.duration_seconds = signal.duration_seconds.max(progress.duration_seconds);
        signal.completed |= progress.completed;
        if progress.target.season().is_some() && progress.completed {
            signal.meaningful_episodes = signal.meaningful_episodes.saturating_add(1);
        }
        signal.updated_at = signal.updated_at.max(progress.updated_at);
    }
    let signals = grouped_signals.into_values().collect();
    let cutoff: DateTime<Utc> = SystemTime::now().into();
    let activity_days = history
        .iter()
        .filter(|progress| {
            let age = cutoff.signed_duration_since(progress.updated_at);
            age >= chrono::Duration::zero() && age <= chrono::Duration::days(31)
        })
        .map(|progress| progress.updated_at.format("%Y-%m-%d").to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .rev()
        .take(30)
        .collect();
    Some(RecommendationInputs {
        completed: profile.preferences.completed,
        format: match profile.preferences.format {
            stream_core::accounts::PreferredFormat::Both => ContentPreference::Both,
            stream_core::accounts::PreferredFormat::Movie => ContentPreference::Movie,
            stream_core::accounts::PreferredFormat::Tv => ContentPreference::Tv,
        },
        genres: profile.preferences.genres,
        favorites,
        signals,
        activity_days,
    })
}

async fn queue_profile_recommendations(
    state: AppState,
    profile_id: String,
    mark_dirty: bool,
    preserve_stale: bool,
) -> bool {
    let worker = match state.catalog_worker.clone() {
        Some(worker) if worker.available() => worker,
        _ => {
            if preserve_stale {
                state.set_catalog_generation_stale(
                    &profile_id,
                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                );
            } else {
                state.set_catalog_generation(
                    &profile_id,
                    CatalogGenerationStatus::Unavailable,
                    Some(CATALOG_CONFIG_GUIDANCE.to_owned()),
                );
            }
            return false;
        }
    };
    let Some(inputs) = recommendation_inputs(&state, &profile_id).await else {
        return false;
    };
    let job_key = format!("profile:{profile_id}:recommendations");
    let started = if mark_dirty {
        state.try_start_catalog_job(&job_key)
    } else {
        state.ensure_catalog_job(&job_key)
    };
    if !started {
        if !preserve_stale {
            state.set_catalog_generation(&profile_id, CatalogGenerationStatus::Pending, None);
        }
        return state.is_catalog_job_running(&job_key);
    }
    if !preserve_stale {
        state.set_catalog_generation(&profile_id, CatalogGenerationStatus::Pending, None);
    }
    let state_for_job = state;
    tokio::spawn(async move {
        let mut current_inputs = inputs;
        loop {
            let result = worker
                .refresh_recommendations(&profile_id, current_inputs)
                .await;
            let changed_during_run = state_for_job.finish_catalog_job(&job_key);
            if state_for_job
                .accounts
                .get_profile(&state_for_job.household_account_id, &profile_id)
                .await
                .is_err()
            {
                let _ = state_for_job
                    .catalog
                    .delete("recommendations", &profile_id)
                    .await;
                let _ = state_for_job
                    .catalog
                    .delete("profile_generation", &profile_id)
                    .await;
                state_for_job.clear_profile_catalog_state(&profile_id);
                return;
            };
            let latest_inputs = recommendation_inputs(&state_for_job, &profile_id).await;
            let record_fingerprint = match &result {
                Ok(record) => record.data.fingerprint.as_str(),
                Err(_) => "",
            };
            let successful_result_mismatch = result.is_ok()
                && !latest_inputs.as_ref().is_some_and(|latest| {
                    crate::catalog_service::recommendation_fingerprint(&profile_id, latest)
                        .is_ok_and(|fingerprint| fingerprint == record_fingerprint)
                });
            if changed_during_run || successful_result_mismatch {
                if let Some(latest_inputs) = latest_inputs {
                    if state_for_job.try_start_catalog_job(&job_key) {
                        current_inputs = latest_inputs;
                        continue;
                    }
                    if !state_for_job.is_catalog_job_running(&job_key) {
                        state_for_job.set_catalog_request_status(
                            &job_key,
                            CatalogGenerationStatus::Failed,
                            Some(
                                "Recommendation generation is busy. Try again shortly.".to_owned(),
                            ),
                        );
                    }
                } else {
                    state_for_job.set_catalog_request_status(
                        &job_key,
                        CatalogGenerationStatus::Unavailable,
                        Some(
                            "Complete profile preferences to generate recommendations.".to_owned(),
                        ),
                    );
                    let _ = state_for_job
                        .catalog
                        .delete("recommendations", &profile_id)
                        .await;
                    state_for_job.set_catalog_generation(
                        &profile_id,
                        CatalogGenerationStatus::Unavailable,
                        Some(
                            "Complete profile preferences to generate recommendations.".to_owned(),
                        ),
                    );
                }
                return;
            }
            let (status, reason) = match &result {
                Ok(record) if record.data.sections.is_empty() => (
                    CatalogGenerationStatus::Unavailable,
                    Some("No matching catalog titles are available yet.".to_owned()),
                ),
                Ok(_) => (CatalogGenerationStatus::Ready, None),
                Err(_) => (
                    CatalogGenerationStatus::Failed,
                    Some("Couldn’t regenerate recommendations. Check the catalog configuration and try again.".to_owned()),
                ),
            };
            state_for_job.set_catalog_request_status(&job_key, status, reason);
            if let Some(outcome) = state_for_job.catalog_request_status(&job_key)
                && let Ok(value) = serde_json::to_value(outcome)
            {
                let _ = state_for_job
                    .accounts
                    .put_profile_metadata(
                        &state_for_job.household_account_id,
                        &profile_id,
                        "recommendation-outcome",
                        value,
                    )
                    .await;
            }
            match result {
                Ok(record) if record.data.sections.is_empty() && preserve_stale => state_for_job.set_catalog_generation_stale(&profile_id, Some("No new matching titles; showing the last saved recommendations.".to_owned())),
                Ok(record) if record.data.sections.is_empty() => state_for_job.set_catalog_generation(&profile_id, CatalogGenerationStatus::Unavailable, Some("No matching catalog titles are available yet.".to_owned())),
                Ok(_) => state_for_job.set_catalog_generation(&profile_id, CatalogGenerationStatus::Ready, None),
                Err(_) if preserve_stale => state_for_job.set_catalog_generation_stale(&profile_id, Some("Personalized catalog refresh failed; showing the last saved recommendations.".to_owned())),
                Err(_) => state_for_job.set_catalog_generation(&profile_id, CatalogGenerationStatus::Failed, Some("Personalized catalog refresh failed. Check the TMDB key in Mello TV Settings > Server and try again.".to_owned())),
            }
            break;
        }
    });
    true
}

/// Explicit regeneration preserves the saved catalog until a replacement is ready.
async fn refresh_profile_recommendations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let profile = state
        .accounts
        .get_profile(&state.household_account_id, &id)
        .await?;
    if !profile.preferences.completed {
        return Err(AppError::BadRequest(
            "Complete this profile’s taste setup first.".into(),
        ));
    }
    if !state
        .catalog_worker
        .as_ref()
        .is_some_and(|worker| worker.available())
    {
        return Err(AppError::Unprocessable(CATALOG_CONFIG_GUIDANCE.to_owned()));
    }
    // Repeated clicks join the existing job rather than scheduling another rebuild.
    if !queue_profile_recommendations(state.clone(), id, false, true).await {
        return Err(AppError::Conflict(
            "Recommendation generation is busy. Try again shortly.".into(),
        ));
    }
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "pending" }))))
}

async fn recommendation_refresh_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    state
        .accounts
        .get_profile(&state.household_account_id, &id)
        .await?;
    let key = format!("profile:{id}:recommendations");
    if state.is_catalog_job_running(&key) {
        return Ok(Json(json!({ "status": "pending" })));
    }
    // Unlike catalog-status, this records the job outcome even if an older cache is usable.
    Ok(Json(match state.catalog_request_status(&key) {
        Some(status) => serde_json::to_value(status)?,
        None => state
            .accounts
            .get_profile_metadata(&state.household_account_id, &id, "recommendation-outcome")
            .await?
            .unwrap_or_else(|| json!({ "status": "idle" })),
    }))
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
        .get_profile(&state.household_account_id, &id)
        .await?;
    state.catalog.delete("recommendations", &id).await?;
    state.catalog.delete("profile_generation", &id).await?;
    state
        .accounts
        .delete_profile(&state.household_account_id, &id)
        .await?;
    // A generation task may have completed between the first cleanup and account deletion.
    state.catalog.delete("recommendations", &id).await?;
    state.catalog.delete("profile_generation", &id).await?;
    state.clear_profile_catalog_state(&id);
    Ok(StatusCode::NO_CONTENT)
}

async fn get_playback_settings(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProfilePlaybackSettings>, AppError> {
    let value = state
        .accounts
        .get_profile_metadata(&state.household_account_id, &id, "playback-settings")
        .await?;
    Ok(Json(match value {
        Some(value) => serde_json::from_value(value)?,
        None => ProfilePlaybackSettings::default(),
    }))
}

async fn put_playback_settings(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(settings): Json<ProfilePlaybackSettings>,
) -> Result<Json<ProfilePlaybackSettings>, AppError> {
    settings.validate()?;
    state
        .accounts
        .put_profile_metadata(
            &state.household_account_id,
            &id,
            "playback-settings",
            serde_json::to_value(&settings)?,
        )
        .await?;
    Ok(Json(settings))
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
    let refresh = preferences.completed;
    let updated = state
        .accounts
        .update_preferences(&state.household_account_id, &id, preferences)
        .await?;
    if refresh {
        queue_profile_recommendations(state.clone(), id, true, false).await;
    }
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
    queue_profile_recommendations(state.clone(), id, true, false).await;
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
    queue_profile_recommendations(state.clone(), id, true, true).await;
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
    queue_profile_recommendations(state.clone(), id, true, true).await;
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
    let media = params.media_type.as_deref().unwrap_or("all");
    let shelf_filter = params.shelf.as_deref();
    let mut candidates = Vec::<Shelf>::new();

    // Keep exact-seed recommendations available to title details. Home composition
    // works on a copy of the same cached shelves and applies page-wide constraints.
    if let Ok(record) = state.catalog.get("recommendations", &id, now).await
        && let Some(sections) = record
            .data
            .get("sections")
            .and_then(|value| value.as_array())
    {
        candidates.extend(
            sections
                .iter()
                .filter_map(|section| serde_json::from_value(section.clone()).ok()),
        );
    }
    let records = state.catalog.list("shelf", 1000, now).await?;
    candidates.extend(records.into_iter().filter_map(|record| {
        serde_json::from_value::<Shelf>(record.data)
            .ok()
            .filter(|shelf| {
                shelf_filter.is_some()
                    || !(shelf.id.starts_with("onboarding-") || shelf.id.starts_with("favorites-"))
            })
    }));

    if params.expand
        && let Some(target) = shelf_filter
        && !hidden_topics.contains(target)
        && let Some(shelf) = candidates.iter_mut().find(|shelf| shelf.id == target)
        && let Some(worker) = state.catalog_worker.as_ref()
    {
        shelf.titles.extend(worker.shelf_expansion(shelf).await);
    }

    let mut shelf_ids = HashSet::new();
    let media_prefix = format!("{media}:");
    candidates.retain_mut(|shelf| {
        if hidden_topics.contains(&shelf.id)
            || shelf_filter.is_some_and(|target| shelf.id != target)
        {
            return false;
        }
        let mut seen = HashSet::new();
        shelf.titles.retain(|title| {
            let key = title.identity.key();
            (media == "all" || key.starts_with(&media_prefix))
                && !excluded_titles.contains(&key)
                && seen.insert(key)
        });
        !shelf.titles.is_empty() && shelf_ids.insert(shelf.id.clone())
    });

    let (sections, featured) = if shelf_filter.is_some() {
        (candidates, Vec::new())
    } else {
        compose_home(candidates, &format!("{id}:{media}"), rotation_day(now))
    };
    let mut payload = json!({
        "sections": sections,
        "featured": featured,
        "catalog_state": state.catalog_generation(&id),
    });
    normalize_artwork_url(&mut payload);
    let digest = Sha256::digest(payload["sections"].to_string() + &payload["featured"].to_string());
    let page_version: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    payload["page_version"] = json!(page_version);
    Ok(Json(payload))
}

fn is_anime_shelf(shelf: &Shelf) -> bool {
    shelf.id.ends_with("-animation") && matches!(shelf.kind.as_str(), "taste" | "discover")
}

fn home_shelf_order(shelf: &Shelf) -> u8 {
    if shelf.id.starts_with("personalized-") {
        return 0;
    }
    if shelf.id == "trending" {
        return 1;
    }
    if is_anime_shelf(shelf) {
        return 6;
    }
    if matches!(shelf.id.as_str(), "movie-popular" | "tv-popular") {
        return 4;
    }
    match shelf.kind.as_str() {
        "because" if shelf.reason == "watched" => 2,
        "because" | "taste" => 3,
        _ => 5,
    }
}

fn compose_home(
    mut candidates: Vec<Shelf>,
    profile_id: &str,
    day: i64,
) -> (Vec<Shelf>, Vec<Title>) {
    const ROW_SIZE: usize = 12;
    // Seven carousel cards plus a separate editorial spotlight.
    const FEATURED_LIMIT: usize = 8;
    const MAX_ROWS: usize = 28;
    const ANIME_IN_MIXED_ROWS: usize = 5;
    let anime_ids: HashSet<_> = candidates
        .iter()
        .filter(|shelf| is_anime_shelf(shelf))
        .flat_map(|shelf| &shelf.titles)
        .map(|title| title.identity.key())
        .collect();
    let is_anime = |title: &Title| {
        title.genres.iter().any(|genre| genre == "Anime")
            || anime_ids.contains(&title.identity.key())
    };
    candidates.sort_by_cached_key(|shelf| {
        (
            home_shelf_order(shelf),
            rotation_key(profile_id, day, &shelf.id),
        )
    });

    let mut used_titles = HashSet::new();
    let mut sections = Vec::new();
    let mut topic_counts = std::collections::HashMap::new();
    let mut anime_in_mixed_rows = 0;
    let mut has_anime_shelf = false;

    for shelf in &candidates {
        let anime_shelf = is_anime_shelf(shelf);
        let (group, limit) = match shelf.kind.as_str() {
            "because" | "taste" => (shelf.kind.as_str(), 3),
            "recent" | "discover" | "personal" | "trending" => (shelf.kind.as_str(), MAX_ROWS),
            _ => ("shared", 6),
        };
        let count = topic_counts.entry(group).or_insert(0);
        if (anime_shelf && has_anime_shelf) || (!anime_shelf && *count >= limit) {
            continue;
        }
        let mut unique: Vec<_> = shelf
            .titles
            .iter()
            .filter(|title| !used_titles.contains(&title.identity.key()))
            .collect();
        if home_shelf_order(shelf) == 0 {
            // Keep four leading matches, then sample from the next best
            // candidates and from deeper in the same ranked pool.
            let deeper = unique.len().min(20);
            if let Some(nearby) = unique.get_mut(4..deeper) {
                nearby.sort_by_cached_key(|title| {
                    rotation_key(profile_id, day, &title.identity.key())
                });
            }
            if unique.len() > deeper {
                let mut tail = unique.split_off(deeper);
                tail.sort_by_cached_key(|title| {
                    rotation_key(profile_id, day, &title.identity.key())
                });
                unique.splice(8..8, tail.into_iter().take(4));
            }
        } else if matches!(shelf.kind.as_str(), "recent" | "discover" | "taste") {
            unique.sort_by_cached_key(|title| rotation_key(profile_id, day, &title.identity.key()));
        }
        let mut row_anime = 0;
        let anime_allowance = (ANIME_IN_MIXED_ROWS - anime_in_mixed_rows)
            .min(if shelf.kind == "personal" { 2 } else { 1 });
        let row_size = if anime_shelf { 6 } else { ROW_SIZE };
        let titles: Vec<_> = unique
            .into_iter()
            .filter(|title| {
                if anime_shelf || !is_anime(title) {
                    return true;
                }
                if row_anime == anime_allowance {
                    return false;
                }
                row_anime += 1;
                true
            })
            .take(row_size)
            .cloned()
            .collect();
        if titles.is_empty() || (titles.len() < 3 && home_shelf_order(shelf) > 1) {
            continue;
        }
        anime_in_mixed_rows += row_anime;
        has_anime_shelf |= anime_shelf;
        used_titles.extend(titles.iter().map(|title| title.identity.key()));
        sections.push(Shelf {
            titles,
            ..shelf.clone()
        });
        *count += usize::from(!anime_shelf);
        if sections.len() == MAX_ROWS {
            break;
        }
    }

    // Sparse or offline catalogs should still show what is available.
    if sections.is_empty()
        && let Some(shelf) = candidates.first()
    {
        used_titles.extend(shelf.titles.iter().map(|title| title.identity.key()));
        sections.push(shelf.clone());
    }

    let mut featured = Vec::new();
    // Draw at most one title per row per pass to mix topics in the carousel.
    let mut remaining: Vec<_> = candidates
        .into_iter()
        .map(|shelf| {
            shelf
                .titles
                .into_iter()
                .filter(|title| !title.backdrop.is_empty() && !is_anime(title))
        })
        .collect();
    while featured.len() < FEATURED_LIMIT {
        let start = featured.len();
        for titles in &mut remaining {
            if let Some(title) = titles.find(|title| used_titles.insert(title.identity.key())) {
                featured.push(title);
            }
            if featured.len() == FEATURED_LIMIT {
                break;
            }
        }
        if featured.len() == start {
            break;
        }
    }
    (sections, featured)
}

async fn get_recommendations(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<RecommendationsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !is_safe_identifier(&id) {
        return Err(AppError::BadRequest("invalid profile identifier".into()));
    }
    let profile = state
        .accounts
        .get_profile(&state.household_account_id, &id)
        .await?;
    let media_filter = params.media_type.as_deref().unwrap_or("all");
    if !profile.preferences.completed {
        return Ok(Json(
            json!({"status":"unavailable","reason":"Complete profile preferences to generate recommendations.","data":{"sections":[],"pool":[]}}),
        ));
    }
    let (excluded_titles, hidden_topics) = state
        .accounts
        .get_recommendation_exclusions(&state.household_account_id, &id)
        .await?;
    let now: DateTime<Utc> = SystemTime::now().into();
    let cached = state.catalog.get("recommendations", &id, now).await.ok();
    let inputs = recommendation_inputs(&state, &id).await;
    let matching = match (cached, inputs.as_ref()) {
        (Some(record), Some(inputs)) => {
            crate::catalog_service::recommendation_fingerprint(&id, inputs)
                .is_ok_and(|fingerprint| {
                    record
                        .data
                        .get("fingerprint")
                        .and_then(|value| value.as_str())
                        == Some(fingerprint.as_str())
                })
                .then_some(record)
        }
        _ => None,
    };
    let status;
    let mut reason: Option<String> = None;
    if let Some(record) = matching {
        let stale = record.expires_at <= now || record.state == "stale";
        if stale {
            status = "ready";
            let configured = state
                .catalog_worker
                .as_ref()
                .is_some_and(|worker| worker.available());
            reason = if configured {
                None
            } else {
                Some(CATALOG_CONFIG_GUIDANCE.to_owned())
            };
            state.set_catalog_generation_stale(&id, reason.clone());
            if configured {
                queue_profile_recommendations(state.clone(), id.clone(), false, true).await;
            }
        } else {
            let generation = state.catalog_generation(&id);
            status = if generation.status == CatalogGenerationStatus::Pending
                && state.is_catalog_job_running(&format!("profile:{id}:recommendations"))
            {
                "pending"
            } else {
                "ready"
            };
        }

        // Preserve the persisted CatalogRecord envelope and fingerprint. Filter its
        // data in place so both web and iOS can decode their SavedRecord DTOs.
        let mut response =
            serde_json::to_value(&record).map_err(|e| AppError::Internal(e.to_string()))?;
        let include_title = |title: &serde_json::Value| {
            let media_ok = media_filter == "all"
                || title.get("type").and_then(|value| value.as_str()) == Some(media_filter);
            let key = format!(
                "{}:{}",
                title
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
                title
                    .get("id")
                    .and_then(|value| value.as_i64())
                    .unwrap_or(0)
            );
            media_ok && !excluded_titles.contains(&key)
        };
        let pool = response["data"]["pool"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(&include_title)
            .collect::<Vec<_>>();
        let raw_sections = response["data"]["sections"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut sections = Vec::new();
        for mut section in raw_sections {
            let shelf_id = section
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if hidden_topics.contains(shelf_id) {
                continue;
            }
            let titles = section
                .get("titles")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(&include_title)
                .collect::<Vec<_>>();
            if titles.is_empty() {
                continue;
            }
            if let Some(object) = section.as_object_mut() {
                object.insert("titles".to_owned(), json!(titles));
            }
            sections.push(section);
        }
        response["data"]["pool"] = json!(pool);
        response["data"]["sections"] = json!(sections);
        response["status"] = json!(status);
        response["stale"] = json!(stale);
        if let Some(reason) = reason {
            response["reason"] = json!(reason);
        }
        normalize_artwork_url(&mut response);
        return Ok(Json(response));
    }

    let generation = state.catalog_generation(&id);
    let cooldown_active = matches!(
        generation.status,
        CatalogGenerationStatus::Failed | CatalogGenerationStatus::Unavailable
    ) && generation.updated_at.is_some_and(|updated| {
        SystemTime::now()
            .duration_since(updated.into())
            .is_ok_and(|age| age < std::time::Duration::from_secs(30))
    });
    let (status, reason) = if cooldown_active {
        (
            match generation.status {
                CatalogGenerationStatus::Unavailable => "unavailable",
                _ => "failed",
            },
            generation.reason,
        )
    } else if state
        .catalog_worker
        .as_ref()
        .is_some_and(|worker| worker.available())
    {
        queue_profile_recommendations(state.clone(), id.clone(), false, false).await;
        ("pending", None)
    } else {
        ("unavailable", Some(CATALOG_CONFIG_GUIDANCE.to_owned()))
    };
    let mut payload = json!({"status":status,"stale":false,"data":{"sections":[],"pool":[]}});
    if let Some(reason) = reason {
        payload["reason"] = json!(reason);
    }
    Ok(Json(payload))
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

async fn remove_from_continue_watching(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
) -> Result<StatusCode, AppError> {
    let identity = TitleIdentity::new(parse_media_type(&media_type)?, title_id)?;
    state
        .accounts
        .remove_from_continue_watching(&state.household_account_id, &id, identity)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_watched(
    State(state): State<AppState>,
    Path((id, media_type, title_id)): Path<(String, String, i64)>,
    Json(payload): Json<WatchedTarget>,
) -> Result<StatusCode, AppError> {
    let identity = TitleIdentity::new(parse_media_type(&media_type)?, title_id)?;
    let target = Target::new(identity, payload.season, payload.episode)?;
    state
        .accounts
        .mark_watched(&state.household_account_id, &id, target)
        .await?;
    Ok(StatusCode::NO_CONTENT)
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
    let playback_id = update.playback_id.clone();
    let progress = state
        .accounts
        .update_progress_at(&state.household_account_id, &id, update, now)
        .await?;
    state.relay.note_playback_activity(&playback_id);

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
    let playback_id = payload.update.playback_id.clone();
    let progress = state
        .accounts
        .update_progress_at(
            &state.household_account_id,
            &profile_id,
            payload.update,
            now,
        )
        .await?;
    state.relay.note_playback_activity(&playback_id);

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

#[cfg(test)]
mod home_tests {
    use super::*;

    fn shelf(id: &str, kind: &str, first: i64, last: i64) -> Result<Shelf, serde_json::Error> {
        let titles: Vec<_> = (first..=last)
            .map(|id| {
                json!({
                    "id": id, "type": "movie", "title": format!("Title {id}"),
                    "overview": "", "rating": 7.5, "details": false,
                    "backdrop": "https://example.com/backdrop.jpg"
                })
            })
            .collect();
        serde_json::from_value::<Shelf>(json!({
            "id": id, "heading": id, "kind": kind, "titles": titles
        }))
    }

    #[test]
    fn home_keeps_leading_matches_and_rotates_unique_discovery() -> Result<(), serde_json::Error> {
        let candidates = vec![
            shelf("personalized-test", "personal", 1, 50)?,
            shelf("discover-movie-crime", "discover", 1, 16)?,
            shelf("taste-movie-action", "taste", 40, 64)?,
            shelf("redundant", "because", 1, 4)?,
        ];
        let (today, featured) = compose_home(candidates.clone(), "test", 20_000);
        assert_eq!(
            (today.clone(), featured.clone()),
            compose_home(candidates.clone(), "test", 20_000)
        );
        let (tomorrow, _) = compose_home(candidates, "test", 20_001);
        assert_eq!(today[0].id, "personalized-test");
        assert_eq!(today[0].titles[..4], tomorrow[0].titles[..4]);
        let today_picks: HashSet<_> = today[0].titles[4..]
            .iter()
            .map(|title| title.identity.id())
            .collect();
        let tomorrow_picks: HashSet<_> = tomorrow[0].titles[4..]
            .iter()
            .map(|title| title.identity.id())
            .collect();
        assert_ne!(today_picks, tomorrow_picks);
        assert!(today.iter().any(|shelf| shelf.id == "discover-movie-crime"));
        assert!(today.iter().all(|shelf| shelf.id != "redundant"));
        assert_eq!(featured.len(), 8);
        let mut ids = HashSet::new();
        assert!(
            today
                .iter()
                .flat_map(|shelf| &shelf.titles)
                .chain(&featured)
                .all(|title| ids.insert(title.identity.key()))
        );
        Ok(())
    }

    #[test]
    fn deeper_home_mixes_recent_and_rated_rows_with_limited_anime() -> Result<(), serde_json::Error>
    {
        let mut personal = shelf("personalized-test", "personal", 1, 30)?;
        for title in &mut personal.titles[..8] {
            title.genres = vec!["Anime".into()];
        }
        let mut candidates = vec![
            personal,
            shelf("trending", "trending", 50, 69)?,
            shelf("taste-movie-animation", "taste", 70, 89)?,
            shelf("taste-tv-animation", "taste", 90, 109)?,
        ];
        for index in 0..16 {
            let kind = if index % 2 == 0 { "recent" } else { "discover" };
            let first = 200 + index * 20;
            candidates.push(shelf(&format!("{kind}-{index}"), kind, first, first + 19)?);
        }

        for index in 0..22 {
            let kind = if index % 2 == 0 { "browse" } else { "topic" };
            let first = 1000 + index * 20;
            candidates.push(shelf(&format!("legacy-{index}"), kind, first, first + 19)?);
        }

        let (today, featured) = compose_home(candidates.clone(), "test", 20_000);
        let (tomorrow, _) = compose_home(candidates, "test", 20_001);
        assert_eq!(today.len(), 25);
        assert_eq!(today[0].id, "personalized-test");
        assert_eq!(today[1].id, "trending");
        assert_eq!(
            today.iter().filter(|shelf| shelf.kind == "recent").count(),
            8
        );
        assert_ne!(today[2].id, tomorrow[2].id);
        assert_eq!(
            today
                .iter()
                .filter(|shelf| shelf.kind == "discover")
                .count(),
            8
        );
        assert_eq!(
            today
                .iter()
                .filter(|shelf| shelf.id.starts_with("legacy-"))
                .count(),
            6
        );
        assert!(is_anime_shelf(&today[24]));
        assert_eq!(today[24].titles.len(), 6);
        assert!(
            today[0]
                .titles
                .iter()
                .filter(|title| title.genres.iter().any(|genre| genre == "Anime"))
                .count()
                <= 2
        );
        let mut ids = HashSet::new();
        assert!(
            today
                .iter()
                .flat_map(|shelf| &shelf.titles)
                .chain(&featured)
                .all(|title| ids.insert(title.identity.key()))
        );
        Ok(())
    }
}
