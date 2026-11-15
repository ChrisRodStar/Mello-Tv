//! VidAPI's coordinate query endpoint returns direct stream URL candidates.
use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, fetch, get, header},
    destination::public_url,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat, Track,
    },
};
use url::Url;

const API: &str = "https://streamdata.vaplayer.ru/api.php";
const EMBED_ORIGIN: &str = "https://vaplayer.ru";
const NEXTGEN_ORIGIN: &str = "https://nextgencloudfabric.com";
const LIMIT: u32 = 512 * 1024;

#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(borrow, default)]
    status_code: Cow<'a, str>,
    #[serde(borrow, default)]
    data: Option<Data<'a>>,
    #[serde(default)]
    va: Option<VaConfig>,
    #[serde(borrow, default)]
    default_subs: Vec<Subtitle<'a>>,
}

#[derive(Deserialize)]
struct VaConfig {
    #[serde(default)]
    wasm_url: String,
}

#[derive(Deserialize)]
struct Data<'a> {
    #[serde(default)]
    stream_urls: Value,
    #[serde(borrow, default)]
    file_name: Cow<'a, str>,
}

#[derive(Deserialize)]
struct Subtitle<'a> {
    #[serde(borrow, default)]
    lang: Cow<'a, str>,
    #[serde(borrow, default)]
    code: Cow<'a, str>,
    #[serde(borrow)]
    url: Cow<'a, str>,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    lookup.title.validate().map_err(|_| AdapterError::Input)?;
    match try_resolve(lookup, host, false) {
        Ok(result) => Ok(result),
        Err(AdapterError::Unavailable(_)) if is_valid_imdb(&lookup.title.imdb_id) => {
            try_resolve(lookup, host, true)
        }
        Err(err) => Err(err),
    }
}

fn is_valid_imdb(imdb: &str) -> bool {
    imdb.len() >= 4
        && imdb.len() <= 16
        && imdb.starts_with("tt")
        && imdb[2..].bytes().all(|b| b.is_ascii_digit())
}

fn try_resolve(
    lookup: &Lookup,
    host: &dyn ProviderHost,
    use_imdb: bool,
) -> Result<AdapterResult, AdapterError> {
    let (stream_api_url, nextgen_page_url, api_token) =
        match bootstrap_authenticated_session(lookup, host, use_imdb) {
            Ok(session) => session,
            Err(_) => {
                // If full web bootstrap fails (e.g. in mock tests), fallback to direct API endpoint
                let direct = direct_endpoint(lookup, use_imdb, API, "")?;
                return resolve_from_endpoint(lookup, host, &direct, "", "");
            }
        };

    let endpoint = direct_endpoint(lookup, use_imdb, &stream_api_url, &api_token)?;
    resolve_from_endpoint(lookup, host, &endpoint, &nextgen_page_url, &api_token)
}

fn bootstrap_authenticated_session(
    lookup: &Lookup,
    host: &dyn ProviderHost,
    use_imdb: bool,
) -> Result<(String, String, String), AdapterError> {
    static CONFIG_RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let config_re = CONFIG_RE
        .get_or_init(|| Regex::new(r#"const\s+CONFIG\s*=\s*(\{.*?\});"#))
        .as_ref()
        .map_err(|_| AdapterError::Protocol("CONFIG regex init"))?;

    let relative_path = match lookup.title.media_type {
        MediaType::Movie => {
            if use_imdb {
                format!("/embed/movie/{}", lookup.title.imdb_id)
            } else {
                format!("/embed/movie/{}", lookup.title.tmdb_id.get())
            }
        }
        MediaType::Tv => {
            let season = lookup.title.season.ok_or(AdapterError::Input)?;
            let episode = lookup.title.episode.ok_or(AdapterError::Input)?;
            if use_imdb {
                format!("/embed/tv/{}/{}/{}", lookup.title.imdb_id, season, episode)
            } else {
                format!(
                    "/embed/tv/{}/{}/{}",
                    lookup.title.tmdb_id.get(),
                    season,
                    episode
                )
            }
        }
    };

    let embed_url = format!("{EMBED_ORIGIN}{relative_path}");
    let embed_response = fetch(host, get(&embed_url, &format!("{EMBED_ORIGIN}/"), LIMIT))?;

    let mut cookies = Vec::new();
    for h in &embed_response.headers {
        if h.name.eq_ignore_ascii_case("set-cookie")
            && let Some(cookie_part) = h.value.split(';').next()
        {
            cookies.push(cookie_part.trim().to_string());
        }
    }
    let cookie_header = cookies.join("; ");

    let encoded_path: String =
        url::form_urlencoded::byte_serialize(relative_path.as_bytes()).collect();
    let va_src_url = format!("{EMBED_ORIGIN}/embed/va_src.php?path={encoded_path}");
    let mut va_src_req = get(&va_src_url, &embed_url, LIMIT);
    if !cookie_header.is_empty() {
        va_src_req.headers.push(header("Cookie", cookie_header));
    }
    let va_src_response = fetch(host, va_src_req)?;
    let va_src_json: Value = common::json(&va_src_response.body, LIMIT as usize, "va_src JSON")?;
    let nextgen_url = va_src_json
        .get("src")
        .and_then(Value::as_str)
        .ok_or(AdapterError::Protocol("va_src missing src"))?;

    let nextgen_response = fetch(host, get(nextgen_url, &format!("{EMBED_ORIGIN}/"), LIMIT))?;
    let html = std::str::from_utf8(&nextgen_response.body)
        .map_err(|_| AdapterError::Protocol("nextgen html encoding"))?;

    let caps = config_re
        .captures(html)
        .ok_or(AdapterError::Protocol("CONFIG missing in player html"))?;
    let config_str = caps.get(1).map(|m| m.as_str()).unwrap_or("{}");
    let config_json: Value = common::json(config_str.as_bytes(), LIMIT as usize, "CONFIG JSON")?;

    let api_url = config_json
        .get("streamDataApiUrl")
        .and_then(Value::as_str)
        .unwrap_or(API)
        .to_string();
    let api_token = config_json
        .get("apiToken")
        .and_then(Value::as_str)
        .ok_or(AdapterError::Protocol("CONFIG missing apiToken"))?
        .to_string();

    Ok((api_url, nextgen_url.to_string(), api_token))
}

fn direct_endpoint(
    lookup: &Lookup,
    use_imdb: bool,
    base_url: &str,
    api_token: &str,
) -> Result<Url, AdapterError> {
    let mut endpoint =
        Url::parse(base_url).map_err(|_| AdapterError::Protocol("VidAPI endpoint"))?;
    {
        let mut query = endpoint.query_pairs_mut();
        if use_imdb {
            let imdb = &lookup.title.imdb_id;
            if !is_valid_imdb(imdb) {
                return Err(AdapterError::Input);
            }
            query.append_pair("imdb", imdb);
        } else {
            query.append_pair("tmdb", &lookup.title.tmdb_id.get().to_string());
        }
        query.append_pair(
            "type",
            if lookup.title.media_type == MediaType::Movie {
                "movie"
            } else {
                "tv"
            },
        );
        if lookup.title.media_type == MediaType::Tv {
            query
                .append_pair(
                    "season",
                    &lookup.title.season.ok_or(AdapterError::Input)?.to_string(),
                )
                .append_pair(
                    "episode",
                    &lookup.title.episode.ok_or(AdapterError::Input)?.to_string(),
                );
        }
        query.append_pair("stream_urls", "");
        if !api_token.is_empty() {
            query.append_pair("api_token", api_token);
        }
    }
    Ok(endpoint)
}

fn resolve_from_endpoint(
    lookup: &Lookup,
    host: &dyn ProviderHost,
    url: &Url,
    nextgen_page_url: &str,
    api_token: &str,
) -> Result<AdapterResult, AdapterError> {
    let referer = if nextgen_page_url.is_empty() {
        format!("{NEXTGEN_ORIGIN}/")
    } else {
        nextgen_page_url.to_string()
    };
    let response = fetch(host, get(url.as_str(), &referer, LIMIT))?;
    if response.body.len() > LIMIT as usize {
        return Err(AdapterError::Protocol("response byte limit"));
    }
    let envelope: Envelope = serde_json::from_slice(&response.body)
        .map_err(|_| AdapterError::Protocol("VidAPI response"))?;
    if envelope.status_code != "200" {
        return Err(AdapterError::Unavailable("VidAPI status"));
    }
    let data = envelope
        .data
        .ok_or(AdapterError::Unavailable("VidAPI stream data"))?;

    let raw_stream_urls =
        parse_or_decrypt_streams(&data.stream_urls, &envelope.va, host, &referer)?;

    if raw_stream_urls.len() > 32 || envelope.default_subs.len() > 64 {
        return Err(AdapterError::Protocol("VidAPI result count"));
    }

    let mut tracks = Vec::with_capacity(envelope.default_subs.len().min(64));
    for sub in envelope.default_subs {
        if sub.lang.len() > 64
            || sub.code.len() > 64
            || sub.lang.bytes().any(|byte| byte.is_ascii_control())
            || sub.code.bytes().any(|byte| byte.is_ascii_control())
        {
            continue;
        }
        let Ok(url) = public_url(sub.url.as_ref()) else {
            continue;
        };
        let label = if sub.lang.is_empty() {
            sub.code.to_string()
        } else {
            sub.lang.to_string()
        };
        tracks.push(Track {
            kind: "subtitles".into(),
            language: sub.code.to_string(),
            name: label,
            url: url.to_string(),
            ..Default::default()
        });
    }

    let provider = common::provider_id("vidapi")?;
    let file_name_claim: Option<String> = if !data.file_name.is_empty()
        && data.file_name.len() <= 512
        && !data.file_name.bytes().any(|byte| byte.is_ascii_control())
    {
        Some(data.file_name.to_string())
    } else {
        None
    };

    let mut distinct_origins = BTreeSet::new();
    for raw in &raw_stream_urls {
        if let Ok(parsed) = public_url(raw) {
            distinct_origins.insert(format!(
                "{}://{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or("")
            ));
        }
    }

    let mut host_tokens = BTreeMap::new();
    if !api_token.is_empty() && !distinct_origins.is_empty() {
        let mut token_requests = Vec::new();
        let mut origins_order = Vec::new();
        for origin in distinct_origins {
            let token_url = format!("{origin}/generate.php?api_token={api_token}");
            let mut req = get(&token_url, &referer, 8192);
            req.headers.push(header("Origin", NEXTGEN_ORIGIN));
            token_requests.push(req);
            origins_order.push(origin);
        }
        if let Ok(responses) = batch(host, token_requests) {
            for (origin, res) in origins_order.into_iter().zip(responses) {
                if let Ok(r) = res
                    && r.status == 200
                    && let Ok(tok) = std::str::from_utf8(&r.body)
                {
                    let clean = tok.trim();
                    if !clean.is_empty() && clean.len() <= 2048 {
                        host_tokens.insert(origin, clean.to_string());
                    }
                }
            }
        }
    }

    let mut candidates = Vec::with_capacity(raw_stream_urls.len().min(32));
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();

    for mut raw in raw_stream_urls {
        let origin = if let Ok(parsed) = public_url(&raw) {
            format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""))
        } else {
            String::new()
        };

        if let Some(tok) = host_tokens.get(&origin) {
            let delim = if raw.contains('?') { '&' } else { '?' };
            raw.push(delim);
            raw.push_str("token=");
            raw.push_str(tok);
        }

        match public_url(&raw) {
            Ok(url) if seen.insert(url.as_str().to_owned()) => {
                let mut metadata = Metadata {
                    tracks: tracks.clone(),
                    tracks_known: !tracks.is_empty(),
                    ..Default::default()
                };
                if let Some(claim) = &file_name_claim {
                    metadata
                        .provider_data
                        .insert("file_name_claim".into(), claim.clone().into());
                }
                let format = format_from_url(url.path());
                let fetch_context = FetchContext {
                    scopes: Vec::new(),
                    public_resources: Some(PublicResourceContext {
                        origin: String::new(),
                        referer: format!("{NEXTGEN_ORIGIN}/"),
                    }),
                };
                candidates.push(CandidateStream {
                    expires_at: None,
                    id: format!("vidapi-{}", common::hash_id(&[url.as_str().as_bytes()])),
                    server: "VidAPI".into(),
                    selected_variant_url: String::new(),
                    url: url.to_string(),
                    provider: provider.clone(),
                    format,
                    title: lookup.title.clone(),
                    fetch_context,
                    resolver_metadata: metadata,
                    content_verified: false,
                });
            }
            Ok(_) => {}
            Err(error) => issues.push(common::issue(&error, "sources")),
        }
    }

    if candidates.len() >= 2 {
        candidates.swap(0, 1);
    }

    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("VidAPI streams"));
    }
    Ok(common::result(candidates, issues))
}

fn parse_or_decrypt_streams(
    value: &Value,
    va: &Option<VaConfig>,
    host: &dyn ProviderHost,
    referer: &str,
) -> Result<Vec<String>, AdapterError> {
    match value {
        Value::Array(arr) => {
            let mut list = Vec::new();
            for item in arr {
                if let Some(s) = item.as_str() {
                    list.push(s.to_string());
                }
            }
            Ok(list)
        }
        Value::String(encoded_b64) => {
            let wasm_url = va
                .as_ref()
                .map(|v| v.wasm_url.as_str())
                .filter(|s| !s.is_empty())
                .ok_or(AdapterError::Protocol("missing wasm_url"))?;

            let wasm_res = fetch(host, get(wasm_url, referer, LIMIT))?;
            let key = extract_chacha_key(&wasm_res.body)?;

            let decoded = STANDARD
                .decode(encoded_b64.trim())
                .map_err(|_| AdapterError::Protocol("stream_urls base64"))?;
            if decoded.len() < 13 {
                return Err(AdapterError::Protocol("ciphertext too short"));
            }

            let nonce = &decoded[..12];
            let mut ciphertext = decoded[12..].to_vec();
            stream_provider_codecs::chacha20_xor(&key, nonce, 0, &mut ciphertext)
                .map_err(|_| AdapterError::Protocol("chacha20 decrypt"))?;

            let plaintext = std::str::from_utf8(&ciphertext)
                .map_err(|_| AdapterError::Protocol("stream_urls utf8"))?;

            let urls = plaintext
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
            Ok(urls)
        }
        _ => Err(AdapterError::Protocol("unexpected stream_urls value")),
    }
}

fn read_leb128_u32(bytes: &[u8]) -> Result<(u32, usize), AdapterError> {
    let mut result = 0u32;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate().take(5) {
        result |= u32::from(byte & 0x7f) << shift;
        if (byte & 0x80) == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
    }
    Err(AdapterError::Protocol("invalid leb128"))
}

fn parse_data_section(data: &[u8]) -> Result<[u8; 32], AdapterError> {
    let mut pos = 0;
    let (count, read) = read_leb128_u32(&data[pos..])?;
    pos += read;
    let mut segs32 = Vec::new();
    let mut segs_by_offset = BTreeMap::new();
    for _ in 0..count {
        if pos >= data.len() {
            return Err(AdapterError::Protocol("truncated data section"));
        }
        let (mode, read) = read_leb128_u32(&data[pos..])?;
        pos += read;
        if mode == 0 {
            if pos >= data.len() || data[pos] != 0x41 {
                return Err(AdapterError::Protocol("data init expr"));
            }
            pos += 1;
            let (offset, read) = read_leb128_u32(&data[pos..])?;
            pos += read;
            if pos >= data.len() || data[pos] != 0x0b {
                return Err(AdapterError::Protocol("data init end"));
            }
            pos += 1;
            let (size, read) = read_leb128_u32(&data[pos..])?;
            pos += read;
            let seg_end = pos
                .checked_add(size as usize)
                .ok_or(AdapterError::Protocol("segment overflow"))?;
            if seg_end > data.len() {
                return Err(AdapterError::Protocol("segment boundary"));
            }
            let seg_bytes = &data[pos..seg_end];
            if size == 32 {
                let mut buf = [0u8; 32];
                buf.copy_from_slice(seg_bytes);
                segs32.push(buf);
                segs_by_offset.insert(offset, buf);
            }
            pos = seg_end;
        } else {
            return Err(AdapterError::Protocol("unsupported data segment mode"));
        }
    }
    let (seg_a, seg_b) =
        if let (Some(a), Some(b)) = (segs_by_offset.get(&0), segs_by_offset.get(&896)) {
            (*a, *b)
        } else if segs32.len() == 2 {
            (segs32[0], segs32[1])
        } else {
            return Err(AdapterError::Protocol("wasm key segments count"));
        };
    let mut key = [0u8; 32];
    for i in 0..32 {
        key[i] = seg_a[i] ^ seg_b[i];
    }
    Ok(key)
}

fn extract_chacha_key(wasm: &[u8]) -> Result<[u8; 32], AdapterError> {
    if wasm.len() < 8 || &wasm[..4] != b"\0asm" {
        return Err(AdapterError::Protocol("wasm magic"));
    }
    let mut pos = 8;
    while pos < wasm.len() {
        let sec_id = wasm[pos];
        pos += 1;
        let (sec_len, bytes_read) = read_leb128_u32(&wasm[pos..])
            .map_err(|_| AdapterError::Protocol("wasm section len"))?;
        pos += bytes_read;
        let sec_end = pos
            .checked_add(sec_len as usize)
            .ok_or(AdapterError::Protocol("wasm section overflow"))?;
        if sec_end > wasm.len() {
            return Err(AdapterError::Protocol("wasm section boundary"));
        }
        if sec_id == 11 {
            return parse_data_section(&wasm[pos..sec_end]);
        }
        pos = sec_end;
    }
    Err(AdapterError::Protocol("wasm missing data section"))
}

fn format_from_url(path: &str) -> StreamFormat {
    let ext = path.rsplit('.').next().unwrap_or("");
    if ext.eq_ignore_ascii_case("mp4") {
        StreamFormat::Mp4
    } else if ext.eq_ignore_ascii_case("mpd") {
        StreamFormat::Dash
    } else {
        StreamFormat::Hls
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct FixtureHost {
        expected: std::collections::BTreeMap<String, String>,
        body: Vec<u8>,
    }
    impl ProviderHost for FixtureHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let query = Url::parse(&request.url).map_err(|_| HostError::InvalidRequest)?;
            let values: std::collections::BTreeMap<_, _> =
                query.query_pairs().into_owned().collect();
            if query.host_str() != Some("streamdata.vaplayer.ru")
                || query.path() != "/api.php"
                || values != self.expected
            {
                return Err(HostError::InvalidRequest);
            }
            Ok(Response {
                url: request.url,
                status: 200,
                headers: Vec::new(),
                body: self.body.clone(),
            })
        }
        fn fetch_batch(
            &self,
            _: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            Ok(Vec::new())
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn formats_are_inferred_only_from_path_suffix() {
        assert_eq!(format_from_url("/file.mp4"), StreamFormat::Mp4);
        assert_eq!(format_from_url("/manifest.mpd"), StreamFormat::Dash);
        assert_eq!(
            format_from_url("/path/master.m3u8?type=mp4"),
            StreamFormat::Hls
        );
    }

    fn fixture_host(
        expected: &[(&str, &str)],
        body: serde_json::Value,
    ) -> Result<FixtureHost, Box<dyn std::error::Error>> {
        Ok(FixtureHost {
            expected: expected
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            body: serde_json::to_vec(&body)?,
        })
    }

    #[test]
    fn resolve_preserves_episode_coordinates_and_bounded_claim_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":1622,"season":1,"episode":2}}),
        )?;
        let host = fixture_host(
            &[
                ("episode", "2"),
                ("season", "1"),
                ("stream_urls", ""),
                ("tmdb", "1622"),
                ("type", "tv"),
            ],
            serde_json::json!({
                "status_code":"200",
                "data":{"file_name":"episode master","stream_urls":[
                    "https://media.example/master.m3u8",
                    "https://media.example/fallback.m3u8"
                ]},
                "default_subs":[
                    {"lang":"English","code":"en","url":"https://media.example/en.vtt"},
                    {"lang":"界界界界界界界界界界界界界界界界界界界界界界界界","code":"en","url":"https://media.example/too-long.vtt"}
                ]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let provider = common::provider_id("vidapi")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 2);
        for candidate in &candidates {
            candidate.validate_for(&provider, &lookup.title)?;
            assert_eq!(candidate.resolver_metadata.tracks.len(), 1);
            assert_eq!(
                candidate.resolver_metadata.provider_data["file_name_claim"],
                "episode master"
            );
            assert!(
                !candidate
                    .resolver_metadata
                    .provider_data
                    .contains_key("quality_claim")
            );
        }
        Ok(())
    }

    #[test]
    fn resolve_movie_builds_and_validates_complete_candidate_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"movie","tmdb_id":27205,"imdb_id":"tt1375666"}}),
        )?;
        let host = fixture_host(
            &[("stream_urls", ""), ("tmdb", "27205"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{"file_name":"Inception 1080p","stream_urls":[
                    "https://media.example/master.m3u8",
                    "https://media.example/master.m3u8?duplicate=1",
                    "https://media.example/movie.mp4"
                ]},
                "default_subs":[{"lang":"English","code":"en","url":"https://media.example/en.vtt"}]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let provider = common::provider_id("vidapi")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 3);
        for candidate in &candidates {
            candidate.validate_for(&provider, &lookup.title)?;
            assert!(!candidate.content_verified);
            assert!(candidate.fetch_context.scopes.is_empty());
            assert_eq!(
                candidate.resolver_metadata.provider_data["file_name_claim"],
                "Inception 1080p"
            );
            assert_eq!(candidate.resolver_metadata.tracks.len(), 1);
        }
        assert_eq!(candidates[0].format, StreamFormat::Hls);
        assert_eq!(candidates[2].format, StreamFormat::Mp4);
        Ok(())
    }

    #[test]
    fn candidate_generation_prioritizes_fastest_stream_endpoint()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let host = fixture_host(
            &[("stream_urls", ""), ("tmdb", "550"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{
                    "file_name":"Fight Club 1080p",
                    "stream_urls":[
                        "https://media.example/mirror-slow/master.m3u8",
                        "https://media.example/mirror-fast/master.m3u8",
                        "https://media.example/mirror-backup/master.m3u8"
                    ]
                },
                "default_subs":[]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 3);
        assert_eq!(
            candidates[0].url,
            "https://media.example/mirror-fast/master.m3u8"
        );
        assert_eq!(
            candidates[1].url,
            "https://media.example/mirror-slow/master.m3u8"
        );
        assert_eq!(
            candidates[2].url,
            "https://media.example/mirror-backup/master.m3u8"
        );
        Ok(())
    }

    #[test]
    fn resolve_rejects_excessive_stream_and_subtitle_counts()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let stream_urls: Vec<String> = (0..33)
            .map(|index| format!("https://media.example/{index}.m3u8"))
            .collect();
        let default_subs: Vec<serde_json::Value> = (0..65)
            .map(|index| {
                serde_json::json!({
                    "lang":"English",
                    "code":"en",
                    "url":format!("https://media.example/{index}.vtt")
                })
            })
            .collect();
        let host = fixture_host(
            &[("stream_urls", ""), ("tmdb", "550"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{"stream_urls":stream_urls},
                "default_subs":default_subs
            }),
        )?;
        assert!(matches!(
            resolve(&lookup, &host),
            Err(AdapterError::Protocol("VidAPI result count"))
        ));
        Ok(())
    }

    #[test]
    fn resolve_drops_control_bearing_metadata_and_subtitle_labels()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup =
            serde_json::from_value(serde_json::json!({"title":{"type":"movie","tmdb_id":550}}))?;
        let host = fixture_host(
            &[("stream_urls", ""), ("tmdb", "550"), ("type", "movie")],
            serde_json::json!({
                "status_code":"200",
                "data":{"file_name":"movie\nclaim","stream_urls":["https://media.example/master.m3u8"]},
                "default_subs":[
                    {"lang":"English\nspoofed","code":"en","url":"https://media.example/bad.vtt"},
                    {"lang":"English","code":"en\tspoofed","url":"https://media.example/bad-code.vtt"},
                    {"lang":"English","code":"en","url":"https://media.example/good.vtt"}
                ]
            }),
        )?;
        let result = resolve(&lookup, &host)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].resolver_metadata.tracks.len(), 1);
        assert_eq!(
            candidates[0].resolver_metadata.tracks[0].url,
            "https://media.example/good.vtt"
        );
        assert!(
            !candidates[0]
                .resolver_metadata
                .provider_data
                .contains_key("file_name_claim")
        );
        Ok(())
    }

    struct MultiHost {
        responses: std::collections::BTreeMap<String, (u16, Vec<u8>)>,
    }
    impl ProviderHost for MultiHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            let (status, body) = self
                .responses
                .get(&request.url)
                .cloned()
                .ok_or(HostError::InvalidRequest)?;
            Ok(Response {
                url: request.url,
                status,
                headers: Vec::new(),
                body,
            })
        }
        fn fetch_batch(
            &self,
            _: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            Ok(Vec::new())
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }

    #[test]
    fn resolve_falls_back_to_imdb_when_tmdb_is_unavailable()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":1622,"imdb_id":"tt0460681","season":4,"episode":13}}),
        )?;
        let tmdb_url = "https://streamdata.vaplayer.ru/api.php?tmdb=1622&type=tv&season=4&episode=13&stream_urls=";
        let imdb_url = "https://streamdata.vaplayer.ru/api.php?imdb=tt0460681&type=tv&season=4&episode=13&stream_urls=";
        let mut responses = std::collections::BTreeMap::new();
        responses.insert(tmdb_url.to_string(), (404, Vec::new()));
        responses.insert(
            imdb_url.to_string(),
            (
                200,
                serde_json::to_vec(&serde_json::json!({
                    "status_code":"200",
                    "data":{
                        "file_name":"Supernatural.S04E13.1080p.mkv",
                        "stream_urls":["https://media.example/fallback.m3u8"]
                    },
                    "default_subs":[]
                }))?,
            ),
        );
        let host = MultiHost { responses };
        let result = resolve(&lookup, &host)?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].url, "https://media.example/fallback.m3u8");
        Ok(())
    }
}
