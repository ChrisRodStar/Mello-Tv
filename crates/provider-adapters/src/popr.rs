//! Popr's Vidnest mirror API. Stream type and quality stay provider claims.
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use stream_core::providers::{
    AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
    PublicResourceContext, StreamFormat, Track,
};
use url::Url;

use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, get},
    destination::public_url,
};

const ORIGIN: &str = "https://popr.ink";
const SERVERS: [&str; 10] = [
    "default",
    "catflix",
    "hexa",
    "Gama",
    "Liligoon",
    "Sigma",
    "Prime",
    "Alfa",
    "Lamda",
    "ynx_vidsrc",
];
const LIMIT: u32 = 384 * 1024;

#[derive(Deserialize, Default)]
struct ApiResponse {
    #[serde(default)]
    results: Vec<ResultEntry>,
}
#[derive(Deserialize, Default)]
struct ResultEntry {
    #[serde(default)]
    server: String,
    #[serde(default)]
    streams: Vec<Stream>,
    #[serde(default)]
    subtitles: Vec<Subtitle>,
}
#[derive(Deserialize, Default)]
struct Stream {
    #[serde(default)]
    url: String,
    #[serde(default)]
    quality: String,
    #[serde(default, rename = "isM3U8")]
    is_m3u8: bool,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}
#[derive(Deserialize, Default)]
struct Subtitle {
    #[serde(default)]
    url: String,
    #[serde(default)]
    lang: String,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let mut requests = Vec::with_capacity(SERVERS.len());
    for server in SERVERS {
        let mut url = Url::parse(&format!("{ORIGIN}/api/vidnest"))
            .map_err(|_| AdapterError::Protocol("Popr endpoint"))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("id", &lookup.title.tmdb_id.get().to_string());
            query.append_pair(
                "type",
                if lookup.title.media_type == stream_core::domain::MediaType::Tv {
                    "tv"
                } else {
                    "movie"
                },
            );
            if lookup.title.media_type == stream_core::domain::MediaType::Tv {
                query.append_pair(
                    "season",
                    &lookup.title.season.ok_or(AdapterError::Input)?.to_string(),
                );
                query.append_pair(
                    "episode",
                    &lookup.title.episode.ok_or(AdapterError::Input)?.to_string(),
                );
            }
            if server != "default" {
                query.append_pair("server", server);
            }
        }
        let mut request = get(url.as_str(), &format!("{ORIGIN}/"), LIMIT);
        request.timeout_ms = 1_000;
        requests.push(request);
    }
    let responses = batch(host, requests)?;
    let provider = common::provider_id("popr")?;
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    let mut tracks = BTreeMap::<String, Track>::new();
    'servers: for (requested_server, response) in SERVERS.into_iter().zip(responses) {
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                issues.push(common::issue(&error, "sources"));
                continue;
            }
        };
        let parsed: ApiResponse =
            match common::json(&response.body, LIMIT as usize, "Popr response") {
                Ok(parsed) => parsed,
                Err(error) => {
                    issues.push(common::issue(&error, "sources"));
                    continue;
                }
            };
        if parsed.results.len() > 16 {
            issues.push(common::issue(
                &AdapterError::Protocol("Popr result count"),
                "sources",
            ));
            continue;
        }
        for entry in parsed.results {
            if entry.streams.len() > 32 || entry.subtitles.len() > 64 {
                issues.push(common::issue(
                    &AdapterError::Protocol("Popr source count"),
                    "sources",
                ));
                continue;
            }
            for subtitle in entry.subtitles {
                if subtitle.url.len() <= 8192
                    && public_url(&subtitle.url).is_ok()
                    && tracks.len() < 64
                {
                    let name = truncate_utf8(&subtitle.lang, 256);
                    let language = truncate_utf8(&subtitle.lang, 64);
                    tracks.entry(subtitle.url.clone()).or_insert(Track {
                        kind: "subtitles".into(),
                        language,
                        name,
                        url: subtitle.url,
                        ..Default::default()
                    });
                }
            }
            for stream in entry.streams {
                if candidates.len() >= 32 {
                    break 'servers;
                }
                if stream.url.is_empty() || !seen.insert(stream.url.clone()) {
                    continue;
                }
                if public_url(&stream.url).is_err() {
                    issues.push(common::issue(
                        &AdapterError::Protocol("Popr source destination"),
                        "sources",
                    ));
                    continue;
                }
                let context = match stream_context(&stream.headers, &stream.url) {
                    Ok(context) => context,
                    Err(error) => {
                        issues.push(common::issue(&error, "sources"));
                        continue;
                    }
                };
                let server = if entry.server.is_empty() {
                    requested_server
                } else {
                    entry.server.as_str()
                };
                if server.len() > 128 || stream.quality.len() > 128 {
                    continue;
                }
                let mut metadata = Metadata {
                    tracks: tracks.values().cloned().collect(),
                    tracks_known: !tracks.is_empty(),
                    ..Default::default()
                };
                metadata
                    .provider_data
                    .insert("quality_claim".into(), json!(stream.quality));
                candidates.push(CandidateStream {
                    id: format!(
                        "popr-{}",
                        common::hash_id(&[server.as_bytes(), stream.url.as_bytes()])
                    ),
                    server: server.to_owned(),
                    selected_variant_url: String::new(),
                    url: stream.url,
                    provider: provider.clone(),
                    format: if stream.is_m3u8 {
                        StreamFormat::Hls
                    } else {
                        StreamFormat::Unknown
                    },
                    title: lookup.title.clone(),
                    fetch_context: context,
                    resolver_metadata: metadata,
                    content_verified: false,
                    expires_at: None,
                });
            }
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("Popr mirrors"));
    }
    // Subtitle declarations can occur after earlier server responses. Preserve
    // the complete bounded track inventory consistently across all candidates.
    let all_tracks: Vec<_> = tracks.into_values().collect();
    for candidate in &mut candidates {
        candidate.resolver_metadata.tracks = all_tracks.clone();
        candidate.resolver_metadata.tracks_known = !all_tracks.is_empty();
    }
    Ok(common::result(candidates, issues))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next = index + character.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    value[..end].to_owned()
}

fn stream_context(
    headers: &BTreeMap<String, String>,
    stream_url: &str,
) -> Result<FetchContext, AdapterError> {
    if headers.len() > 8 {
        return Err(AdapterError::Protocol("Popr stream header count"));
    }
    let stream = public_url(stream_url)?;
    let stream_origin = stream.origin().ascii_serialization();
    let mut scoped = BTreeMap::new();
    for (name, value) in headers {
        let canonical = name.to_ascii_lowercase();
        if !matches!(canonical.as_str(), "origin" | "referer")
            || value.len() > 2048
            || value.bytes().any(|b| b.is_ascii_control())
        {
            return Err(AdapterError::Protocol("Popr stream headers"));
        }
        let parsed = public_url(value)?;
        if canonical == "origin"
            && (parsed.path() != "/"
                || parsed.query().is_some()
                || parsed.origin().ascii_serialization() != value.trim_end_matches('/'))
        {
            return Err(AdapterError::Protocol("Popr stream origin"));
        }
        scoped.insert(canonical, value.clone());
    }
    let scopes = if scoped.is_empty() {
        Vec::new()
    } else {
        vec![FetchScope {
            headers: scoped,
            origin: stream_origin,
            path_prefix: "/".into(),
            ..Default::default()
        }]
    };
    Ok(FetchContext {
        scopes,
        public_resources: Some(PublicResourceContext {
            origin: ORIGIN.into(),
            referer: format!("{ORIGIN}/"),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};
    struct Host {
        expected_type: &'static str,
        streams_per_server: usize,
        long_subtitle: bool,
    }
    impl ProviderHost for Host {
        fn fetch(&self, _: Request) -> Result<Response, HostError> {
            Err(HostError::Denied)
        }
        fn fetch_batch(
            &self,
            requests: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            assert_eq!(requests.len(), SERVERS.len());
            assert!(requests.iter().all(|request| request.timeout_ms == 1_000));
            if self.expected_type == "type=tv" {
                assert!(
                    requests
                        .iter()
                        .any(|request| request.url.contains("type=tv")
                            && request.url.contains("season=1")
                            && request.url.contains("episode=2"))
                );
            }
            assert!(
                requests
                    .iter()
                    .all(|request| request.url.contains(self.expected_type))
            );
            Ok(requests
                .into_iter()
                .enumerate()
                .map(|(server, request)| {
                    let body = if self.streams_per_server > 0 || server == 0 {
                        let streams: Vec<_> = (0..self.streams_per_server.max(1))
                            .map(|index| serde_json::json!({
                                "url": format!("https://cdn-{server}-{index}.example.org/master.m3u8"),
                                "quality": "1080p", "isM3U8": true,
                                "headers": {"Referer":"https://popr.ink/"}
                            }))
                            .collect();
                        let language = if self.long_subtitle { "é".repeat(100) } else { "en".into() };
                        serde_json::json!({"results":[{"server":"default","streams":streams,"subtitles":[{"url":"https://cdn.example.org/en.vtt","lang":language}]}]}).to_string().into_bytes()
                    } else {
                        br#"{"results":[]}"#.to_vec()
                    };
                    Ok(Response { url: request.url, status: 200, headers: Vec::new(), body })
                })
                .collect())
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn batches_all_mirrors_and_keeps_claims_unverified() -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            json!({"title":{"type":"tv","tmdb_id":1622,"season":1,"episode":2}}),
        )?;
        let result = resolve(
            &lookup,
            &Host {
                expected_type: "type=tv",
                streams_per_server: 0,
                long_subtitle: false,
            },
        )?;
        result.validate_for(&common::provider_id("popr")?, &lookup.title, 32)?;
        let candidates = result.candidates.ok_or("missing candidates")?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].format, StreamFormat::Hls);
        assert!(!candidates[0].content_verified);
        assert_eq!(candidates[0].resolver_metadata.tracks.len(), 1);
        Ok(())
    }

    #[test]
    fn caps_global_candidates_and_bounds_unicode_subtitle_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let result = resolve(
            &lookup,
            &Host {
                expected_type: "type=movie",
                streams_per_server: 4,
                long_subtitle: true,
            },
        )?;
        result.validate_for(&common::provider_id("popr")?, &lookup.title, 32)?;
        let candidates = result.candidates.ok_or("missing candidates")?;
        assert_eq!(candidates.len(), 32);
        let track = &candidates[0].resolver_metadata.tracks[0];
        assert!(track.language.len() <= 64);
        assert!(track.name.len() <= 256);
        Ok(())
    }
    #[test]
    fn validates_stream_context_and_rejects_unsafe_headers() {
        assert!(stream_context(&BTreeMap::new(), "http://127.0.0.1/a.m3u8").is_err());
        assert!(
            stream_context(
                &BTreeMap::from([("Authorization".into(), "token".into())]),
                "https://cdn.example.org/a.m3u8"
            )
            .is_err()
        );
    }
}
