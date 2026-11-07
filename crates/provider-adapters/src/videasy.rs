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
const SERVERS: [&str; 6] = ["atlanta", "oakland", "phoenix", "reno", "boise", "miami"];
const DEAD_DOMAINS: [&str; 1] = ["sun1.finalsignal.top"];

fn is_dead_domain(url: &str) -> bool {
    DEAD_DOMAINS.iter().any(|dead| url.contains(dead))
}

fn candidate_rank(candidate: &CandidateStream) -> (u8, u8, u8) {
    let domain_rank = if candidate.url.contains("sun2.finalsignal.top") {
        0
    } else if candidate.url.contains("workers.dev") {
        1
    } else {
        2
    };

    let server_rank = match candidate.server.as_str() {
        "atlanta" => 0,
        "oakland" => 1,
        "phoenix" => 2,
        "reno" => 3,
        _ => 4,
    };

    let quality_rank = candidate
        .resolver_metadata
        .provider_data
        .get("quality_claim")
        .and_then(|v| v.as_str())
        .map(|q| {
            let q = q.to_ascii_lowercase();
            if q.contains("1080") || q.contains("2160") || q.contains("4k") {
                0
            } else if q.contains("720") {
                1
            } else if q.contains("auto") {
                2
            } else if q.contains("480") {
                3
            } else if q.contains("360") {
                4
            } else {
                5
            }
        })
        .unwrap_or(2);

    (domain_rank, server_rank, quality_rank)
}

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
        let mut declared = Vec::with_capacity(payload.sources.len().max(1));
        if payload.sources.is_empty() {
            if !payload.playlist.is_empty() {
                declared.push((format!("videasy-{server}-master"), payload.playlist, None));
            }
        } else {
            for (index, source) in payload.sources.into_iter().enumerate() {
                declared.push((
                    format!("videasy-{server}-{index}"),
                    source.url,
                    Some(source.quality),
                ));
            }
        }
        for (id, url, quality) in declared {
            if candidates.len() >= 32 {
                break;
            }
            if url.is_empty() || seen.contains(&url) || is_dead_domain(&url) {
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
    candidates.sort_by_key(candidate_rank);
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
    use base64::{Engine, engine::general_purpose::STANDARD};
    use stream_core::domain::MediaType;
    use stream_core::providers::{DeviceCapabilities, ProviderTitle, QualityPreference, TmdbId};
    use stream_provider_codecs::videasy::apply_keystream;
    use stream_provider_sdk::http::{HostError, Request, Response};

    fn make_encrypted_payload(seed: &str, tmdb_id: i64, json: &str) -> Vec<u8> {
        let key = u32::try_from(tmdb_id).unwrap_or(u32::MAX);
        let mut data = format!("mvm1{json}").into_bytes();
        apply_keystream(seed, key, &mut data).unwrap();
        STANDARD.encode(&data).into_bytes()
    }

    struct MockVideasyHost {
        seed: String,
        responses: BTreeMap<String, Vec<u8>>,
    }

    impl ProviderHost for MockVideasyHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            if request.url.contains("/seed?") {
                let body = serde_json::to_vec(&json!({ "seed": self.seed })).unwrap();
                return Ok(Response {
                    url: request.url,
                    status: 200,
                    headers: Vec::new(),
                    body,
                });
            }
            Err(HostError::InvalidRequest)
        }

        fn fetch_batch(
            &self,
            requests: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            let mut results = Vec::with_capacity(requests.len());
            for req in requests {
                let server = req
                    .url
                    .split('/')
                    .nth(3)
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("");
                if let Some(body) = self.responses.get(server) {
                    results.push(Ok(Response {
                        url: req.url,
                        status: 200,
                        headers: Vec::new(),
                        body: body.clone(),
                    }));
                } else {
                    results.push(Err(HostError::UpstreamFailure));
                }
            }
            Ok(results)
        }

        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }

        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }

    #[test]
    fn resolves_tv_severance_prioritizes_fast_cdn_and_filters_dead_domains() {
        let tmdb_id = 97951;
        let seed = "59800312.YK-EMZd2cN0UD0ivTaMZaV";

        let atlanta_json = json!({
            "sources": [
                { "quality": "1080p", "url": "https://sun2.finalsignal.top/r6/atl-1080.m3u8" },
                { "quality": "720p", "url": "https://sun2.finalsignal.top/r6/atl-720.m3u8" },
                { "quality": "Auto", "url": "https://sun1.finalsignal.top/b3/dead.m3u8" }
            ],
            "playlist": "",
            "subtitles": []
        })
        .to_string();

        let oakland_json = json!({
            "sources": [
                { "quality": "Auto", "url": "https://white-tooth-84b1.wellsrobertsonkmlt.workers.dev/oak.m3u8" }
            ],
            "playlist": "",
            "subtitles": []
        })
        .to_string();

        let boise_json = json!({
            "sources": [
                { "quality": "1080p", "url": "https://moon.goldenkite.top/vdb/boise-1080.m3u8" }
            ],
            "playlist": "https://moon.goldenkite.top/vdb/master.m3u8",
            "subtitles": []
        })
        .to_string();

        let mut responses = BTreeMap::new();
        responses.insert(
            "atlanta".into(),
            make_encrypted_payload(seed, tmdb_id, &atlanta_json),
        );
        responses.insert(
            "oakland".into(),
            make_encrypted_payload(seed, tmdb_id, &oakland_json),
        );
        responses.insert(
            "boise".into(),
            make_encrypted_payload(seed, tmdb_id, &boise_json),
        );

        let host = MockVideasyHost {
            seed: seed.into(),
            responses,
        };

        let title = ProviderTitle::episode(TmdbId::try_from(tmdb_id).unwrap(), 1, 1).unwrap();
        let lookup = Lookup {
            title,
            preferences: QualityPreference::default(),
            device: DeviceCapabilities::default(),
            context_key: String::new(),
            startup_height: 1080,
            wait_for_complete: false,
            refresh: true,
        };

        let result = resolve(&lookup, &host).unwrap();
        let candidates = result.candidates.unwrap();

        // 1. atlanta 1080p on sun2.finalsignal.top is prioritized at candidate 0
        assert_eq!(candidates[0].id, "videasy-atlanta-0");
        assert_eq!(
            candidates[0].url,
            "https://sun2.finalsignal.top/r6/atl-1080.m3u8"
        );
        assert_eq!(candidates[0].server, "atlanta");

        // 2. atlanta 720p on sun2.finalsignal.top is candidate 1
        assert_eq!(candidates[1].id, "videasy-atlanta-1");
        assert_eq!(
            candidates[1].url,
            "https://sun2.finalsignal.top/r6/atl-720.m3u8"
        );

        // 3. oakland on workers.dev is candidate 2
        assert_eq!(candidates[2].id, "videasy-oakland-0");
        assert_eq!(candidates[2].server, "oakland");

        // 4. boise on moon.goldenkite.top is candidate 3
        assert_eq!(candidates[3].id, "videasy-boise-0");

        // 5. dead domain sun1.finalsignal.top must be dropped completely
        assert!(
            !candidates
                .iter()
                .any(|c| c.url.contains("sun1.finalsignal.top"))
        );

        // 6. duplicate master playlist is eliminated when sources exist
        assert!(!candidates.iter().any(|c| c.id.contains("master")));

        // 7. title coordinates are preserved for all candidates
        for c in &candidates {
            assert_eq!(c.title.media_type, MediaType::Tv);
            assert_eq!(c.title.tmdb_id.get(), 97951);
            assert_eq!(c.title.season, Some(1));
            assert_eq!(c.title.episode, Some(1));
        }
    }

    #[test]
    fn candidate_rank_ordering() {
        let title = ProviderTitle::movie(TmdbId::try_from(603).unwrap());
        let mk = |id: &str, server: &str, url: &str, quality: Option<&str>| {
            let mut metadata = Metadata::default();
            if let Some(q) = quality {
                metadata
                    .provider_data
                    .insert("quality_claim".into(), json!(q));
            }
            CandidateStream {
                expires_at: None,
                id: id.into(),
                server: server.into(),
                selected_variant_url: url.into(),
                url: url.into(),
                provider: common::provider_id("videasy").unwrap(),
                format: StreamFormat::Hls,
                title: title.clone(),
                fetch_context: FetchContext::default(),
                resolver_metadata: metadata,
                content_verified: false,
            }
        };

        let c_atlanta_1080 = mk(
            "c1",
            "atlanta",
            "https://sun2.finalsignal.top/1080.m3u8",
            Some("1080p"),
        );
        let c_atlanta_720 = mk(
            "c2",
            "atlanta",
            "https://sun2.finalsignal.top/720.m3u8",
            Some("720p"),
        );
        let c_oakland_auto = mk(
            "c3",
            "oakland",
            "https://foo.workers.dev/video.m3u8",
            Some("Auto"),
        );
        let c_boise_1080 = mk(
            "c4",
            "boise",
            "https://moon.goldenkite.top/1080.m3u8",
            Some("1080p"),
        );
        let c_boise_480 = mk(
            "c5",
            "boise",
            "https://moon.goldenkite.top/480.m3u8",
            Some("480p"),
        );

        let rank1 = candidate_rank(&c_atlanta_1080);
        let rank2 = candidate_rank(&c_atlanta_720);
        let rank3 = candidate_rank(&c_oakland_auto);
        let rank4 = candidate_rank(&c_boise_1080);
        let rank5 = candidate_rank(&c_boise_480);

        assert!(rank1 < rank2);
        assert!(rank2 < rank3);
        assert!(rank3 < rank4);
        assert!(rank4 < rank5);
    }
}
