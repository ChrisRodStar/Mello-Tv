use std::{
    collections::BTreeMap,
    fmt::Write,
    pin::Pin,
    sync::Mutex,
    time::{Duration, Instant},
};

use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use stream_core::providers::{FetchContext, ProbedStream, Selection, StreamFormat};
use thiserror::Error;
use url::Url;

use super::hls::HlsManifestRewriter;

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36";
const MAX_SESSIONS: usize = 256;
const MAX_RESOURCES_PER_SESSION: usize = 16_384;
const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("playback session not found")]
    SessionNotFound,
    #[error("invalid stream token")]
    InvalidToken,
    #[error("stream resource not found")]
    ResourceNotFound,
    #[error("no backup source available")]
    NoBackupAvailable,
    #[error("upstream request failed: {0}")]
    Upstream(String),
    #[error("upstream returned status {0}")]
    UpstreamStatus(u16),
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),
}

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

pub enum RelayResponse {
    Manifest(String),
    Binary {
        status: u16,
        content_type: String,
        content_length: Option<u64>,
        content_range: Option<String>,
        accept_ranges: Option<String>,
        stream: ByteStream,
    },
}

struct SessionState {
    token: String,
    active: ProbedStream,
    backups: Vec<ProbedStream>,
    backup_index: usize,
    resources: BTreeMap<String, String>,
    created_at: Instant,
}

pub struct MediaRelay {
    sessions: Mutex<BTreeMap<String, SessionState>>,
    client: reqwest::Client,
}

impl Default for MediaRelay {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaRelay {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(6))
            .read_timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            sessions: Mutex::new(BTreeMap::new()),
            client,
        }
    }

    pub fn generate_token() -> String {
        let mut bytes = [0u8; 16];
        let _ = getrandom::fill(&mut bytes);
        let mut out = String::with_capacity(32);
        for b in bytes {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    pub fn resource_id(url: &str) -> String {
        let digest = stream_provider_codecs::sha256(&[url.as_bytes()]);
        let mut out = String::with_capacity(24);
        for b in digest.iter().take(12) {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    pub fn register_session(
        &self,
        session_id: String,
        token: String,
        selection: Selection,
    ) -> bool {
        let Some(selected) = selection.selected else {
            return false;
        };
        let backups: Vec<ProbedStream> = selection
            .backups
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.candidate)
            .collect();

        if let Ok(mut map) = self.sessions.lock() {
            if map.len() >= MAX_SESSIONS
                && !map.contains_key(&session_id)
                && let Some(oldest_key) = map
                    .iter()
                    .min_by_key(|(_, s)| s.created_at)
                    .map(|(k, _)| k.clone())
            {
                map.remove(&oldest_key);
            }
            map.insert(
                session_id,
                SessionState {
                    token,
                    active: selected.candidate,
                    backups,
                    backup_index: 0,
                    resources: BTreeMap::new(),
                    created_at: Instant::now(),
                },
            );
            true
        } else {
            false
        }
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .map(|map| map.contains_key(session_id))
            .unwrap_or(false)
    }

    pub fn token_for(&self, session_id: &str) -> String {
        if let Ok(map) = self.sessions.lock()
            && let Some(session) = map.get(session_id)
        {
            return session.token.clone();
        }
        // Deterministic fallback token for sessions without an active relay entry (e.g. unit tests)
        let digest = stream_provider_codecs::sha256(&[b"stream-token:", session_id.as_bytes()]);
        let mut out = String::with_capacity(24);
        for b in digest.iter().take(12) {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    pub fn active_info(&self, session_id: &str) -> Option<(StreamFormat, Vec<i64>)> {
        let map = self.sessions.lock().ok()?;
        let session = map.get(session_id)?;
        let format = match session.active.observation.format {
            StreamFormat::Mp4 => StreamFormat::Mp4,
            _ => StreamFormat::Hls,
        };
        let mut heights: Vec<i64> = session
            .active
            .observation
            .playlist_metadata
            .variants
            .iter()
            .chain(session.active.source.resolver_metadata.variants.iter())
            .filter_map(|v| v.height)
            .filter(|h| *h > 0 && *h <= 16384)
            .collect();
        if let Some(v) = &session.active.observation.variant
            && let Some(h) = v.height
            && h > 0
            && h <= 16384
        {
            heights.push(h);
        }
        heights.sort_unstable_by(|a, b| b.cmp(a));
        heights.dedup();
        if format != StreamFormat::Hls {
            heights.clear();
        }
        Some((format, heights))
    }

    /// Height of the measured active rendition, when the source exposes one.
    pub fn active_height(&self, session_id: &str) -> Option<i64> {
        let map = self.sessions.lock().ok()?;
        map.get(session_id)?
            .active
            .observation
            .variant
            .as_ref()
            .and_then(|variant| variant.height)
            .filter(|height| *height > 0 && *height <= 16384)
    }

    /// Resolve public playback details from a measured selection before it is
    /// installed into a session.
    pub fn selection_info(selection: &Selection) -> Option<(StreamFormat, Vec<i64>, i64)> {
        let active = &selection.selected.as_ref()?.candidate;
        let format = match active.observation.format {
            StreamFormat::Mp4 => StreamFormat::Mp4,
            _ => StreamFormat::Hls,
        };
        let mut heights: Vec<i64> = active
            .observation
            .playlist_metadata
            .variants
            .iter()
            .chain(active.source.resolver_metadata.variants.iter())
            .filter_map(|variant| variant.height)
            .filter(|height| *height > 0 && *height <= 16384)
            .collect();
        if let Some(height) = active
            .observation
            .variant
            .as_ref()
            .and_then(|variant| variant.height)
            .filter(|height| *height > 0 && *height <= 16384)
        {
            heights.push(height);
        }
        heights.sort_unstable_by(|a, b| b.cmp(a));
        heights.dedup();
        if format != StreamFormat::Hls {
            heights.clear();
        }
        let active_height = active
            .observation
            .variant
            .as_ref()
            .and_then(|variant| variant.height)
            .filter(|height| *height > 0 && *height <= 16384)?;
        Some((format, heights, active_height))
    }

    pub fn activate_backup(&self, session_id: &str) -> Result<String, RelayError> {
        let mut map = self
            .sessions
            .lock()
            .map_err(|_| RelayError::SessionNotFound)?;
        let session = map.get_mut(session_id).ok_or(RelayError::SessionNotFound)?;
        if session.backup_index >= session.backups.len() {
            return Err(RelayError::NoBackupAvailable);
        }
        session.active = session.backups[session.backup_index].clone();
        session.backup_index += 1;
        session.resources.clear();
        let new_token = Self::generate_token();
        session.token = new_token.clone();
        Ok(new_token)
    }

    pub fn replace_selection(&self, session_id: &str, selection: Selection) -> Option<String> {
        let new_token = Self::generate_token();
        if self.register_session(session_id.to_owned(), new_token.clone(), selection) {
            Some(new_token)
        } else {
            None
        }
    }

    pub fn revoke_session(&self, session_id: &str) {
        if let Ok(mut map) = self.sessions.lock() {
            map.remove(session_id);
        }
    }

    pub async fn fetch_entry(
        &self,
        session_id: &str,
        token: &str,
        range_header: Option<&str>,
    ) -> Result<RelayResponse, RelayError> {
        let (upstream_url, context) = {
            let map = self
                .sessions
                .lock()
                .map_err(|_| RelayError::SessionNotFound)?;
            let session = map.get(session_id).ok_or(RelayError::SessionNotFound)?;
            if session.token != token {
                return Err(RelayError::InvalidToken);
            }
            (
                session.active.source.url.clone(),
                session.active.source.fetch_context.clone(),
            )
        };

        self.proxy_url(session_id, token, &upstream_url, &context, range_header)
            .await
    }

    pub async fn fetch_resource(
        &self,
        session_id: &str,
        resource_id: &str,
        token: &str,
        range_header: Option<&str>,
    ) -> Result<RelayResponse, RelayError> {
        let (upstream_url, context) = {
            let map = self
                .sessions
                .lock()
                .map_err(|_| RelayError::SessionNotFound)?;
            let session = map.get(session_id).ok_or(RelayError::SessionNotFound)?;
            if session.token != token {
                return Err(RelayError::InvalidToken);
            }
            let url = session
                .resources
                .get(resource_id)
                .cloned()
                .ok_or(RelayError::ResourceNotFound)?;
            (url, session.active.source.fetch_context.clone())
        };

        self.proxy_url(session_id, token, &upstream_url, &context, range_header)
            .await
    }

    async fn proxy_url(
        &self,
        session_id: &str,
        token: &str,
        upstream_url: &str,
        context: &FetchContext,
        range_header: Option<&str>,
    ) -> Result<RelayResponse, RelayError> {
        let mut req = self.client.get(upstream_url);
        req = req.header("User-Agent", DEFAULT_USER_AGENT);
        req = req.header("Accept", "*/*");

        if let Some(public) = &context.public_resources {
            if !public.referer.is_empty() {
                req = req.header("Referer", &public.referer);
            }
            if !public.origin.is_empty() {
                req = req.header("Origin", &public.origin);
            }
        }

        for scope in &context.scopes {
            for (k, v) in &scope.headers {
                req = req.header(k, v);
            }
            if !scope.cookies.is_empty() {
                req = req.header("Cookie", &scope.cookies);
            }
        }

        let is_likely_manifest = upstream_url.contains(".m3u8");
        if let Some(range) = range_header
            && !is_likely_manifest
        {
            req = req.header("Range", range);
        }

        let res = req
            .send()
            .await
            .map_err(|e| RelayError::Upstream(e.to_string()))?;

        let status = res.status().as_u16();
        if !res.status().is_success() {
            return Err(RelayError::UpstreamStatus(status));
        }

        let final_url = res.url().clone();
        let headers = res.headers().clone();
        let content_type_header = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let content_length = headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let content_range = headers
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let accept_ranges = headers
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        let mut byte_stream = res.bytes_stream();
        let first_chunk = match byte_stream.next().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(e)) => return Err(RelayError::Upstream(e.to_string())),
            None => Bytes::new(),
        };

        if looks_like_hls_manifest(&first_chunk) {
            let mut full = BytesMut::from(first_chunk.as_ref());
            while let Some(next) = byte_stream.next().await {
                let chunk = next.map_err(|e| RelayError::Upstream(e.to_string()))?;
                if full.len() + chunk.len() > MAX_MANIFEST_BYTES {
                    return Err(RelayError::InvalidManifest(
                        "manifest exceeds byte limit".into(),
                    ));
                }
                full.extend_from_slice(&chunk);
            }

            let mut discovered = Vec::new();
            let rewritten = HlsManifestRewriter::rewrite(
                &full,
                &final_url,
                session_id,
                token,
                |resolved_url| {
                    let rid = Self::resource_id(&resolved_url);
                    discovered.push((rid.clone(), resolved_url));
                    rid
                },
            )
            .map_err(|e| RelayError::InvalidManifest(e.to_string()))?;

            if let Ok(mut map) = self.sessions.lock()
                && let Some(session) = map.get_mut(session_id)
            {
                for (rid, url) in discovered {
                    if session.resources.len() < MAX_RESOURCES_PER_SESSION
                        || session.resources.contains_key(&rid)
                    {
                        session.resources.insert(rid, url);
                    }
                }
            }

            return Ok(RelayResponse::Manifest(rewritten));
        }

        let sniffed_content_type =
            sniff_media_content_type(&first_chunk, &final_url, &content_type_header);
        let combined = futures_util::stream::once(async move { Ok(first_chunk) })
            .chain(byte_stream.map(|item| item.map_err(std::io::Error::other)));

        Ok(RelayResponse::Binary {
            status,
            content_type: sniffed_content_type,
            content_length,
            content_range,
            accept_ranges: accept_ranges.or_else(|| Some("bytes".into())),
            stream: Box::pin(combined),
        })
    }
}

fn looks_like_hls_manifest(bytes: &[u8]) -> bool {
    let prefix = &bytes[..bytes.len().min(64)];
    let trimmed = prefix
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(prefix)
        .trim_ascii_start();
    trimmed.starts_with(b"#EXTM3U")
}

fn sniff_media_content_type(first_chunk: &[u8], url: &Url, declared: &str) -> String {
    if first_chunk.first() == Some(&0x47)
        && (first_chunk.len() < 189 || first_chunk.get(188) == Some(&0x47))
    {
        return "video/mp2t".into();
    }
    if first_chunk.len() >= 8
        && matches!(
            &first_chunk[4..8],
            b"ftyp" | b"styp" | b"moof" | b"sidx" | b"mdat"
        )
    {
        return "video/mp4".into();
    }
    let trimmed = first_chunk
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(first_chunk)
        .trim_ascii_start();
    if trimmed.starts_with(b"WEBVTT") || url.path().ends_with(".vtt") {
        return "text/vtt; charset=utf-8".into();
    }
    declared.to_owned()
}
