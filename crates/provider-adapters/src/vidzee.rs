//! VidZee's current plain-stream endpoint (`/streams/...?...&e=0`).
use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, get, header},
    destination::public_url,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
        PublicResourceContext, StreamFormat,
    },
};

const API: &str = "https://core.vidzee.wtf";
const PLAYER: &str = "https://player.vidzee.wtf";
const SERVERS: [&str; 4] = ["dcloud", "tik", "v6:Hindi", "v6:English"];
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
fn stream_context(url: &str, raw: &serde_json::Value) -> Result<FetchContext, AdapterError> {
    let object = raw
        .as_object()
        .ok_or(AdapterError::Protocol("VidZee stream headers"))?;
    if object.len() > 32 {
        return Err(AdapterError::Protocol("VidZee stream header count"));
    }
    let destination = public_url(url)?;
    let destination_origin = destination.origin().ascii_serialization();
    let player_origin = url::Url::parse(PLAYER)
        .map_err(|_| AdapterError::Protocol("VidZee player origin"))?
        .origin()
        .ascii_serialization();
    let mut headers = BTreeMap::new();
    for (name, value) in object {
        let value = value
            .as_str()
            .ok_or(AdapterError::Protocol("VidZee stream header value"))?;
        if value.len() > 2048 || value.bytes().any(|b| b.is_ascii_control()) {
            return Err(AdapterError::Protocol("VidZee stream header value"));
        }
        let canonical = if name.eq_ignore_ascii_case("referer") {
            let referer = public_url(value)?;
            let origin = referer.origin().ascii_serialization();
            if origin != destination_origin && origin != player_origin {
                return Err(AdapterError::Protocol("VidZee stream referer origin"));
            }
            "Referer"
        } else if name.eq_ignore_ascii_case("origin") {
            let origin = url::Url::parse(value)
                .map_err(|_| AdapterError::Protocol("VidZee stream origin"))?;
            if origin.path() != "/"
                || origin.query().is_some()
                || origin.fragment().is_some()
                || origin.username() != ""
                || origin.password().is_some()
                || (origin.origin().ascii_serialization() != destination_origin
                    && origin.origin().ascii_serialization() != player_origin)
            {
                return Err(AdapterError::Protocol("VidZee stream origin"));
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
    let mut context = FetchContext {
        scopes: Vec::new(),
        public_resources: Some(PublicResourceContext {
            origin: PLAYER.into(),
            referer: format!("{PLAYER}/"),
        }),
    };
    context.scopes.push(FetchScope {
        headers,
        origin: format!("{destination_origin}/"),
        path_prefix: "/".into(),
        ..Default::default()
    });
    Ok(context)
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    lookup.title.validate().map_err(|_| AdapterError::Input)?;
    let path = if lookup.title.media_type == MediaType::Tv {
        format!(
            "tv/{}/{}/{}",
            lookup.title.tmdb_id.get(),
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        )
    } else {
        format!("movie/{}", lookup.title.tmdb_id.get())
    };
    let mut requests = Vec::with_capacity(SERVERS.len());
    for server in SERVERS {
        let mut endpoint = url::Url::parse(&format!("{API}/streams/{path}"))
            .map_err(|_| AdapterError::Protocol("VidZee endpoint"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("s", server)
            .append_pair("e", "0");
        let mut request = get(endpoint.as_str(), &format!("{PLAYER}/"), LIMIT);
        request.timeout_ms = 3_000;
        request.headers.push(header("Origin", PLAYER));
        requests.push(request);
    }
    let responses = batch(host, requests)?;
    let provider = common::provider_id("vidzee")?;
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    for (server, response) in SERVERS.into_iter().zip(responses) {
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
        if stream.url.len() > 8192 || public_url(&stream.url).is_err() || seen.contains(&stream.url)
        {
            continue;
        }
        let fetch_context = match stream_context(&stream.url, &stream.headers) {
            Ok(c) => c,
            Err(e) => {
                issues.push(common::issue(&e, "headers"));
                continue;
            }
        };
        let lower = stream.url.to_ascii_lowercase();
        let format = if lower
            .split('?')
            .next()
            .is_some_and(|p| p.ends_with(".m3u8"))
        {
            StreamFormat::Hls
        } else if lower.split('?').next().is_some_and(|p| p.ends_with(".mpd")) {
            StreamFormat::Dash
        } else if lower.split('?').next().is_some_and(|p| p.ends_with(".mp4")) {
            StreamFormat::Mp4
        } else {
            issues.push(common::issue(
                &AdapterError::Protocol("VidZee stream format"),
                "server",
            ));
            continue;
        };
        let mut metadata = Metadata::default();
        if !stream.language.is_empty()
            && stream.language.len() <= 128
            && !stream.language.bytes().any(|b| b.is_ascii_control())
        {
            metadata.provider_data.insert(
                "audio_language_claim".into(),
                serde_json::json!(stream.language),
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
                let url = public_url(&subtitle.url).ok()?;
                Some(stream_core::providers::Track {
                    kind: "subtitles".into(),
                    language: subtitle.language,
                    name: if subtitle.label.is_empty() {
                        "Unknown".into()
                    } else {
                        subtitle.label
                    },
                    url: url.to_string(),
                    ..Default::default()
                })
            })
            .collect();
        let source_url = stream.url;
        seen.insert(source_url.clone());
        candidates.push(CandidateStream {
            id: format!(
                "vidzee-{server}-{}",
                common::hash_id(&[source_url.as_bytes()])
            ),
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
