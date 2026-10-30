use std::{
    collections::BTreeMap,
    fmt::Write,
    pin::Pin,
    sync::{Arc, Mutex},
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
// Players buffer ahead and request segments in bursts; retain activity between requests.
const PLAYBACK_IDLE_GRACE: Duration = Duration::from_secs(180);

type PlaybackActivity = Arc<Mutex<BTreeMap<String, ActivityState>>>;

struct ActivityState {
    last_observed: Instant,
    in_flight: usize,
}

impl ActivityState {
    fn is_active(&self, now: Instant) -> bool {
        self.in_flight > 0
            || now.saturating_duration_since(self.last_observed) < PLAYBACK_IDLE_GRACE
    }
}

struct ActivityGuard {
    activity: PlaybackActivity,
    session_id: String,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        if let Ok(mut activity) = self.activity.lock()
            && let Some(session) = activity.get_mut(&self.session_id)
        {
            session.in_flight = session.in_flight.saturating_sub(1);
            session.last_observed = Instant::now();
        }
    }
}

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
    public_url: Option<String>,
}

pub struct MediaRelay {
    sessions: Mutex<BTreeMap<String, SessionState>>,
    client: reqwest::Client,
    activity: PlaybackActivity,
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
            activity: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Recent playback activity, including response bodies still being delivered.
    /// None means activity could not be inspected; callers must defer a restart.
    pub fn active_streams(&self) -> Option<usize> {
        let now = Instant::now();
        let mut activity = self.activity.lock().ok()?;
        activity.retain(|_, session| session.is_active(now));
        Some(activity.len())
    }

    /// Includes response bodies still being delivered and the buffering grace period.
    pub fn session_active(&self, session_id: &str) -> bool {
        self.activity
            .lock()
            .map(|activity| {
                activity
                    .get(session_id)
                    .is_some_and(|session| session.is_active(Instant::now()))
            })
            .unwrap_or(false)
    }

    pub fn note_playback_activity(&self, session_id: &str) {
        if let Ok(mut activity) = self.activity.lock() {
            let now = Instant::now();
            activity.retain(|_, session| session.is_active(now));
            activity
                .entry(session_id.to_owned())
                .and_modify(|session| {
                    session.last_observed = now;
                })
                .or_insert(ActivityState {
                    last_observed: now,
                    in_flight: 0,
                });
        }
    }

    fn begin_delivery(&self, session_id: &str) -> Result<ActivityGuard, RelayError> {
        let mut activity = self
            .activity
            .lock()
            .map_err(|_| RelayError::SessionNotFound)?;
        let now = Instant::now();
        activity.retain(|_, session| session.is_active(now));
        let session = activity
            .entry(session_id.to_owned())
            .or_insert(ActivityState {
                last_observed: now,
                in_flight: 0,
            });
        session.last_observed = now;
        session.in_flight += 1;
        Ok(ActivityGuard {
            activity: self.activity.clone(),
            session_id: session_id.to_owned(),
        })
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
        self.register_session_inner(session_id, token, selection, None)
    }

    /// Register a remote-library session with an explicitly configured URL.
    /// Existing native/web sessions continue to emit relative resource URLs.
    pub fn register_session_with_public_url(
        &self,
        session_id: String,
        token: String,
        selection: Selection,
        public_url: &str,
    ) -> bool {
        let Ok(parsed) = Url::parse(public_url) else {
            return false;
        };
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return false;
        }
        self.register_session_inner(
            session_id,
            token,
            selection,
            Some(parsed.as_str().trim_end_matches('/').to_owned()),
        )
    }

    fn register_session_inner(
        &self,
        session_id: String,
        token: String,
        selection: Selection,
        public_url: Option<String>,
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
                    public_url,
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
        if let Ok(mut activity) = self.activity.lock()
            && activity
                .get(session_id)
                .is_some_and(|session| session.in_flight == 0)
        {
            activity.remove(session_id);
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
        let activity = self.begin_delivery(session_id)?;
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
            .map_err(|e| RelayError::Upstream(e.without_url().to_string()))?;

        let status = res.status().as_u16();
        if !res.status().is_success() {
            return Err(RelayError::UpstreamStatus(status));
        }

        let final_url = res.url().clone();
        let headers = res.headers();
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
            Some(Err(e)) => return Err(RelayError::Upstream(e.without_url().to_string())),
            None => Bytes::new(),
        };

        if looks_like_hls_manifest(&first_chunk) {
            let mut full = BytesMut::from(first_chunk.as_ref());
            while let Some(next) = byte_stream.next().await {
                let chunk = next.map_err(|e| RelayError::Upstream(e.without_url().to_string()))?;
                if full.len() + chunk.len() > MAX_MANIFEST_BYTES {
                    return Err(RelayError::InvalidManifest(
                        "manifest exceeds byte limit".into(),
                    ));
                }
                full.extend_from_slice(&chunk);
            }

            let mut discovered = Vec::new();
            let public_url = self.sessions.lock().ok().and_then(|map| {
                map.get(session_id)
                    .and_then(|session| session.public_url.clone())
            });
            let rewritten = HlsManifestRewriter::rewrite_with_public_url(
                &full,
                &final_url,
                session_id,
                token,
                public_url.as_deref(),
                |resolved_url| {
                    let mut rid = Self::resource_id(&resolved_url);
                    if public_url.is_some() {
                        // ffmpeg's HLS demuxer checks resource URL extensions before
                        // fetching. Keep native URLs unchanged; remote-library URLs
                        // need a media extension even for signed/extensionless CDNs.
                        rid.push('.');
                        rid.push_str(remote_resource_extension(&resolved_url));
                    }
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
            .chain(
                byte_stream
                    .map(|item| item.map_err(|error| std::io::Error::other(error.without_url()))),
            )
            .map(move |item| {
                let _keep_delivery_active = &activity;
                item
            });

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

fn remote_resource_extension(url: &str) -> &'static str {
    let extension = Url::parse(url).ok().and_then(|url| {
        url.path()
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
    });
    match extension.as_deref() {
        Some("m3u8") => "m3u8",
        Some("m4s") => "m4s",
        Some("mp4") => "mp4",
        Some("m4a") => "m4a",
        Some("m4v") => "m4v",
        Some("aac") => "aac",
        Some("mp3") => "mp3",
        Some("vtt") => "vtt",
        Some("webvtt") => "webvtt",
        Some("key") => "key",
        Some("bin") => "bin",
        Some("webm") => "webm",
        Some("ogg") => "ogg",
        Some("opus") => "opus",
        Some("flac") => "flac",
        _ => "ts",
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

#[cfg(test)]
mod activity_tests {
    use super::*;

    #[test]
    fn activity_expires_without_counting_retained_sessions_forever() {
        let now = Instant::now();
        let idle = ActivityState {
            last_observed: now,
            in_flight: 0,
        };
        assert!(idle.is_active(now + PLAYBACK_IDLE_GRACE - Duration::from_secs(1)));
        assert!(!idle.is_active(now + PLAYBACK_IDLE_GRACE));
        let delivering = ActivityState {
            last_observed: now,
            in_flight: 1,
        };
        assert!(delivering.is_active(now + PLAYBACK_IDLE_GRACE * 2));
    }

    #[test]
    fn response_body_lifetime_and_cancellation_keep_restart_deferred() -> Result<(), RelayError> {
        let relay = MediaRelay::new();
        assert_eq!(relay.active_streams(), Some(0));
        let body = relay.begin_delivery("playing")?;
        if let Ok(mut activity) = relay.activity.lock() {
            for session in activity.values_mut() {
                session.last_observed = Instant::now() - PLAYBACK_IDLE_GRACE * 2;
            }
        }
        assert_eq!(relay.active_streams(), Some(1));
        relay.revoke_session("playing");
        assert_eq!(relay.active_streams(), Some(1));
        drop(body);
        assert_eq!(relay.active_streams(), Some(1));
        if let Ok(mut activity) = relay.activity.lock() {
            for session in activity.values_mut() {
                assert_eq!(session.in_flight, 0);
                session.last_observed = Instant::now() - PLAYBACK_IDLE_GRACE;
            }
        }
        assert_eq!(relay.active_streams(), Some(0));
        Ok(())
    }

    #[tokio::test]
    async fn relay_response_holds_activity_until_its_body_is_dropped()
    -> Result<(), Box<dyn std::error::Error>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (close, closed) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await?;
                request.push(byte[0]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nContent-Type: video/mp2t\r\n\r\n",
                )
                .await?;
            socket.write_all(&[0x47; 188]).await?;
            let _ = closed.await;
            Ok::<_, std::io::Error>(())
        });
        let relay = MediaRelay::new();
        let response = relay
            .proxy_url(
                "playing",
                "token",
                &format!("http://{address}/segment.ts"),
                &FetchContext::default(),
                None,
            )
            .await?;
        let RelayResponse::Binary { stream, .. } = response else {
            return Err("expected binary response".into());
        };
        if let Ok(mut activity) = relay.activity.lock() {
            for session in activity.values_mut() {
                session.last_observed = Instant::now() - PLAYBACK_IDLE_GRACE * 2;
            }
        }
        assert_eq!(relay.active_streams(), Some(1));
        drop(stream);
        // Cancellation starts the grace period, and a later explicit revoke clears it.
        assert_eq!(relay.active_streams(), Some(1));
        relay.revoke_session("playing");
        assert_eq!(relay.active_streams(), Some(0));
        let _ = close.send(());
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn upstream_connection_errors_do_not_expose_signed_urls()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        drop(listener);
        let relay = MediaRelay::new();
        let result = relay
            .proxy_url(
                "failed",
                "relay-token",
                &format!("http://{address}/stream?key=secret-provider-token"),
                &FetchContext::default(),
                None,
            )
            .await;
        match result {
            Err(error) => {
                let message = error.to_string();
                assert!(!message.contains("secret-provider-token"));
                assert!(!message.contains("?key="));
            }
            Ok(_) => return Err("expected closed upstream to fail".into()),
        }
        Ok(())
    }

    #[test]
    fn progress_keeps_buffered_playback_active_until_revoke() {
        let relay = MediaRelay::new();
        relay.note_playback_activity("playing");
        relay.note_playback_activity("playing");
        assert_eq!(relay.active_streams(), Some(1));
        assert!(relay.session_active("playing"));
        assert!(!relay.session_active("unknown"));
        relay.revoke_session("playing");
        assert_eq!(relay.active_streams(), Some(0));
        assert!(!relay.session_active("playing"));
    }
}
