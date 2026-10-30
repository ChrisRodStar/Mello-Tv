use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use stream_core::providers::{FetchContext, ProbedStream, Selection, StreamFormat};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::hls::{HlsManifestRewriter, ResourceKind};
use crate::network::{FetchRequest, HttpClient, TransportError};

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
    #[error("upstream request failed: {0}")]
    Upstream(String),
    #[error("upstream returned status {0}")]
    UpstreamStatus(u16),
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),
}

impl From<TransportError> for RelayError {
    fn from(error: TransportError) -> Self {
        Self::Upstream(error.to_string())
    }
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
    resources: HashMap<String, (String, ResourceKind)>,
    last_used: Instant,
}

/// In-memory playback sessions. Every upstream request goes through the
/// guarded `HttpClient`, so playlist URIs and redirects can only reach public
/// addresses, and provider credentials are only sent to their own origin.
pub struct MediaRelay {
    sessions: Mutex<BTreeMap<String, SessionState>>,
    client: HttpClient,
    activity: PlaybackActivity,
}

impl MediaRelay {
    pub fn new(client: HttpClient) -> Self {
        Self {
            sessions: Mutex::new(BTreeMap::new()),
            client,
            activity: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Recent playback activity, including response bodies still being delivered.
    pub fn active_streams(&self) -> Option<usize> {
        let now = Instant::now();
        let mut activity = self.activity.lock().ok()?;
        activity.retain(|_, session| session.is_active(now));
        Some(activity.len())
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

    /// 128-bit random hex token.
    pub fn generate_token() -> Option<String> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).ok()?;
        let mut out = String::with_capacity(32);
        for b in bytes {
            let _ = write!(out, "{b:02x}");
        }
        Some(out)
    }

    fn resource_id(url: &str) -> String {
        let digest = stream_provider_codecs::sha256(&[url.as_bytes()]);
        let mut out = String::with_capacity(24);
        for b in digest.iter().take(12) {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    /// Installs a resolved selection. Returns the delivery format on success.
    pub fn register_session(
        &self,
        session_id: String,
        token: String,
        selection: Selection,
    ) -> Option<StreamFormat> {
        let selected = selection.selected?;
        let backups: Vec<ProbedStream> = selection
            .backups
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.candidate)
            .collect();
        let format = delivery_format(&selected.candidate);
        let mut map = self.sessions.lock().ok()?;
        if map.len() >= MAX_SESSIONS
            && !map.contains_key(&session_id)
            && let Some(oldest_key) = map
                .iter()
                .min_by_key(|(_, s)| s.last_used)
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
                resources: HashMap::new(),
                last_used: Instant::now(),
            },
        );
        Some(format)
    }

    /// Time since the session last served a request, if it still exists.
    pub fn session_idle(&self, session_id: &str) -> Option<Duration> {
        let map = self.sessions.lock().ok()?;
        map.get(session_id).map(|s| s.last_used.elapsed())
    }

    /// Restarts the idle clock, e.g. when a prefetched session is claimed.
    pub fn touch_session(&self, session_id: &str) {
        if let Ok(mut map) = self.sessions.lock()
            && let Some(session) = map.get_mut(session_id)
        {
            session.last_used = Instant::now();
        }
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .map(|map| map.contains_key(session_id))
            .unwrap_or(false)
    }

    pub fn revoke_session(&self, session_id: &str) {
        if let Ok(mut map) = self.sessions.lock() {
            map.remove(session_id);
        }
    }

    fn authorize<'a>(
        map: &'a mut BTreeMap<String, SessionState>,
        session_id: &str,
        token: &str,
    ) -> Result<&'a mut SessionState, RelayError> {
        let session = map.get_mut(session_id).ok_or(RelayError::SessionNotFound)?;
        if !bool::from(session.token.as_bytes().ct_eq(token.as_bytes())) {
            return Err(RelayError::InvalidToken);
        }
        session.last_used = Instant::now();
        Ok(session)
    }

    /// Fetches the session's entry media. If the active source fails before
    /// anything was delivered, the next measured backup is tried automatically.
    /// `url_base` is prefixed to rewritten playlist URIs (see `HlsManifestRewriter`).
    pub async fn fetch_entry(
        &self,
        session_id: &str,
        token: &str,
        url_base: &str,
        range_header: Option<&str>,
    ) -> Result<RelayResponse, RelayError> {
        loop {
            let (upstream_url, context, kind) = {
                let mut map = self
                    .sessions
                    .lock()
                    .map_err(|_| RelayError::SessionNotFound)?;
                let session = Self::authorize(&mut map, session_id, token)?;
                // Progressive files need Range for seeking; playlists never do.
                let kind = if delivery_format(&session.active) == StreamFormat::Mp4 {
                    ResourceKind::Segment
                } else {
                    ResourceKind::Playlist
                };
                (
                    session.active.source.url.clone(),
                    session.active.source.fetch_context.clone(),
                    kind,
                )
            };
            let result = self
                .proxy_url(
                    session_id,
                    token,
                    url_base,
                    &upstream_url,
                    kind,
                    &context,
                    range_header,
                )
                .await;
            match result {
                Err(RelayError::Upstream(_) | RelayError::UpstreamStatus(_))
                    if self.advance_backup(session_id) =>
                {
                    tracing::info!("entry source failed; switched to the next backup source");
                }
                other => return other,
            }
        }
    }

    fn advance_backup(&self, session_id: &str) -> bool {
        let Ok(mut map) = self.sessions.lock() else {
            return false;
        };
        let Some(session) = map.get_mut(session_id) else {
            return false;
        };
        let Some(next) = session.backups.get(session.backup_index).cloned() else {
            return false;
        };
        session.active = next;
        session.backup_index += 1;
        session.resources.clear();
        true
    }

    pub async fn fetch_resource(
        &self,
        session_id: &str,
        resource_file: &str,
        token: &str,
        url_base: &str,
        range_header: Option<&str>,
    ) -> Result<RelayResponse, RelayError> {
        let resource_id = resource_file
            .split_once('.')
            .map_or(resource_file, |(id, _)| id);
        let (upstream_url, kind, context) = {
            let mut map = self
                .sessions
                .lock()
                .map_err(|_| RelayError::SessionNotFound)?;
            let session = Self::authorize(&mut map, session_id, token)?;
            let (url, kind) = session
                .resources
                .get(resource_id)
                .cloned()
                .ok_or(RelayError::ResourceNotFound)?;
            (url, kind, session.active.source.fetch_context.clone())
        };

        self.proxy_url(
            session_id,
            token,
            url_base,
            &upstream_url,
            kind,
            &context,
            range_header,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn proxy_url(
        &self,
        session_id: &str,
        token: &str,
        url_base: &str,
        upstream_url: &str,
        kind: ResourceKind,
        context: &FetchContext,
        range_header: Option<&str>,
    ) -> Result<RelayResponse, RelayError> {
        let activity = self.begin_delivery(session_id)?;
        let mut request = FetchRequest::get(upstream_url);
        request.context = context.clone();
        request
            .headers
            .insert("accept".to_owned(), "*/*".to_owned());
        if let Some(range) = range_header
            && !kind.is_playlist()
            && range.len() <= 128
        {
            request.headers.insert("range".to_owned(), range.to_owned());
        }

        let response = self
            .client
            .fetch_media(request, &CancellationToken::new())
            .await?;

        let status = response.status.as_u16();
        if !response.status.is_success() {
            return Err(RelayError::UpstreamStatus(status));
        }

        let final_url = response.url.clone();
        let headers = &response.headers;
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let content_type_header =
            header("content-type").unwrap_or_else(|| "application/octet-stream".to_owned());
        let content_length = header("content-length").and_then(|v| v.parse::<u64>().ok());
        let content_range = header("content-range");
        let accept_ranges = header("accept-ranges");

        let mut byte_stream = response.body;
        let first_chunk = match byte_stream.next().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(e)) => return Err(e.into()),
            None => Bytes::new(),
        };

        if looks_like_hls_manifest(&first_chunk) {
            let mut full = BytesMut::from(first_chunk.as_ref());
            while let Some(next) = byte_stream.next().await {
                let chunk = next?;
                if full.len() + chunk.len() > MAX_MANIFEST_BYTES {
                    return Err(RelayError::InvalidManifest(
                        "manifest exceeds byte limit".into(),
                    ));
                }
                full.extend_from_slice(&chunk);
            }

            let playlist_kind = if kind == ResourceKind::SubtitlePlaylist {
                ResourceKind::SubtitlePlaylist
            } else {
                ResourceKind::Playlist
            };
            let mut discovered = Vec::new();
            let rewritten = HlsManifestRewriter::rewrite(
                &full,
                &final_url,
                url_base,
                session_id,
                token,
                playlist_kind,
                |resolved_url, kind| {
                    let rid = Self::resource_id(&resolved_url);
                    discovered.push((rid.clone(), resolved_url, kind));
                    rid
                },
            )
            .map_err(|e| RelayError::InvalidManifest(e.to_string()))?;

            if let Ok(mut map) = self.sessions.lock()
                && let Some(session) = map.get_mut(session_id)
            {
                for (rid, url, kind) in discovered {
                    if session.resources.len() < MAX_RESOURCES_PER_SESSION
                        || session.resources.contains_key(&rid)
                    {
                        session.resources.insert(rid, (url, kind));
                    }
                }
            }

            return Ok(RelayResponse::Manifest(rewritten));
        }

        let content_type = media_content_type(&first_chunk, &final_url, &content_type_header, kind);
        let combined = futures_util::stream::once(async move { Ok(first_chunk) })
            .chain(byte_stream.map(|item| item.map_err(std::io::Error::other)))
            .map(move |item| {
                let _keep_delivery_active = &activity;
                item
            });

        Ok(RelayResponse::Binary {
            status,
            content_type,
            content_length,
            content_range,
            accept_ranges: accept_ranges.or_else(|| Some("bytes".into())),
            stream: Box::pin(combined),
        })
    }
}

/// HLS sources are served as playlists; progressive files as MP4.
pub fn delivery_format(stream: &ProbedStream) -> StreamFormat {
    match stream.observation.format {
        StreamFormat::Mp4 => StreamFormat::Mp4,
        _ => StreamFormat::Hls,
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

/// Only media types leave the relay. Anything else (an HTML error page, a
/// script) is labelled as opaque bytes so a browser never renders it.
fn media_content_type(first_chunk: &[u8], url: &Url, declared: &str, kind: ResourceKind) -> String {
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
    if trimmed.starts_with(b"WEBVTT")
        || url.path().ends_with(".vtt")
        || kind == ResourceKind::SubtitleSegment
    {
        return "text/vtt; charset=utf-8".into();
    }
    let declared = declared.trim().to_ascii_lowercase();
    if declared.starts_with("video/") || declared.starts_with("audio/") {
        return declared;
    }
    "application/octet-stream".into()
}

#[cfg(test)]
mod tests {
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
    fn non_media_content_types_are_neutralised() {
        let url = Url::parse("https://cdn.example.com/x").unwrap_or_else(|_| unreachable!());
        assert_eq!(
            media_content_type(b"<html>", &url, "text/html", ResourceKind::Segment),
            "application/octet-stream"
        );
        assert_eq!(
            media_content_type(b"\0\0\0\0", &url, "Audio/AAC", ResourceKind::Segment),
            "audio/aac"
        );
        assert_eq!(
            media_content_type(
                b"1\n00:00",
                &url,
                "text/plain",
                ResourceKind::SubtitleSegment
            ),
            "text/vtt; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn relay_refuses_private_destinations() {
        let Ok(client) = HttpClient::new() else {
            return;
        };
        let relay = MediaRelay::new(client);
        let result = relay
            .proxy_url(
                "s",
                "t",
                "",
                "http://127.0.0.1:9/secret",
                ResourceKind::Segment,
                &FetchContext::default(),
                None,
            )
            .await;
        assert!(matches!(result, Err(RelayError::Upstream(_))));
    }
}
