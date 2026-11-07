//! VidLink session/token discovery and source normalization. Session cookies are
//! local to this lookup and never inherited by returned CDN fetch contexts.

use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get, header},
    destination::public_url,
};
use cookie::Cookie;
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
        PublicResourceContext, StreamFormat, Variant,
    },
};
use stream_provider_codecs::vidlink as codec;
use stream_provider_sdk::http::Response;
use url::Url;

const ORIGIN: &str = "https://vidlink.pro";

fn cached_player() -> &'static RwLock<Option<(codec::RecognizedPlayer, Instant)>> {
    static CACHE: OnceLock<RwLock<Option<(codec::RecognizedPlayer, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let path = codec::embed_path(&lookup.title).map_err(|_| AdapterError::Input)?;
    let referer = format!("{ORIGIN}{path}");
    let mut jar = SessionCookies::default();
    let now = Instant::now();

    let recognized = cached_player().read().ok().and_then(|guard| {
        guard
            .as_ref()
            .filter(|(_, exp)| *exp > now)
            .map(|(player, _)| *player)
    });

    let (recognized, cached) = match recognized {
        Some(player) => (player, true),
        None => {
            let player_resp = fetch_session(
                host,
                &mut jar,
                format!("{ORIGIN}/fu.wasm"),
                &referer,
                3 * 1024 * 1024,
            )?;
            let player = codec::recognize_player(&player_resp.body)
                .map_err(|_| AdapterError::Protocol("VidLink player version"))?;
            if let Ok(mut guard) = cached_player().write() {
                *guard = Some((player, now + Duration::from_secs(7200)));
            }
            (player, false)
        }
    };

    let unix_now = host.now_unix_ms()?.div_euclid(1000);
    let api = codec::api_path(recognized, &lookup.title, unix_now)
        .map_err(|_| AdapterError::Protocol("VidLink token"))?;

    let sources_url = format!("{ORIGIN}{api}");
    let sources = match fetch_session(host, &mut jar, sources_url, &referer, 1024 * 1024) {
        Ok(resp) => resp,
        Err(_) if cached => {
            if let Ok(mut guard) = cached_player().write() {
                *guard = None;
            }
            let embed = fetch_session(
                host,
                &mut jar,
                format!("{ORIGIN}{path}"),
                &referer,
                512 * 1024,
            )?;
            drop(embed);
            let player = fetch_session(
                host,
                &mut jar,
                format!("{ORIGIN}/fu.wasm"),
                &referer,
                3 * 1024 * 1024,
            )?;
            let player = codec::recognize_player(&player.body)
                .map_err(|_| AdapterError::Protocol("VidLink player version"))?;
            if let Ok(mut guard) = cached_player().write() {
                *guard = Some((player, Instant::now() + Duration::from_secs(7200)));
            }
            let api = codec::api_path(player, &lookup.title, host.now_unix_ms()?.div_euclid(1000))
                .map_err(|_| AdapterError::Protocol("VidLink token"))?;
            fetch_session(
                host,
                &mut jar,
                format!("{ORIGIN}{api}"),
                &referer,
                1024 * 1024,
            )?
        }
        Err(err) => return Err(err),
    };

    normalize_sources(&sources.body, &lookup.title)
}

fn fetch_session(
    host: &dyn ProviderHost,
    jar: &mut SessionCookies,
    url: String,
    referer: &str,
    limit: u32,
) -> Result<Response, AdapterError> {
    let now = host.now_unix_ms()?.div_euclid(1000);
    let parsed = public_url(&url)?;
    if !common::same_origin(&parsed, ORIGIN) {
        return Err(AdapterError::Protocol("VidLink session origin"));
    }
    let mut request = get(url, referer, limit);
    request.headers.extend([
        header("Origin", ORIGIN),
        header("X-Playback-Environment", "hls"),
    ]);
    if let Some(cookies) = jar.header(parsed.path(), now)? {
        request.headers.push(header("Cookie", cookies));
    }
    let response = fetch(host, request)?;
    jar.receive(&response, parsed.path(), now)?;
    Ok(response)
}

#[derive(Default)]
struct SessionCookies {
    cookies: Vec<SessionCookie>,
}
struct SessionCookie {
    name: String,
    value: String,
    path: String,
    expires: Option<i64>,
}
impl SessionCookies {
    fn receive(
        &mut self,
        response: &Response,
        request_path: &str,
        now: i64,
    ) -> Result<(), AdapterError> {
        for header in response
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("set-cookie"))
        {
            if header.value.len() > 4096 {
                return Err(AdapterError::Protocol("session cookie bytes"));
            }
            let cookie = Cookie::parse(header.value.as_str())
                .map_err(|_| AdapterError::Protocol("session cookie"))?;
            if cookie.domain().is_some_and(|domain| {
                !domain
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case("vidlink.pro")
            }) {
                continue;
            }
            if cookie.name().is_empty()
                || cookie
                    .name()
                    .bytes()
                    .any(|byte| !byte.is_ascii_graphic() || b"()<>@,;:\\\"/[]?={} ".contains(&byte))
                || cookie
                    .value()
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte == b';')
            {
                return Err(AdapterError::Protocol("session cookie value"));
            }
            let path = cookie
                .path()
                .filter(|path| path.starts_with('/'))
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    request_path
                        .rsplit_once('/')
                        .map(|(prefix, _)| {
                            if prefix.is_empty() {
                                "/".into()
                            } else {
                                prefix.into()
                            }
                        })
                        .unwrap_or_else(|| "/".into())
                });
            let expires = if let Some(age) = cookie.max_age() {
                Some(now.saturating_add(age.whole_seconds()))
            } else {
                cookie
                    .expires_datetime()
                    .map(|expiry| expiry.unix_timestamp())
            };
            self.cookies
                .retain(|prior| !(prior.name == cookie.name() && prior.path == path));
            if expires.is_some_and(|expiry| expiry <= now) {
                continue;
            }
            if self.cookies.len() >= 64 {
                return Err(AdapterError::Protocol("session cookie count"));
            }
            self.cookies.push(SessionCookie {
                name: cookie.name().into(),
                value: cookie.value().into(),
                path,
                expires,
            });
        }
        Ok(())
    }
    fn header(&mut self, path: &str, now: i64) -> Result<Option<String>, AdapterError> {
        self.cookies
            .retain(|cookie| cookie.expires.is_none_or(|expiry| expiry > now));
        let mut matching: Vec<_> = self
            .cookies
            .iter()
            .filter(|cookie| {
                path == cookie.path
                    || path
                        .strip_prefix(&cookie.path)
                        .is_some_and(|suffix| cookie.path.ends_with('/') || suffix.starts_with('/'))
            })
            .collect();
        matching.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        let value = matching
            .into_iter()
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ");
        if value.len() > 8192 {
            return Err(AdapterError::Protocol("session cookie budget"));
        }
        Ok((!value.is_empty()).then_some(value))
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "sourceId")]
    source_id: String,
    stream: Option<Value>,
}
#[derive(Deserialize)]
struct Stream {
    #[serde(default)]
    playlist: String,
    #[serde(default, rename = "deliveryType")]
    delivery_type: String,
    #[serde(default, rename = "playlistHeaders")]
    playlist_headers: BTreeMap<String, String>,
    #[serde(default)]
    qualities: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    alternates: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    captions: Value,
    #[serde(default, rename = "TTL")]
    ttl: Option<Value>,
}
#[derive(Deserialize)]
struct Source {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    playlist: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default, rename = "se")]
    season: Option<i64>,
    #[serde(default, rename = "ep")]
    episode: Option<i64>,
}

pub fn normalize_sources(
    body: &[u8],
    title: &ProviderTitle,
) -> Result<AdapterResult, AdapterError> {
    title.validate().map_err(|_| AdapterError::Input)?;
    let envelope: Option<Envelope> = common::json(body, 1024 * 1024, "VidLink source envelope")?;
    let envelope = envelope.ok_or(AdapterError::Unavailable("VidLink stream"))?;
    if !common::opaque(&envelope.source_id, 128) {
        return Err(AdapterError::Protocol("VidLink source identity"));
    }
    let raw = envelope
        .stream
        .ok_or(AdapterError::Unavailable("VidLink stream"))?;
    let stream =
        Stream::deserialize(&raw).map_err(|_| AdapterError::Protocol("VidLink stream schema"))?;
    let qualities = stream.qualities.unwrap_or_default();
    let alternates = stream.alternates.unwrap_or_default();
    if qualities.len() + alternates.len() > 31
        || stream
            .captions
            .as_array()
            .is_some_and(|captions| captions.len() > 64)
    {
        return Err(AdapterError::Protocol("VidLink source count"));
    }
    let mut records = Vec::new();
    if !stream.playlist.is_empty() {
        records.push((
            "primary".into(),
            Source {
                url: stream.playlist,
                kind: stream.delivery_type,
                headers: stream.playlist_headers,
                playlist: String::new(),
                season: None,
                episode: None,
            },
            raw,
        ));
    }
    let mut issues = Vec::new();
    for (prefix, items) in [("quality", qualities), ("alternate", alternates)] {
        for (identity, raw) in items {
            match Source::deserialize(&raw) {
                Ok(mut source) => {
                    if source.url.is_empty() {
                        source.url = std::mem::take(&mut source.playlist);
                    }
                    records.push((format!("{prefix}:{identity}"), source, raw));
                }
                Err(_) => issues.push(common::issue(
                    &AdapterError::Protocol("source record"),
                    "sources",
                )),
            }
        }
    }
    let mut candidates = Vec::new();
    for (identity, record, raw) in records {
        match normalize_source(record, title, &envelope.source_id, &identity) {
            Ok(mut candidate) => {
                candidate.resolver_metadata.provider_data = BTreeMap::from([
                    ("source_record".into(), raw),
                    ("captions".into(), stream.captions.clone()),
                ]);
                if let Some(ttl) = &stream.ttl {
                    candidate
                        .resolver_metadata
                        .provider_data
                        .insert("ttl".into(), ttl.clone());
                }
                candidates.push(candidate);
            }
            Err(error) => issues.push(common::issue(&error, "sources")),
        }
    }
    if candidates.is_empty() {
        return Err(if issues.is_empty() {
            AdapterError::Unavailable("VidLink sources")
        } else {
            AdapterError::Protocol("VidLink sources rejected")
        });
    }
    // Highest quality / lowest latency CDN candidate is ranked first
    candidates.sort_by_key(|a| std::cmp::Reverse(candidate_rank(a)));
    Ok(common::result(candidates, issues))
}

fn candidate_rank(candidate: &CandidateStream) -> (i64, u8, u8) {
    let height = candidate
        .resolver_metadata
        .variants
        .first()
        .and_then(|v| v.height)
        .or_else(|| parse_height_from_url(&candidate.url))
        .unwrap_or(if candidate.format == StreamFormat::Hls {
            1080
        } else {
            0
        });

    let format_priority = match candidate.format {
        StreamFormat::Hls => 2,
        StreamFormat::Mp4 => 1,
        _ => 0,
    };

    let direct_cdn = candidate
        .resolver_metadata
        .provider_data
        .get("source_record")
        .and_then(|raw| raw.get("requiresProxy"))
        .and_then(|val| val.as_bool())
        .map(|proxy| if !proxy { 1 } else { 0 })
        .unwrap_or(1);

    (height, format_priority, direct_cdn)
}

fn parse_height_from_url(url_str: &str) -> Option<i64> {
    for pattern in [
        "/2160", "2160p", "/1440", "1440p", "/1080", "1080p", "/720", "720p", "/540", "540p",
        "/480", "480p", "/360", "360p",
    ] {
        if url_str.contains(pattern) {
            let digits: String = pattern.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(h) = digits.parse::<i64>() {
                return Some(h);
            }
        }
    }
    None
}

fn normalize_source(
    record: Source,
    title: &ProviderTitle,
    server: &str,
    identity: &str,
) -> Result<CandidateStream, AdapterError> {
    let url = public_url(&record.url)?;
    if title.media_type == MediaType::Tv
        && (record
            .season
            .is_some_and(|season| Some(season) != title.season)
            || record
                .episode
                .is_some_and(|episode| Some(episode) != title.episode))
    {
        return Err(AdapterError::Protocol("source episode coordinates"));
    }
    let format = match record.kind.to_ascii_lowercase().as_str() {
        "mp4" => StreamFormat::Mp4,
        "hls" => StreamFormat::Hls,
        "dash" => StreamFormat::Dash,
        _ => return Err(AdapterError::Unsupported),
    };
    let context = source_context(&url, record.headers)?;
    let height = identity
        .strip_prefix("quality:")
        .and_then(|quality| quality.parse::<i64>().ok())
        .filter(|height| (1..=16384).contains(height))
        .or_else(|| parse_height_from_url(&record.url));
    let variants = height
        .map(|height| Variant {
            url: record.url.clone(),
            height: Some(height),
            ..Variant::default()
        })
        .into_iter()
        .collect();
    Ok(CandidateStream {
        expires_at: None,
        id: common::hash_id(&[identity.as_bytes(), b"\0", record.url.as_bytes()]),
        server: server.into(),
        selected_variant_url: String::new(),
        url: record.url,
        provider: common::provider_id("vidlink")?,
        format,
        title: title.clone(),
        fetch_context: context,
        resolver_metadata: Metadata {
            variants,
            ..Metadata::default()
        },
        content_verified: false,
    })
}

pub fn source_context(
    resource: &Url,
    headers: BTreeMap<String, String>,
) -> Result<FetchContext, AdapterError> {
    let mut scope = FetchScope {
        origin: resource.origin().ascii_serialization(),
        path_prefix: resource.path().into(),
        ..FetchScope::default()
    };
    let mut public = PublicResourceContext::default();
    let mut names = BTreeSet::new();
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        if !names.insert(name.clone()) || value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(AdapterError::Protocol("source headers"));
        }
        match name.as_str() {
            "cookie" => {
                scope.cookie_path = resource.path().into();
                scope.cookies = value;
            }
            "referer" | "origin" => {
                let root = public_url(&value)
                    .ok()
                    .filter(|url| url.query().is_none() && url.path() == "/");
                if root.is_some() {
                    if name == "origin" {
                        public.origin = value;
                    } else {
                        public.referer = value;
                    }
                } else {
                    scope.headers.insert(name, value);
                }
            }
            "authorization" | "user-agent" | "accept" | "accept-language" => {
                scope.headers.insert(name, value);
            }
            _ => return Err(AdapterError::Protocol("source header mapping")),
        }
    }
    if (!scope.cookies.is_empty() || scope.headers.contains_key("authorization"))
        && resource.path() == "/"
    {
        return Err(AdapterError::Protocol("unresolved source credential path"));
    }
    let context = FetchContext {
        scopes: if scope.headers.is_empty() && scope.cookies.is_empty() {
            Vec::new()
        } else {
            vec![scope]
        },
        public_resources: if public.origin.is_empty() && public.referer.is_empty() {
            None
        } else {
            Some(public)
        },
    };
    context
        .validate()
        .map_err(|_| AdapterError::Protocol("source fetch context"))?;
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_core::providers::{ProviderTitle, TmdbId};

    fn sample_title() -> Result<ProviderTitle, Box<dyn std::error::Error>> {
        Ok(ProviderTitle {
            media_type: MediaType::Tv,
            tmdb_id: TmdbId::try_from(1399)?,
            imdb_id: String::new(),
            my_anime_list_id: None,
            season: Some(1),
            episode: Some(1),
        })
    }

    #[test]
    fn normalize_sources_ranks_highest_quality_candidate_first()
    -> Result<(), Box<dyn std::error::Error>> {
        let title = sample_title()?;
        let body = br#"{
            "sourceId": "mwVault",
            "stream": {
                "id": "primary",
                "deliveryType": "mp4",
                "playlist": "https://cdn.example.com/720.mp4",
                "qualities": {
                    "360": {"type": "mp4", "url": "https://cdn.example.com/360.mp4", "requiresProxy": true},
                    "1080": {"type": "mp4", "url": "https://cdn.example.com/1080.mp4", "requiresProxy": false},
                    "540": {"type": "mp4", "url": "https://cdn.example.com/540.mp4", "requiresProxy": true}
                }
            }
        }"#;

        let result = normalize_sources(body, &title)?;
        let candidates = result.candidates.ok_or("candidates present")?;
        assert_eq!(candidates.len(), 4);

        // 1080p must be ranked FIRST!
        assert_eq!(
            candidates[0]
                .resolver_metadata
                .variants
                .first()
                .and_then(|v| v.height),
            Some(1080)
        );
        // 720p second
        assert_eq!(
            candidates[1]
                .resolver_metadata
                .variants
                .first()
                .and_then(|v| v.height),
            Some(720)
        );
        // 540p third
        assert_eq!(
            candidates[2]
                .resolver_metadata
                .variants
                .first()
                .and_then(|v| v.height),
            Some(540)
        );
        // 360p fourth
        assert_eq!(
            candidates[3]
                .resolver_metadata
                .variants
                .first()
                .and_then(|v| v.height),
            Some(360)
        );
        Ok(())
    }

    #[test]
    fn normalize_sources_prioritizes_direct_cdn_over_proxy_for_same_height()
    -> Result<(), Box<dyn std::error::Error>> {
        let title = sample_title()?;
        let body = br#"{
            "sourceId": "mwVault",
            "stream": {
                "id": "primary",
                "deliveryType": "mp4",
                "playlist": "",
                "qualities": {
                    "1080proxied": {"type": "mp4", "url": "https://cdn.example.com/1080_p.mp4", "requiresProxy": true},
                    "1080direct": {"type": "mp4", "url": "https://cdn.example.com/1080_d.mp4", "requiresProxy": false}
                }
            }
        }"#;

        let result = normalize_sources(body, &title)?;
        let candidates = result.candidates.ok_or("candidates present")?;
        assert_eq!(candidates.len(), 2);
        assert!(candidates[0].url.contains("1080_d.mp4"));
        assert!(candidates[1].url.contains("1080_p.mp4"));
        Ok(())
    }

    #[test]
    fn normalize_sources_prioritizes_hls_adaptive_over_single_mp4()
    -> Result<(), Box<dyn std::error::Error>> {
        let title = sample_title()?;
        let body = br#"{
            "sourceId": "mwVault",
            "stream": {
                "id": "primary",
                "deliveryType": "hls",
                "playlist": "https://cdn.example.com/master.m3u8",
                "qualities": {
                    "1080": {"type": "mp4", "url": "https://cdn.example.com/1080.mp4"}
                }
            }
        }"#;

        let result = normalize_sources(body, &title)?;
        let candidates = result.candidates.ok_or("candidates present")?;
        assert_eq!(candidates.len(), 2);
        // HLS candidate has format priority over MP4 for equal height
        assert_eq!(candidates[0].format, StreamFormat::Hls);
        assert_eq!(candidates[1].format, StreamFormat::Mp4);
        Ok(())
    }

    #[test]
    fn normalize_sources_rejects_mismatched_coordinates() -> Result<(), Box<dyn std::error::Error>>
    {
        let title = sample_title()?;
        let body = br#"{
            "sourceId": "mwVault",
            "stream": {
                "id": "primary",
                "deliveryType": "mp4",
                "playlist": "",
                "qualities": {
                    "720": {"type": "mp4", "url": "https://cdn.example.com/720.mp4", "se": 2, "ep": 1}
                }
            }
        }"#;

        assert!(normalize_sources(body, &title).is_err());
        Ok(())
    }

    #[test]
    fn session_cookies_filter_and_sort_by_path_specificity()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut jar = SessionCookies::default();
        let resp = Response {
            url: "https://vidlink.pro/test".into(),
            status: 200,
            headers: vec![
                crate::common::header("Set-Cookie", "root=val1; Path=/; Max-Age=3600"),
                crate::common::header("Set-Cookie", "sub=val2; Path=/test; Max-Age=3600"),
            ],
            body: Vec::new(),
        };
        jar.receive(&resp, "/test", 1000)?;
        let header = jar.header("/test/page", 1000)?.ok_or("header expected")?;
        assert_eq!(header, "sub=val2; root=val1");
        Ok(())
    }
}
