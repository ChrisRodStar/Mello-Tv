//! Videasy discovery; provider labels remain claims until native probing.

use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, fetch, get, header},
    destination::public_url,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat, Variant,
    },
};
use stream_provider_codecs::videasy::decrypt_payload;

const API: &str = "https://api.wecollege.net";
const ORIGIN: &str = "https://vidy.st";
const SERVERS: [&str; 9] = [
    "boise", "miami", "phoenix", "seattle", "atlanta", "portland", "vegas", "reno", "oakland",
];

#[derive(Deserialize)]
struct Seed {
    seed: String,
}
#[derive(Deserialize)]
struct Subtitle {
    #[serde(default)]
    lang: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    url: String,
}
#[derive(Deserialize)]
struct Source {
    quality: String,
    url: String,
}
#[derive(Deserialize)]
struct Payload {
    #[serde(default, deserialize_with = "common::nullable_vec")]
    sources: Vec<Source>,
    #[serde(default)]
    subtitles: serde_json::Value,
    #[serde(default)]
    playlist: String,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let seed_url = format!("{API}/seed?mediaId={}", lookup.title.tmdb_id.get());
    let mut seed_request = get(seed_url, &format!("{ORIGIN}/"), 32 * 1024);
    seed_request.headers.push(header("Origin", ORIGIN));
    let response = fetch(host, seed_request)?;
    let seed: Seed = common::json(&response.body, 32 * 1024, "Videasy seed")?;
    if seed.seed.is_empty()
        || seed.seed.len() > 4096
        || seed.seed.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(AdapterError::Protocol("Videasy seed"));
    }
    let mut requests = Vec::with_capacity(SERVERS.len());
    for server in SERVERS {
        let mut url = url::Url::parse(&format!("{API}/{server}/sources"))
            .map_err(|_| AdapterError::Protocol("Videasy endpoint"))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("title", "media");
            query.append_pair(
                "mediaType",
                if lookup.title.media_type == MediaType::Tv {
                    "tv"
                } else {
                    "movie"
                },
            );
            if lookup.title.media_type == MediaType::Tv {
                query.append_pair(
                    "episodeId",
                    &lookup.title.episode.ok_or(AdapterError::Input)?.to_string(),
                );
                query.append_pair(
                    "seasonId",
                    &lookup.title.season.ok_or(AdapterError::Input)?.to_string(),
                );
            }
            query
                .append_pair("tmdbId", &lookup.title.tmdb_id.get().to_string())
                .append_pair("imdbId", &lookup.title.imdb_id)
                .append_pair("enc", "2")
                .append_pair("seed", &seed.seed);
        }
        let mut request = get(url.as_str(), &format!("{ORIGIN}/"), 512 * 1024);
        request.timeout_ms = 1_000;
        request.headers.push(header("Origin", ORIGIN));
        requests.push(request);
    }
    let responses = batch(host, requests)?;
    let provider = common::provider_id("videasy")?;
    let context = FetchContext {
        scopes: Vec::new(),
        public_resources: Some(PublicResourceContext {
            origin: ORIGIN.into(),
            referer: format!("{ORIGIN}/"),
        }),
    };
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    for (server, response) in SERVERS.into_iter().zip(responses) {
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                issues.push(common::issue(&error, "sources"));
                continue;
            }
        };
        let decoded = match std::str::from_utf8(&response.body)
            .ok()
            .and_then(|body| decrypt_payload(body, &seed.seed, lookup.title.tmdb_id).ok())
        {
            Some(decoded) => decoded,
            None => {
                issues.push(common::issue(
                    &AdapterError::Protocol("Videasy payload"),
                    "sources",
                ));
                continue;
            }
        };
        let payload: Payload = match common::json(&decoded, 512 * 1024, "Videasy sources") {
            Ok(payload) => payload,
            Err(error) => {
                issues.push(common::issue(&error, "sources"));
                continue;
            }
        };
        if payload.sources.len() > 32
            || payload
                .subtitles
                .as_array()
                .is_some_and(|items| items.len() > 64)
        {
            issues.push(common::issue(
                &AdapterError::Protocol("Videasy source count"),
                "sources",
            ));
            continue;
        }
        let subtitle_items: Vec<Subtitle> =
            serde_json::from_value(payload.subtitles.clone()).unwrap_or_default();
        let tracks: Vec<stream_core::providers::Track> = subtitle_items
            .into_iter()
            .filter(|s| !s.url.is_empty())
            .map(|s| {
                let lang = if !s.language.is_empty() {
                    s.language
                } else {
                    s.lang
                };
                stream_core::providers::Track {
                    kind: "subtitles".into(),
                    language: lang.clone(),
                    name: lang,
                    url: s.url,
                    ..Default::default()
                }
            })
            .collect();
        let tracks_known = !tracks.is_empty();
        let claims = BTreeMap::from([("subtitle_claims".into(), payload.subtitles)]);
        let mut declared = Vec::with_capacity(payload.sources.len() + 1);
        if !payload.playlist.is_empty() {
            declared.push((format!("videasy-{server}-master"), payload.playlist, None));
        }
        for (index, source) in payload.sources.into_iter().enumerate() {
            declared.push((
                format!("videasy-{server}-{index}"),
                source.url,
                Some(source.quality),
            ));
        }
        for (id, url, quality) in declared {
            if candidates.len() >= 32 {
                break;
            }
            if url.is_empty() || seen.contains(&url) {
                continue;
            }
            if public_url(&url).is_err() {
                issues.push(common::issue(
                    &AdapterError::Protocol("source destination"),
                    "sources",
                ));
                continue;
            }
            seen.insert(url.clone());
            let variants = quality
                .as_ref()
                .map(|quality| variant(quality, &url))
                .into_iter()
                .collect();
            let mut metadata = Metadata {
                provider_data: claims.clone(),
                variants,
                tracks: tracks.clone(),
                tracks_known,
            };
            if let Some(quality) = &quality {
                metadata
                    .provider_data
                    .insert("quality_claim".into(), json!(quality));
            }
            candidates.push(CandidateStream {
                expires_at: None,
                id,
                server: server.into(),
                selected_variant_url: if quality.is_some() {
                    url.clone()
                } else {
                    String::new()
                },
                url,
                provider: provider.clone(),
                format: StreamFormat::Hls,
                title: lookup.title.clone(),
                fetch_context: context.clone(),
                resolver_metadata: metadata,
                content_verified: false,
            });
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("Videasy servers"));
    }
    Ok(common::result(candidates, issues))
}

fn variant(quality: &str, url: &str) -> Variant {
    let quality = quality.to_ascii_lowercase();
    let dimensions = if quality.contains("2160") || quality.contains("4k") {
        Some((3840, 2160))
    } else if quality.contains("1080") {
        Some((1920, 1080))
    } else if quality.contains("720") {
        Some((1280, 720))
    } else if quality.contains("480") {
        Some((854, 480))
    } else if quality.contains("360") {
        Some((640, 360))
    } else {
        None
    };
    Variant {
        url: url.into(),
        width: dimensions.map(|(width, _)| width),
        height: dimensions.map(|(_, height)| height),
        ..Variant::default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_videasy_subtitle_parsing() {
        let raw = serde_json::json!([
            {"lang": "en", "language": "English", "url": "https://vidy.st/sub/en.vtt"},
            {"lang": "es", "language": "", "url": "https://vidy.st/sub/es.vtt"},
            {"lang": "", "language": "", "url": ""}
        ]);
        let items: Vec<Subtitle> = serde_json::from_value(raw).unwrap();
        let tracks: Vec<stream_core::providers::Track> = items
            .into_iter()
            .filter(|s| !s.url.is_empty())
            .map(|s| {
                let lang = if !s.language.is_empty() {
                    s.language
                } else {
                    s.lang
                };
                stream_core::providers::Track {
                    kind: "subtitles".into(),
                    language: lang.clone(),
                    name: lang,
                    url: s.url,
                    ..Default::default()
                }
            })
            .collect();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].language, "English");
        assert_eq!(tracks[1].language, "es");
    }
}
