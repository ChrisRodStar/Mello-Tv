//! VidFast protocol discovery and source normalization.

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
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, ProviderTitle,
        PublicResourceContext, StreamFormat,
    },
};
use stream_provider_codecs::vidfast as codec;
use stream_provider_sdk::http::Request;

const ORIGIN: &str = "https://vidfast.vc";

fn material_post(
    url: impl Into<String>,
    embed_url: &str,
    limit: u32,
    material: &codec::ProtocolMaterial,
) -> Request {
    let mut req = post(url, embed_url, limit);
    req.timeout_ms = 2_000;
    req.headers.push(header("Origin", ORIGIN));
    if !material.csrf_token().is_empty() {
        req.headers
            .push(header("X-Csrf-Token", material.csrf_token()));
    }
    req
}

fn material_cache() -> &'static RwLock<BTreeMap<String, (codec::ProtocolMaterial, Instant)>> {
    static CACHE: OnceLock<RwLock<BTreeMap<String, (codec::ProtocolMaterial, Instant)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| {
        let mut map = BTreeMap::new();
        let known_url = format!("{ORIGIN}{}", codec::KNOWN_PLAYER_PATH);
        map.insert(
            known_url,
            (
                codec::known_material(),
                Instant::now() + Duration::from_secs(3600 * 24 * 365),
            ),
        );
        RwLock::new(map)
    })
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
    #[serde(default, rename = "tmdbId", alias = "tmdb_id", alias = "id")]
    tmdb_id: Value,
    #[serde(default)]
    mp4: Option<bool>,
    #[serde(default, rename = "noReferrer")]
    no_referrer: Option<bool>,
    #[serde(default, alias = "seasonNumber", alias = "s")]
    season: Option<Value>,
    #[serde(default, alias = "episodeNumber", alias = "ep", alias = "e")]
    episode: Option<Value>,
    #[serde(default)]
    tracks: Option<Vec<SourceTrack>>,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let path = embed_path(&lookup.title)?;
    let embed_url = format!("{ORIGIN}{path}");
    let mut embed_req = get(&embed_url, &format!("{ORIGIN}/"), 512 * 1024);
    embed_req.timeout_ms = 2_000;
    let embed = fetch(host, embed_req)?;
    let (token, player_script) = parse_bootstrap(&embed.body, &lookup.title)?;

    let player_url = format!("{ORIGIN}{player_script}");
    let now = Instant::now();
    let material = if let Some(m) = material_cache().read().ok().and_then(|cache| {
        cache
            .get(&player_url)
            .filter(|(_, exp)| *exp > now)
            .map(|(m, _)| m.clone())
    }) {
        m
    } else {
        let mut player_req = get(&player_url, &embed_url, 3 * 1024 * 1024);
        player_req.timeout_ms = 2_000;
        let player = fetch(host, player_req)?;
        let mat = codec::recognize_player(&player.body)
            .map_err(|_| AdapterError::Protocol("VidFast player version"))?;
        if let Ok(mut cache) = material_cache().write() {
            cache.insert(
                player_url,
                (mat.clone(), now + Duration::from_secs(3600 * 24)),
            );
        }
        mat
    };

    let salt = host.random_bytes(16)?;
    let timestamp = host.now_unix_ms()?;
    let packed = codec::pack_request(&token, &salt, timestamp)
        .map_err(|_| AdapterError::Protocol("VidFast pack request"))?;

    let challenge_url = format!("{ORIGIN}{}/JgQXcWPNH7s/{packed}", material.root());
    let challenge_resp = fetch(host, material_post(challenge_url, &embed_url, 4096, &material))?;

    let ticket = codec::challenge_token(&challenge_resp.body)
        .map_err(|_| AdapterError::Protocol("VidFast challenge token"))?;

    let list_url = format!("{ORIGIN}{}/dBWKuO8aW6U/{ticket}", material.root());
    let list_resp = fetch(host, material_post(list_url, &embed_url, 256 * 1024, &material))?;

    let servers_encoded = std::str::from_utf8(&list_resp.body)
        .map_err(|_| AdapterError::Protocol("VidFast servers encoding"))?;
    let servers = codec::decode_servers(servers_encoded)
        .map_err(|_| AdapterError::Protocol("VidFast decode servers"))?;

    if servers.is_empty() {
        return Err(AdapterError::Unavailable("VidFast servers"));
    }

    let servers = prioritize_servers(servers);
    let mut unlock_requests = Vec::new();
    let mut active_servers = Vec::new();
    let mut seen_servers = BTreeSet::new();

    for server in servers.into_iter().take(3) {
        if server.name.is_empty() || seen_servers.contains(&server.name) || server.data.is_empty() {
            continue;
        }
        seen_servers.insert(server.name.clone());
        let unlock_url = format!("{ORIGIN}{}/M6SyMXJBpeg/{}", material.root(), server.data);
        unlock_requests.push(material_post(unlock_url, &embed_url, 256 * 1024, &material));
        active_servers.push(server.name);
    }

    if unlock_requests.is_empty() {
        return Err(AdapterError::Unavailable("VidFast servers"));
    }

    let responses = batch(host, unlock_requests)?;
    let provider = common::provider_id("vidfast")?;
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
        let body_str = match std::str::from_utf8(&resp.body) {
            Ok(s) => s,
            Err(_) => {
                issues.push(common::issue(
                    &AdapterError::Protocol("VidFast unlock text"),
                    "unlock",
                ));
                continue;
            }
        };
        let plaintext = match codec::decrypt_envelope(body_str) {
            Ok(p) => p,
            Err(_) => {
                issues.push(common::issue(
                    &AdapterError::Protocol("VidFast decrypt source"),
                    "unlock",
                ));
                continue;
            }
        };
        let record: SourceRecord =
            match common::json(&plaintext, 256 * 1024, "VidFast source record") {
                Ok(r) => r,
                Err(e) => {
                    issues.push(common::issue(&e, "unlock"));
                    continue;
                }
            };
        if record.url.is_empty() || seen_urls.contains(&record.url) {
            continue;
        }

        let tmdb_matches = match &record.tmdb_id {
            Value::Number(n) => n.as_i64() == Some(lookup.title.tmdb_id.get()),
            Value::String(s) => parse_coordinate_str(s) == Some(lookup.title.tmdb_id.get()),
            Value::Null => true,
            _ => false,
        };
        if !tmdb_matches {
            issues.push(common::issue(
                &AdapterError::Protocol("VidFast tmdb mismatch"),
                "sources",
            ));
            continue;
        }

        if lookup.title.media_type == MediaType::Tv
            && (!coordinate_matches(record.season.as_ref(), lookup.title.season)
                || !coordinate_matches(record.episode.as_ref(), lookup.title.episode))
        {
            // Bootstrap validation binds records without coordinates to the requested TV episode.
            // Supplied coordinates still have to match exactly.
            issues.push(common::issue(
                &AdapterError::Protocol("VidFast episode coordinates"),
                "sources",
            ));
            continue;
        }

        if public_url(&record.url).is_err() {
            issues.push(common::issue(
                &AdapterError::Protocol("VidFast source destination"),
                "sources",
            ));
            continue;
        }
        seen_urls.insert(record.url.clone());

        if record.no_referrer.is_none() {
            issues.push(common::issue(
                &AdapterError::Protocol("VidFast media referrer policy unknown"),
                "sources",
            ));
            continue;
        }

        let mut format = if record.url.ends_with(".m3u8") {
            StreamFormat::Hls
        } else if record.url.ends_with(".mp4") {
            StreamFormat::Mp4
        } else if record.url.ends_with(".mpd") {
            StreamFormat::Dash
        } else {
            StreamFormat::Unknown
        };
        if record.mp4 == Some(true) {
            if format != StreamFormat::Unknown && format != StreamFormat::Mp4 {
                issues.push(common::issue(
                    &AdapterError::Protocol("VidFast format declarations conflict"),
                    "sources",
                ));
                continue;
            }
            format = StreamFormat::Mp4;
        } else if format == StreamFormat::Unknown {
            format = StreamFormat::Hls;
        }

        let no_ref = record.no_referrer == Some(true);
        let public_resources = if no_ref {
            None
        } else {
            Some(PublicResourceContext {
                origin: ORIGIN.into(),
                referer: format!("{ORIGIN}/"),
            })
        };

        let context = FetchContext {
            scopes: Vec::new(),
            public_resources,
        };

        let mut tracks = Vec::new();
        if let Some(source_tracks) = record.tracks {
            for track in source_tracks {
                if track.file.is_empty() || public_url(&track.file).is_err() {
                    continue;
                }
                tracks.push(stream_core::providers::Track {
                    kind: "subtitles".into(),
                    language: track.label.clone(),
                    name: track.label,
                    url: track.file,
                    ..Default::default()
                });
            }
        }
        let tracks_known = !tracks.is_empty();
        let metadata = Metadata {
            tracks,
            tracks_known,
            ..Default::default()
        };

        let id = hash_id(&[server_name.as_bytes(), b"\x00", record.url.as_bytes()]);
        candidates.push(CandidateStream {
            expires_at: None,
            id,
            server: server_name,
            selected_variant_url: String::new(),
            url: record.url,
            provider: provider.clone(),
            format,
            title: lookup.title.clone(),
            fetch_context: context,
            resolver_metadata: metadata,
            content_verified: false,
        });
    }

    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("VidFast candidates"));
    }

    Ok(common::result(candidates, issues))
}

fn prioritize_servers(mut servers: Vec<codec::Server>) -> Vec<codec::Server> {
    const PREFERRED: &[&str] = &["vRapid", "vBlaze", "Cine"];
    servers.sort_by_key(|s| {
        PREFERRED
            .iter()
            .position(|&p| p.eq_ignore_ascii_case(&s.name))
            .unwrap_or(usize::MAX)
    });
    servers
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

fn parse_coordinate_str(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if let Ok(n) = trimmed.parse::<i64>() {
        return Some(n);
    }
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok()
}

fn coordinate_matches(value: Option<&Value>, expected: Option<i64>) -> bool {
    let Some(value) = value else {
        return true;
    };
    if value.is_null() {
        return false;
    }
    let actual = match value {
        Value::Number(number) => number.as_i64(),
        Value::String(string) => parse_coordinate_str(string),
        _ => None,
    };
    actual == expected
}

fn parse_bootstrap(body: &[u8], title: &ProviderTitle) -> Result<(String, String), AdapterError> {
    static PATTERNS: OnceLock<Result<(Regex, Regex), regex::Error>> = OnceLock::new();
    let (flight_pat, script_pat) = PATTERNS
        .get_or_init(|| {
            Ok((
                Regex::new(r#"self\.__next_f\.push\(\[1,("(?:[^"\\]|\\.)*")\]\)"#)?,
                Regex::new(r#"src="(/_next/static/chunks/365-[a-zA-Z0-9-]+\.js)""#)?,
            ))
        })
        .as_ref()
        .map_err(|_| AdapterError::Protocol("regex init"))?;

    let html =
        std::str::from_utf8(body).map_err(|_| AdapterError::Protocol("VidFast html encoding"))?;

    let mut flight = String::new();
    for cap in flight_pat.captures_iter(html) {
        if let Some(m) = cap.get(1)
            && let Ok(chunk) = serde_json::from_str::<String>(m.as_str())
        {
            flight.push_str(&chunk);
        }
    }

    let mut found_token = None;
    for line in flight.lines() {
        let Some((_, payload)) = line.split_once(':') else {
            continue;
        };
        if !payload.starts_with(r#"["$","$L"#) {
            continue;
        }
        let Ok(component) = serde_json::from_str::<Vec<Value>>(payload) else {
            continue;
        };
        if component.len() != 4 {
            continue;
        }
        let Some(props) = component[3].as_object() else {
            continue;
        };
        let Some(en) = props.get("en").and_then(|v| v.as_str()) else {
            continue;
        };

        if let Some(host) = props.get("host").and_then(|v| v.as_str())
            && !host.contains("vidfast")
        {
            continue;
        }

        let id_matches = match props.get("id") {
            Some(Value::Number(n)) => n.as_i64() == Some(title.tmdb_id.get()),
            Some(Value::String(s)) => parse_coordinate_str(s) == Some(title.tmdb_id.get()),
            _ => false,
        };
        if !id_matches {
            continue;
        }

        if title.media_type == MediaType::Tv {
            let season_matches = match props.get("season") {
                Some(Value::Number(n)) => n.as_i64() == title.season,
                Some(Value::String(s)) => parse_coordinate_str(s) == title.season,
                _ => false,
            };
            let ep_matches = match props.get("episode") {
                Some(Value::Number(n)) => n.as_i64() == title.episode,
                Some(Value::String(s)) => parse_coordinate_str(s) == title.episode,
                _ => false,
            };
            if !season_matches || !ep_matches {
                continue;
            }
        }

        found_token = Some(en.to_owned());
        break;
    }

    let token = found_token.ok_or(AdapterError::Protocol("VidFast token missing"))?;

    let script_cap = script_pat
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .ok_or(AdapterError::Protocol("VidFast player script missing"))?;

    Ok((token, script_cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tv_source_coordinates_may_be_omitted_but_must_match_when_present() {
        let expected_season = Some(1);
        let expected_episode = Some(1);
        assert!(coordinate_matches(None, expected_season));
        assert!(coordinate_matches(
            Some(&serde_json::json!(1)),
            expected_episode
        ));
        assert!(coordinate_matches(
            Some(&serde_json::json!("1")),
            expected_season
        ));
        assert!(coordinate_matches(
            Some(&serde_json::json!("S01")),
            expected_season
        ));
        assert!(coordinate_matches(
            Some(&serde_json::json!("E01")),
            expected_episode
        ));
        assert!(!coordinate_matches(
            Some(&serde_json::json!(2)),
            expected_season
        ));
        assert!(!coordinate_matches(
            Some(&serde_json::json!("not-a-number")),
            expected_episode
        ));
        assert!(!coordinate_matches(
            Some(&serde_json::json!(null)),
            expected_episode
        ));
    }

    #[test]
    fn parses_source_record_with_tracks() -> Result<(), Box<dyn std::error::Error>> {
        let json_payload = serde_json::json!({
            "url": "https://moon.goldenkite.top/vd/master.m3u8",
            "tmdbId": 1622,
            "noReferrer": false,
            "tracks": [
                {
                    "file": "https://sub.vidfast.vc/en.srt",
                    "label": "English"
                }
            ]
        });
        let record: SourceRecord = serde_json::from_value(json_payload)?;
        assert_eq!(record.url, "https://moon.goldenkite.top/vd/master.m3u8");
        assert_eq!(record.no_referrer, Some(false));
        let tracks = record.tracks.as_ref().ok_or("missing tracks")?;
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].file, "https://sub.vidfast.vc/en.srt");
        assert_eq!(tracks[0].label, "English");
        Ok(())
    }

    #[test]
    fn parses_source_record_with_aliases() -> Result<(), Box<dyn std::error::Error>> {
        let json_payload = serde_json::json!({
            "url": "https://moon.goldenkite.top/vd/master.m3u8",
            "tmdb_id": 60625,
            "seasonNumber": 1,
            "episodeNumber": 7,
            "noReferrer": true,
        });
        let record: SourceRecord = serde_json::from_value(json_payload)?;
        assert_eq!(record.url, "https://moon.goldenkite.top/vd/master.m3u8");
        assert_eq!(record.no_referrer, Some(true));
        assert_eq!(record.tmdb_id, serde_json::json!(60625));
        assert_eq!(record.season, Some(serde_json::json!(1)));
        assert_eq!(record.episode, Some(serde_json::json!(7)));
        Ok(())
    }

    #[test]
    fn parses_bootstrap_from_fixture_html() -> Result<(), Box<dyn std::error::Error>> {
        use stream_core::providers::TmdbId;
        let movie_fixture =
            include_bytes!("../../provider-codecs/testdata/providers/vidfast/movie.html");
        let movie_title = ProviderTitle::movie(TmdbId::try_from(27205)?);
        let (token, script) = parse_bootstrap(movie_fixture, &movie_title)?;
        assert_eq!(token, "synthetic_bootstrap_token_only");
        assert_eq!(script, "/_next/static/chunks/365-fixture.js");

        let tv_fixture = include_bytes!("../../provider-codecs/testdata/providers/vidfast/tv.html");
        let tv_title = ProviderTitle::episode(TmdbId::try_from(60625)?, 0, 7)?;
        let (tv_token, tv_script) = parse_bootstrap(tv_fixture, &tv_title)?;
        assert_eq!(tv_token, "synthetic_bootstrap_token_only");
        assert_eq!(tv_script, "/_next/static/chunks/365-fixture.js");
        Ok(())
    }

    #[test]
    fn parses_bootstrap_with_numeric_coordinates() -> Result<(), Box<dyn std::error::Error>> {
        use stream_core::providers::TmdbId;
        let html = br#"<!doctype html><script src="/_next/static/chunks/365-57c331b65ee30ab6.js"></script>
<script>self.__next_f.push([1,"5:[\"$\",\"$L11\",null,{\"en\":\"test_token\",\"id\":1399,\"season\":2,\"episode\":5}]\n"])</script>"#;
        let tv_title = ProviderTitle::episode(TmdbId::try_from(1399)?, 2, 5)?;
        let (token, script) = parse_bootstrap(html, &tv_title)?;
        assert_eq!(token, "test_token");
        assert_eq!(script, "/_next/static/chunks/365-57c331b65ee30ab6.js");
        Ok(())
    }
}
