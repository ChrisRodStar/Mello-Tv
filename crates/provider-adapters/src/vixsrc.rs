//! VixSrc embed API, tokenized HLS master, and bounded manifest inspection.
use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get},
    destination::public_url,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat, Variant,
    },
};
use url::Url;

const ORIGIN: &str = "https://vixsrc.to";
const LIMIT: u32 = 512 * 1024;
const MAX_VARIANTS: usize = 32;

#[derive(Deserialize)]
struct Embed {
    src: String,
}
struct TokenData {
    token: String,
    expires: i64,
    playlist: String,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    lookup.title.validate().map_err(|_| AdapterError::Input)?;
    let path = match lookup.title.media_type {
        MediaType::Movie => format!("/api/movie/{}", lookup.title.tmdb_id.get()),
        MediaType::Tv => format!(
            "/api/tv/{}/{}/{}",
            lookup.title.tmdb_id.get(),
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        ),
    };
    let page_url = format!("{ORIGIN}{path}");
    let response = fetch(host, get(&page_url, &format!("{ORIGIN}/"), 32 * 1024))?;
    let embed: Embed = common::json(&response.body, 32 * 1024, "VixSrc embed response")?;
    if !embed.src.starts_with('/') || embed.src.starts_with("//") {
        return Err(AdapterError::Protocol("VixSrc embed path"));
    }
    let embed_url = Url::parse(&page_url)
        .map_err(|_| AdapterError::Protocol("VixSrc API URL"))?
        .join(&embed.src)
        .map_err(|_| AdapterError::Protocol("VixSrc embed URL"))?;
    if !common::same_origin(&embed_url, ORIGIN) {
        return Err(AdapterError::Protocol("VixSrc embed origin"));
    }
    let html = fetch(host, get(embed_url.as_str(), &page_url, LIMIT))?;
    let html = std::str::from_utf8(&html.body)
        .map_err(|_| AdapterError::Protocol("VixSrc page encoding"))?;
    let token_data = extract_token_data(html)?;
    let now = host.now_unix_ms()? / 1000;
    let expires_at = valid_expiration(token_data.expires, now)?;
    let mut master = public_url(&token_data.playlist)
        .map_err(|_| AdapterError::Protocol("VixSrc playlist URL"))?;
    master
        .query_pairs_mut()
        .append_pair("token", &token_data.token)
        .append_pair("expires", &token_data.expires.to_string())
        .append_pair("h", "1");
    let master_url = master.to_string();
    let mut request = get(&master_url, &page_url, LIMIT);
    // Cold CDN manifests can take longer than an API response. Keep the wait
    // bounded while allowing ordinary startup variance.
    request.timeout_ms = 6_000;
    let manifest = fetch(host, request)?;
    let body = std::str::from_utf8(&manifest.body)
        .map_err(|_| AdapterError::Protocol("VixSrc manifest encoding"))?;
    if !body.starts_with("#EXTM3U") {
        return Err(AdapterError::Protocol("VixSrc manifest signature"));
    }
    let variants = parse_variants(body, &master_url)?;
    let provider = common::provider_id("vixsrc")?;
    let candidate = CandidateStream {
        expires_at: Some(expires_at),
        id: format!("vixsrc-{}", common::hash_id(&[master_url.as_bytes()])),
        server: "VixSrc".into(),
        selected_variant_url: String::new(),
        url: master_url,
        provider,
        format: StreamFormat::Hls,
        title: lookup.title.clone(),
        fetch_context: FetchContext {
            scopes: Vec::new(),
            public_resources: Some(PublicResourceContext {
                origin: ORIGIN.into(),
                referer: format!("{ORIGIN}/"),
            }),
        },
        resolver_metadata: Metadata {
            variants,
            ..Default::default()
        },
        content_verified: false,
    };
    Ok(common::result(vec![candidate], Vec::new()))
}

fn valid_expiration(expires: i64, now_unix_seconds: i64) -> Result<DateTime<Utc>, AdapterError> {
    if expires <= now_unix_seconds.saturating_add(5) {
        return Err(AdapterError::Unavailable("VixSrc token expired"));
    }
    DateTime::from_timestamp(expires, 0).ok_or(AdapterError::Protocol("VixSrc expiration range"))
}

fn extract_token_data(html: &str) -> Result<TokenData, AdapterError> {
    if html.len() > LIMIT as usize {
        return Err(AdapterError::Protocol("VixSrc page byte limit"));
    }

    if let Some(data) = fast_extract_token_data(html) {
        return validate_token_data(data);
    }

    static PATTERNS: OnceLock<Result<(Regex, Regex, Regex), regex::Error>> = OnceLock::new();
    let (token_re, expires_re, playlist_re) = PATTERNS
        .get_or_init(|| {
            Ok((
                Regex::new(r#"['"]?token['"]?\s*:\s*['"]([^'"]+)['"]"#)?,
                Regex::new(r#"['"]?expires['"]?\s*:\s*['"]?(\d+)['"]?"#)?,
                Regex::new(r#"['"]?url['"]?\s*:\s*['"]([^'"]+)['"]"#)?,
            ))
        })
        .as_ref()
        .map_err(|_| AdapterError::Protocol("VixSrc token parser"))?;

    let token = token_re
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str())
        .ok_or(AdapterError::Protocol("VixSrc token"))?;
    let expires = expires_re
        .captures(html)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<i64>().ok())
        .ok_or(AdapterError::Protocol("VixSrc expiration"))?;
    let playlist = playlist_re
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().replace("\\/", "/"))
        .ok_or(AdapterError::Protocol("VixSrc playlist"))?;

    validate_token_data(TokenData {
        token: token.into(),
        expires,
        playlist,
    })
}

fn fast_extract_token_data(html: &str) -> Option<TokenData> {
    let token = extract_string_field(html, "token")?;
    let expires_str = extract_value_field(html, "expires")?;
    let expires = expires_str.parse::<i64>().ok()?;
    let playlist = extract_string_field(html, "url")?.replace("\\/", "/");
    Some(TokenData {
        token: token.to_string(),
        expires,
        playlist,
    })
}

fn extract_string_field<'a>(html: &'a str, key: &str) -> Option<&'a str> {
    let pos = html.find(key)?;
    let after_key = &html[pos + key.len()..];
    let mut chars = after_key.char_indices();
    let mut next = chars.next()?;
    if next.1 == '"' || next.1 == '\'' {
        next = chars.next()?;
    }
    while next.1.is_whitespace() {
        next = chars.next()?;
    }
    if next.1 != ':' {
        return None;
    }
    next = chars.next()?;
    while next.1.is_whitespace() {
        next = chars.next()?;
    }
    let quote = next.1;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let val_start = next.0 + quote.len_utf8();
    let val_end = after_key[val_start..].find(quote)?;
    Some(&after_key[val_start..val_start + val_end])
}

fn extract_value_field<'a>(html: &'a str, key: &str) -> Option<&'a str> {
    let pos = html.find(key)?;
    let after_key = &html[pos + key.len()..];
    let mut chars = after_key.char_indices();
    let mut next = chars.next()?;
    if next.1 == '"' || next.1 == '\'' {
        next = chars.next()?;
    }
    while next.1.is_whitespace() {
        next = chars.next()?;
    }
    if next.1 != ':' {
        return None;
    }
    next = chars.next()?;
    while next.1.is_whitespace() {
        next = chars.next()?;
    }
    if next.1 == '"' || next.1 == '\'' {
        let quote = next.1;
        let val_start = next.0 + quote.len_utf8();
        let val_end = after_key[val_start..].find(quote)?;
        Some(&after_key[val_start..val_start + val_end])
    } else {
        let val_start = next.0;
        let digits_len = after_key[val_start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .map(|c| c.len_utf8())
            .sum::<usize>();
        if digits_len == 0 {
            None
        } else {
            Some(&after_key[val_start..val_start + digits_len])
        }
    }
}

fn validate_token_data(data: TokenData) -> Result<TokenData, AdapterError> {
    if data.token.is_empty()
        || data.token.len() > 4096
        || data.token.bytes().any(|b| b.is_ascii_control())
        || data.playlist.len() > 8192
    {
        return Err(AdapterError::Protocol("VixSrc token bounds"));
    }
    Ok(data)
}

fn parse_variants(body: &str, manifest: &str) -> Result<Vec<Variant>, AdapterError> {
    static PATTERNS: OnceLock<Result<(Regex, Regex), regex::Error>> = OnceLock::new();
    let (res_re, bw_re) = PATTERNS
        .get_or_init(|| {
            Ok((
                Regex::new(r"(?:^|,)RESOLUTION=(\d+)x(\d+)(?:,|$)")?,
                Regex::new(r"(?:^|,)BANDWIDTH=(\d+)(?:,|$)")?,
            ))
        })
        .as_ref()
        .map_err(|_| AdapterError::Protocol("VixSrc manifest parser"))?;
    let base = Url::parse(manifest).map_err(|_| AdapterError::Protocol("VixSrc manifest URL"))?;
    let mut variants = Vec::new();
    let mut lines = body.lines().map(str::trim).filter(|l| !l.is_empty());
    while let Some(line) = lines.next() {
        if !line.starts_with("#EXT-X-STREAM-INF:") {
            continue;
        }
        let inf = line.trim_start_matches("#EXT-X-STREAM-INF:");
        let Some(caps) = res_re.captures(inf) else {
            continue;
        };
        let Some(variant_uri) = lines.next() else {
            break;
        };
        if variant_uri.starts_with('#') {
            continue;
        }
        if variants.len() >= MAX_VARIANTS {
            return Err(AdapterError::Protocol("VixSrc variant count"));
        }
        let width = caps[1]
            .parse::<i64>()
            .map_err(|_| AdapterError::Protocol("VixSrc variant width"))?;
        let height = caps[2]
            .parse::<i64>()
            .map_err(|_| AdapterError::Protocol("VixSrc variant height"))?;
        let bitrate = bw_re.captures(inf).and_then(|c| c[1].parse::<i64>().ok());
        let url = base
            .join(variant_uri)
            .map_err(|_| AdapterError::Protocol("VixSrc variant URL"))?;
        public_url(url.as_str())?;
        variants.push(Variant {
            url: url.to_string(),
            bitrate,
            width: Some(width),
            height: Some(height),
            ..Default::default()
        });
    }
    // Highest quality variant ranked first
    variants.sort_by(|a, b| {
        b.height
            .cmp(&a.height)
            .then_with(|| b.bitrate.cmp(&a.bitrate))
            .then_with(|| b.width.cmp(&a.width))
    });
    Ok(variants)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct FixtureHost;
    impl ProviderHost for FixtureHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let body = if request.url.ends_with("/api/tv/1399/1/1") {
                br#"{"src":"/embed/tv/1399/1/1"}"#.to_vec()
            } else if request.url.ends_with("/embed/tv/1399/1/1") {
                br#"{"token":"fixture-token","expires":"2000000000","url":"https:\/\/vixsrc.to\/playlist\/master.m3u8"}"#.to_vec()
            } else if request.url.contains("/playlist/master.m3u8?") {
                b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n720/index.m3u8\n"
                    .to_vec()
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
            Err(HostError::Denied)
        }
    }

    struct MockHost {
        expected_api_path: &'static str,
        expected_embed_path: &'static str,
        api_body: Vec<u8>,
        embed_body: Vec<u8>,
        manifest_body: Vec<u8>,
        now_ms: i64,
    }
    impl ProviderHost for MockHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let body = if request.url.ends_with(self.expected_api_path) {
                self.api_body.clone()
            } else if request.url.ends_with(self.expected_embed_path) {
                self.embed_body.clone()
            } else if request.url.contains("master.m3u8?") {
                self.manifest_body.clone()
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
            Ok(self.now_ms)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }

    fn mock_host(embed_body: &[u8], manifest_body: &[u8], now_ms: i64) -> MockHost {
        MockHost {
            expected_api_path: "/api/tv/1399/2/7",
            expected_embed_path: "/embed/tv/1399/2/7",
            api_body: br#"{"src":"/embed/tv/1399/2/7"}"#.to_vec(),
            embed_body: embed_body.to_vec(),
            manifest_body: manifest_body.to_vec(),
            now_ms,
        }
    }

    fn tv_lookup() -> Result<Lookup, serde_json::Error> {
        serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":1399,"season":2,"episode":7}}),
        )
    }
    #[test]
    fn token_parser_accepts_fields_and_rejects_malformed_expiration()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = extract_token_data(
            r#"player({token: "abc", expires: "2000000000", url: "https://cdn.example/master.m3u8"})"#,
        )?;
        assert_eq!(data.token, "abc");
        assert_eq!(data.expires, 2_000_000_000);
        let quoted_js_keys = extract_token_data(
            r#"window.player = {'token': 'xyz', 'expires': 2000000000, 'url': 'https://cdn.example/master.m3u8'};"#,
        )?;
        assert_eq!(quoted_js_keys.token, "xyz");
        assert_eq!(quoted_js_keys.expires, 2_000_000_000);
        assert!(
            extract_token_data("token: 'x', expires: 'bad', url: 'https://cdn.example/a'").is_err()
        );
        Ok(())
    }
    #[test]
    fn tokenized_playlist_retains_existing_query() -> Result<(), Box<dyn std::error::Error>> {
        let mut url = Url::parse("https://cdn.example/master.m3u8?x=1")?;
        url.query_pairs_mut()
            .append_pair("token", "abc")
            .append_pair("expires", "99")
            .append_pair("h", "1");
        assert!(url.as_str().contains("?x=1&token=abc&expires=99&h=1"));
        Ok(())
    }
    #[test]
    fn source_expiry_requires_probe_margin_and_valid_timestamp()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(valid_expiration(106, 100).is_ok());
        assert!(matches!(
            valid_expiration(105, 100),
            Err(AdapterError::Unavailable(_))
        ));
        assert!(matches!(
            valid_expiration(i64::MAX, 0),
            Err(AdapterError::Protocol(_))
        ));
        Ok(())
    }

    #[test]
    fn resolve_validates_title_contract_and_preserves_token_expiry()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":1399,"season":1,"episode":1}}),
        )?;
        let result = resolve(&lookup, &FixtureHost)?;
        let provider = common::provider_id("vixsrc")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        candidate.validate_for(&provider, &lookup.title)?;
        assert_eq!(candidate.resolver_metadata.variants.len(), 1);
        assert_eq!(candidate.resolver_metadata.variants[0].height, Some(720));
        assert_eq!(
            candidate.expires_at,
            DateTime::from_timestamp(2_000_000_000, 0)
        );
        assert_eq!(
            candidate
                .fetch_context
                .public_resources
                .as_ref()
                .map(|context| context.referer.as_str()),
            Some("https://vixsrc.to/")
        );
        Ok(())
    }

    #[test]
    fn full_resolution_preserves_episode_coordinates_and_respects_variant_limit()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup = tv_lookup()?;
        let embed = br#"{"token":"fixture-token","expires":2000000000,"url":"https://cdn.example/master.m3u8"}"#;
        let manifest =
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n720/index.m3u8\n";
        let host = mock_host(embed, manifest, 1_700_000_000_000);
        let result = resolve(&lookup, &host)?;
        let provider = common::provider_id("vixsrc")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        candidate.validate_for(&provider, &lookup.title)?;
        assert_eq!(candidate.title.season, Some(2));
        assert_eq!(candidate.title.episode, Some(7));
        assert_eq!(candidate.resolver_metadata.variants.len(), 1);
        assert_eq!(candidate.resolver_metadata.variants[0].height, Some(720));
        assert!(candidate.url.contains("token=fixture-token"));
        assert!(candidate.url.contains("expires=2000000000"));
        Ok(())
    }

    #[test]
    fn full_resolution_rejects_near_expiry_and_private_variant_urls()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup = tv_lookup()?;
        let near_expiry = br#"{"token":"fixture-token","expires":1700000005,"url":"https://cdn.example/master.m3u8"}"#;
        let valid_manifest =
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n720/index.m3u8\n";
        assert!(matches!(
            resolve(
                &lookup,
                &mock_host(near_expiry, valid_manifest, 1_700_000_000_000)
            ),
            Err(AdapterError::Unavailable(_))
        ));

        let valid_embed = br#"{"token":"fixture-token","expires":2000000000,"url":"https://cdn.example/master.m3u8"}"#;
        let private_variant = b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\nhttp://127.0.0.1/private.m3u8\n";
        assert!(matches!(
            resolve(
                &lookup,
                &mock_host(valid_embed, private_variant, 1_700_000_000_000)
            ),
            Err(AdapterError::Protocol(_))
        ));
        Ok(())
    }

    #[test]
    fn variant_count_is_bounded_by_provider_contract() -> Result<(), Box<dyn std::error::Error>> {
        let mut manifest = String::from("#EXTM3U\n");
        for index in 0..=MAX_VARIANTS {
            manifest.push_str(&format!(
                "#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n{index}/index.m3u8\n"
            ));
        }
        assert!(matches!(
            parse_variants(&manifest, "https://cdn.example/master.m3u8"),
            Err(AdapterError::Protocol("VixSrc variant count"))
        ));
        Ok(())
    }
}
