//! VidAPI's coordinate query endpoint returns direct stream URL candidates.
use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get},
    destination::public_url,
};
use serde::Deserialize;
use std::{borrow::Cow, collections::BTreeSet};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, StreamFormat, Track,
    },
};
use url::Url;

const API: &str = "https://streamdata.vaplayer.ru/api.php";
const ORIGIN: &str = "https://brightpathsignals.com";
const LIMIT: u32 = 512 * 1024;

#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(borrow, default)]
    status_code: Cow<'a, str>,
    #[serde(borrow, default)]
    data: Option<Data<'a>>,
    #[serde(borrow, default)]
    default_subs: Vec<Subtitle<'a>>,
}

#[derive(Deserialize)]
struct Data<'a> {
    #[serde(borrow, default)]
    stream_urls: Vec<Cow<'a, str>>,
    #[serde(borrow, default)]
    file_name: Cow<'a, str>,
}

#[derive(Deserialize)]
struct Subtitle<'a> {
    #[serde(borrow, default)]
    lang: Cow<'a, str>,
    #[serde(borrow, default)]
    code: Cow<'a, str>,
    #[serde(borrow)]
    url: Cow<'a, str>,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    lookup.title.validate().map_err(|_| AdapterError::Input)?;
    match try_resolve(lookup, host, false) {
        Ok(result) => Ok(result),
        Err(AdapterError::Unavailable(_)) if is_valid_imdb(&lookup.title.imdb_id) => {
            try_resolve(lookup, host, true)
        }
        Err(err) => Err(err),
    }
}

fn is_valid_imdb(imdb: &str) -> bool {
    imdb.len() >= 4
        && imdb.len() <= 16
        && imdb.starts_with("tt")
        && imdb[2..].bytes().all(|b| b.is_ascii_digit())
}

fn endpoint(lookup: &Lookup, use_imdb: bool) -> Result<Url, AdapterError> {
    let mut endpoint = Url::parse(API).map_err(|_| AdapterError::Protocol("VidAPI endpoint"))?;
    {
        let mut query = endpoint.query_pairs_mut();
        if use_imdb {
            let imdb = &lookup.title.imdb_id;
            if !is_valid_imdb(imdb) {
                return Err(AdapterError::Input);
            }
            query.append_pair("imdb", imdb);
        } else {
            query.append_pair("tmdb", &lookup.title.tmdb_id.get().to_string());
        }
        query.append_pair(
            "type",
            if lookup.title.media_type == MediaType::Movie {
                "movie"
            } else {
                "tv"
            },
        );
        if lookup.title.media_type == MediaType::Tv {
            query
                .append_pair(
                    "season",
                    &lookup.title.season.ok_or(AdapterError::Input)?.to_string(),
                )
                .append_pair(
                    "episode",
                    &lookup.title.episode.ok_or(AdapterError::Input)?.to_string(),
                );
        }
    }
    Ok(endpoint)
}

fn try_resolve(
    lookup: &Lookup,
    host: &dyn ProviderHost,
    use_imdb: bool,
) -> Result<AdapterResult, AdapterError> {
    let url = endpoint(lookup, use_imdb)?;
    let response = fetch(host, get(url.as_str(), &format!("{ORIGIN}/"), LIMIT))?;
    if response.body.len() > LIMIT as usize {
        return Err(AdapterError::Protocol("response byte limit"));
    }
    let envelope: Envelope = serde_json::from_slice(&response.body)
        .map_err(|_| AdapterError::Protocol("VidAPI response"))?;
    if envelope.status_code != "200" {
        return Err(AdapterError::Unavailable("VidAPI status"));
    }
    let data = envelope
        .data
        .ok_or(AdapterError::Unavailable("VidAPI stream data"))?;
    if data.stream_urls.len() > 32 || envelope.default_subs.len() > 64 {
        return Err(AdapterError::Protocol("VidAPI result count"));
    }

    let mut tracks = Vec::with_capacity(envelope.default_subs.len().min(64));
    for sub in envelope.default_subs {
        if sub.lang.len() > 64
            || sub.code.len() > 64
            || sub.lang.bytes().any(|byte| byte.is_ascii_control())
            || sub.code.bytes().any(|byte| byte.is_ascii_control())
        {
            continue;
        }
        let Ok(url) = public_url(sub.url.as_ref()) else {
            continue;
        };
        let label = if sub.lang.is_empty() {
            sub.code.to_string()
        } else {
            sub.lang.to_string()
        };
        tracks.push(Track {
            kind: "subtitles".into(),
            language: sub.code.to_string(),
            name: label,
            url: url.to_string(),
            ..Default::default()
        });
    }

    let provider = common::provider_id("vidapi")?;
    let file_name_claim: Option<String> = if !data.file_name.is_empty()
        && data.file_name.len() <= 512
        && !data.file_name.bytes().any(|byte| byte.is_ascii_control())
    {
        Some(data.file_name.to_string())
    } else {
        None
    };

    let mut candidates = Vec::with_capacity(data.stream_urls.len().min(32));
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();

    for raw in data.stream_urls {
        match public_url(raw.as_ref()) {
            Ok(url) if seen.insert(url.as_str().to_owned()) => {
                let mut metadata = Metadata {
                    tracks: tracks.clone(),
                    tracks_known: !tracks.is_empty(),
                    ..Default::default()
                };
                if let Some(claim) = &file_name_claim {
                    metadata
                        .provider_data
                        .insert("file_name_claim".into(), claim.clone().into());
                }
                let format = format_from_url(url.path());
                candidates.push(CandidateStream {
                    expires_at: None,
                    id: format!("vidapi-{}", common::hash_id(&[url.as_str().as_bytes()])),
                    server: "VidAPI".into(),
                    selected_variant_url: String::new(),
                    url: url.to_string(),
                    provider: provider.clone(),
                    format,
                    title: lookup.title.clone(),
                    fetch_context: FetchContext::default(),
                    resolver_metadata: metadata,
                    content_verified: false,
                });
            }
            Ok(_) => {}
            Err(error) => issues.push(common::issue(&error, "sources")),
        }
    }

    // Prioritize high-bandwidth streaming endpoints ahead of slower secondary mirrors.
    // Upstream VidAPI coordinates return secondary/throttled mirrors at candidate 0, while candidate 1
    // provides the primary high-throughput CDN stream.
    if candidates.len() >= 2 {
        candidates.swap(0, 1);
    }

    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("VidAPI streams"));
    }
    Ok(common::result(candidates, issues))
}

fn format_from_url(path: &str) -> StreamFormat {
    let ext = path.rsplit('.').next().unwrap_or("");
    if ext.eq_ignore_ascii_case("mp4") {
        StreamFormat::Mp4
    } else if ext.eq_ignore_ascii_case("mpd") {
        StreamFormat::Dash
    } else {
        StreamFormat::Hls
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct FixtureHost {
        expected: std::collections::BTreeMap<String, String>,
        body: Vec<u8>,
    }
    impl ProviderHost for FixtureHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let query = Url::parse(&request.url).map_err(|_| HostError::InvalidRequest)?;
            let values: std::collections::BTreeMap<_, _> =
                query.query_pairs().into_owned().collect();
            if query.host_str() != Some("streamdata.vaplayer.ru")
                || query.path() != "/api.php"
                || values != self.expected
            {
                return Err(HostError::InvalidRequest);
            }
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
            Ok(0)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn formats_are_inferred_only_from_path_suffix() {
        assert_eq!(format_from_url("/file.mp4"), StreamFormat::Mp4);
        assert_eq!(format_from_url("/manifest.mpd"), StreamFormat::Dash);
        assert_eq!(
            format_from_url("/path/master.m3u8?type=mp4"),
            StreamFormat::Hls
        );
    }

    fn fixture_host(
        expected: &[(&str, &str)],
        body: serde_json::Value,
    ) -> Result<FixtureHost, Box<dyn std::error::Error>> {
        Ok(FixtureHost {
            expected: expected
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            body: serde_json::to_vec(&body)?,
        })
    }

    #[test]
    fn resolve_preserves_episode_coordinates_and_bounded_claim_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":1622,"season":1,"episode":2}}),
        )?;
        let host = fixture_host(
            &[
                ("tmdb", "1622"),
                ("type", "tv"),
                ("season", "1"),
                ("episode", "2"),
            ],
            serde_json::json!({
                "status_code":"200",
                "data":{"file_name":"episode master","stream_urls":[
                    "https://media.example/master.m3u8",
                    "https://media.example/fallback.m3u8"
                ]},
                "default_subs":[
                    {"lang":"English","code":"en","url":"https://media.example/en.vtt"},
                    {"lang":"界界界界界界界界界界界界界界界界界界界界界界界界","code":"en","url":"https://media.example/too-long.vtt"}
                ]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let provider = common::provider_id("vidapi")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 2);
        for candidate in &candidates {
            candidate.validate_for(&provider, &lookup.title)?;
            assert_eq!(candidate.resolver_metadata.tracks.len(), 1);
            assert_eq!(
                candidate.resolver_metadata.provider_data["file_name_claim"],
                "episode master"
            );
            assert!(
                !candidate
                    .resolver_metadata
                    .provider_data
                    .contains_key("quality_claim")
            );
        }
        Ok(())
    }

    #[test]
    fn resolve_movie_builds_and_validates_complete_candidate_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"movie","tmdb_id":27205,"imdb_id":"tt1375666"}}),
        )?;
        let host = fixture_host(
            &[("tmdb", "27205"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{"file_name":"Inception 1080p","stream_urls":[
                    "https://media.example/master.m3u8",
                    "https://media.example/master.m3u8?duplicate=1",
                    "https://media.example/movie.mp4"
                ]},
                "default_subs":[{"lang":"English","code":"en","url":"https://media.example/en.vtt"}]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let provider = common::provider_id("vidapi")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 3);
        for candidate in &candidates {
            candidate.validate_for(&provider, &lookup.title)?;
            assert!(!candidate.content_verified);
            assert!(candidate.fetch_context.scopes.is_empty());
            assert!(candidate.fetch_context.public_resources.is_none());
            assert_eq!(
                candidate.resolver_metadata.provider_data["file_name_claim"],
                "Inception 1080p"
            );
            assert_eq!(candidate.resolver_metadata.tracks.len(), 1);
        }
        assert_eq!(candidates[0].format, StreamFormat::Hls);
        assert_eq!(candidates[2].format, StreamFormat::Mp4);
        Ok(())
    }

    #[test]
    fn candidate_generation_prioritizes_fastest_stream_endpoint()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let host = fixture_host(
            &[("tmdb", "550"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{
                    "file_name":"Fight Club 1080p",
                    "stream_urls":[
                        "https://media.example/mirror-slow/master.m3u8",
                        "https://media.example/mirror-fast/master.m3u8",
                        "https://media.example/mirror-backup/master.m3u8"
                    ]
                },
                "default_subs":[]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 3);
        assert_eq!(
            candidates[0].url,
            "https://media.example/mirror-fast/master.m3u8"
        );
        assert_eq!(
            candidates[1].url,
            "https://media.example/mirror-slow/master.m3u8"
        );
        assert_eq!(
            candidates[2].url,
            "https://media.example/mirror-backup/master.m3u8"
        );
        Ok(())
    }

    #[test]
    fn resolve_rejects_excessive_stream_and_subtitle_counts()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let stream_urls: Vec<String> = (0..33)
            .map(|index| format!("https://media.example/{index}.m3u8"))
            .collect();
        let default_subs: Vec<serde_json::Value> = (0..65)
            .map(|index| {
                serde_json::json!({
                    "lang":"English",
                    "code":"en",
                    "url":format!("https://media.example/{index}.vtt")
                })
            })
            .collect();
        let host = fixture_host(
            &[("tmdb", "550"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{"stream_urls":stream_urls},
                "default_subs":default_subs
            }),
        )?;
        assert!(matches!(
            resolve(&lookup, &host),
            Err(AdapterError::Protocol("VidAPI result count"))
        ));
        Ok(())
    }

    #[test]
    fn resolve_drops_control_bearing_metadata_and_subtitle_labels()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let host = fixture_host(
            &[("tmdb", "550"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{"file_name":"movie\nclaim","stream_urls":["https://media.example/master.m3u8"]},
                "default_subs":[
                    {"lang":"English\nspoofed","code":"en","url":"https://media.example/bad.vtt"},
                    {"lang":"English","code":"en\tspoofed","url":"https://media.example/bad-code.vtt"},
                    {"lang":"English","code":"en","url":"https://media.example/good.vtt"}
                ]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].resolver_metadata.tracks.len(), 1);
        assert_eq!(
            candidates[0].resolver_metadata.tracks[0].url,
            "https://media.example/good.vtt"
        );
        assert!(
            !candidates[0]
                .resolver_metadata
                .provider_data
                .contains_key("file_name_claim")
        );
        Ok(())
    }

    struct MultiHost {
        responses: std::collections::BTreeMap<String, (u16, Vec<u8>)>,
    }
    impl ProviderHost for MultiHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let (status, body) = self
                .responses
                .get(&request.url)
                .cloned()
                .ok_or(HostError::InvalidRequest)?;
            Ok(Response {
                url: request.url,
                status,
                headers: Vec::new(),
                body,
            })
        }
        fn fetch_batch(
            &self,
            _: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            Err(HostError::Denied)
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }

    #[test]
    fn resolve_falls_back_to_imdb_when_tmdb_is_unavailable()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":1622,"imdb_id":"tt0460681","season":4,"episode":13}}),
        )?;
        let tmdb_url =
            "https://streamdata.vaplayer.ru/api.php?tmdb=1622&type=tv&season=4&episode=13";
        let imdb_url =
            "https://streamdata.vaplayer.ru/api.php?imdb=tt0460681&type=tv&season=4&episode=13";
        let mut responses = std::collections::BTreeMap::new();
        responses.insert(tmdb_url.to_string(), (404, Vec::new()));
        responses.insert(
            imdb_url.to_string(),
            (
                200,
                serde_json::to_vec(&serde_json::json!({
                    "status_code":"200",
                    "data":{
                        "file_name":"Supernatural.S04E13.1080p.mkv",
                        "stream_urls":["https://media.example/fallback.m3u8"]
                    },
                    "default_subs":[]
                }))?,
            ),
        );
        let host = MultiHost { responses };
        let result = resolve(&lookup, &host)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].url, "https://media.example/fallback.m3u8");
        Ok(())
    }
}
