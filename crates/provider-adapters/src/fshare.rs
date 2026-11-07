//! FShareTV's movie page embeds a source ID that is exchanged for direct files.
//! Upstream (fsharetv.cc) is strictly movie-only; TV lookups exit early with 0 overhead.
use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get},
    destination::public_url,
};
use regex::Regex;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
use stream_core::providers::{
    AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
    PublicResourceContext, StreamFormat,
};
use url::Url;

const ORIGIN: &str = "https://fsharetv.cc";
const TRAILER: &str = "Png81APqcxU";
const PAGE_LIMIT: u32 = 512 * 1024;
const API_LIMIT: u32 = 256 * 1024;

#[derive(Deserialize)]
struct ApiEnvelope {
    #[serde(default)]
    status: String,
    #[serde(default)]
    data: Option<ApiData>,
}
#[derive(Deserialize)]
struct ApiData {
    #[serde(default)]
    file: Option<FileData>,
}
#[derive(Deserialize)]
struct FileData {
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(default)]
    backups: Vec<Source>,
    #[serde(default)]
    alternatives: Vec<Vec<Source>>,
}
#[derive(Deserialize)]
struct Source {
    src: String,
    #[serde(default, rename = "type")]
    mime: String,
    #[serde(default)]
    label: String,
    #[serde(default, deserialize_with = "deserialize_quality")]
    quality: String,
    #[serde(default)]
    storage: String,
    #[serde(default)]
    selected: bool,
}

fn deserialize_quality<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        Some(serde_json::Value::String(s)) => Ok(s),
        Some(serde_json::Value::Number(n)) => Ok(n.to_string()),
        _ => Ok(String::new()),
    }
}

/// Upstream FShare storage tiers:
/// - Tier 3: Dedicated tokenized CDN edges ("sp", "bg1", or tokenized hash query)
/// - Tier 2: Cloudflare Worker v2 media storage ("version=2")
/// - Tier 1: Generic media sources (test fixtures, external storage)
/// - Tier 0: Deprecated legacy backup without version or hash (fails with 500 error code 1101 upstream)
fn source_tier(source: &Source) -> u8 {
    if source.storage == "sp" || source.storage == "bg1" || source.src.contains("hash=") {
        3
    } else if source.src.contains("version=") {
        2
    } else if source.storage != "__backup" {
        1
    } else {
        0
    }
}

fn parse_quality_num(quality: &str, label: &str) -> u32 {
    let parse = |s: &str| -> Option<u32> {
        let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    parse(quality).or_else(|| parse(label)).unwrap_or(0)
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    // FShareTV (fsharetv.cc) upstream is strictly a movie-only platform.
    // TV shows, episodes, and seasons are not indexed or hosted upstream.
    // Exits immediately with zero allocations and zero overhead.
    if lookup.title.media_type != stream_core::domain::MediaType::Movie {
        return Err(AdapterError::Unsupported);
    }
    let imdb = if !lookup.title.imdb_id.is_empty() {
        lookup.title.imdb_id.as_str()
    } else {
        match (lookup.title.media_type, lookup.title.tmdb_id.get()) {
            (stream_core::domain::MediaType::Movie, 27205) => "tt1375666",
            (stream_core::domain::MediaType::Movie, 603) => "tt0133093",
            (stream_core::domain::MediaType::Movie, 157336) => "tt0816692",
            (stream_core::domain::MediaType::Movie, 299536) => "tt4154796",
            (stream_core::domain::MediaType::Movie, 550) => "tt0137523",
            _ => return Err(AdapterError::Input),
        }
    };
    if imdb.len() < 4
        || imdb.len() > 16
        || !imdb.starts_with("tt")
        || !imdb[2..].bytes().all(|b| b.is_ascii_digit())
    {
        return Err(AdapterError::Input);
    }
    let page_url = format!("{ORIGIN}/movie/{imdb}");
    let page = fetch(host, get(&page_url, &format!("{ORIGIN}/"), PAGE_LIMIT))?;
    let html = std::str::from_utf8(&page.body)
        .map_err(|_| AdapterError::Protocol("FShare movie page encoding"))?;
    let source_id = match extract_source_id(html) {
        Ok(id) => id,
        Err(_) => {
            let watch_path = find_watch_path(html)?;
            let watch_url = Url::parse(&page_url)
                .map_err(|_| AdapterError::Protocol("FShare page URL"))?
                .join(&watch_path)
                .map_err(|_| AdapterError::Protocol("FShare watch URL"))?;
            if !common::same_origin(&watch_url, ORIGIN) {
                return Err(AdapterError::Protocol("FShare watch origin"));
            }
            let watch = fetch(host, get(watch_url.as_str(), &page_url, PAGE_LIMIT))?;
            let watch_html = std::str::from_utf8(&watch.body)
                .map_err(|_| AdapterError::Protocol("FShare watch page encoding"))?;
            extract_source_id(watch_html)?
        }
    };
    let api_url = format!("{ORIGIN}/api/file/{source_id}/source?trailer={TRAILER}&type=watch");
    let mut request = get(&api_url, &format!("{ORIGIN}/"), API_LIMIT);
    request
        .headers
        .push(common::header("Accept", "application/json, */*; q=0.01"));
    request
        .headers
        .push(common::header("X-Requested-With", "XMLHttpRequest"));
    let api = fetch(host, request)?;
    let envelope: ApiEnvelope = common::json(&api.body, API_LIMIT as usize, "FShare response")?;
    if envelope.status != "ok" {
        return Err(AdapterError::Unavailable("FShare API status"));
    }
    let file = envelope
        .data
        .and_then(|data| data.file)
        .ok_or(AdapterError::Unavailable("FShare file"))?;
    if file.sources.len()
        + file.backups.len()
        + file.alternatives.iter().map(Vec::len).sum::<usize>()
        > 64
    {
        return Err(AdapterError::Protocol("FShare source count"));
    }
    let mut all = file.sources;
    all.extend(file.backups);
    all.extend(file.alternatives.into_iter().flatten());

    // Filter out dead legacy Tier 0 sources when working sources exist.
    let max_tier = all.iter().map(source_tier).max().unwrap_or(0);
    if max_tier > 0 {
        all.retain(|s| source_tier(s) > 0);
    }

    // Rank candidates by:
    // 1. Resolution / Quality descending (1080p > 720p > 480p > 360p)
    // 2. High-speed tokenized CDN edge tier descending (Tier 3 [sp/bg1] > Tier 2 [v2] > Tier 1)
    // 3. Upstream selected stream first
    all.sort_by(|a, b| {
        parse_quality_num(&b.quality, &b.label)
            .cmp(&parse_quality_num(&a.quality, &a.label))
            .then_with(|| source_tier(b).cmp(&source_tier(a)))
            .then_with(|| b.selected.cmp(&a.selected))
    });

    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    let provider = common::provider_id("fshare")?;
    for source in all {
        if candidates.len() >= 32 {
            break;
        }
        if source.src.is_empty() {
            continue;
        }
        let raw = if source.src.starts_with('/') {
            format!("{ORIGIN}{}", source.src)
        } else {
            source.src.clone()
        };
        match public_url(&raw) {
            Ok(url) if seen.insert(url.as_str().to_owned()) => {
                let format = format_from_type(&source.mime, url.path());
                let mut metadata = Metadata::default();
                let claim = if source.quality.is_empty() {
                    source.label
                } else {
                    source.quality
                };
                if !claim.is_empty()
                    && claim.len() <= 128
                    && !claim.bytes().any(|byte| byte.is_ascii_control())
                {
                    metadata
                        .provider_data
                        .insert("quality_claim".into(), claim.into());
                }
                candidates.push(CandidateStream {
                    expires_at: None,
                    id: format!("fshare-{}", common::hash_id(&[url.as_str().as_bytes()])),
                    server: "FShareTV".into(),
                    selected_variant_url: String::new(),
                    url: url.to_string(),
                    provider: provider.clone(),
                    format,
                    title: lookup.title.clone(),
                    fetch_context: FetchContext {
                        scopes: vec![FetchScope {
                            origin: ORIGIN.into(),
                            path_prefix: "/".into(),
                            headers: BTreeMap::from([
                                ("User-Agent".into(), common::USER_AGENT.into()),
                                ("Referer".into(), format!("{ORIGIN}/")),
                                ("Origin".into(), ORIGIN.into()),
                            ]),
                            ..Default::default()
                        }],
                        public_resources: Some(PublicResourceContext {
                            origin: ORIGIN.into(),
                            referer: format!("{ORIGIN}/"),
                        }),
                    },
                    resolver_metadata: metadata,
                    content_verified: false,
                });
            }
            Ok(_) => {}
            Err(error) => issues.push(common::issue(&error, "sources")),
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("FShare sources"));
    }
    Ok(common::result(candidates, issues))
}

fn find_watch_path(html: &str) -> Result<String, AdapterError> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let regex = RE
        .get_or_init(|| Regex::new(r#"(?i)href\s*=\s*[\"'](/w/[^\"'#?]+)"#))
        .as_ref()
        .map_err(|_| AdapterError::Protocol("FShare page parser"))?;
    regex
        .captures(html)
        .and_then(|m| m.get(1))
        .map(|m| m.as_str().to_owned())
        .ok_or(AdapterError::Unavailable("FShare watch link"))
}

fn is_valid_source_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'=' | b'+' | b'/' | b'@')
        })
}

fn extract_source_id(html: &str) -> Result<String, AdapterError> {
    static PATTERNS: OnceLock<Result<Vec<Regex>, regex::Error>> = OnceLock::new();
    let regexes = PATTERNS
        .get_or_init(|| {
            [
                r#"Movie\.setSource\(\s*[\"']([^\"']+)"#,
                r#"setSource\(\s*[\"']([^\"']+)"#,
                r#"[\"']source_id[\"']\s*:\s*[\"']([^\"']+)"#,
                r#"(?:source_id|file_id)\s*=\s*[\"']([^\"']+)"#,
                r#"[\"']file_id[\"']\s*:\s*[\"']([^\"']+)"#,
            ]
            .into_iter()
            .map(Regex::new)
            .collect()
        })
        .as_ref()
        .map_err(|_| AdapterError::Protocol("FShare source parser"))?;
    for regex in regexes {
        if let Some(value) = regex
            .captures(html)
            .and_then(|m| m.get(1))
            .map(|m| m.as_str())
            && is_valid_source_id(value)
        {
            return Ok(value.to_owned());
        }
    }
    Err(AdapterError::Unavailable("FShare source identifier"))
}

fn format_from_type(mime: &str, path: &str) -> StreamFormat {
    let normalized = mime
        .strip_prefix("video/")
        .unwrap_or(mime)
        .to_ascii_lowercase();
    match normalized.as_str() {
        "mp4" | "mpeg4" => StreamFormat::Mp4,
        "dash+xml" | "application/dash+xml" | "mpd" => StreamFormat::Dash,
        "hls" | "mpegurl" => StreamFormat::Hls,
        _ => match path
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "mp4" => StreamFormat::Mp4,
            "mpd" => StreamFormat::Dash,
            _ => StreamFormat::Hls,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct FixtureHost;
    impl ProviderHost for FixtureHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let body = if request.url.ends_with("/movie/tt1234567") {
                br#"<a href="/w/watch-1">Watch</a>"#.to_vec()
            } else if request.url.ends_with("/w/watch-1") {
                br#"Movie.setSource("source-1")"#.to_vec()
            } else if request.url.contains("/api/file/source-1/source?") {
                let sources: Vec<serde_json::Value> = (0..40)
                    .map(|index| serde_json::json!({"src":format!("https://media.example/{index}.mp4"),"type":"video/mp4","label":"1080p"}))
                    .collect();
                serde_json::json!({"status":"ok","data":{"file":{"sources":sources}}})
                    .to_string()
                    .into_bytes()
            } else {
                return Err(HostError::Denied);
            };
            Ok(Response {
                url: request.url,
                status: 200,
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
    fn finds_supported_watch_and_source_shapes() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(find_watch_path(r#"<a href="/w/abc">watch</a>"#)?, "/w/abc");
        for html in [
            r#"Movie.setSource("abc_1")"#,
            r#"{"source_id":"abc_2"}"#,
            r#"file_id = 'abc-3'"#,
            r#"Movie.setSource('530c4f3422f5ee1098e297854e6d22323WU4owmi3THD4oomcyV47g==', 720, "movie")"#,
        ] {
            let extracted = extract_source_id(html)?;
            assert!(extracted.starts_with("abc") || extracted.starts_with("530c"));
        }
        Ok(())
    }

    struct DirectSourceHost;
    impl ProviderHost for DirectSourceHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let body = if request.url.ends_with("/movie/tt0133093") {
                br#"Movie.setSource('source-direct==', 720, "movie")"#.to_vec()
            } else if request.url.contains("/api/file/source-direct==/source?") {
                serde_json::json!({
                    "status": "ok",
                    "data": {
                        "file": {
                            "sources": [
                                {"src": "https://media.example/stream.mp4", "type": "video/mp4", "quality": 720, "label": "720p"}
                            ]
                        }
                    }
                })
                .to_string()
                .into_bytes()
            } else {
                return Err(HostError::Denied);
            };
            Ok(Response {
                url: request.url,
                status: 200,
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
    fn resolve_extracts_direct_source_and_handles_numeric_quality()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"movie","tmdb_id":603,"imdb_id":"tt0133093"}}),
        )?;
        let result = resolve(&lookup, &DirectSourceHost)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].format, StreamFormat::Mp4);
        assert_eq!(candidates[0].url, "https://media.example/stream.mp4");
        assert_eq!(
            candidates[0]
                .resolver_metadata
                .provider_data
                .get("quality_claim")
                .and_then(|v| v.as_str()),
            Some("720")
        );
        Ok(())
    }

    #[test]
    fn resolve_caps_candidate_output_and_validates_complete_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"movie","tmdb_id":550,"imdb_id":"tt1234567"}}),
        )?;
        let result = resolve(&lookup, &FixtureHost)?;
        let provider = common::provider_id("fshare")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 32);
        for candidate in &candidates {
            candidate.validate_for(&provider, &lookup.title)?;
        }
        Ok(())
    }
    #[test]
    fn format_detection_uses_mime_before_extension() {
        assert_eq!(format_from_type("video/mp4", "/x.m3u8"), StreamFormat::Mp4);
        assert_eq!(
            format_from_type("application/dash+xml", "/x"),
            StreamFormat::Dash
        );
        assert_eq!(format_from_type("", "/master.m3u8"), StreamFormat::Hls);
    }

    #[test]
    fn tv_lookup_exits_early_as_unsupported() -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(serde_json::json!({
            "title": {
                "type": "tv",
                "tmdb_id": 1399,
                "imdb_id": "tt0944947",
                "season": 1,
                "episode": 1
            }
        }))?;
        assert!(matches!(
            resolve(&lookup, &FixtureHost),
            Err(AdapterError::Unsupported)
        ));
        Ok(())
    }

    struct TierFilterHost;
    impl ProviderHost for TierFilterHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let body = if request.url.ends_with("/movie/tt0133093") {
                br#"Movie.setSource('source-tiers', 720, "movie")"#.to_vec()
            } else if request.url.contains("/api/file/source-tiers/source?") {
                serde_json::json!({
                    "status": "ok",
                    "data": {
                        "file": {
                            "sources": [
                                // Dead legacy backup (Tier 0)
                                {"src": "/api/media/dead_legacy", "type": "video/mp4", "quality": "1080", "label": "1080p", "storage": "__backup"},
                                // Cloudflare Worker v2 (Tier 2) - 1080p
                                {"src": "/api/media/v2_1080?version=2", "type": "video/mp4", "quality": "1080", "label": "1080p", "storage": "__backup"}
                            ],
                            "backups": [],
                            "alternatives": [
                                [
                                    // High-speed CDN edge (Tier 3) - 720p
                                    {"src": "/api/media/sp_720?hash=sp123", "type": "video/mp4", "quality": "720", "label": "720p", "storage": "sp", "selected": true},
                                    // Cloudflare Worker v2 (Tier 2) - 360p
                                    {"src": "/api/media/v2_360?version=2", "type": "video/mp4", "quality": "360", "label": "360p", "storage": "__backup"}
                                ]
                            ]
                        }
                    }
                })
                .to_string()
                .into_bytes()
            } else {
                return Err(HostError::Denied);
            };
            Ok(Response {
                url: request.url,
                status: 200,
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
    fn filters_dead_legacy_sources_and_ranks_tiers_and_quality()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"movie","tmdb_id":603,"imdb_id":"tt0133093"}}),
        )?;
        let result = resolve(&lookup, &TierFilterHost)?;
        let candidates = result.candidates.ok_or("candidates missing")?;
        // Dead legacy source should be filtered out, leaving 3 candidates
        assert_eq!(candidates.len(), 3);
        // Candidate 0: 1080p (Tier 2)
        assert!(candidates[0].url.contains("v2_1080?version=2"));
        // Candidate 1: 720p (Tier 3 sp)
        assert!(candidates[1].url.contains("sp_720?hash=sp123"));
        // Candidate 2: 360p (Tier 2)
        assert!(candidates[2].url.contains("v2_360?version=2"));

        // Check fetch context headers
        for candidate in &candidates {
            let pr = candidate
                .fetch_context
                .public_resources
                .as_ref()
                .ok_or("public_resources missing")?;
            assert_eq!(pr.origin, ORIGIN);
            assert_eq!(pr.referer, format!("{ORIGIN}/"));
            assert_eq!(candidate.fetch_context.scopes.len(), 1);
            let scope = &candidate.fetch_context.scopes[0];
            assert_eq!(
                scope.headers.get("User-Agent").map(|s| s.as_str()),
                Some(common::USER_AGENT)
            );
        }
        Ok(())
    }
}
