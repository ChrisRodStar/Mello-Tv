//! Stable, playback-only URLs exported to Jellyfin. No account playback or
//! progress rows are created: Jellyfin owns those users and their watch history.
use std::{
    collections::HashMap,
    fmt::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::AppState;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use stream_core::{
    accounts::{ProfilePlaybackSettings, Target},
    catalog::{MediaType, TitleIdentity},
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, StreamFormat, TmdbId,
    },
};

const CACHE_LIMIT: usize = 128;
const REUSE_WINDOW: Duration = Duration::from_secs(300);
type ResolutionSlot = Arc<tokio::sync::Mutex<Option<CachedPlayback>>>;
type ResolutionSlots = Arc<Mutex<HashMap<String, ResolutionSlot>>>;

struct CachedPlayback {
    session_id: String,
    url: String,
    reusable_until: Instant,
    source_expires_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct PlayQuery {
    key: Option<String>,
}

pub fn routes() -> Router<AppState> {
    let slots: ResolutionSlots = Arc::new(Mutex::new(HashMap::new()));
    Router::new()
        .route(
            "/integrations/jellyfin/play/{media_type}/{id}/{season}/{episode}",
            get(move |state, path, query| play(state, path, query, slots.clone())),
        )
        .layer(axum::middleware::from_fn(no_cache))
}

async fn no_cache(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        "referrer-policy",
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response
}

fn authorized_key(supplied: Option<&str>, expected: &str) -> bool {
    let Some(supplied) = supplied else {
        return false;
    };
    let supplied = Sha256::digest(supplied.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());
    supplied
        .iter()
        .zip(expected.iter())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn target(media_type: &str, id: i64, season: i64, episode: i64) -> Option<Target> {
    let media_type = match media_type {
        "movie" if season == 0 && episode == 0 => MediaType::Movie,
        "tv" => MediaType::Tv,
        _ => return None,
    };
    let identity = TitleIdentity::new(media_type, id).ok()?;
    match media_type {
        MediaType::Movie => Target::new(identity, None, None).ok(),
        MediaType::Tv => Target::new(identity, Some(season), Some(episode)).ok(),
    }
}

fn random_id() -> Result<String, StatusCode> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut value = String::with_capacity(48);
    for byte in bytes {
        let _ = write!(value, "{byte:02x}");
    }
    Ok(value)
}

fn resolution_slot(slots: &ResolutionSlots, key: String) -> Result<ResolutionSlot, StatusCode> {
    let mut slots = slots
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(slot) = slots.get(&key) {
        return Ok(slot.clone());
    }
    if slots.len() >= CACHE_LIMIT {
        // Never evict a resolution that a concurrent request is awaiting.
        let removable = slots
            .iter()
            .find(|(_, value)| Arc::strong_count(value) == 1)
            .map(|(key, _)| key.clone());
        if let Some(key) = removable {
            slots.remove(&key);
        } else {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    }
    let slot = Arc::new(tokio::sync::Mutex::new(None));
    slots.insert(key, slot.clone());
    Ok(slot)
}

async fn play(
    State(state): State<AppState>,
    Path((media_type, id, season, episode)): Path<(String, i64, i64, i64)>,
    Query(query): Query<PlayQuery>,
    slots: ResolutionSlots,
) -> Response {
    let Some(bridge) = &state.jellyfin else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !authorized_key(query.key.as_deref(), &bridge.config.play_key) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(target) = target(&media_type, id, season, episode) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let title = match bridge.playback_title(&state, &target).await {
        Ok(title) => title,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let settings = match state
        .accounts
        .get_profile_metadata(
            &state.household_account_id,
            &bridge.config.profile_id,
            "playback-settings",
        )
        .await
    {
        Ok(Some(value)) => match serde_json::from_value::<ProfilePlaybackSettings>(value) {
            Ok(settings) => settings,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        Ok(None) => ProfilePlaybackSettings::default(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if settings.validate().is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let settings_key = match serde_json::to_string(&settings) {
        Ok(value) => value,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let slot = match resolution_slot(&slots, format!("{}:{settings_key}", target.key())) {
        Ok(slot) => slot,
        Err(status) => return status.into_response(),
    };
    let mut slot = slot.lock().await;
    if let Some(cached) = slot.as_ref()
        && (cached.reusable_until > Instant::now()
            || state.relay.session_active(&cached.session_id))
        && cached.source_expires_at.is_none_or(|expires| {
            expires
                > DateTime::<Utc>::from(std::time::SystemTime::now())
                    + chrono::Duration::seconds(30)
        })
        && state.relay.has_session(&cached.session_id)
    {
        return redirect(&cached.url);
    }
    let max_height = if settings.max_height > 0 {
        settings.max_height
    } else {
        1080
    };
    let Ok(tmdb_id) = TmdbId::try_from(id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let lookup = Lookup {
        title: ProviderTitle {
            media_type: target.identity().media_type(),
            tmdb_id,
            imdb_id: title.imdb_id,
            my_anime_list_id: (title.mal_id > 0).then_some(title.mal_id),
            season: target.season(),
            episode: target.episode(),
        },
        preferences: QualityPreference {
            audio_languages: if settings.audio_language.is_empty() {
                vec![]
            } else {
                vec![settings.audio_language]
            },
            subtitle_languages: if settings.subtitles_enabled
                && !settings.subtitle_language.is_empty()
            {
                vec![settings.subtitle_language]
            } else {
                vec![]
            },
            max_height,
            ..Default::default()
        },
        device: DeviceCapabilities {
            formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
            codecs: [
                "avc1", "h264", "hvc1", "hev1", "hevc", "vp9", "vp09", "av01", "av1", "mp4a",
                "aac", "ac3", "ac-3", "eac3", "ec-3", "opus", "flac",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            max_height,
            allow_unknown_codecs: true,
            allow_unknown_resolution: true,
            supports_encrypted_hls: true,
        },
        context_key: format!(
            "jellyfin-{}",
            stream_media::MediaRelay::resource_id(&bridge.config.profile_id)
        ),
        startup_height: if max_height > 0 { max_height } else { 1080 },
        wait_for_complete: false,
        refresh: false,
    };
    let selection = match state.media.resolve_stream(&lookup).await {
        Ok(selection) if selection.selected.is_some() => selection,
        _ => return StatusCode::BAD_GATEWAY.into_response(),
    };
    let source_expires_at = selection
        .selected
        .as_ref()
        .and_then(|selected| selected.candidate.source.expires_at);
    if source_expires_at
        .is_some_and(|expires| expires <= DateTime::<Utc>::from(std::time::SystemTime::now()))
    {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    let (session_id, token) = match (random_id(), random_id()) {
        (Ok(id), Ok(token)) => (id, token),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if !state.relay.register_session_with_public_url(
        session_id.clone(),
        token.clone(),
        selection,
        &bridge.config.public_url,
    ) {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    let url = format!(
        "{}/stream/{session_id}/entry?st={token}",
        bridge.config.public_url
    );
    *slot = Some(CachedPlayback {
        session_id,
        url: url.clone(),
        reusable_until: Instant::now() + REUSE_WINDOW,
        source_expires_at,
    });
    redirect(&url)
}

fn redirect(url: &str) -> Response {
    let mut response = Redirect::temporary(url).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        "referrer-policy",
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_and_coordinates_are_validated() {
        assert!(!authorized_key(None, "secret"));
        assert!(!authorized_key(Some("different"), "secret"));
        assert!(authorized_key(Some("secret"), "secret"));
        assert!(target("movie", 27205, 0, 0).is_some());
        assert!(target("movie", 27205, 1, 1).is_none());
        assert!(target("tv", 1399, 0, 1).is_some());
        assert!(target("tv", 1399, 1, 0).is_none());
        assert!(target("tv", 0, 1, 1).is_none());
    }
}
