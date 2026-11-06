//! VidNest's supported server catalog using its custom-base64 response codec.
use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, get},
    destination::public_url,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
        PublicResourceContext, StreamFormat, Track,
    },
};
use stream_provider_codecs::vidnest::{decrypt, decrypt_v2};
use stream_provider_sdk::http::HostError;

const ORIGIN: &str = "https://vidnest.fun";
const API: &str = "https://new.vidnest.fun";
const SERVERS: [(&str, &str); 12] = [
    ("catflix", "yflix"),
    ("filxer", "rogflix"),
    ("gama", "vidzee"),
    ("lamda", "ophim"),
    ("beta", "flixhq"),
    ("alfa", "onehd"),
    ("prime", "rpmvid"),
    ("zeta", "nextgencloudfabric"),
    ("ophim", "klikxxi"),
    ("sigma", "hollymoviehd"),
    ("hexa", "vidlink"),
    ("delta", "allmovies"),
];
const LIMIT: u32 = 512 * 1024;
struct Source {
    url: String,
    format: String,
    language: String,
    headers: serde_json::Map<String, Value>,
}
#[derive(Deserialize, Default)]
struct Envelope {
    #[serde(default)]
    v: u8,
    #[serde(default)]
    d: String,
    #[serde(default)]
    n: String,
    #[serde(default)]
    encrypted: bool,
    #[serde(default)]
    data: String,
}
fn collect_sources(value: &Value, out: &mut Vec<Source>, depth: usize) {
    if depth > 8 || out.len() >= 64 {
        return;
    }
    match value {
        Value::Object(map) => {
            // Subtitle records often have a `url` field too. Never let their
            // text rendition URLs enter the video candidate list.
            let record_kind = map
                .get("kind")
                .or_else(|| map.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let subtitle_record = matches!(
                record_kind.as_str(),
                "subtitle" | "subtitles" | "caption" | "captions" | "text" | "vtt" | "srt"
            ) || (map.contains_key("url")
                && (map.contains_key("lang") || map.contains_key("language"))
                && (map.contains_key("label") || map.contains_key("name"))
                && !map.contains_key("quality")
                && !map.contains_key("format"));
            if subtitle_record {
                return;
            }
            for (key, item) in map {
                if matches!(
                    key.as_str(),
                    "url" | "file" | "playlist" | "link" | "stream_url"
                ) {
                    if let Some(url) = item.as_str().filter(|s| s.len() <= 8192) {
                        let typ = map
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let headers = map
                            .get("headers")
                            .and_then(Value::as_object)
                            .cloned()
                            .unwrap_or_default();
                        let language = map
                            .get("language")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        out.push(Source {
                            url: url.to_owned(),
                            format: typ,
                            language,
                            headers,
                        });
                    }
                } else if matches!(
                    key.as_str(),
                    "sources" | "streams" | "data" | "stream" | "url"
                ) {
                    collect_sources(item, out, depth + 1);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(64) {
                collect_sources(item, out, depth + 1);
            }
        }
        _ => {}
    }
}
fn stream_context(
    url: &str,
    raw_headers: &serde_json::Map<String, Value>,
) -> Result<FetchContext, AdapterError> {
    if raw_headers.len() > 32 {
        return Err(AdapterError::Protocol("VidNest stream header count"));
    }
    let mut headers = std::collections::BTreeMap::new();
    let mut public_origin = ORIGIN.to_owned();
    let mut public_referer = format!("{ORIGIN}/");
    for (name, value) in raw_headers {
        let value = value
            .as_str()
            .ok_or(AdapterError::Protocol("VidNest stream header value"))?;
        if value.len() > 2048 || value.bytes().any(|b| b.is_ascii_control()) {
            return Err(AdapterError::Protocol("VidNest stream header value"));
        }
        let canonical = if name.eq_ignore_ascii_case("referer") {
            "Referer"
        } else if name.eq_ignore_ascii_case("origin") {
            "Origin"
        } else if name.eq_ignore_ascii_case("user-agent") {
            "User-Agent"
        } else if name.eq_ignore_ascii_case("accept") {
            "Accept"
        } else if name.eq_ignore_ascii_case("accept-language") {
            "Accept-Language"
        } else if name.eq_ignore_ascii_case("authorization") {
            "Authorization"
        } else {
            continue;
        };
        if matches!(canonical, "Referer" | "Origin") {
            let parsed = public_url(value)?;
            if canonical == "Origin"
                && (parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some())
            {
                return Err(AdapterError::Protocol("VidNest stream origin"));
            }
            if canonical == "Origin" {
                public_origin.clear();
            } else {
                public_referer.clear();
            }
        }
        headers.insert(canonical.to_owned(), value.to_owned());
    }
    let mut context = FetchContext {
        scopes: Vec::new(),
        public_resources: Some(PublicResourceContext {
            origin: public_origin,
            referer: public_referer,
        }),
    };
    if !headers.is_empty() {
        let destination = public_url(url)?;
        context.scopes.push(FetchScope {
            headers,
            origin: format!("{}/", destination.origin().ascii_serialization()),
            path_prefix: "/".into(),
            ..Default::default()
        });
    }
    Ok(context)
}
fn collect_tracks(value: &Value, tracks: &mut Vec<Track>, depth: usize) {
    if depth > 8 || tracks.len() >= 64 {
        return;
    }
    match value {
        Value::Object(map) => {
            let label = map
                .get("lang")
                .or_else(|| map.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let name = map
                .get("label")
                .or_else(|| map.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(label);
            let url = map.get("url").and_then(Value::as_str).unwrap_or("");
            if !label.is_empty()
                && label.len() <= 64
                && !label.bytes().any(|byte| byte.is_ascii_control())
                && !name.is_empty()
                && name.len() <= 256
                && !name.bytes().any(|byte| byte.is_ascii_control())
                && !url.is_empty()
                && url.len() <= 8192
                && !url.bytes().any(|byte| byte.is_ascii_control())
                && public_url(url).is_ok()
            {
                tracks.push(Track {
                    kind: "subtitles".into(),
                    name: name.to_owned(),
                    language: label.to_owned(),
                    url: url.into(),
                    ..Default::default()
                });
            }
            for (key, item) in map {
                if matches!(key.as_str(), "captions" | "subtitles" | "tracks" | "data") {
                    collect_tracks(item, tracks, depth + 1);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(64) {
                collect_tracks(item, tracks, depth + 1);
            }
        }
        _ => {}
    }
}
pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let mut requests = Vec::with_capacity(SERVERS.len());
    for (_, path) in SERVERS {
        let media_path = if lookup.title.media_type == MediaType::Tv {
            format!(
                "tv/{}/{}/{}",
                lookup.title.tmdb_id.get(),
                lookup.title.season.ok_or(AdapterError::Input)?,
                lookup.title.episode.ok_or(AdapterError::Input)?
            )
        } else {
            format!("movie/{}", lookup.title.tmdb_id.get())
        };
        let random = host.random_bytes(16)?;
        if random.len() != 16 {
            return Err(AdapterError::Protocol("VidNest nonce length"));
        }
        let nonce = URL_SAFE_NO_PAD.encode(random);
        let mut endpoint = url::Url::parse(&format!("{API}/{path}/{media_path}"))
            .map_err(|_| AdapterError::Protocol("VidNest endpoint"))?;
        endpoint.query_pairs_mut().append_pair("_n", &nonce);
        let mut req = get(endpoint.as_str(), &format!("{ORIGIN}/"), LIMIT);
        req.timeout_ms = 3_000;
        req.headers.push(common::header("Origin", ORIGIN));
        requests.push(req);
    }
    let mut responses = batch(host, requests.clone())?;
    let retry_indices = responses
        .iter()
        .enumerate()
        .filter_map(|(index, response)| {
            matches!(
                response,
                Err(AdapterError::Host(
                    HostError::Timeout | HostError::UpstreamFailure
                ))
            )
            .then_some(index)
        })
        .collect::<Vec<_>>();
    if !retry_indices.is_empty() {
        let retry_requests = retry_indices
            .iter()
            .map(|index| requests[*index].clone())
            .collect();
        let retry_results = batch(host, retry_requests)?;
        for (index, retry) in retry_indices.into_iter().zip(retry_results) {
            responses[index] = retry;
        }
    }
    let provider = common::provider_id("vidnest")?;
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    for ((server, _), response) in SERVERS.into_iter().zip(responses) {
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                issues.push(common::issue(&e, "server"));
                continue;
            }
        };
        let envelope: Envelope =
            match common::json(&response.body, LIMIT as usize, "VidNest response") {
                Ok(v) => v,
                Err(e) => {
                    issues.push(common::issue(&e, "server"));
                    continue;
                }
            };
        let data = if envelope.v == 2 && !envelope.d.is_empty() && !envelope.n.is_empty() {
            match decrypt_v2(&envelope.d, &envelope.n) {
                Ok(v) => v,
                Err(_) => {
                    issues.push(common::issue(
                        &AdapterError::Protocol("VidNest v2 envelope"),
                        "decrypt",
                    ));
                    continue;
                }
            }
        } else if envelope.encrypted {
            match decrypt(&envelope.data) {
                Ok(v) => v,
                Err(_) => {
                    issues.push(common::issue(
                        &AdapterError::Protocol("VidNest envelope"),
                        "decrypt",
                    ));
                    continue;
                }
            }
        } else {
            response.body.clone()
        };
        let root: Value = match common::json(&data, LIMIT as usize, "VidNest payload") {
            Ok(v) => v,
            Err(e) => {
                issues.push(common::issue(&e, "decrypt"));
                continue;
            }
        };
        let mut sources = Vec::new();
        collect_sources(&root, &mut sources, 0);
        let mut tracks = Vec::new();
        collect_tracks(&root, &mut tracks, 0);
        let mut track_urls = BTreeSet::new();
        tracks.retain(|track| track_urls.insert(track.url.clone()));
        let track_count = tracks.len();
        for (index, source) in sources.into_iter().enumerate() {
            let url = source.url;
            if public_url(&url).is_err() || seen.contains(&url) {
                continue;
            }
            let fetch_context = match stream_context(&url, &source.headers) {
                Ok(context) => context,
                Err(error) => {
                    issues.push(common::issue(&error, "headers"));
                    continue;
                }
            };
            let lower = url.to_ascii_lowercase();
            let format = match source.format.to_ascii_lowercase().as_str() {
                "mp4" => StreamFormat::Mp4,
                "hls" | "m3u8" => StreamFormat::Hls,
                "dash" | "mpd" => StreamFormat::Dash,
                "" if lower
                    .split('?')
                    .next()
                    .is_some_and(|path| path.ends_with(".mp4")) =>
                {
                    StreamFormat::Mp4
                }
                "" if lower
                    .split('?')
                    .next()
                    .is_some_and(|path| path.ends_with(".mpd")) =>
                {
                    StreamFormat::Dash
                }
                "" if lower.split('?').next().is_some_and(|path| {
                    path.ends_with(".m3u8")
                        || (path.ends_with(".txt")
                            && (path.contains("master") || path.contains("hls")))
                }) =>
                {
                    StreamFormat::Hls
                }
                _ => continue,
            };
            candidates.push(CandidateStream {
                id: format!(
                    "vidnest-{}-{index}-{}",
                    server.split('?').next().unwrap_or(server),
                    common::hash_id(&[url.as_bytes()])
                ),
                server: server.into(),
                url: url.clone(),
                format,
                provider: provider.clone(),
                title: lookup.title.clone(),
                fetch_context,
                resolver_metadata: {
                    let mut metadata = Metadata {
                        tracks: tracks.clone(),
                        tracks_known: track_count > 0,
                        ..Default::default()
                    };
                    if !source.language.is_empty()
                        && source.language.len() <= 128
                        && !source.language.bytes().any(|b| b.is_ascii_control())
                    {
                        metadata.provider_data.insert(
                            "audio_language_claim".into(),
                            serde_json::json!(source.language),
                        );
                    }
                    metadata
                },
                content_verified: false,
                expires_at: None,
                selected_variant_url: String::new(),
            });
            seen.insert(url);
        }
        if candidates.len() >= 32 {
            break;
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("VidNest servers"));
    }
    Ok(common::result(candidates, issues))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct FixtureHost;

    impl ProviderHost for FixtureHost {
        fn fetch(&self, _: Request) -> Result<Response, HostError> {
            Err(HostError::Denied)
        }

        fn fetch_batch(
            &self,
            requests: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            let mut responses = Vec::with_capacity(requests.len());
            for request in requests {
                assert!(request.url.contains("_n="));
                assert_eq!(request.timeout_ms, 3_000);
                let path = if request.url.contains("/tv/") {
                    "/tv/1399/2/5"
                } else {
                    "/movie/550"
                };
                assert!(request.url.contains(path));
                let body = r#"{"sources":[{"url":"https://cdn.example/master.m3u8","type":"hls"}],"subtitles":[{"url":"https://cdn.example/en.vtt","language":"en","label":"English"},{"url":"https://cdn.example/fr.vtt","language":"fr","label":"Français"}]}"#.as_bytes().to_vec();
                responses.push(Ok(Response {
                    url: request.url,
                    status: 200,
                    headers: Vec::new(),
                    body,
                }));
            }
            Ok(responses)
        }

        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }

        fn random_bytes(&self, length: u32) -> Result<Vec<u8>, HostError> {
            Ok(vec![7; length as usize])
        }
    }

    #[test]
    fn resolves_movie_and_tv_with_validated_candidates_and_subtitles()
    -> Result<(), Box<dyn std::error::Error>> {
        for title in [
            serde_json::json!({"type":"movie","tmdb_id":550}),
            serde_json::json!({"type":"tv","tmdb_id":1399,"season":2,"episode":5}),
        ] {
            let lookup: Lookup = serde_json::from_value(serde_json::json!({"title":title}))?;
            let provider = common::provider_id("vidnest")?;
            let result = resolve(&lookup, &FixtureHost)?;
            result.validate_for(&provider, &lookup.title, 32)?;
            let candidates = result.candidates.ok_or("candidates missing")?;
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].format, StreamFormat::Hls);
            assert_eq!(candidates[0].resolver_metadata.tracks.len(), 2);
            assert_eq!(candidates[0].resolver_metadata.tracks[1].name, "Français");
        }
        Ok(())
    }

    #[test]
    fn excludes_subtitle_records_from_video_sources() {
        let payload = serde_json::json!({"sources":[
            {"url":"https://cdn.example/video.m3u8","type":"hls"},
            {"url":"https://cdn.example/captions.m3u8","language":"en","label":"English","type":"subtitle"}
        ]});
        let mut sources = Vec::new();
        collect_sources(&payload, &mut sources, 0);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url, "https://cdn.example/video.m3u8");
    }

    #[test]
    fn source_headers_override_only_their_root_fallback() -> Result<(), Box<dyn std::error::Error>>
    {
        let source_headers = serde_json::Map::from_iter([
            ("Origin".into(), serde_json::json!("https://vidnest.fun/")),
            (
                "Referer".into(),
                serde_json::json!("https://vidnest.fun/embed/550"),
            ),
            ("User-Agent".into(), serde_json::json!("VidNest source UA")),
        ]);
        let context = stream_context("https://cdn.example/master.m3u8", &source_headers)?;
        let public = context
            .public_resources
            .as_ref()
            .ok_or("public resources missing")?;
        assert!(public.origin.is_empty());
        assert!(public.referer.is_empty());
        assert_eq!(context.scopes.len(), 1);
        assert_eq!(context.scopes[0].headers.len(), 3);
        assert_eq!(
            context.scopes[0].headers["Referer"],
            "https://vidnest.fun/embed/550"
        );
        assert!(context.scopes[0].headers.contains_key("User-Agent"));
        let root_fallback =
            stream_context("https://cdn.example/master.m3u8", &serde_json::Map::new())?;
        let root = root_fallback
            .public_resources
            .ok_or("public resources missing")?;
        assert_eq!(root.origin, ORIGIN);
        assert_eq!(root.referer, "https://vidnest.fun/");
        Ok(())
    }
}
