//! Fmovies4U encrypted movie and episode source API.
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
        PublicResourceContext, StreamFormat, Track,
    },
};

use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get, header},
    destination::public_url,
};
use stream_provider_codecs::fmovies4u::decrypt_payload;

const ORIGIN: &str = "https://fmovies4u.com";
const LIMIT: u32 = 512 * 1024;

#[derive(Deserialize, Default)]
struct ApiResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    sources: Vec<ApiSource>,
}
#[derive(Deserialize, Default)]
struct ApiSource {
    #[serde(default)]
    url: Vec<StreamUrl>,
    #[serde(default)]
    tracks: Vec<ApiTrack>,
}
#[derive(Deserialize, Default)]
struct StreamUrl {
    #[serde(default)]
    link: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    quality: String,
}
#[derive(Deserialize, Default)]
struct ApiTrack {
    #[serde(default)]
    kind: String,
    #[serde(default, rename = "type")]
    type_name: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    srclang: String,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    language: String,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    lookup.title.validate().map_err(|_| AdapterError::Input)?;
    let (api_path, referer_path) = match lookup.title.media_type {
        MediaType::Movie => (
            format!("movie/{}", lookup.title.tmdb_id.get()),
            format!("movie/{}?autoPlay=false", lookup.title.tmdb_id.get()),
        ),
        MediaType::Tv => {
            let season = lookup.title.season.ok_or(AdapterError::Input)?;
            let episode = lookup.title.episode.ok_or(AdapterError::Input)?;
            (
                format!("tv/{}/{season}/{episode}", lookup.title.tmdb_id.get()),
                format!(
                    "tv/{}/{season}/{episode}?autoPlay=false",
                    lookup.title.tmdb_id.get()
                ),
            )
        }
    };
    let api_url = format!("{ORIGIN}/api/{api_path}?_cb={}", host.now_unix_ms()?.max(0));
    let referer = format!("{ORIGIN}/embed/{referer_path}");
    let mut request = get(api_url, &referer, LIMIT);
    request.headers.push(header(
        "Accept",
        "application/json, text/javascript, */*; q=0.01",
    ));
    request
        .headers
        .push(header("Accept-Language", "en-US,en;q=0.9"));
    let response = fetch(host, request)?;
    let encrypted = std::str::from_utf8(&response.body)
        .map_err(|_| AdapterError::Protocol("Fmovies4U response encoding"))?;
    if encrypted.is_empty() {
        return Err(AdapterError::Unavailable("Fmovies4U encrypted sources"));
    }
    let decrypted = decrypt_payload(encrypted)
        .map_err(|_| AdapterError::Protocol("Fmovies4U encrypted response"))?;
    let api: ApiResponse = common::json(&decrypted, LIMIT as usize, "Fmovies4U plaintext")?;
    if !api.success {
        return Err(AdapterError::Unavailable("Fmovies4U sources"));
    }
    if api.sources.len() > 32 {
        return Err(AdapterError::Protocol("Fmovies4U source count"));
    }
    let provider = common::provider_id("fmovies4u")?;
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    let mut tracks = BTreeMap::new();
    for source in api.sources {
        if source.url.len() > 32 || source.tracks.len() > 128 {
            issues.push(common::issue(
                &AdapterError::Protocol("Fmovies4U item count"),
                "sources",
            ));
            continue;
        }
        for track in source.tracks {
            let kind = if track.kind.is_empty() {
                track.type_name.to_ascii_lowercase()
            } else {
                track.kind.to_ascii_lowercase()
            };
            if !matches!(kind.as_str(), "subtitle" | "subtitles" | "captions")
                && track.file.is_empty()
            {
                continue;
            }
            let url = if track.file.is_empty() {
                track.url
            } else {
                track.file
            };
            if url.is_empty() || url.len() > 8192 || public_url(&url).is_err() || tracks.len() >= 64
            {
                continue;
            }
            let label = [track.label, track.srclang, track.lang, track.language]
                .into_iter()
                .find(|value| !value.is_empty())
                .unwrap_or_else(|| "Unknown subtitle".into());
            tracks
                .entry(url.clone())
                .or_insert_with(|| subtitle_track(&label, url));
        }
        for stream in source.url {
            if candidates.len() >= 32 || stream.link.is_empty() || !seen.insert(stream.link.clone())
            {
                continue;
            }
            let parsed = match public_url(&stream.link) {
                Ok(parsed) => parsed,
                Err(_) => {
                    issues.push(common::issue(
                        &AdapterError::Protocol("Fmovies4U source destination"),
                        "sources",
                    ));
                    continue;
                }
            };
            let special = stream.link.contains("file2");
            let origin = parsed.origin().ascii_serialization();
            let (scope_origin, headers) = if special {
                (
                    origin,
                    BTreeMap::from([
                        ("referer".into(), "https://videostr.net/".into()),
                        ("origin".into(), "https://videostr.net".into()),
                    ]),
                )
            } else {
                (
                    origin,
                    BTreeMap::from([
                        ("user-agent".into(), common::USER_AGENT.into()),
                        ("accept".into(), "*/*".into()),
                        ("referer".into(), referer.clone()),
                    ]),
                )
            };
            let context = FetchContext {
                scopes: vec![FetchScope {
                    headers,
                    origin: scope_origin,
                    path_prefix: "/".into(),
                    ..Default::default()
                }],
                public_resources: Some(PublicResourceContext {
                    origin: ORIGIN.into(),
                    referer: format!("{ORIGIN}/"),
                }),
            };
            let format = match stream.kind.to_ascii_lowercase().as_str() {
                "hls" | "m3u8" => StreamFormat::Hls,
                "mp4" => StreamFormat::Mp4,
                _ => StreamFormat::Unknown,
            };
            let mut metadata = Metadata {
                tracks: tracks.values().cloned().collect(),
                tracks_known: !tracks.is_empty(),
                ..Default::default()
            };
            metadata
                .provider_data
                .insert("quality_claim".into(), json!(stream.quality));
            metadata
                .provider_data
                .insert("audio_language_claim".into(), json!("English"));
            candidates.push(CandidateStream {
                id: format!("fmovies4u-{}", common::hash_id(&[stream.link.as_bytes()])),
                server: "default".into(),
                selected_variant_url: String::new(),
                url: stream.link,
                provider: provider.clone(),
                format,
                title: lookup.title.clone(),
                fetch_context: context,
                resolver_metadata: metadata,
                content_verified: false,
                expires_at: None,
            });
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("Fmovies4U streams"));
    }
    let tracks: Vec<_> = tracks.into_values().collect();
    for candidate in &mut candidates {
        candidate.resolver_metadata.tracks = tracks.clone();
        candidate.resolver_metadata.tracks_known = !tracks.is_empty();
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

fn subtitle_track(label: &str, url: String) -> Track {
    Track {
        kind: "subtitles".into(),
        name: truncate_utf8(label, 256),
        language: truncate_utf8(label, 64),
        url,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};
    const FIXTURE: &str = "U2FsdGVkX1/8PPSrrYbAWr+UDKgbuD35f5fyNpe2a2iSL4sV38qy32I6OFsW7L09lF79f0KVWFS7016P+C41NXiFPlM3HLET8ANn8/Ca6Rq9TSnypTXzV+Fpye9rMgZMvvqBTXINdLhf4zJ2qsPLtbhdQKMazDzErnk5auFroCLG0893CNq8Uk3SHEXYt8c7E8Lg+cp9aC0iUIPWqA6nvJtDC9bU7A/7rmQV1iUlD1nhPgv2bcdsvIrb+IS+yNfJRbqQfJIaATJrg0YVh2QlwZhJiSc+4HR1T0PJev9VeSc=";
    struct Host {
        expected: String,
    }
    impl ProviderHost for Host {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            assert_eq!(request.url, self.expected);
            assert!(
                request
                    .headers
                    .iter()
                    .any(|header| header.name == "Referer" && header.value.contains("/embed/"))
            );
            Ok(Response {
                url: request.url,
                status: 200,
                headers: Vec::new(),
                body: FIXTURE.as_bytes().to_vec(),
            })
        }
        fn fetch_batch(
            &self,
            _: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            Err(HostError::Denied)
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(1762450000)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn resolves_movie_and_tv_with_decrypted_sources_and_tracks()
    -> Result<(), Box<dyn std::error::Error>> {
        for (title, path) in [
            (json!({"type":"movie","tmdb_id":550}), "movie/550"),
            (
                json!({"type":"tv","tmdb_id":1622,"season":1,"episode":2}),
                "tv/1622/1/2",
            ),
        ] {
            let lookup: Lookup = serde_json::from_value(json!({"title":title}))?;
            let expected = format!("{ORIGIN}/api/{path}?_cb=1762450000");
            let result = resolve(&lookup, &Host { expected })?;
            result.validate_for(&common::provider_id("fmovies4u")?, &lookup.title, 32)?;
            let candidates = result.candidates.ok_or("missing candidates")?;
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].title, lookup.title);
            assert_eq!(candidates[0].format, StreamFormat::Hls);
            assert!(!candidates[0].content_verified);
            assert_eq!(candidates[0].resolver_metadata.tracks.len(), 1);
        }
        Ok(())
    }
    #[test]
    fn rejects_invalid_and_private_destination_urls() {
        assert!(public_url("http://127.0.0.1/evil.m3u8").is_err());
    }

    #[test]
    fn long_unicode_subtitle_label_keeps_candidate_valid() -> Result<(), Box<dyn std::error::Error>>
    {
        let lookup: Lookup =
            serde_json::from_value(json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let provider = common::provider_id("fmovies4u")?;
        let candidate = CandidateStream {
            id: "fmovies4u-unicode-track".into(),
            server: "default".into(),
            selected_variant_url: String::new(),
            url: "https://cdn.example.org/master.m3u8".into(),
            provider: provider.clone(),
            format: StreamFormat::Hls,
            title: lookup.title.clone(),
            fetch_context: FetchContext {
                scopes: Vec::new(),
                public_resources: Some(PublicResourceContext {
                    origin: ORIGIN.into(),
                    referer: format!("{ORIGIN}/"),
                }),
            },
            resolver_metadata: Metadata {
                tracks: vec![subtitle_track(
                    &"é".repeat(200),
                    "https://cdn.example.org/en.vtt".into(),
                )],
                tracks_known: true,
                ..Default::default()
            },
            content_verified: false,
            expires_at: None,
        };
        candidate.validate_for(&provider, &lookup.title)?;
        let track = &candidate.resolver_metadata.tracks[0];
        assert!(track.language.len() <= 64);
        assert!(track.name.len() <= 256);
        Ok(())
    }

    #[test]
    fn rejects_invalid_title_input() -> Result<(), Box<dyn std::error::Error>> {
        let host = Host {
            expected: String::new(),
        };
        let lookup: Lookup = serde_json::from_value(json!({"title":{"type":"tv","tmdb_id":1396}}))?;
        assert!(matches!(resolve(&lookup, &host), Err(AdapterError::Input)));
        Ok(())
    }

    #[test]
    fn resolves_special_file2_stream_link_with_videostr_headers()
    -> Result<(), Box<dyn std::error::Error>> {
        const FILE2_FIXTURE: &str = "U2FsdGVkX19sOwRw5GkLr2yGgN25M41N1kWkhDuE4FumkP9OxM+0/yEO9LVIKkUN91wN/o0e1iOTWsyxU8YoacAA6etJfLu1veONBK94Ut1tUlVqL4EV3K8Py6j4ohLayo/+CrmsyH7Le7soUKFdPtnSvqJt7/++NgzD+fNaFt1BS7CT8KXXJKSOAZoARB0LLEuHt3vDt5WVoBdCNidhgw==";
        struct File2Host;
        impl ProviderHost for File2Host {
            fn fetch(&self, request: Request) -> Result<Response, HostError> {
                Ok(Response {
                    url: request.url,
                    status: 200,
                    headers: Vec::new(),
                    body: FILE2_FIXTURE.as_bytes().to_vec(),
                })
            }
            fn fetch_batch(
                &self,
                _: Vec<Request>,
            ) -> Result<Vec<Result<Response, HostError>>, HostError> {
                Err(HostError::Denied)
            }
            fn now_unix_ms(&self) -> Result<i64, HostError> {
                Ok(1762450000)
            }
            fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
                Err(HostError::Denied)
            }
        }
        let lookup: Lookup =
            serde_json::from_value(json!({"title":{"type":"movie","tmdb_id":27205}}))?;
        let result = resolve(&lookup, &File2Host)?;
        let candidates = result.candidates.ok_or("missing candidates")?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].url,
            "https://cdn.example.org/file2/master.m3u8"
        );
        let scope = &candidates[0].fetch_context.scopes[0];
        assert_eq!(
            scope.headers.get("referer").map(String::as_str),
            Some("https://videostr.net/")
        );
        assert_eq!(
            scope.headers.get("origin").map(String::as_str),
            Some("https://videostr.net")
        );
        Ok(())
    }

    #[test]
    fn handles_upstream_api_failure_and_corrupt_payload() -> Result<(), Box<dyn std::error::Error>>
    {
        const FAIL_FIXTURE: &str =
            "U2FsdGVkX19WjbBqOrjOsrlf65FalxfNmCugI/xtnl/n4H8LlRYTIw2oA7zNyHEf";
        struct MockHost {
            body: Vec<u8>,
        }
        impl ProviderHost for MockHost {
            fn fetch(&self, request: Request) -> Result<Response, HostError> {
                Ok(Response {
                    url: request.url,
                    status: 200,
                    headers: Vec::new(),
                    body: self.body.clone(),
                })
            }
            fn fetch_batch(
                &self,
                _: Vec<Request>,
            ) -> Result<Vec<Result<Response, HostError>>, HostError> {
                Err(HostError::Denied)
            }
            fn now_unix_ms(&self) -> Result<i64, HostError> {
                Ok(1762450000)
            }
            fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
                Err(HostError::Denied)
            }
        }
        let lookup: Lookup =
            serde_json::from_value(json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let host_fail = MockHost {
            body: FAIL_FIXTURE.as_bytes().to_vec(),
        };
        assert!(matches!(
            resolve(&lookup, &host_fail),
            Err(AdapterError::Unavailable(_))
        ));

        let host_empty = MockHost { body: Vec::new() };
        assert!(matches!(
            resolve(&lookup, &host_empty),
            Err(AdapterError::Unavailable(_))
        ));

        let host_corrupt = MockHost {
            body: b"invalid ciphertext".to_vec(),
        };
        assert!(matches!(
            resolve(&lookup, &host_corrupt),
            Err(AdapterError::Protocol(_))
        ));
        Ok(())
    }
}
