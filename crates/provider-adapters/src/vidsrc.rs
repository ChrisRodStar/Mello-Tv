//! VidSrc's static embed/gate/player/source protocol. Every endpoint is bound to
//! requested coordinates; decoded programs are fingerprinted, never executed.

use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, fetch, get},
    destination::public_url,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use regex::Regex;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, ProviderTitle,
        PublicResourceContext, StreamFormat,
    },
};
use stream_provider_codecs::vidsrc::decrypt_streams;
use url::Url;

const ORIGIN: &str = "https://vidsrc.sh";
const PLAYER: &str = "https://stellarconductornexus.com";
const DATA: &str = "https://data.vidsrc.sh";
type Query = BTreeMap<String, Vec<String>>;

#[derive(Deserialize)]
struct Gate {
    src: String,
}
#[derive(Deserialize)]
struct Landing {
    #[serde(rename = "playerUrl")]
    player_url: String,
}
#[derive(Deserialize)]
struct PlayerConfig {
    #[serde(rename = "mediaType")]
    media_type: String,
    #[serde(rename = "idType")]
    id_type: String,
    #[serde(rename = "mediaId")]
    media_id: String,
    tmdb: String,
    season: Option<i64>,
    episode: Option<i64>,
    #[serde(default)]
    api: String,
    #[serde(default, rename = "streamBase")]
    stream_base: String,
    #[serde(rename = "apiToken")]
    api_token: String,
}
#[derive(Deserialize)]
struct SourceEnvelope {
    status_code: String,
    data: SourceData,
    #[serde(default)]
    vs: Value,
}
#[derive(Deserialize)]
struct SourceData {
    #[serde(default)]
    imdb_id: String,
    #[serde(default)]
    season: Option<Value>,
    #[serde(default)]
    episode: Option<Value>,
    #[serde(default)]
    stream_urls: Option<Value>,
}
#[derive(Deserialize)]
struct Protection {
    w: i64,
    #[serde(default)]
    wasm_url: String,
    #[serde(default)]
    wasm: String,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let path = embed_path(&lookup.title)?;
    let embed_url = format!("{ORIGIN}/embed{path}");
    let embed = fetch(host, get(&embed_url, &format!("{ORIGIN}/"), 256 * 1024))?;
    let gate_url = gate_endpoint(&embed.body, &lookup.title)?;
    let gate = fetch(host, get(gate_url, &embed_url, 8 * 1024))?;
    let gate: Gate = common::json(&gate.body, 8 * 1024, "VidSrc gate envelope")?;
    let landing_url = player_endpoint(&gate.src, &format!("/embed{path}"))?;
    let landing = fetch(host, get(&landing_url, &format!("{ORIGIN}/"), 256 * 1024))?;
    let config: Landing = parse_config(&landing.body, "CFG")?;
    let player_url = player_endpoint(&config.player_url, &format!("/embed/player{path}"))?;
    let player = fetch(host, get(&player_url, &format!("{PLAYER}/"), 256 * 1024))?;
    let config: PlayerConfig = parse_config(&player.body, "CONFIG")?;
    let source_url = source_endpoint(config, &lookup.title)?;
    let response = fetch(host, get(source_url, &format!("{PLAYER}/"), 256 * 1024))?;
    sources(&response.body, &lookup.title, host)
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

fn patterns() -> Result<&'static (Regex, Regex, Regex), AdapterError> {
    static PATTERNS: OnceLock<Result<(Regex, Regex, Regex), regex::Error>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            Ok((
                Regex::new(r"(?is)<iframe\b[^>]*>")?,
                Regex::new(r#"(?i)\b(id|data-api)\s*=\s*["']([^"']*)["']"#)?,
                Regex::new(r"(?s)window\.(CFG|CONFIG)\s*=\s*(\{[^<]*?\})\s*;")?,
            ))
        })
        .as_ref()
        .map_err(|_| AdapterError::Protocol("static parser"))
}

fn parse_config<T: DeserializeOwned>(body: &[u8], name: &str) -> Result<T, AdapterError> {
    if body.len() > 256 * 1024 {
        return Err(AdapterError::Protocol("player byte limit"));
    }
    let source =
        std::str::from_utf8(body).map_err(|_| AdapterError::Protocol("player encoding"))?;
    let mut selected = None;
    for captures in patterns()?.2.captures_iter(source) {
        if captures.get(1).is_some_and(|value| value.as_str() == name) {
            if selected.is_some() {
                return Err(AdapterError::Protocol("ambiguous player configuration"));
            }
            selected = captures.get(2).map(|value| value.as_str());
        }
    }
    common::json(
        selected
            .ok_or(AdapterError::Protocol("missing player configuration"))?
            .as_bytes(),
        256 * 1024,
        "player JSON configuration",
    )
}

fn query(url: &Url) -> Result<Query, AdapterError> {
    let raw = url.query().unwrap_or("");
    if raw.contains(';') {
        return Err(AdapterError::Protocol("ambiguous query delimiter"));
    }
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if bytes
                .get(index + 1..index + 3)
                .is_none_or(|digits| !digits.iter().all(u8::is_ascii_hexdigit))
            {
                return Err(AdapterError::Protocol("malformed query escape"));
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    let mut query = Query::new();
    for (name, value) in url.query_pairs() {
        query
            .entry(name.into_owned())
            .or_default()
            .push(value.into_owned());
    }
    Ok(query)
}
fn single(query: &Query, key: &str, expected: &str) -> bool {
    query.get(key).is_some_and(|values| {
        values.len() == 1 && values.first().is_some_and(|value| value == expected)
    })
}
fn coordinates(query: &Query, title: &ProviderTitle, identifier: &str) -> bool {
    single(
        query,
        "type",
        if title.media_type == MediaType::Movie {
            "movie"
        } else {
            "tv"
        },
    ) && single(query, identifier, &title.tmdb_id.get().to_string())
        && match title.media_type {
            MediaType::Movie => !query.contains_key("season") && !query.contains_key("episode"),
            MediaType::Tv => {
                title
                    .season
                    .is_some_and(|season| single(query, "season", &season.to_string()))
                    && title
                        .episode
                        .is_some_and(|episode| single(query, "episode", &episode.to_string()))
            }
        }
}

pub fn gate_endpoint(body: &[u8], title: &ProviderTitle) -> Result<String, AdapterError> {
    let source = std::str::from_utf8(body).map_err(|_| AdapterError::Protocol("embed encoding"))?;
    if body.len() > 256 * 1024 {
        return Err(AdapterError::Protocol("embed byte limit"));
    }
    let (frames, attributes, _) = patterns()?;
    let mut endpoint = None;
    for (count, frame) in frames.find_iter(source).enumerate() {
        if count >= 64 {
            return Err(AdapterError::Protocol("embed frame count"));
        }
        let mut fields = BTreeMap::new();
        for captures in attributes.captures_iter(frame.as_str()) {
            let name = captures
                .get(1)
                .ok_or(AdapterError::Protocol("frame attribute"))?
                .as_str()
                .to_ascii_lowercase();
            let value = html_escape::decode_html_entities(
                captures
                    .get(2)
                    .ok_or(AdapterError::Protocol("frame attribute"))?
                    .as_str(),
            )
            .into_owned();
            if fields.insert(name, value).is_some() {
                return Err(AdapterError::Protocol("duplicate frame attribute"));
            }
        }
        if fields.get("id").map(String::as_str) != Some("player_iframe") {
            continue;
        }
        if endpoint.is_some() {
            return Err(AdapterError::Protocol("ambiguous player frame"));
        }
        endpoint = fields.remove("data-api");
    }
    let endpoint = endpoint.ok_or(AdapterError::Protocol("missing gate endpoint"))?;
    if !endpoint.starts_with('/') || endpoint.starts_with("//") || endpoint.contains(['\\', '#']) {
        return Err(AdapterError::Protocol("gate endpoint"));
    }
    let url = public_url(&format!("{ORIGIN}{endpoint}"))?;
    if url.path() != "/vs_src.php" {
        return Err(AdapterError::Protocol("gate path"));
    }
    let values = query(&url)?;
    let expected = if title.media_type == MediaType::Tv {
        4
    } else {
        2
    };
    if values.len() != expected || !coordinates(&values, title, "id") {
        return Err(AdapterError::Protocol("gate title coordinates"));
    }
    Ok(url.into())
}

fn player_endpoint(raw: &str, path: &str) -> Result<String, AdapterError> {
    if raw.len() > 8192
        || raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return Err(AdapterError::Protocol("player endpoint"));
    }
    let base = Url::parse(PLAYER).map_err(|_| AdapterError::Protocol("player origin"))?;
    let url = base
        .join(raw)
        .map_err(|_| AdapterError::Protocol("player endpoint"))?;
    public_url(url.as_str())?;
    if !common::same_origin(&url, PLAYER) || url.path() != path {
        return Err(AdapterError::Protocol("player coordinates"));
    }
    let values = query(&url)?;
    if values.len() != 1
        || values.get("vs").is_none_or(|values| {
            values.len() != 1
                || values
                    .first()
                    .is_none_or(|value| value.is_empty() || value.len() > 2048)
        })
    {
        return Err(AdapterError::Protocol("player gate token"));
    }
    Ok(url.into())
}

fn source_endpoint(config: PlayerConfig, title: &ProviderTitle) -> Result<String, AdapterError> {
    if config.media_type
        != if title.media_type == MediaType::Movie {
            "movie"
        } else {
            "tv"
        }
        || config.id_type != "tmdb"
        || config.media_id != title.tmdb_id.get().to_string()
        || config.tmdb != config.media_id
    {
        return Err(AdapterError::Protocol("player title identifiers"));
    }
    match title.media_type {
        MediaType::Tv if config.season != title.season || config.episode != title.episode => {
            return Err(AdapterError::Protocol("player episode coordinates"));
        }
        MediaType::Movie if config.season.is_some() || config.episode.is_some() => {
            return Err(AdapterError::Protocol("movie episode coordinates"));
        }
        _ => {}
    }
    if title.media_type == MediaType::Tv && !config.api.is_empty() {
        return Err(AdapterError::Protocol("TV API configuration"));
    }
    let mut url = public_url(if title.media_type == MediaType::Tv {
        &config.stream_base
    } else {
        &config.api
    })?;
    if !common::same_origin(&url, DATA) || url.path() != "/api.php" {
        return Err(AdapterError::Protocol("stream API endpoint"));
    }
    let mut values = query(&url)?;
    if title.media_type == MediaType::Tv {
        if values.len() != 2
            || !single(&values, "type", "tv")
            || !single(&values, "tmdb", &title.tmdb_id.get().to_string())
        {
            return Err(AdapterError::Protocol("TV stream base"));
        }
        values.insert(
            "season".into(),
            vec![title.season.ok_or(AdapterError::Input)?.to_string()],
        );
        values.insert(
            "episode".into(),
            vec![title.episode.ok_or(AdapterError::Input)?.to_string()],
        );
        values.insert("stream_urls".into(), vec![String::new()]);
    }
    let expected = if title.media_type == MediaType::Tv {
        5
    } else {
        3
    };
    if values.len() != expected
        || !coordinates(&values, title, "tmdb")
        || !single(&values, "stream_urls", "")
    {
        return Err(AdapterError::Protocol("stream API coordinates"));
    }
    if config.api_token.is_empty()
        || config.api_token.len() > 4096
        || config.api_token.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(AdapterError::Protocol("API token"));
    }
    values.insert("api_token".into(), vec![config.api_token]);
    {
        let mut pairs = url.query_pairs_mut();
        pairs.clear();
        for (name, values) in values {
            for value in values {
                pairs.append_pair(&name, &value);
            }
        }
    }
    Ok(url.into())
}

fn decode_sources(
    envelope: &SourceEnvelope,
    host: &dyn ProviderHost,
) -> Result<Vec<String>, AdapterError> {
    let streams = envelope
        .data
        .stream_urls
        .as_ref()
        .ok_or(AdapterError::Unavailable("VidSrc stream list"))?;
    if let Some(encoded) = streams.as_str() {
        let protection = Protection::deserialize(&envelope.vs)
            .map_err(|_| AdapterError::Protocol("static decryptor envelope"))?;
        if protection.w < 0 {
            return Err(AdapterError::Protocol("static module window"));
        }
        let module = if !protection.wasm_url.is_empty() {
            let url = public_url(&protection.wasm_url)?;
            if !common::same_origin(&url, DATA) || url.path() != "/wasm.php" {
                return Err(AdapterError::Protocol("static module endpoint"));
            }
            let values = query(&url)?;
            if values.len() != 2
                || !single(&values, "w", &protection.w.to_string())
                || values.get("_").is_none_or(|values| values.len() != 1)
            {
                return Err(AdapterError::Protocol("static module window"));
            }
            fetch(host, get(url.as_str(), &format!("{PLAYER}/"), 128 * 1024))?.body
        } else {
            if protection.wasm.len() > 192 * 1024 {
                return Err(AdapterError::Protocol("static module byte limit"));
            }
            let encoded: Vec<_> = protection
                .wasm
                .bytes()
                .filter(|byte| !matches!(byte, b'\r' | b'\n'))
                .collect();
            STANDARD
                .decode(encoded)
                .map_err(|_| AdapterError::Protocol("static module encoding"))?
        };
        decrypt_streams(encoded, &module)
            .map_err(|_| AdapterError::Protocol("unrecognized static cipher"))
    } else {
        Vec::<String>::deserialize(streams)
            .map_err(|_| AdapterError::Protocol("stream list schema"))
    }
}

fn val_to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub fn sources(
    body: &[u8],
    title: &ProviderTitle,
    host: &dyn ProviderHost,
) -> Result<AdapterResult, AdapterError> {
    let envelope: SourceEnvelope = common::json(body, 256 * 1024, "VidSrc stream envelope")?;
    match envelope.status_code.as_str() {
        "200" => {}
        "404" | "410" => return Err(AdapterError::Unavailable("VidSrc stream API")),
        "401" | "403" | "429" | "500" | "503" => {
            return Err(AdapterError::Host(
                stream_provider_sdk::http::HostError::UpstreamFailure,
            ));
        }
        _ => return Err(AdapterError::Protocol("stream API status")),
    }
    if !title.imdb_id.is_empty()
        && !envelope.data.imdb_id.is_empty()
        && title.imdb_id != envelope.data.imdb_id
    {
        return Err(AdapterError::Protocol("source IMDb cross-ID"));
    }
    if title.media_type == MediaType::Tv {
        let season_matches =
            envelope.data.season.as_ref().map(val_to_str) == title.season.map(|v| v.to_string());
        let episode_matches =
            envelope.data.episode.as_ref().map(val_to_str) == title.episode.map(|v| v.to_string());
        if !season_matches || !episode_matches {
            return Err(AdapterError::Protocol("source episode coordinates"));
        }
    } else if envelope.data.season.is_some() || envelope.data.episode.is_some() {
        return Err(AdapterError::Protocol("movie source coordinates"));
    }
    let streams = decode_sources(&envelope, host)?;
    if streams.is_empty() {
        return Err(AdapterError::Unavailable("VidSrc stream list"));
    }
    if streams.len() > 16 {
        return Err(AdapterError::Protocol("stream count"));
    }
    let metadata: BTreeMap<String, Value> = common::json(body, 256 * 1024, "source metadata")?;
    let mut origins = BTreeSet::new();
    for source in &streams {
        if let Ok(url) = public_url(source)
            && url.scheme() == "https"
        {
            origins.insert(url.origin().ascii_serialization());
        }
    }
    let mut token_results = BTreeMap::new();
    let start = host.now_unix_ms()?;
    let origins: Vec<_> = origins.into_iter().collect();
    for group in origins.chunks(3) {
        if host.now_unix_ms()?.saturating_sub(start) >= 3000 {
            break;
        }
        let requests = group
            .iter()
            .map(|origin| {
                let mut request = get(
                    format!("{origin}/generate.php"),
                    &format!("{PLAYER}/"),
                    8 * 1024,
                );
                request.timeout_ms = 1000;
                request
            })
            .collect();
        match batch(host, requests) {
            Ok(responses) => {
                for (origin, response) in group.iter().zip(responses) {
                    token_results.insert(
                        origin.clone(),
                        response.and_then(|response| parse_token(&response.body)),
                    );
                }
            }
            Err(error) => {
                if matches!(
                    error,
                    AdapterError::Host(stream_provider_sdk::http::HostError::Cancelled)
                ) {
                    return Err(error);
                }
                break;
            }
        }
    }
    let provider = common::provider_id("vidsrc")?;
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    for mut source in streams {
        if !seen.insert(source.clone()) {
            continue;
        }
        let url = match public_url(&source) {
            Ok(url) if url.scheme() == "https" => url,
            _ => {
                issues.push(common::issue(
                    &AdapterError::Protocol("source URL"),
                    "sources",
                ));
                continue;
            }
        };
        let origin = url.origin().ascii_serialization();
        let token = match token_results.get(&origin) {
            Some(Ok(token)) => token.as_str(),
            Some(Err(error)) => {
                issues.push(common::issue(error, "media_token"));
                ""
            }
            None => "",
        };
        if source.contains("__TOKEN__") {
            if token.is_empty() {
                continue;
            }
            let encoded: String = url::form_urlencoded::byte_serialize(token.as_bytes()).collect();
            source = source.replace("__TOKEN__", &encoded);
        } else if !token.is_empty() {
            source.push(if source.contains('?') { '&' } else { '?' });
            source.push_str("token=");
            source.extend(url::form_urlencoded::byte_serialize(token.as_bytes()));
        }
        let resource = match public_url(&source) {
            Ok(url) if url.origin().ascii_serialization() == origin => url,
            _ => {
                issues.push(common::issue(
                    &AdapterError::Protocol("tokenized source URL"),
                    "sources",
                ));
                continue;
            }
        };
        let path = resource.path().to_ascii_lowercase();
        let format = if path.ends_with(".m3u8") {
            StreamFormat::Hls
        } else if path.ends_with(".mp4") {
            StreamFormat::Mp4
        } else {
            StreamFormat::Unknown
        };
        candidates.push(CandidateStream {
            expires_at: None,
            id: common::hash_id(&[source.as_bytes()]),
            server: resource.host_str().unwrap_or("").into(),
            selected_variant_url: String::new(),
            url: source,
            provider: provider.clone(),
            format,
            title: title.clone(),
            fetch_context: FetchContext {
                scopes: Vec::new(),
                public_resources: Some(PublicResourceContext {
                    origin: String::new(),
                    referer: format!("{PLAYER}/"),
                }),
            },
            resolver_metadata: Metadata {
                provider_data: metadata.clone(),
                ..Metadata::default()
            },
            content_verified: false,
        });
    }
    if candidates.is_empty() {
        return Err(AdapterError::Protocol("VidSrc sources rejected"));
    }
    Ok(common::result(candidates, issues))
}

pub fn parse_token(body: &[u8]) -> Result<String, AdapterError> {
    if body.len() > 8 * 1024 {
        return Err(AdapterError::Protocol("media token byte limit"));
    }
    let raw = std::str::from_utf8(body)
        .map_err(|_| AdapterError::Protocol("media token encoding"))?
        .trim();
    let token = if raw.starts_with('"') {
        common::json(body, 8 * 1024, "media token string")?
    } else if raw.starts_with('{') {
        let fields: BTreeMap<String, Value> = common::json(body, 8 * 1024, "media token object")?;
        let mut token = String::new();
        for name in ["token", "data", "string", "result"] {
            if let Some(value) = fields.get(name) {
                token = value
                    .as_str()
                    .ok_or(AdapterError::Protocol("media token schema"))?
                    .into();
                if !token.is_empty() {
                    break;
                }
            }
        }
        token
    } else {
        raw.into()
    };
    if token.len() > 4096
        || token.chars().any(|ch| {
            matches!(
                ch,
                '\r' | '\n' | '\0' | '<' | '>' | '[' | ']' | '{' | '}' | ' ' | '\t'
            )
        })
    {
        return Err(AdapterError::Protocol("invalid media token"));
    }
    Ok(token)
}
