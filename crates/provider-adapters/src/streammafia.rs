//! StreamMafia session/token flow and authenticated payload resolution.
use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, fetch, get, header, post},
    destination::public_url,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat,
    },
};
use stream_provider_codecs::streammafia::decrypt;
use stream_provider_sdk::http::Request;

const ORIGIN: &str = "https://player.nhdapi.com";
const LIMIT: u32 = 256 * 1024;
#[derive(Deserialize, Default)]
struct Cipher {
    #[serde(default)]
    iv: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    data: String,
}
#[derive(Deserialize, Default)]
struct Download {
    #[serde(default)]
    quality: String,
    #[serde(default)]
    url: String,
}
#[derive(Deserialize, Default)]
struct Stream {
    #[serde(default)]
    hls_streaming: String,
    #[serde(default)]
    download: Vec<Download>,
}
#[derive(Deserialize, Default)]
struct Switch {
    #[serde(default)]
    file_code: String,
}
#[derive(Deserialize, Default)]
struct Requested {
    #[serde(default)]
    id: i64,
}
#[derive(Deserialize, Default)]
struct Api {
    #[serde(default)]
    requested: Requested,
    #[serde(default)]
    stream: Stream,
    #[serde(default)]
    switches: Vec<Switch>,
}
#[derive(Deserialize)]
struct Token {
    token: String,
}
fn request_with_auth(mut request: Request, cookie: &str, token: &str, media_id: i64) -> Request {
    request.headers.push(header("Cookie", cookie.to_owned()));
    request
        .headers
        .push(header("x-api-token", token.to_owned()));
    request
        .headers
        .push(header("x-content-id", media_id.to_string()));
    request.headers.push(header("Origin", ORIGIN));
    request
}
fn decode(response: &[u8]) -> Result<Api, AdapterError> {
    let envelope: Cipher = common::json(response, LIMIT as usize, "StreamMafia envelope")?;
    let plain = decrypt(&envelope.iv, &envelope.tag, &envelope.data)
        .map_err(|_| AdapterError::Protocol("StreamMafia cipher"))?;
    common::json(&plain, LIMIT as usize, "StreamMafia payload")
}
pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let media_id = lookup.title.tmdb_id.get();
    let mut session = post(
        format!("{ORIGIN}/api/session"),
        &format!("{ORIGIN}/"),
        32 * 1024,
    );
    session.body.clear();
    let session_response = fetch(host, session)?;
    let cookie = session_response
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("set-cookie"))
        .map(|h| h.value.split(';').next().unwrap_or("").to_owned())
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .ok_or(AdapterError::Unavailable("StreamMafia session"))?;
    let mut token_request = get(
        format!("{ORIGIN}/api/token"),
        &format!("{ORIGIN}/"),
        32 * 1024,
    );
    token_request.headers.push(header("Cookie", cookie.clone()));
    token_request
        .headers
        .push(header("x-content-id", media_id.to_string()));
    token_request.headers.push(header("Origin", ORIGIN));
    let token_response = fetch(host, token_request)?;
    let token: Token = common::json(&token_response.body, 32 * 1024, "StreamMafia token")?;
    if token.token.is_empty()
        || token.token.len() > 4096
        || token.token.bytes().any(|b| b.is_ascii_control())
    {
        return Err(AdapterError::Protocol("StreamMafia token"));
    }
    let path = if lookup.title.media_type == MediaType::Tv {
        format!(
            "/api/?tv={media_id}&season={}&episode={}",
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        )
    } else {
        format!("/api/movie/?id={media_id}")
    };
    let page_request = request_with_auth(
        get(format!("{ORIGIN}{path}"), &format!("{ORIGIN}/"), LIMIT),
        &cookie,
        &token.token,
        media_id,
    );
    let page_response = fetch(host, page_request)?;
    let api = decode(&page_response.body)?;
    if api.requested.id != media_id {
        return Err(AdapterError::Protocol("StreamMafia content identity"));
    }
    if api.switches.len() > 32 || api.stream.download.len() > 32 {
        return Err(AdapterError::Protocol("StreamMafia source count"));
    }
    let switch_requests = api
        .switches
        .iter()
        .filter(|s| {
            !s.file_code.is_empty()
                && s.file_code.len() <= 256
                && s.file_code
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .map(|s| {
            request_with_auth(
                get(
                    format!("{ORIGIN}/api/source/{}", s.file_code),
                    &format!("{ORIGIN}/"),
                    LIMIT,
                ),
                &cookie,
                &token.token,
                media_id,
            )
        })
        .collect();
    let switch_results = if api.switches.is_empty() {
        Vec::new()
    } else {
        batch(host, switch_requests)?
    };
    let provider = common::provider_id("streammafia")?;
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
    let mut all_apis = vec![("main".to_owned(), api)];
    for (index, result) in switch_results.into_iter().enumerate() {
        match result.and_then(|r| decode(&r.body)) {
            Ok(a) if a.requested.id == media_id => all_apis.push((format!("switch-{index}"), a)),
            Ok(_) => issues.push(common::issue(
                &AdapterError::Protocol("StreamMafia switch identity"),
                "switch",
            )),
            Err(e) => issues.push(common::issue(&e, "switch")),
        }
    }
    for (server, api) in all_apis {
        let mut streams = Vec::with_capacity(api.stream.download.len() + 1);
        if !api.stream.hls_streaming.is_empty() {
            streams.push((api.stream.hls_streaming, "hls".to_owned()));
        }
        for download in api.stream.download {
            streams.push((download.url, download.quality));
        }
        for (index, (url, claim)) in streams.into_iter().enumerate() {
            if url.len() > 8192 || public_url(&url).is_err() || !seen.insert(url.clone()) {
                continue;
            }
            let lower = url.to_ascii_lowercase();
            let format = if claim.eq_ignore_ascii_case("mp4")
                || lower.split('?').next().is_some_and(|p| p.ends_with(".mp4"))
            {
                StreamFormat::Mp4
            } else {
                StreamFormat::Hls
            };
            let mut metadata = Metadata::default();
            if claim.len() <= 64 {
                metadata
                    .provider_data
                    .insert("quality_claim".into(), serde_json::json!(claim));
            }
            candidates.push(CandidateStream {
                id: format!(
                    "streammafia-{server}-{index}-{}",
                    common::hash_id(&[url.as_bytes()])
                ),
                server: server.clone(),
                url,
                format,
                provider: provider.clone(),
                title: lookup.title.clone(),
                fetch_context: context.clone(),
                resolver_metadata: metadata,
                content_verified: false,
                expires_at: None,
                selected_variant_url: String::new(),
            });
            if candidates.len() >= 32 {
                break;
            }
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("StreamMafia sources"));
    }
    Ok(common::result(candidates, issues))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};
    struct Host {
        movie_id: i64,
        page_path: &'static str,
    }
    impl ProviderHost for Host {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let (status, headers, body) = if request.url.ends_with("/api/session") {
                (
                    200,
                    vec![common::header(
                        "Set-Cookie",
                        "vid_session=fixture; Path=/; Secure",
                    )],
                    b"{}".to_vec(),
                )
            } else if request.url.ends_with("/api/token") {
                (200, vec![], br#"{"token":"fixture-token"}"#.to_vec())
            } else if request.url.ends_with(self.page_path) {
                let envelope = if self.movie_id == 550 {
                    r#"{"iv":"CQkJCQkJCQkJCQkJ","tag":"8Q3nmKQRdGmJyR38Mqg2Ow==","data":"vFl097ybg030KiNJYbVx3GE/YAGCt+nOeOEXKXPyRYrDDvatJp5HmkXG0d/sqEWoG9heLfaMlqe6Y1H3mY4ieEVrgD3Ud/2usK6df9bXQu2dASRwLok/2AH/NWs8s0QUM2HvCbtUJs+n7oFrDOP+XQdTDJGsUWE="}"#
                } else {
                    r#"{"iv":"CgoKCgoKCgoKCgoK","tag":"SyD5KymBTh+o2tXaSWh7vQ==","data":"c3qeiJKSjnTg6AiTTSZbPL4GQTdVNC60+9XGasd1m35bNQ3SnRLAamydkGX4qMfG2vSAGf4gwD0TWANHezahGOBVoRYgsw6TCQ9RgmvjLLE6+iBNpW0+BLYnuUqXaaPg4Gr/JKFkKYpQZUgPl+n5qcEoXo2fSuO4"}"#
                };
                (200, vec![], envelope.as_bytes().to_vec())
            } else {
                return Err(HostError::Denied);
            };
            if request.url.ends_with(self.page_path) {
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|h| h.name == "x-content-id" && h.value == self.movie_id.to_string())
                );
            }
            Ok(Response {
                url: request.url,
                status,
                headers,
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
            Ok(1)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn resolve_movie_and_episode_decrypts_content_matched_candidate()
    -> Result<(), Box<dyn std::error::Error>> {
        for (title, id, page) in [
            (
                serde_json::json!({"type":"movie","tmdb_id":550}),
                550,
                "/api/movie/?id=550",
            ),
            (
                serde_json::json!({"type":"tv","tmdb_id":1399,"season":2,"episode":5}),
                1399,
                "/api/?tv=1399&season=2&episode=5",
            ),
        ] {
            let lookup: Lookup = serde_json::from_value(serde_json::json!({"title":title}))?;
            let result = resolve(
                &lookup,
                &Host {
                    movie_id: id,
                    page_path: page,
                },
            )?;
            result.validate_for(&common::provider_id("streammafia")?, &lookup.title, 32)?;
            let candidates = result.candidates.ok_or("missing candidates")?;
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].url, "https://media.example.com/master.m3u8");
            assert_eq!(candidates[0].title, lookup.title);
            assert!(!candidates[0].content_verified);
        }
        Ok(())
    }
}
