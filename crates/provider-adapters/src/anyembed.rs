//! AnyEmbed session and stream discovery.
use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get, header},
    destination::public_url,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata,
        PublicResourceContext, StreamFormat, Track,
    },
};
use url::Url;

const API: &str = "https://api.anyembed.xyz";
const ORIGIN: &str = "https://anyembed.xyz";
const LIMIT: u32 = 512 * 1024;

#[derive(Deserialize)]
struct Attest {
    success: bool,
    #[serde(default)]
    nonce: String,
    #[serde(default)]
    turnstile_required: bool,
}
#[derive(Deserialize)]
struct Session {
    success: bool,
    token: String,
    stream_sign_key: String,
}
#[derive(Deserialize)]
struct Envelope {
    success: bool,
    #[serde(default)]
    sources: Vec<Group>,
}
#[derive(Deserialize)]
struct Group {
    #[serde(default)]
    streams: Vec<Stream>,
}
#[derive(Deserialize)]
struct Stream {
    url: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
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

fn acquire_session(host: &dyn ProviderHost, embed_url: &str) -> Result<Session, AdapterError> {
    for _ in 0..2 {
        let attest_url = format!("{API}/api/v1/embed-attest");
        let mut req = get(&attest_url, embed_url, 32 * 1024);
        req.headers.push(header("Origin", ORIGIN));
        let attest_resp = match fetch(host, req) {
            Ok(resp) => resp,
            Err(_) => continue,
        };
        let attest: Attest = match common::json(&attest_resp.body, 32 * 1024, "AnyEmbed attest") {
            Ok(attest) => attest,
            Err(_) => continue,
        };
        if !attest.success || !common::opaque(&attest.nonce, 256) {
            if attest.turnstile_required {
                return Err(AdapterError::Unavailable(
                    "AnyEmbed Turnstile challenge required",
                ));
            }
            continue;
        }

        let session_url = format!("{API}/api/v1/session");
        let mut req = get(&session_url, embed_url, 32 * 1024);
        req.headers.extend([
            header("Origin", ORIGIN),
            header("x-embed-attest", attest.nonce),
        ]);
        let session_resp = match fetch(host, req) {
            Ok(resp) => resp,
            Err(_) => continue,
        };
        let session: Session = match common::json(&session_resp.body, 32 * 1024, "AnyEmbed session")
        {
            Ok(session) => session,
            Err(_) => continue,
        };
        if session.success
            && common::opaque(&session.token, 4096)
            && common::opaque(&session.stream_sign_key, 4096)
        {
            return Ok(session);
        }
    }
    Err(AdapterError::Unavailable("AnyEmbed session unavailable"))
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let title = &lookup.title;
    let tmdb_id = title.tmdb_id.get();
    let embed_url = format!("{ORIGIN}/embed/{tmdb_id}");

    let session = acquire_session(host, &embed_url)?;

    let sign_path = format!("/api/v1/stream/{tmdb_id}");
    let stream_url = match title.media_type {
        MediaType::Movie => format!("{API}{sign_path}"),
        MediaType::Tv => {
            let season = title.season.unwrap_or(1);
            let episode = title.episode.unwrap_or(1);
            format!("{API}{sign_path}?is_tv=true&season={season}&episode={episode}")
        }
    };
    let timestamp = (host.now_unix_ms()? / 1000).to_string();
    let random = host.random_bytes(16)?;
    let nonce = uuid_v4(&random)?;
    let msg = format!("{timestamp}:{sign_path}:{nonce}");
    let sig =
        stream_provider_codecs::hmac_sha256(session.stream_sign_key.as_bytes(), msg.as_bytes())
            .map_err(|_| AdapterError::Protocol("AnyEmbed HMAC signature"))?;
    let sig_hex: String = sig.iter().map(|b| format!("{b:02x}")).collect();

    let mut req = get(&stream_url, &embed_url, LIMIT);
    req.timeout_ms = 8_000;
    req.headers.extend([
        header("Origin", ORIGIN),
        header("x-session-token", session.token),
        header("x-anyembed-timestamp", timestamp),
        header("x-anyembed-nonce", nonce),
        header("x-anyembed-signature", sig_hex),
    ]);
    let envelope: Envelope =
        common::json(&fetch(host, req)?.body, LIMIT as usize, "AnyEmbed streams")?;
    if !envelope.success {
        return Err(AdapterError::Unavailable("AnyEmbed API unsuccessful"));
    }
    let provider = common::provider_id("anyembed")?;
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    if envelope.sources.len() > 32
        || envelope
            .sources
            .iter()
            .any(|group| group.streams.len() > 32)
    {
        return Err(AdapterError::Protocol("AnyEmbed source count"));
    }
    for group in envelope.sources {
        if candidates.len() >= 32 {
            break;
        }
        for stream in group.streams {
            if candidates.len() >= 32 {
                break;
            }
            let parsed = match public_url(&stream.url) {
                Ok(url) => url,
                Err(error) => {
                    issues.push(common::issue(&error, "sources"));
                    continue;
                }
            };
            if !seen.insert(parsed.as_str().to_owned()) {
                continue;
            }
            let format = format_from_url(parsed.path());
            let tracks = stream
                .subtitles
                .into_iter()
                .take(64)
                .filter_map(|sub| {
                    let url = public_url(&sub.url).ok()?;
                    if !safe_track_text(&sub.language, 64) || !safe_track_text(&sub.label, 256) {
                        return None;
                    }
                    Some(Track {
                        kind: "subtitles".into(),
                        language: sub.language,
                        name: if sub.label.is_empty() {
                            "Unknown".into()
                        } else {
                            sub.label
                        },
                        url: url.to_string(),
                        ..Default::default()
                    })
                })
                .collect();
            let mut metadata = Metadata {
                tracks,
                ..Default::default()
            };
            if !stream.quality.is_empty() && stream.quality.len() <= 64 {
                metadata
                    .provider_data
                    .insert("quality_claim".into(), stream.quality.into());
            }
            let scopes = source_scope(&parsed, stream.headers)?;
            let public_resources = if scopes.iter().any(|s| s.headers.contains_key("referer")) {
                None
            } else {
                Some(PublicResourceContext {
                    origin: ORIGIN.into(),
                    referer: format!("{ORIGIN}/"),
                })
            };
            candidates.push(CandidateStream {
                expires_at: None,
                id: format!(
                    "anyembed-{}",
                    common::hash_id(&[parsed.as_str().as_bytes()])
                ),
                server: "AnyEmbed".into(),
                selected_variant_url: String::new(),
                url: parsed.to_string(),
                provider: provider.clone(),
                format,
                title: title.clone(),
                fetch_context: FetchContext {
                    scopes,
                    public_resources,
                },
                resolver_metadata: metadata,
                content_verified: false,
            });
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("AnyEmbed streams"));
    }
    Ok(common::result(candidates, issues))
}

fn uuid_v4(bytes: &[u8]) -> Result<String, AdapterError> {
    if bytes.len() != 16 {
        return Err(AdapterError::Protocol("random bytes length"));
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(bytes);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    ))
}

fn source_scope(
    url: &Url,
    headers: BTreeMap<String, String>,
) -> Result<Vec<FetchScope>, AdapterError> {
    if headers.len() > 8 {
        return Err(AdapterError::Protocol("AnyEmbed stream header count"));
    }
    let allowed = [
        "accept",
        "accept-language",
        "origin",
        "referer",
        "user-agent",
    ];
    let mut safe = BTreeMap::new();
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        if !allowed.contains(&name.as_str())
            || value.len() > 1024
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            continue;
        }
        if matches!(name.as_str(), "origin" | "referer") && public_url(&value).is_err() {
            continue;
        }
        safe.insert(name, value);
    }
    if safe.is_empty() {
        return Ok(Vec::new());
    }
    let origin = url.origin().ascii_serialization();
    let path_prefix = if url.path() == "/" {
        String::new()
    } else {
        url.path()
            .rsplit_once('/')
            .map(|(directory, _)| format!("{directory}/"))
            .filter(|directory| directory.len() > 1)
            .unwrap_or_default()
    };
    Ok(vec![FetchScope {
        headers: safe,
        origin,
        path_prefix,
        ..Default::default()
    }])
}

fn safe_track_text(value: &str, max_bytes: usize) -> bool {
    value.len() <= max_bytes && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn format_from_url(path: &str) -> StreamFormat {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mpd" => StreamFormat::Dash,
        "mp4" => StreamFormat::Mp4,
        _ => StreamFormat::Hls,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct MockHost;
    impl ProviderHost for MockHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let body = if request.url.ends_with("/api/v1/embed-attest") {
                assert!(request.headers.iter().any(
                    |header| header.name == "Origin" && header.value == "https://anyembed.xyz"
                ));
                br#"{"success":true,"nonce":"mock-nonce","turnstile_required":false}"#.to_vec()
            } else if request.url.ends_with("/api/v1/session") {
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|header| header.name == "x-embed-attest"
                            && header.value == "mock-nonce")
                );
                br#"{"success":true,"token":"session-token","stream_sign_key":"sign-key"}"#.to_vec()
            } else if request.url.contains("/api/v1/stream/550") {
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|header| header.name == "x-session-token"
                            && header.value == "session-token")
                );
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|header| header.name == "x-anyembed-timestamp"
                            && header.value == "1700000000")
                );
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|header| header.name == "x-anyembed-nonce"
                            && header.value == "2a2a2a2a-2a2a-4a2a-aa2a-2a2a2a2a2a2a")
                );
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|header| header.name == "x-anyembed-signature"
                            && header.value == "9bec98af7e755fd192998b083db26cb99c7feb262a6658728052b60882b08538")
                );
                br#"{"success":true,"sources":[{"streams":[{"url":"https://media.example/master.m3u8","quality":"1080p","headers":{"Referer":"https://anyembed.xyz/","Cookie":"must-not-propagate"},"subtitles":[{"url":"https://media.example/en.vtt","label":"English","language":"en"}]}]}]}"#.to_vec()
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
            Ok(1_700_000_000_000)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Ok(vec![0x2a; 16])
        }
    }
    #[test]
    fn format_detection_is_path_based_and_defaults_to_hls() {
        assert_eq!(format_from_url("/a/master.m3u8"), StreamFormat::Hls);
        assert_eq!(format_from_url("/a/v.mpd"), StreamFormat::Dash);
        assert_eq!(format_from_url("/a/v.mp4"), StreamFormat::Mp4);
        assert_eq!(format_from_url("/a/v.mkv"), StreamFormat::Hls);
    }
    #[test]
    fn upstream_sources_do_not_inherit_provider_quality_as_measured_metadata() {
        let parsed = serde_json::from_str::<Envelope>(
            r#"{"success":true,"sources":[{"streams":[{"url":"https://media.example/master.m3u8","quality":"1080p"}]}]}"#,
        );
        assert!(parsed.is_ok());
        let envelope = parsed.unwrap_or(Envelope {
            success: false,
            sources: Vec::new(),
        });
        assert!(envelope.success);
        assert_eq!(envelope.sources[0].streams[0].quality, "1080p");
    }

    #[test]
    fn source_headers_are_limited_to_noncredential_media_headers()
    -> Result<(), Box<dyn std::error::Error>> {
        let headers = BTreeMap::from([
            ("Referer".into(), "https://anyembed.xyz/".into()),
            ("Cookie".into(), "session=secret".into()),
            ("x-session-token".into(), "session-secret".into()),
        ]);
        let scopes = source_scope(
            &Url::parse("https://media.example/path/master.m3u8")?,
            headers,
        )?;
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].headers.len(), 1);
        assert_eq!(scopes[0].headers["referer"], "https://anyembed.xyz/");
        assert_eq!(scopes[0].path_prefix, "/path/");
        assert!(!safe_track_text(&"界".repeat(22), 64));
        Ok(())
    }

    #[test]
    fn resolve_runs_session_and_stream_protocol_without_leaking_session_credentials()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let result = resolve(&lookup, &MockHost)?;
        result.validate_for(&common::provider_id("anyembed")?, &lookup.title, 32)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.url, "https://media.example/master.m3u8");
        assert_eq!(candidate.resolver_metadata.tracks.len(), 1);
        assert_eq!(
            candidate.resolver_metadata.provider_data["quality_claim"],
            "1080p"
        );
        assert_eq!(candidate.fetch_context.scopes[0].headers.len(), 1);
        assert!(
            !candidate.fetch_context.scopes[0]
                .headers
                .contains_key("cookie")
        );
        assert!(
            !candidate.fetch_context.scopes[0]
                .headers
                .contains_key("x-session-token")
        );
        assert!(
            !candidate.fetch_context.scopes[0]
                .headers
                .contains_key("x-embed-attest")
        );
        let tv: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":550,"season":1,"episode":1}}),
        )?;
        let tv_result = resolve(&tv, &MockHost)?;
        tv_result.validate_for(&common::provider_id("anyembed")?, &tv.title, 32)?;
        let tv_candidates = tv_result.candidates.ok_or("candidate list missing")?;
        assert_eq!(tv_candidates.len(), 1);
        Ok(())
    }
}
