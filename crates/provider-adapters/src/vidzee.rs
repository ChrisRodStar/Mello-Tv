//! VidZee's current plain-stream endpoint (`/streams/...?...&e=0`).
use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, header},
    destination::public_url,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
        PublicResourceContext, StreamFormat,
    },
};
use stream_provider_sdk::http::Request;

const API: &str = "https://core.vidzee.wtf";
const PLAYER: &str = "https://player.vidzee.wtf";
const PLAYER_REFERER: &str = "https://player.vidzee.wtf/";
const SERVERS: [(&str, &str); 4] = [
    ("dcloud", "?s=dcloud&e=0"),
    ("tik", "?s=tik&e=0"),
    ("v6:Hindi", "?s=v6%3AHindi&e=0"),
    ("v6:English", "?s=v6%3AEnglish&e=0"),
];
const LIMIT: u32 = 128 * 1024;

#[derive(Deserialize)]
struct Stream {
    url: String,
    language: String,
    headers: serde_json::Value,
    #[serde(default)]
    subtitles: Vec<Subtitle>,
}

#[derive(Deserialize)]
struct Subtitle {
    url: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    language: String,
}

#[inline]
fn detect_format(url: &str) -> Option<StreamFormat> {
    let path = url.split('?').next()?;
    let bytes = path.as_bytes();
    if bytes.len() >= 5 && bytes[bytes.len() - 5..].eq_ignore_ascii_case(b".m3u8") {
        Some(StreamFormat::Hls)
    } else if bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".mpd") {
        Some(StreamFormat::Dash)
    } else if bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".mp4") {
        Some(StreamFormat::Mp4)
    } else {
        None
    }
}

fn candidate_id(server: &str, source_url: &str) -> String {
    let hash = stream_provider_codecs::sha256(&[source_url.as_bytes()]);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut id = String::with_capacity(7 + server.len() + 1 + 24);
    id.push_str("vidzee-");
    id.push_str(server);
    id.push('-');
    for &b in &hash[..12] {
        id.push(HEX[(b >> 4) as usize] as char);
        id.push(HEX[(b & 0x0f) as usize] as char);
    }
    id
}

#[cfg(test)]
fn stream_context(url: &str, raw: &serde_json::Value) -> Result<FetchContext, AdapterError> {
    let destination = public_url(url)?;
    stream_context_inner(&destination, raw)
}

fn stream_context_inner(
    destination: &url::Url,
    raw: &serde_json::Value,
) -> Result<FetchContext, AdapterError> {
    let object = raw
        .as_object()
        .ok_or(AdapterError::Protocol("VidZee stream headers"))?;
    if object.len() > 32 {
        return Err(AdapterError::Protocol("VidZee stream header count"));
    }
    let destination_origin = destination.origin().ascii_serialization();
    let mut headers = BTreeMap::new();
    for (name, value) in object {
        let value = value
            .as_str()
            .ok_or(AdapterError::Protocol("VidZee stream header value"))?;
        if value.len() > 2048 || value.bytes().any(|b| b.is_ascii_control()) {
            return Err(AdapterError::Protocol("VidZee stream header value"));
        }
        let canonical = if name.eq_ignore_ascii_case("referer") {
            if value != PLAYER_REFERER && value != PLAYER && value != destination_origin {
                let referer = public_url(value)?;
                let origin = referer.origin().ascii_serialization();
                if origin != destination_origin && origin != PLAYER {
                    return Err(AdapterError::Protocol("VidZee stream referer origin"));
                }
            }
            "Referer"
        } else if name.eq_ignore_ascii_case("origin") {
            if value != PLAYER && value != destination_origin {
                let origin = url::Url::parse(value)
                    .map_err(|_| AdapterError::Protocol("VidZee stream origin"))?;
                if origin.path() != "/"
                    || origin.query().is_some()
                    || origin.fragment().is_some()
                    || !origin.username().is_empty()
                    || origin.password().is_some()
                {
                    return Err(AdapterError::Protocol("VidZee stream origin"));
                }
                let origin_str = origin.origin().ascii_serialization();
                if origin_str != destination_origin && origin_str != PLAYER {
                    return Err(AdapterError::Protocol("VidZee stream origin"));
                }
            }
            "Origin"
        } else if name.eq_ignore_ascii_case("user-agent") {
            if value.is_empty() || value.len() > 512 {
                return Err(AdapterError::Protocol("VidZee stream user agent"));
            }
            "User-Agent"
        } else if name.eq_ignore_ascii_case("accept") {
            "Accept"
        } else {
            return Err(AdapterError::Protocol("VidZee stream header name"));
        };
        if headers
            .insert(canonical.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(AdapterError::Protocol("VidZee duplicate stream header"));
        }
    }
    let mut scope_origin = String::with_capacity(destination_origin.len() + 1);
    scope_origin.push_str(&destination_origin);
    scope_origin.push('/');
    Ok(FetchContext {
        scopes: vec![FetchScope {
            headers,
            origin: scope_origin,
            path_prefix: "/".into(),
            ..Default::default()
        }],
        public_resources: Some(PublicResourceContext {
            origin: PLAYER.into(),
            referer: PLAYER_REFERER.into(),
        }),
    })
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    lookup.title.validate().map_err(|_| AdapterError::Input)?;
    let base_url = match lookup.title.media_type {
        MediaType::Tv => format!(
            "{API}/streams/tv/{}/{}/{}",
            lookup.title.tmdb_id.get(),
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        ),
        _ => format!("{API}/streams/movie/{}", lookup.title.tmdb_id.get()),
    };

    let mut base_headers = vec![
        header("User-Agent", common::USER_AGENT),
        header("Accept", "*/*"),
        header("Referer", PLAYER_REFERER),
        header("Origin", PLAYER),
    ];

    let mut requests = Vec::with_capacity(SERVERS.len());
    for (i, &(_, query)) in SERVERS.iter().enumerate() {
        let mut url = String::with_capacity(base_url.len() + query.len());
        url.push_str(&base_url);
        url.push_str(query);
        let headers = if i + 1 == SERVERS.len() {
            std::mem::take(&mut base_headers)
        } else {
            base_headers.clone()
        };
        requests.push(Request {
            url,
            method: "GET".into(),
            headers,
            body: Vec::new(),
            max_response_bytes: LIMIT,
            timeout_ms: 3_000,
        });
    }

    let responses = batch(host, requests)?;
    let provider = common::provider_id("vidzee")?;
    let mut candidates = Vec::with_capacity(SERVERS.len());
    let mut issues = Vec::new();

    for ((server, _), response) in SERVERS.into_iter().zip(responses) {
        if candidates.len() >= 32 {
            break;
        }
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                issues.push(common::issue(&e, "server"));
                continue;
            }
        };
        let stream: Stream = match common::json(&response.body, LIMIT as usize, "VidZee stream") {
            Ok(v) => v,
            Err(e) => {
                issues.push(common::issue(&e, "server"));
                continue;
            }
        };
        if stream.url.is_empty()
            || stream.url.len() > 8192
            || candidates
                .iter()
                .any(|c: &CandidateStream| c.url == stream.url)
        {
            continue;
        }
        let destination = match public_url(&stream.url) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let fetch_context = match stream_context_inner(&destination, &stream.headers) {
            Ok(c) => c,
            Err(e) => {
                issues.push(common::issue(&e, "headers"));
                continue;
            }
        };
        let format = match detect_format(&stream.url) {
            Some(f) => f,
            None => {
                issues.push(common::issue(
                    &AdapterError::Protocol("VidZee stream format"),
                    "server",
                ));
                continue;
            }
        };
        let mut metadata = Metadata::default();
        if !stream.language.is_empty()
            && stream.language.len() <= 128
            && !stream.language.bytes().any(|b| b.is_ascii_control())
        {
            metadata.provider_data.insert(
                "audio_language_claim".into(),
                serde_json::Value::String(stream.language),
            );
        }
        metadata.tracks = stream
            .subtitles
            .into_iter()
            .take(64)
            .filter_map(|subtitle| {
                if !safe_track_text(&subtitle.language, 64)
                    || !safe_track_text(&subtitle.label, 256)
                {
                    return None;
                }
                public_url(&subtitle.url).ok()?;
                Some(stream_core::providers::Track {
                    kind: "subtitles".into(),
                    language: subtitle.language,
                    name: if subtitle.label.is_empty() {
                        "Unknown".into()
                    } else {
                        subtitle.label
                    },
                    url: subtitle.url,
                    ..Default::default()
                })
            })
            .collect();

        let source_url = stream.url;
        let id = candidate_id(server, &source_url);
        candidates.push(CandidateStream {
            id,
            server: server.into(),
            url: source_url,
            format,
            provider: provider.clone(),
            title: lookup.title.clone(),
            fetch_context,
            resolver_metadata: metadata,
            content_verified: false,
            expires_at: None,
            selected_variant_url: String::new(),
        });
        if candidates.len() >= 32 {
            break;
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("VidZee servers"));
    }
    Ok(common::result(candidates, issues))
}

fn safe_track_text(value: &str, max_bytes: usize) -> bool {
    value.len() <= max_bytes && !value.bytes().any(|byte| byte.is_ascii_control())
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct FixtureHost {
        expected_path: &'static str,
    }

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
                if !request.url.contains(self.expected_path) {
                    return Err(HostError::InvalidRequest);
                }
                let server = url::Url::parse(&request.url)
                    .map_err(|_| HostError::InvalidRequest)?
                    .query_pairs()
                    .find(|(key, _)| key == "s")
                    .map(|(_, value)| value.into_owned())
                    .ok_or(HostError::InvalidRequest)?;
                let body = serde_json::to_vec(&serde_json::json!({
                    "url": format!("https://media.example/{server}/master.m3u8"),
                    "language": "English",
                    "headers": {
                        "Origin": PLAYER,
                        "Referer": format!("{PLAYER}/"),
                        "User-Agent": "VidZee fixture",
                    },
                    "subtitles": [{
                        "url": "https://media.example/en.vtt",
                        "label": "English",
                        "language": "en"
                    }]
                }))
                .map_err(|_| HostError::InvalidRequest)?;
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

        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }

    fn lookup(value: serde_json::Value) -> Result<Lookup, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn resolves_movie_and_tv_coordinates_with_bounded_valid_candidates()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = common::provider_id("vidzee")?;
        for (lookup, expected_path) in [
            (
                lookup(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?,
                "/streams/movie/550",
            ),
            (
                lookup(
                    serde_json::json!({"title":{"type":"tv","tmdb_id":1396,"season":1,"episode":1}}),
                )?,
                "/streams/tv/1396/1/1",
            ),
        ] {
            let result = resolve(&lookup, &FixtureHost { expected_path })?;
            result.validate_for(&provider, &lookup.title, 32)?;
            let candidates = result.candidates.ok_or("candidate list missing")?;
            assert_eq!(candidates.len(), SERVERS.len());
            assert!(candidates.len() <= 32);
            for candidate in &candidates {
                candidate.validate_for(&provider, &lookup.title)?;
                assert_eq!(candidate.format, StreamFormat::Hls);
                assert!(!candidate.content_verified);
                assert_eq!(candidate.fetch_context.scopes.len(), 1);
                assert_eq!(
                    candidate.fetch_context.scopes[0].origin,
                    "https://media.example/"
                );
                assert_eq!(candidate.fetch_context.scopes[0].headers["Origin"], PLAYER);
                assert_eq!(
                    candidate.fetch_context.scopes[0].headers["User-Agent"],
                    "VidZee fixture"
                );
                let public = candidate
                    .fetch_context
                    .public_resources
                    .as_ref()
                    .ok_or("public context missing")?;
                assert_eq!(public.origin, PLAYER);
                assert_eq!(public.referer, format!("{PLAYER}/"));
                assert!(
                    !candidate
                        .resolver_metadata
                        .provider_data
                        .contains_key("quality_claim")
                );
                assert_eq!(candidate.resolver_metadata.tracks.len(), 1);
                assert_eq!(candidate.resolver_metadata.tracks[0].language, "en");
                assert_eq!(candidate.resolver_metadata.tracks[0].name, "English");
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_private_stream_urls_and_unsafe_source_headers() {
        assert!(stream_context("http://127.0.0.1/master.m3u8", &serde_json::json!({})).is_err());
        assert!(
            stream_context(
                "https://media.example/master.m3u8",
                &serde_json::json!({"Cookie":"session=secret"})
            )
            .is_err()
        );
        assert!(
            stream_context(
                "https://media.example/master.m3u8",
                &serde_json::json!({"Origin":"https://attacker.example"})
            )
            .is_err()
        );
        assert!(
            stream_context(
                "https://media.example/master.m3u8",
                &serde_json::json!({"Referer":"http://127.0.0.1/"})
            )
            .is_err()
        );
    }

    #[test]
    fn source_header_map_is_bounded() {
        let headers: serde_json::Map<String, serde_json::Value> = (0..33)
            .map(|index| (format!("X-{index}"), serde_json::json!("value")))
            .collect();
        assert!(
            stream_context(
                "https://media.example/master.m3u8",
                &serde_json::Value::Object(headers)
            )
            .is_err()
        );
    }

    #[test]
    fn subtitle_metadata_uses_utf8_byte_limits() {
        assert!(safe_track_text(&"é".repeat(32), 64));
        assert!(!safe_track_text(&"é".repeat(33), 64));
        assert!(!safe_track_text("English\ntrack", 64));
    }
}
