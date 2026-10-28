//! VidCore protocol discovery and source normalization.

use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, fetch, get, hash_id, header, post},
    destination::public_url,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, FetchScope, Lookup, Metadata, ProviderTitle,
        PublicResourceContext, StreamFormat,
    },
};
use stream_provider_codecs::vidcore::Protocol;

const ORIGIN: &str = "https://vidcore.io";

fn protocol_cache() -> &'static RwLock<BTreeMap<String, (Protocol, Instant)>> {
    static CACHE: OnceLock<RwLock<BTreeMap<String, (Protocol, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BTreeMap::new()))
}

fn latest_protocol() -> &'static RwLock<Option<(Protocol, Instant)>> {
    static PROTO: OnceLock<RwLock<Option<(Protocol, Instant)>>> = OnceLock::new();
    PROTO.get_or_init(|| RwLock::new(None))
}

fn candidate_chunks(chunks: &[String]) -> Vec<&str> {
    let mut prioritized = Vec::new();
    let mut framework = Vec::new();

    for rel_path in chunks {
        let lower = rel_path.to_ascii_lowercase();
        if lower.contains("webpack")
            || lower.contains("framework")
            || lower.contains("main-app")
            || lower.contains("main-")
            || lower.contains("main.")
            || lower.contains("polyfills")
            || lower.contains("layout")
            || lower.contains("/app/")
            || lower.contains("page-")
        {
            framework.push(rel_path.as_str());
        } else {
            prioritized.push(rel_path.as_str());
        }
    }
    prioritized.reverse();
    prioritized.extend(framework);
    prioritized
}

#[derive(Deserialize)]
struct ServerRecord {
    name: String,
    data: String,
}

#[derive(Deserialize)]
struct SourceTrack {
    #[serde(default)]
    file: String,
    #[serde(default)]
    label: String,
}

#[derive(Deserialize)]
struct SourceRecord {
    url: String,
    #[serde(default, rename = "tmdbId")]
    tmdb_id: Option<Value>,
    #[serde(default, rename = "noReferrer")]
    no_referrer: Option<bool>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    tracks: Option<Vec<SourceTrack>>,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let path = embed_path(&lookup.title)?;
    let embed_url = format!("{ORIGIN}{path}");
    let mut embed_req = get(&embed_url, &format!("{ORIGIN}/"), 512 * 1024);
    embed_req.timeout_ms = 2_500;
    let embed = fetch(host, embed_req)?;
    let (token, chunks) = parse_embed(&embed.body)?;

    let ordered_chunks = candidate_chunks(&chunks);
    let now = Instant::now();
    let mut recognized = latest_protocol().read().ok().and_then(|cached| {
        cached
            .as_ref()
            .filter(|(_, expires_at)| *expires_at > now)
            .map(|(protocol, _)| protocol.clone())
    });

    if recognized.is_none() {
        let cache_guard = protocol_cache().read();
        if let Ok(cache) = &cache_guard {
            for chunk in &ordered_chunks {
                let chunk_url = format!("{ORIGIN}{chunk}");
                if let Some((proto, _)) = cache.get(&chunk_url).filter(|(_, exp)| *exp > now) {
                    recognized = Some(proto.clone());
                    break;
                }
            }
        }
    }

    if recognized.is_none() {
        for chunk in &ordered_chunks {
            let chunk_url = format!("{ORIGIN}{chunk}");
            let mut chunk_req = get(&chunk_url, &embed_url, 2_500_000);
            chunk_req.timeout_ms = 2_000;
            let chunk_resp = match fetch(host, chunk_req) {
                Ok(resp) => resp,
                Err(_) => continue,
            };
            let Ok(source) = std::str::from_utf8(&chunk_resp.body) else {
                continue;
            };
            if let Ok(proto) = Protocol::from_source(source) {
                if let Ok(mut cache) = protocol_cache().write() {
                    cache.insert(
                        chunk_url,
                        (proto.clone(), now + Duration::from_secs(3600 * 24)),
                    );
                }
                if let Ok(mut guard) = latest_protocol().write() {
                    *guard = Some((proto.clone(), now + Duration::from_secs(3600 * 24)));
                }
                recognized = Some(proto);
                break;
            }
        }
    }
    let protocol = recognized.ok_or(AdapterError::Protocol("VidCore player"))?;

    let nonce = host.random_bytes(16)?;
    let timestamp = host.now_unix_ms()?;
    let challenge_str = protocol
        .challenge(&token, timestamp, &nonce)
        .map_err(|_| AdapterError::Protocol("VidCore challenge"))?;

    let challenge_url = format!(
        "{ORIGIN}{}/{}/{}",
        protocol.base(),
        protocol.challenge_action(),
        challenge_str
    );
    let mut req = post(challenge_url, &embed_url, 64 * 1024);
    req.timeout_ms = 2_000;
    req.headers.push(header("Origin", ORIGIN));
    if !protocol.csrf_token().is_empty() {
        req.headers
            .push(header("X-Csrf-Token", protocol.csrf_token()));
    }
    let challenge_resp = fetch(host, req)?;

    let ticket_str = protocol
        .ticket(&challenge_resp.body)
        .map_err(|_| AdapterError::Protocol("VidCore ticket"))?;

    let list_url = format!(
        "{ORIGIN}{}/{}/{}",
        protocol.base(),
        protocol.list_action(),
        ticket_str
    );
    let mut req = post(list_url, &embed_url, 64 * 1024);
    req.timeout_ms = 2_000;
    req.headers.push(header("Origin", ORIGIN));
    if !protocol.csrf_token().is_empty() {
        req.headers
            .push(header("X-Csrf-Token", protocol.csrf_token()));
    }
    let list_resp = fetch(host, req)?;

    let decrypted_list = protocol
        .decode_response(&list_resp.body)
        .map_err(|_| AdapterError::Protocol("VidCore list decode"))?;

    let servers: Vec<ServerRecord> =
        common::json(&decrypted_list, 64 * 1024, "VidCore server list")?;
    if servers.is_empty() {
        return Err(AdapterError::Unavailable("VidCore servers"));
    }

    let mut unlock_requests = Vec::new();
    let mut active_servers = Vec::new();
    let mut seen_servers = BTreeSet::new();

    for server in servers.into_iter().take(8) {
        if server.name.is_empty()
            || server.name.len() > 128
            || seen_servers.contains(&server.name)
            || server.data.is_empty()
        {
            continue;
        }
        seen_servers.insert(server.name.clone());
        let unlock_url = format!(
            "{ORIGIN}{}/{}/{}",
            protocol.base(),
            protocol.unlock_action(),
            server.data
        );
        let mut req = post(unlock_url, &embed_url, 256 * 1024);
        req.timeout_ms = 1_000;
        req.headers.push(header("Origin", ORIGIN));
        if !protocol.csrf_token().is_empty() {
            req.headers
                .push(header("X-Csrf-Token", protocol.csrf_token()));
        }
        unlock_requests.push(req);
        active_servers.push(server.name);
    }

    if unlock_requests.is_empty() {
        return Err(AdapterError::Unavailable("VidCore servers"));
    }

    let responses = batch(host, unlock_requests)?;
    let provider = common::provider_id("vidcore")?;
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen_urls = BTreeSet::new();

    for (server_name, response) in active_servers.into_iter().zip(responses) {
        let resp = match response {
            Ok(r) => r,
            Err(e) => {
                issues.push(common::issue(&e, "unlock"));
                continue;
            }
        };
        let decrypted = match protocol.decode_response(&resp.body) {
            Ok(d) => d,
            Err(_) => {
                issues.push(common::issue(
                    &AdapterError::Protocol("VidCore decode source"),
                    "unlock",
                ));
                continue;
            }
        };
        let record: SourceRecord =
            match common::json(&decrypted, 256 * 1024, "VidCore source record") {
                Ok(r) => r,
                Err(e) => {
                    issues.push(common::issue(&e, "unlock"));
                    continue;
                }
            };
        if record.url.is_empty() || seen_urls.contains(&record.url) {
            continue;
        }
        if let Some(tmdb) = &record.tmdb_id {
            let matches = match tmdb {
                Value::Number(n) => n.as_i64() == Some(lookup.title.tmdb_id.get()),
                Value::String(s) => s.parse::<i64>().ok() == Some(lookup.title.tmdb_id.get()),
                _ => false,
            };
            if !matches {
                issues.push(common::issue(
                    &AdapterError::Protocol("VidCore tmdb mismatch"),
                    "sources",
                ));
                continue;
            }
        }
        let stream_url = match public_url(&record.url) {
            Ok(url) => url,
            Err(_) => {
                issues.push(common::issue(
                    &AdapterError::Protocol("VidCore source destination"),
                    "sources",
                ));
                continue;
            }
        };
        seen_urls.insert(record.url.clone());

        let stream_url_str = record.url;
        let format = if stream_url_str.ends_with(".m3u8") {
            StreamFormat::Hls
        } else if stream_url_str.ends_with(".mp4") {
            StreamFormat::Mp4
        } else if stream_url_str.ends_with(".mpd") {
            StreamFormat::Dash
        } else {
            StreamFormat::Hls
        };

        let no_ref = record.no_referrer == Some(true);
        let public_resources = Some(PublicResourceContext {
            origin: ORIGIN.into(),
            referer: if no_ref {
                String::new()
            } else {
                format!("{ORIGIN}/")
            },
        });

        let mut headers = record.headers;
        if !no_ref {
            headers.insert("Referer".into(), format!("{ORIGIN}/"));
        }
        headers.insert("Origin".into(), ORIGIN.into());

        let scopes = vec![FetchScope {
            origin: stream_url.origin().ascii_serialization(),
            path_prefix: "/".into(),
            headers,
            ..FetchScope::default()
        }];

        let context = FetchContext {
            scopes,
            public_resources,
        };

        let mut tracks = Vec::new();
        if let Some(source_tracks) = record.tracks {
            for track in source_tracks {
                let Ok(track_url) = public_url(&track.file) else {
                    continue;
                };
                if !track_url.path().to_ascii_lowercase().ends_with(".vtt") {
                    continue;
                }
                let language = match track.label.to_ascii_lowercase().as_str() {
                    "english" | "eng" | "en" => "en",
                    "japanese" | "ja" | "jpn" => "ja",
                    _ => "",
                };
                tracks.push(stream_core::providers::Track {
                    kind: "subtitles".into(),
                    language: language.into(),
                    name: track.label,
                    url: track_url.to_string(),
                    ..Default::default()
                });
            }
        }

        let mut provider_data: BTreeMap<String, Value> =
            common::json(&decrypted, 256 * 1024, "VidCore raw attributes").unwrap_or_default();
        provider_data.remove("url");
        provider_data.remove("headers");
        provider_data.remove("cookies");
        provider_data.remove("cookiePath");

        let id = format!("{}:{}", server_name, hash_id(&[stream_url_str.as_bytes()]));
        candidates.push(CandidateStream {
            expires_at: None,
            id,
            server: server_name,
            selected_variant_url: String::new(),
            url: stream_url_str,
            provider: provider.clone(),
            format,
            title: lookup.title.clone(),
            fetch_context: context,
            resolver_metadata: Metadata {
                tracks,
                tracks_known: false,
                provider_data,
                ..Metadata::default()
            },
            content_verified: false,
        });
    }

    // Parallel master playlist promotion for Supreme and Prime candidates
    let mut master_probe_indices = Vec::new();
    let mut master_probe_requests = Vec::new();
    for (idx, candidate) in candidates.iter().enumerate() {
        if (candidate.server == "Supreme" || candidate.server == "Prime")
            && candidate.url.contains("/sd/")
        {
            let parsed = url::Url::parse(&candidate.url).ok().and_then(|mut u| {
                let pos = u.path().find("/sd/")?;
                let new_path = format!("{}/master.m3u8", &u.path()[..pos]);
                u.set_path(&new_path);
                Some(u.to_string())
            });
            if let Some(master_url_str) = parsed {
                let mut probe_req = get(&master_url_str, &format!("{ORIGIN}/"), 256 * 1024);
                probe_req.timeout_ms = 800;
                master_probe_requests.push(probe_req);
                master_probe_indices.push((idx, master_url_str));
            }
        }
    }

    if !master_probe_requests.is_empty() {
        let probe_res = batch(host, master_probe_requests);
        if let Ok(probe_responses) = probe_res {
            for ((idx, new_url), resp_res) in master_probe_indices.into_iter().zip(probe_responses)
            {
                let is_valid_master = resp_res
                    .is_ok_and(|resp| resp.status == 200 && resp.body.starts_with(b"#EXTM3U"));
                if is_valid_master {
                    candidates[idx].url = new_url;
                    candidates[idx].format = StreamFormat::Hls;
                }
            }
        }
    }

    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("VidCore candidates"));
    }

    Ok(common::result(candidates, issues))
}

fn embed_path(title: &ProviderTitle) -> Result<String, AdapterError> {
    title.validate().map_err(|_| AdapterError::Input)?;
    Ok(match title.media_type {
        MediaType::Movie => format!("/movie/{}", title.tmdb_id.get()),
        MediaType::Tv => format!(
            "/tv/{}/{}/{}",
            title.tmdb_id.get(),
            title.season.ok_or(AdapterError::Input)?,
            title.episode.ok_or(AdapterError::Input)?
        ),
    })
}

fn parse_embed(body: &[u8]) -> Result<(String, Vec<String>), AdapterError> {
    static PATTERNS: OnceLock<Result<(Regex, Regex), regex::Error>> = OnceLock::new();
    let (token_pat, chunk_pat) = PATTERNS
        .get_or_init(|| {
            Ok((
                Regex::new(r#"\\*"en\\*":\\*"([^\\*"]+)\\*""#)?,
                Regex::new(r"/_next/static/chunks/[\w./%-]+\.js")?,
            ))
        })
        .as_ref()
        .map_err(|_| AdapterError::Protocol("regex init"))?;

    let html =
        std::str::from_utf8(body).map_err(|_| AdapterError::Protocol("VidCore html encoding"))?;
    let token = token_pat
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .ok_or(AdapterError::Protocol("VidCore token missing"))?;

    let mut seen = BTreeSet::new();
    let mut chunks = Vec::new();
    for mat in chunk_pat.find_iter(html) {
        let chunk = mat.as_str();
        if seen.insert(chunk) {
            chunks.push(chunk.to_owned());
        }
    }
    if chunks.is_empty() || chunks.len() > 32 {
        return Err(AdapterError::Protocol("VidCore chunk count"));
    }

    Ok((token, chunks))
}
