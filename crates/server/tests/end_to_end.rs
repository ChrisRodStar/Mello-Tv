//! Full playback path against local fakes: a fake provider engine, two fake
//! CDNs (different origins) serving real ffmpeg-generated HLS and MP4, and the
//! real router. When ffmpeg is installed, the real `ffmpeg`/`ffprobe` binaries
//! play the `/play` URL exactly as Jellyfin would.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{Path as AxPath, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::Utc;
use stream_core::{
    domain::MediaType,
    error::DomainError,
    ports::MediaEngine,
    providers::{
        CandidateStream, FetchContext, FetchScope, Lookup, Metadata, ProbeObservation,
        ProbedStream, ProviderId, ProviderTitle, RankedSource, Selection, StreamFormat, TmdbId,
    },
};
use stream_media::{HttpClient, MediaRelay, network::EgressMode};
use stream_server::{
    app::{self, AppState, AppStateInit},
    auth::Keys,
    config::Config,
    library::Library,
    relay_transport,
    tmdb::Tmdb,
};
use url::Url;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn ffmpeg(args: &[&str]) -> Result<(), BoxError> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(())
}

/// 6 s of H.264/AAC as 2 s HLS segments, plus a faststart MP4.
fn make_media(dir: &Path) -> Result<(), BoxError> {
    let lavfi = [
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=6:size=320x240:rate=25",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=6",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-g",
        "50",
        "-c:a",
        "aac",
    ];
    let segment_pattern = dir.join("seg%d.ts");
    let playlist = dir.join("index.m3u8");
    let mut hls: Vec<&str> = lavfi.to_vec();
    hls.extend([
        "-f",
        "hls",
        "-hls_time",
        "2",
        "-hls_playlist_type",
        "vod",
        "-hls_segment_filename",
        segment_pattern.to_str().ok_or("path")?,
        playlist.to_str().ok_or("path")?,
    ]);
    ffmpeg(&hls)?;
    let mp4 = dir.join("movie.mp4");
    let mut progressive: Vec<&str> = lavfi.to_vec();
    progressive.extend(["-movflags", "+faststart", mp4.to_str().ok_or("path")?]);
    ffmpeg(&progressive)
}

// ------------------------------------------------------------------ fake CDN

/// (path, cookie header, range header)
type Request = (String, Option<String>, Option<String>);

#[derive(Clone, Default)]
struct Cdn {
    media: PathBuf,
    /// Origin of the *other* CDN, used for one absolute segment URL.
    other_origin: Arc<Mutex<String>>,
    /// (path, cookie header, range header) for every request.
    log: Arc<Mutex<Vec<Request>>>,
}

impl Cdn {
    fn record(&self, path: &str, headers: &HeaderMap) {
        let get = |name: header::HeaderName| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        if let Ok(mut log) = self.log.lock() {
            log.push((path.to_owned(), get(header::COOKIE), get(header::RANGE)));
        }
    }
    fn requests(&self) -> Vec<Request> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

async fn master(State(cdn): State<Cdn>, headers: HeaderMap) -> Response {
    cdn.record("/hls/master.m3u8", &headers);
    "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=320x240\nvideo/index.m3u8\n"
        .into_response()
}

/// The ffmpeg playlist with segments disguised as images, and the first
/// segment hosted on the other CDN, like real multi-host streams.
async fn variant(State(cdn): State<Cdn>, headers: HeaderMap) -> Response {
    cdn.record("/hls/video/index.m3u8", &headers);
    let Ok(text) = std::fs::read_to_string(cdn.media.join("index.m3u8")) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let other = cdn
        .other_origin
        .lock()
        .map(|o| o.clone())
        .unwrap_or_default();
    let text = text
        .replace("seg0.ts", &format!("{other}/hls/video/seg0.png"))
        .replace(".ts", ".png");
    text.into_response()
}

async fn segment(
    State(cdn): State<Cdn>,
    AxPath(file): AxPath<String>,
    headers: HeaderMap,
) -> Response {
    cdn.record(&format!("/hls/video/{file}"), &headers);
    let name = file.replace(".png", ".ts");
    match std::fs::read(cdn.media.join(name)) {
        // Disguised: claims to be an image, like many pirate CDNs.
        Ok(bytes) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn broken(State(cdn): State<Cdn>, headers: HeaderMap) -> Response {
    cdn.record("/broken/master.m3u8", &headers);
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

async fn mp4(State(cdn): State<Cdn>, headers: HeaderMap) -> Response {
    cdn.record("/mp4/movie.mp4", &headers);
    let Ok(bytes) = std::fs::read(cdn.media.join("movie.mp4")) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let total = bytes.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(start, end)| {
            let start: usize = start.parse().ok()?;
            let end: usize = if end.is_empty() {
                total - 1
            } else {
                end.parse::<usize>().ok()?.min(total - 1)
            };
            (start <= end).then_some((start, end))
        });
    match range {
        Some((start, end)) => (
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, "video/mp4".to_owned()),
                (header::ACCEPT_RANGES, "bytes".to_owned()),
                (
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{total}"),
                ),
            ],
            bytes[start..=end].to_vec(),
        )
            .into_response(),
        None => (
            [
                (header::CONTENT_TYPE, "video/mp4"),
                (header::ACCEPT_RANGES, "bytes"),
            ],
            bytes,
        )
            .into_response(),
    }
}

async fn serve(router: Router) -> Result<String, BoxError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Ok(format!("http://{address}"))
}

fn cdn_router(cdn: Cdn) -> Router {
    Router::new()
        .route("/hls/master.m3u8", get(master))
        .route("/hls/video/index.m3u8", get(variant))
        .route("/hls/video/{file}", get(segment))
        .route("/broken/master.m3u8", get(broken))
        .route("/mp4/movie.mp4", get(mp4))
        .with_state(cdn)
}

// ---------------------------------------------------------- fake provider

fn probed(
    url: &str,
    format: StreamFormat,
    context: FetchContext,
) -> Result<ProbedStream, BoxError> {
    Ok(ProbedStream {
        source: CandidateStream {
            expires_at: None,
            id: "candidate".into(),
            server: String::new(),
            selected_variant_url: String::new(),
            url: url.into(),
            provider: ProviderId::new("vidcore")?,
            format,
            title: ProviderTitle {
                media_type: MediaType::Movie,
                tmdb_id: TmdbId::try_from(1)?,
                imdb_id: String::new(),
                my_anime_list_id: None,
                season: None,
                episode: None,
            },
            fetch_context: context,
            resolver_metadata: Metadata::default(),
            content_verified: true,
        },
        observation: ProbeObservation {
            at: Utc::now(),
            delivery_bps: None,
            reliability: None,
            variant: None,
            first_byte: None,
            sample_bytes: 0,
            format,
            failure: None,
            playlist_metadata: Metadata::default(),
            samples_completed: 3,
            accessible: true,
            encrypted: false,
            protection_known: true,
        },
    })
}

fn ranked(candidate: ProbedStream) -> RankedSource {
    RankedSource {
        candidate,
        reasons: None,
        score: 1.0,
    }
}

struct FakeEngine {
    selections: HashMap<i64, Selection>,
    calls: AtomicUsize,
    /// Simulated provider search time (`MELLO_FAKE_DELAY_SECS`, manual runs).
    delay: Duration,
}

#[async_trait::async_trait]
impl MediaEngine for FakeEngine {
    async fn resolve_stream(&self, lookup: &Lookup) -> Result<Selection, DomainError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.selections
            .get(&lookup.title.tmdb_id.get())
            .cloned()
            .ok_or_else(|| DomainError::NotFound("no source".into()))
    }
}

fn selection(selected: ProbedStream, backups: Vec<ProbedStream>) -> Selection {
    Selection {
        rejected: None,
        selected: Some(ranked(selected)),
        version: "test".into(),
        backups: Some(backups.into_iter().map(ranked).collect()),
        providers: None,
        pending: false,
    }
}

// -------------------------------------------------------------------- test

struct Harness {
    base: String,
    play_key: String,
    engine: Arc<FakeEngine>,
    cdn_a: Cdn,
    cdn_b: Cdn,
    http: reqwest::Client,
    _dirs: Vec<tempfile::TempDir>,
}

async fn harness() -> Result<Option<Harness>, BoxError> {
    if !have("ffmpeg") || !have("ffprobe") {
        eprintln!("skipping end-to-end test: ffmpeg/ffprobe not installed");
        return Ok(None);
    }
    let media = tempfile::tempdir()?;
    make_media(media.path())?;

    let cdn_a = Cdn {
        media: media.path().to_path_buf(),
        ..Cdn::default()
    };
    let cdn_b = Cdn {
        media: media.path().to_path_buf(),
        ..Cdn::default()
    };
    let origin_a = serve(cdn_router(cdn_a.clone())).await?;
    let origin_b = serve(cdn_router(cdn_b.clone())).await?;
    if let Ok(mut other) = cdn_a.other_origin.lock() {
        other.clone_from(&origin_b);
    }

    let cookie_scope = FetchContext {
        scopes: vec![FetchScope {
            origin: format!("{origin_a}/"),
            cookies: "session=secret".into(),
            cookie_path: "/".into(),
            ..FetchScope::default()
        }],
        public_resources: None,
    };
    let hls = |path: &str| {
        probed(
            &format!("{origin_a}{path}"),
            StreamFormat::Hls,
            cookie_scope.clone(),
        )
    };
    let mut selections = HashMap::new();
    selections.insert(1, selection(hls("/hls/master.m3u8")?, Vec::new()));
    selections.insert(
        2,
        selection(hls("/broken/master.m3u8")?, vec![hls("/hls/master.m3u8")?]),
    );
    selections.insert(
        3,
        selection(
            probed(
                &format!("{origin_a}/mp4/movie.mp4"),
                StreamFormat::Mp4,
                FetchContext::default(),
            )?,
            Vec::new(),
        ),
    );
    let engine = Arc::new(FakeEngine {
        selections,
        calls: AtomicUsize::new(0),
        delay: Duration::from_secs(
            std::env::var("MELLO_FAKE_DELAY_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        ),
    });

    let data = tempfile::tempdir()?;
    let library_dir = tempfile::tempdir()?;
    let config = Config {
        host: "127.0.0.1".into(),
        port: 0,
        data_dir: data.path().to_path_buf(),
        library_dir: library_dir.path().to_path_buf(),
        public_url: Url::parse("http://127.0.0.1:1")?,
        public_url_configured: true,
        api_key: None,
        tmdb_api_key: None,
        tmdb_read_token: None,
        proxy: None,
        max_height: 1080,
        audio_languages: Vec::new(),
        sync_interval: Duration::from_secs(3600),
        include_specials: false,
        auto_lists: Vec::new(),
        tmdb_lists: Vec::new(),
        jellyfin_url: None,
        jellyfin_api_key: None,
    };
    let keys = Keys::from_api_key("test-api-key-0123456789".into());
    let tmdb = Tmdb::new(None, None)?;
    let library = Library::open(&config, tmdb.clone(), keys.play_key()).await?;
    let client = HttpClient::with_config(relay_transport(EgressMode::Direct))?
        .with_fixture_origins([origin_a.clone(), origin_b.clone()]);
    let play_key = keys.play_key().to_owned();
    let state = AppState::new(AppStateInit {
        keys,
        media: engine.clone(),
        relay: Arc::new(MediaRelay::new(client)),
        tmdb,
        library,
        public_url: config.public_url.clone(),
        public_url_configured: true,
        max_height: 1080,
        audio_languages: Vec::new(),
    });
    let base = serve(app::router(state)).await?;
    Ok(Some(Harness {
        base,
        play_key,
        engine,
        cdn_a,
        cdn_b,
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        _dirs: vec![media, data, library_dir],
    }))
}

impl Harness {
    fn play_url(&self, id: i64) -> String {
        format!("{}/play/movie/{id}?key={}", self.base, self.play_key)
    }

    async fn location(&self, id: i64) -> Result<String, BoxError> {
        let response = self.http.get(self.play_url(id)).send().await?;
        assert_eq!(response.status(), StatusCode::FOUND, "play {id}");
        Ok(response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or("missing Location")?
            .to_owned())
    }

    async fn text(&self, path: &str) -> Result<String, BoxError> {
        let response = self.http.get(format!("{}{path}", self.base)).send().await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        Ok(response.text().await?)
    }
}

/// The player binaries can be swapped for Jellyfin's own build, e.g.
/// `MELLO_TEST_FFMPEG=/path/to/jellyfin-ffmpeg-wrapper`.
fn player(tool: &str) -> String {
    let var = format!("MELLO_TEST_{}", tool.to_ascii_uppercase());
    std::env::var(var).unwrap_or_else(|_| tool.to_owned())
}

/// Runs blocking ffprobe off the async runtime; returns the duration in seconds.
async fn probe_duration(url: String) -> Result<f64, BoxError> {
    let out = tokio::task::spawn_blocking(move || {
        Command::new(player("ffprobe"))
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=nw=1:nk=1",
                &url,
            ])
            .output()
    })
    .await??;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().parse()?)
}

/// Decodes the whole stream like Jellyfin's ffmpeg does when remuxing.
async fn play_through(url: String) -> Result<(), BoxError> {
    let out = tokio::task::spawn_blocking(move || {
        Command::new(player("ffmpeg"))
            .args(["-hide_banner", "-loglevel", "error", "-i", &url])
            .args(["-map", "0", "-c", "copy", "-f", "null", "-"])
            .output()
    })
    .await??;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn hls_plays_through_play_url_with_disguised_multi_host_segments() -> Result<(), BoxError> {
    let Some(h) = harness().await? else {
        return Ok(());
    };

    // Keys are enforced.
    let unauthorized = h
        .http
        .get(format!("{}/play/movie/1", h.base))
        .send()
        .await?;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    // Redirect to a relay playlist; repeat opens reuse the session.
    let location = h.location(1).await?;
    assert!(location.starts_with("/stream/") && location.contains("/master.m3u8?st="));
    assert_eq!(h.location(1).await?, location);
    assert_eq!(h.engine.calls.load(Ordering::SeqCst), 1);

    // Rewritten URIs carry conventional extensions.
    let master = h.text(&location).await?;
    // URIs are absolute, built from the address this client used.
    let relay_prefix = format!("{}/stream/", h.base);
    let variant_url = master
        .lines()
        .find(|l| l.starts_with(&relay_prefix))
        .ok_or("no absolute variant URI")?;
    assert!(variant_url.contains(".m3u8?st="), "{master}");
    let variant_path = variant_url.trim_start_matches(&h.base);
    let variant = h.text(variant_path).await?;
    let segments: Vec<&str> = variant
        .lines()
        .filter(|l| l.starts_with(&relay_prefix))
        .map(|l| l.trim_start_matches(&h.base))
        .collect();
    assert_eq!(segments.len(), 3, "{variant}");
    assert!(segments.iter().all(|s| s.contains(".ts?st=")), "{variant}");

    // Segments arrive as MPEG-TS even though the CDN calls them PNGs.
    let first = h
        .http
        .get(format!("{}{}", h.base, segments[1]))
        .send()
        .await?;
    assert_eq!(
        first
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("video/mp2t")
    );
    assert_eq!(
        first
            .headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );

    // A wrong token is refused.
    let (path, _) = segments[1].split_once("?st=").ok_or("token")?;
    let forbidden = h
        .http
        .get(format!("{}{path}?st=wrong", h.base))
        .send()
        .await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // Real ffmpeg/ffprobe accept the redirect, playlists and segment names.
    let duration = probe_duration(h.play_url(1)).await?;
    assert!((5.0..7.5).contains(&duration), "duration {duration}");
    play_through(h.play_url(1)).await?;
    assert_eq!(h.engine.calls.load(Ordering::SeqCst), 1);

    // Provider cookies reach only their own origin.
    let a = h.cdn_a.requests();
    let b = h.cdn_b.requests();
    assert!(!a.is_empty() && !b.is_empty());
    assert!(
        a.iter()
            .all(|(_, cookie, _)| cookie.as_deref() == Some("session=secret"))
    );
    assert!(b.iter().all(|(_, cookie, _)| cookie.is_none()), "{b:?}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dead_primary_source_falls_back_to_backup() -> Result<(), BoxError> {
    let Some(h) = harness().await? else {
        return Ok(());
    };
    play_through(h.play_url(2)).await?;
    let paths: Vec<String> = h.cdn_a.requests().into_iter().map(|(p, _, _)| p).collect();
    assert!(paths.contains(&"/broken/master.m3u8".to_owned()));
    assert!(paths.contains(&"/hls/master.m3u8".to_owned()));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn progressive_mp4_supports_range_requests() -> Result<(), BoxError> {
    let Some(h) = harness().await? else {
        return Ok(());
    };
    let location = h.location(3).await?;
    assert!(location.contains("/video.mp4?st="), "{location}");

    let partial = h
        .http
        .get(format!("{}{location}", h.base))
        .header(header::RANGE, "bytes=100-199")
        .send()
        .await?;
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    assert!(
        partial
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("bytes 100-199/"))
    );
    assert_eq!(partial.bytes().await?.len(), 100);

    let duration = probe_duration(h.play_url(3)).await?;
    assert!((5.0..7.5).contains(&duration), "duration {duration}");
    play_through(h.play_url(3)).await?;
    assert!(
        h.cdn_a
            .requests()
            .iter()
            .any(|(_, _, range)| range.as_deref() == Some("bytes=100-199"))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unavailable_titles_fail_fast_on_retry() -> Result<(), BoxError> {
    let Some(h) = harness().await? else {
        return Ok(());
    };
    for _ in 0..3 {
        let response = h.http.get(h.play_url(4)).send().await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(h.engine.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn next_episode_is_ready_before_it_is_played() -> Result<(), BoxError> {
    let Some(h) = harness().await? else {
        return Ok(());
    };
    let episode = |e: i64| format!("{}/play/tv/1/1/{e}?key={}", h.base, h.play_key);
    let first = h.http.get(episode(1)).send().await?;
    assert_eq!(first.status(), StatusCode::FOUND);
    // The background prefetch resolves episode 2.
    for _ in 0..50 {
        if h.engine.calls.load(Ordering::SeqCst) >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.engine.calls.load(Ordering::SeqCst), 2);
    // Playing episode 2 uses the prefetched session (no new search) and
    // prefetches episode 3 in turn.
    let second = h.http.get(episode(2)).send().await?;
    assert_eq!(second.status(), StatusCode::FOUND);
    play_through(episode(2)).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.engine.calls.load(Ordering::SeqCst), 3);
    Ok(())
}

/// Manual harness for testing a real Jellyfin against fake providers:
///
/// ```sh
/// MELLO_FAKE_LIBRARY=/tmp/lib MELLO_FAKE_SECS=900 \
///   cargo test -p stream-server --test end_to_end manual -- --ignored --nocapture
/// ```
///
/// Writes `movies/` and `shows/` .strm files into `MELLO_FAKE_LIBRARY` and logs
/// every request (path and user agent) so scan and playback behaviour is visible.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual: serves fakes for a real Jellyfin"]
async fn manual_fake_server_for_jellyfin() -> Result<(), BoxError> {
    let Some(h) = harness().await? else {
        return Ok(());
    };
    let library = PathBuf::from(std::env::var("MELLO_FAKE_LIBRARY")?);
    let seconds: u64 = std::env::var("MELLO_FAKE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let base = Url::parse(&h.base)?;
    let proxy_port = base.port().ok_or("port")?;
    // Log every request Jellyfin makes by proxying through a logging front.
    let upstream = h.base.clone();
    let http = h.http.clone();
    let front = Router::new().fallback(move |request: axum::extract::Request| {
        let upstream = upstream.clone();
        let http = http.clone();
        async move {
            let ua = request
                .headers()
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
                .to_owned();
            let path = request.uri().path().to_owned();
            let query = request
                .uri()
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default();
            println!("[mello] {} {path} ua={ua}", request.method());
            let mut forward =
                http.request(request.method().clone(), format!("{upstream}{path}{query}"));
            if let Some(range) = request.headers().get(header::RANGE) {
                forward = forward.header(header::RANGE, range);
            }
            match forward.send().await {
                Ok(response) => {
                    let mut out = axum::response::Response::builder().status(response.status());
                    for (name, value) in response.headers() {
                        out = out.header(name, value);
                    }
                    out.body(axum::body::Body::from_stream(response.bytes_stream()))
                        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
                }
                Err(_) => StatusCode::BAD_GATEWAY.into_response(),
            }
        }
    });
    let front_base = serve(front).await?;
    let _ = proxy_port;
    let movie_dir = library.join("movies/Test Movie (2020) [tmdbid-1]");
    let mp4_dir = library.join("movies/Test Progressive (2021) [tmdbid-3]");
    let show_dir = library.join("shows/Test Show (2019) [tmdbid-1]/Season 01");
    for dir in [&movie_dir, &mp4_dir, &show_dir] {
        std::fs::create_dir_all(dir)?;
    }
    let key = &h.play_key;
    std::fs::write(
        movie_dir.join("Test Movie (2020) [tmdbid-1].strm"),
        format!("{front_base}/play/movie/1?key={key}"),
    )?;
    std::fs::write(
        mp4_dir.join("Test Progressive (2021) [tmdbid-3].strm"),
        format!("{front_base}/play/movie/3?key={key}"),
    )?;
    std::fs::write(
        show_dir.join("Test Show S01E01.strm"),
        format!("{front_base}/play/tv/1/1/1?key={key}"),
    )?;
    println!(
        "[mello] fake server at {front_base}, library at {}",
        library.display()
    );
    for _ in 0..seconds {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!(
        "[mello] engine resolutions: {}",
        h.engine.calls.load(Ordering::SeqCst)
    );
    Ok(())
}
