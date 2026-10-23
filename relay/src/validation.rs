//! Pure import checks. No DNS, probing, ranking or source sockets occur here.
use crate::{
    config::Config,
    contracts::{CreateSession, providers::*},
    error::{Error, INVALID},
};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use chrono::{DateTime, Utc};
use url::Url;

pub fn owner(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
pub fn create(request: &CreateSession, config: &Config, now: DateTime<Utc>) -> Result<(), Error> {
    if request.contract_version != "relay.v1" || !owner(&request.owner_id) {
        return Err(INVALID);
    }
    let lease = request
        .expires_at
        .signed_duration_since(now)
        .num_milliseconds();
    if lease <= 0 || lease > config.state.max_session_lifetime_s as i64 * 1000 {
        return Err(INVALID);
    }
    selection(&request.selection, config, now)?;
    let bytes = serde_json::to_vec(request).map_err(|_| INVALID)?;
    if bytes.len() > config.state.max_snapshot_bytes as usize {
        return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "import_too_large"));
    }
    Ok(())
}
pub fn selection(selection: &Selection, config: &Config, now: DateTime<Utc>) -> Result<(), Error> {
    if selection.version != "providers.v1" {
        return Err(INVALID);
    }
    let selected = selection.selected.as_ref().ok_or(INVALID)?;
    let backups = selection.backups.as_deref().unwrap_or_default();
    if backups.len() >= config.state.max_sources_per_session_count as usize {
        return Err(INVALID);
    }
    let title = &selected.candidate.source.title;
    let mut imdb_id: Option<&str> = None;
    let mut my_anime_list_id: Option<i64> = None;
    for ranked in std::iter::once(selected).chain(backups) {
        let source = &ranked.candidate.source;
        let observation = &ranked.candidate.observation;
        validate_title(&source.title)?;
        // Any retained source can establish a cross-ID when the selected source
        // omitted it. Compare all subsequent supplied IDs without filling unknowns.
        if let Some(incoming) = source.title.imdb_id.as_deref() {
            if imdb_id.is_some_and(|known| known != incoming) {
                return Err(INVALID);
            }
            imdb_id = Some(incoming);
        }
        if let Some(incoming) = source.title.my_anime_list_id {
            if my_anime_list_id.is_some_and(|known| known != incoming) {
                return Err(INVALID);
            }
            my_anime_list_id = Some(incoming);
        }
        if !same_title(title, &source.title)
            || source.id.is_empty()
            || !matches!(
                source.provider.as_str(),
                "vidcore" | "vidlink" | "vidsrc" | "vidfast"
            )
            || !observation.accessible
            || observation.failure.is_some()
            || !observation.protection_known
            || observation.first_byte.is_some_and(|v| v < 0)
            || observation.sample_bytes < 0
            || observation.samples_completed < 0
            || observation.delivery_bps.is_some_and(|v| v < 0.0)
            || observation
                .reliability
                .is_some_and(|v| !(0.0..=1.0).contains(&v))
            || !ranked.score.is_finite()
        {
            return Err(INVALID);
        }
        if !matches!(observation.format.as_str(), "hls" | "mp4") {
            return Err(Error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_format",
            ));
        }
        if source.format != "unknown" && source.format != observation.format {
            return Err(INVALID);
        }
        if observation.encrypted && observation.format != "hls" {
            return Err(INVALID);
        }
        // Selected expiry cannot be silently ignored. Expired backups stay ordered in
        // the snapshot but cannot be activated later (stage 6 applies the safety margin).
        if std::ptr::eq(ranked, selected) && source.expires_at.is_some_and(|v| v <= now) {
            return Err(INVALID);
        }
        source_url(&source.url)?;
        if let Some(url) = &source.selected_variant_url {
            if observation.format != "hls" {
                return Err(INVALID);
            }
            source_url(url)?;
        }
        context(&source.fetch_context)?;
        for metadata in [&source.resolver_metadata, &observation.playlist_metadata] {
            for variant in metadata
                .variants
                .iter()
                .flatten()
                .chain(observation.variant.iter())
            {
                source_url(&variant.url)?;
                if [variant.width, variant.height, variant.bitrate]
                    .into_iter()
                    .flatten()
                    .any(|v| v <= 0)
                {
                    return Err(INVALID);
                }
            }
            for track in metadata.tracks.iter().flatten() {
                if let Some(url) = &track.url {
                    source_url(url)?;
                }
            }
        }
    }
    for provider in selection.providers.iter().flatten() {
        if provider.duration < 0 || provider.candidate_count < 0 {
            return Err(INVALID);
        }
    }
    Ok(())
}
fn validate_title(title: &Title) -> Result<(), Error> {
    if title.tmdb_id <= 0 || title.my_anime_list_id.is_some_and(|v| v <= 0) {
        return Err(INVALID);
    }
    match title.r#type.as_str() {
        "movie" if title.season.is_none() && title.episode.is_none() => Ok(()),
        "tv" if title.season.is_some_and(|v| v >= 0) && title.episode.is_some_and(|v| v > 0) => {
            Ok(())
        }
        _ => Err(INVALID),
    }
}
pub(crate) fn same_title(a: &Title, b: &Title) -> bool {
    a.r#type == b.r#type
        && a.tmdb_id == b.tmdb_id
        && a.season == b.season
        && a.episode == b.episode
        && !matches!((&a.imdb_id, &b.imdb_id), (Some(x), Some(y)) if x != y)
        && !matches!((a.my_anime_list_id, b.my_anime_list_id), (Some(x), Some(y)) if x != y)
}
/// Syntax only. Actual destination approval belongs to stage 2 at socket creation.
pub fn source_url(value: &str) -> Result<Url, Error> {
    crate::network::destination::destination(value)
}

fn root(value: &str) -> Result<(), Error> {
    let url = source_url(value)?;
    if url.path() != "/" || url.query().is_some() {
        return Err(INVALID);
    }
    Ok(())
}
fn path(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains(['?', '#', '\\', '%'])
        && !value.bytes().any(|b| b.is_ascii_control())
        && !value.split('/').any(|s| matches!(s, "." | ".."))
}
pub(crate) fn context(context: &FetchContext) -> Result<(), Error> {
    if let Some(public) = &context.public_resources {
        for value in [&public.origin, &public.referer].into_iter().flatten() {
            root(value)?;
        }
    }
    for scope in context.scopes.iter().flatten() {
        root(&scope.origin)?;
        for value in [&scope.path_prefix, &scope.cookie_path]
            .into_iter()
            .flatten()
        {
            if !path(value) {
                return Err(INVALID);
            }
        }
        if scope.cookies.is_some() && scope.cookie_path.is_none() {
            return Err(INVALID);
        }
        if let Some(cookies) = &scope.cookies {
            HeaderValue::from_str(cookies).map_err(|_| INVALID)?;
        }
        let mut seen = std::collections::BTreeSet::new();
        for (name, value) in scope.headers.iter().flatten() {
            let header = HeaderName::from_bytes(name.as_bytes()).map_err(|_| INVALID)?;
            if !seen.insert(header.as_str().to_owned())
                || !matches!(
                    header.as_str(),
                    "user-agent"
                        | "accept"
                        | "accept-language"
                        | "referer"
                        | "origin"
                        | "authorization"
                        | "x-playback-environment"
                        | "x-csrf-token"
                )
            {
                return Err(INVALID);
            }
            HeaderValue::from_str(value).map_err(|_| INVALID)?;
            if matches!(header.as_str(), "origin" | "referer") {
                source_url(value)?;
            }
        }
    }
    Ok(())
}
