//! 02MovieDownloader's session-token and optional AES-CBC source API.
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
        PublicResourceContext, StreamFormat, Track,
    },
};

use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get, header, post},
    destination::public_url,
};
use stream_provider_codecs::moviedownloader02::decrypt_payload;

const ORIGIN: &str = "https://02moviedownloader.site";
const LIMIT: u32 = 512 * 1024;

#[derive(Deserialize, Default)]
struct TokenResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    token: String,
}
#[derive(Deserialize, Default)]
struct Envelope {
    #[serde(default)]
    data: String,
}
#[derive(Deserialize, Default)]
struct ApiResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<DataBlock>,
    #[serde(default, rename = "externalStreams")]
    external_streams: Vec<ExternalStream>,
}
#[derive(Deserialize, Default)]
struct DataBlock {
    #[serde(default, rename = "downloadData")]
    download_data: Option<DownloadBlock>,
}
#[derive(Deserialize, Default)]
struct DownloadBlock {
    #[serde(default)]
    data: Option<DownloadData>,
}
#[derive(Deserialize, Default)]
struct DownloadData {
    #[serde(default)]
    downloads: Vec<Download>,
    #[serde(default)]
    captions: Vec<Caption>,
}
#[derive(Deserialize, Default)]
struct Download {
    #[serde(default)]
    url: String,
    #[serde(default)]
    resolution: serde_json::Value,
}
#[derive(Deserialize, Default)]
struct Caption {
    #[serde(default)]
    url: String,
    #[serde(default)]
    lan: String,
    #[serde(default, rename = "lanName")]
    lan_name: String,
}
#[derive(Deserialize, Default)]
struct ExternalStream {
    #[serde(default)]
    url: String,
    #[serde(default)]
    quality: String,
    #[serde(default, rename = "type")]
    format: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let media_path = match lookup.title.media_type {
        MediaType::Movie => format!("movie/{}", lookup.title.tmdb_id.get()),
        MediaType::Tv => format!(
            "tv/{}/{}/{}",
            lookup.title.tmdb_id.get(),
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        ),
    };
    let api_url = format!("{ORIGIN}/api/download/{media_path}");
    let mut token_request = post(format!("{ORIGIN}/api/verify-robot"), &api_url, 32 * 1024);
    token_request.headers.push(header("Origin", ORIGIN));
    token_request
        .headers
        .push(header("Accept-Language", "en-US,en;q=0.7"));
    token_request.headers.push(header("DNT", "1"));
    let token_response = fetch(host, token_request)?;
    let token: TokenResponse =
        common::json(&token_response.body, 32 * 1024, "02MovieDownloader token")?;
    if !token.success || !valid_token(&token.token) {
        return Err(AdapterError::Unavailable("02MovieDownloader session token"));
    }

    let mut source_request = get(&api_url, &api_url, LIMIT);
    source_request
        .headers
        .push(header("Accept", "application/json"));
    source_request.headers.push(header("Origin", ORIGIN));
    source_request
        .headers
        .push(header("x-session-token", token.token.clone()));
    let source_response = fetch(host, source_request)?;
    let value: serde_json::Value = common::json(
        &source_response.body,
        LIMIT as usize,
        "02MovieDownloader sources",
    )?;
    let response: ApiResponse =
        if value.get("encrypted").and_then(serde_json::Value::as_bool) == Some(true) {
            let envelope: Envelope = serde_json::from_value(value)
                .map_err(|_| AdapterError::Protocol("02MovieDownloader envelope"))?;
            let plaintext = decrypt_payload(&envelope.data, &token.token)
                .map_err(|_| AdapterError::Protocol("02MovieDownloader encryption"))?;
            common::json(&plaintext, LIMIT as usize, "02MovieDownloader plaintext")?
        } else {
            serde_json::from_value(value)
                .map_err(|_| AdapterError::Protocol("02MovieDownloader response"))?
        };
    if !response.success {
        return Err(AdapterError::Unavailable("02MovieDownloader sources"));
    }
    let provider = common::provider_id("moviedownloader02")?;
    let mut output = CandidateOutput::default();
    let mut tracks = Vec::new();
    if let Some(download_data) = response
        .data
        .and_then(|data| data.download_data)
        .and_then(|data| data.data)
    {
        if download_data.downloads.len() > 32 || download_data.captions.len() > 64 {
            return Err(AdapterError::Protocol("02MovieDownloader source count"));
        }
        for caption in download_data.captions {
            if caption.url.len() > 8192 || public_url(&caption.url).is_err() {
                continue;
            }
            let label = if caption.lan_name.is_empty() {
                caption.lan
            } else {
                caption.lan_name
            };
            tracks.push(Track {
                kind: "subtitles".into(),
                name: truncate_utf8(&label, 256),
                language: truncate_utf8(&label, 64),
                url: caption.url,
                ..Default::default()
            });
        }
        for download in download_data.downloads {
            let resolution = match download.resolution {
                serde_json::Value::String(value) => value,
                serde_json::Value::Number(value) => value.to_string(),
                _ => String::new(),
            };
            push_candidate(
                &mut output,
                CandidateInput {
                    lookup,
                    provider: &provider,
                    url: download.url,
                    server: "download",
                    format: StreamFormat::Mp4,
                    quality: resolution,
                    headers: &BTreeMap::new(),
                    tracks: &tracks,
                },
            );
        }
    }
    if response.external_streams.len() > 32 {
        return Err(AdapterError::Protocol("02MovieDownloader external count"));
    }
    for source in response.external_streams {
        let format = match source.format.to_ascii_lowercase().as_str() {
            "hls" | "m3u8" => StreamFormat::Hls,
            "mp4" | "mkv" => StreamFormat::Mp4,
            _ => StreamFormat::Unknown,
        };
        if source.url.contains("111477.xyz") {
            continue;
        }
        push_candidate(
            &mut output,
            CandidateInput {
                lookup,
                provider: &provider,
                url: source.url,
                server: "external",
                format,
                quality: source.quality,
                headers: &source.headers,
                tracks: &tracks,
            },
        );
    }
    if output.candidates.is_empty() {
        return Err(AdapterError::Unavailable("02MovieDownloader streams"));
    }
    Ok(common::result(output.candidates, output.issues))
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 4096
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
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

#[derive(Default)]
struct CandidateOutput {
    candidates: Vec<CandidateStream>,
    issues: Vec<stream_core::providers::ProviderFailure>,
    seen: std::collections::BTreeSet<String>,
}

struct CandidateInput<'a> {
    lookup: &'a Lookup,
    provider: &'a stream_core::providers::ProviderId,
    url: String,
    server: &'a str,
    format: StreamFormat,
    quality: String,
    headers: &'a BTreeMap<String, String>,
    tracks: &'a [Track],
}

fn push_candidate(output: &mut CandidateOutput, input: CandidateInput<'_>) {
    if output.candidates.len() >= 32
        || input.url.is_empty()
        || !output.seen.insert(input.url.clone())
    {
        return;
    }
    let parsed = match public_url(&input.url) {
        Ok(parsed) => parsed,
        Err(_) => {
            output.issues.push(common::issue(
                &AdapterError::Protocol("02MovieDownloader source destination"),
                "sources",
            ));
            return;
        }
    };
    let (scopes, public_resources) = match header_context(input.headers, &parsed) {
        Ok(context) => context,
        Err(error) => {
            output.issues.push(common::issue(&error, "sources"));
            return;
        }
    };
    let mut provider_data = BTreeMap::new();
    provider_data.insert("quality_claim".into(), json!(input.quality));
    provider_data.insert("audio_language_claim".into(), json!("English"));
    output.candidates.push(CandidateStream {
        id: format!(
            "moviedownloader02-{}",
            common::hash_id(&[input.server.as_bytes(), input.url.as_bytes()])
        ),
        server: input.server.into(),
        selected_variant_url: String::new(),
        url: input.url,
        provider: input.provider.clone(),
        format: input.format,
        title: input.lookup.title.clone(),
        fetch_context: FetchContext {
            scopes,
            public_resources,
        },
        resolver_metadata: Metadata {
            provider_data,
            tracks: input.tracks.to_vec(),
            tracks_known: !input.tracks.is_empty(),
            ..Default::default()
        },
        content_verified: false,
        expires_at: None,
    });
}

fn header_context(
    headers: &BTreeMap<String, String>,
    stream_url: &url::Url,
) -> Result<(Vec<FetchScope>, Option<PublicResourceContext>), AdapterError> {
    if headers.len() > 16 {
        return Err(AdapterError::Protocol("02MovieDownloader header count"));
    }
    let mut filtered = BTreeMap::new();
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        if !matches!(
            name.as_str(),
            "origin" | "referer" | "user-agent" | "accept" | "accept-language"
        ) || value.len() > 2048
            || value.bytes().any(|b| b.is_ascii_control())
        {
            return Err(AdapterError::Protocol("02MovieDownloader headers"));
        }
        if matches!(name.as_str(), "origin" | "referer") {
            public_url(value)?;
        }
        filtered.insert(name, value.clone());
    }
    let scopes = if filtered.is_empty() {
        Vec::new()
    } else {
        vec![FetchScope {
            origin: stream_url.origin().ascii_serialization(),
            path_prefix: "/".into(),
            headers: filtered,
            ..Default::default()
        }]
    };
    Ok((
        scopes,
        Some(PublicResourceContext {
            origin: ORIGIN.into(),
            referer: format!("{ORIGIN}/"),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};
    struct Host {
        path: &'static str,
    }
    impl ProviderHost for Host {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            if request.url.ends_with("verify-robot") {
                assert_eq!(request.method, "POST");
                assert!(request.body.is_empty());
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|h| h.name == "Origin" && h.value == ORIGIN)
                );
                Ok(Response {
                    url: request.url,
                    status: 200,
                    headers: Vec::new(),
                    body: br#"{"success":true,"token":"fixture-token"}"#.to_vec(),
                })
            } else {
                assert_eq!(request.url, format!("{ORIGIN}/api/download/{}", self.path));
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|h| h.name == "x-session-token" && h.value == "fixture-token")
                );
                let body = serde_json::json!({"success":true,"externalStreams":[{"url":"https://cdn.example.org/video.mp4","quality":"1080p","type":"mp4"}],"data":{"downloadData":{"data":{"downloads":[],"captions":[{"url":"https://cdn.example.org/en.vtt","lan":"en","lanName":"é".repeat(200)}]}}}}).to_string().into_bytes();
                Ok(Response {
                    url: request.url,
                    status: 200,
                    headers: Vec::new(),
                    body,
                })
            }
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
    fn fetches_token_then_sources_and_preserves_title_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        for (title, path) in [
            (json!({"type":"movie","tmdb_id":550}), "movie/550"),
            (
                json!({"type":"tv","tmdb_id":1622,"season":1,"episode":2}),
                "tv/1622/1/2",
            ),
        ] {
            let lookup: Lookup = serde_json::from_value(json!({"title":title}))?;
            let result = resolve(&lookup, &Host { path })?;
            result.validate_for(
                &common::provider_id("moviedownloader02")?,
                &lookup.title,
                32,
            )?;
            let candidates = result.candidates.ok_or("missing candidates")?;
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].title, lookup.title);
            assert!(!candidates[0].content_verified);
            assert_eq!(
                candidates[0].resolver_metadata.tracks[0].name,
                "é".repeat(128)
            );
            assert!(candidates[0].resolver_metadata.tracks[0].language.len() <= 64);
        }
        Ok(())
    }
}
