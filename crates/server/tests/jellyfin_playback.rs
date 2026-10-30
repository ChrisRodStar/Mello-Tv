#![allow(clippy::unwrap_used, clippy::expect_used)]
use axum::{
    Router,
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use stream_core::{
    accounts::{Preferences, Profile},
    catalog::Title,
    error::DomainError,
    ports::MediaEngine,
    providers::{Lookup, Selection},
};
use stream_server::{
    AppState,
    jellyfin::{JellyfinBridge, JellyfinConfig},
    routes,
};
use stream_storage::{AccountStore, CatalogStore, accounts::ListKind, db::Database};

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct FixtureEngine {
    url: String,
    calls: Arc<AtomicUsize>,
    lookups: Arc<Mutex<Vec<Lookup>>>,
}
#[async_trait::async_trait]
impl MediaEngine for FixtureEngine {
    async fn resolve_stream(&self, lookup: &Lookup) -> Result<Selection, DomainError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.lookups
            .lock()
            .expect("record lookup")
            .push(lookup.clone());
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        serde_json::from_value(json!({"version":"1", "backups":null,"providers":null,"rejected":null,"selected": {
            "score":1.0,"reasons":null,"candidate":{
                "source":{"id":"fixture","url":self.url,"provider":"fixture","format":"hls","title":lookup.title,"fetch_context":{},"resolver_metadata":{}},
                "observation":{"at":DateTime::<Utc>::from(std::time::SystemTime::now()),"sample_bytes":188,"format":"hls","playlist_metadata":{},"samples_completed":1,"accessible":true,"encrypted":false,"protection_known":true}
            }
        }})).map_err(|error| DomainError::Infrastructure(error.to_string().into()))
    }
}
async fn spawn(
    router: Router,
) -> Result<(String, tokio::task::JoinHandle<()>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("fixture server");
    });
    Ok((base, task))
}
#[tokio::test]
async fn stable_url_auth_singleflight_nested_hls_and_ranges() -> TestResult {
    let (upstream,upstream_task) = spawn(Router::new()
        .route("/master.m3u8",get(|| async { "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,AUDIO=\"a\"\nvideo.m3u8\n" }))
        .route("/audio.m3u8",get(|| async { "#EXTM3U\n#EXTINF:4,\naudio.aac\n" }))
        .route("/video.m3u8",get(|| async { "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\nsegment.ts\n#EXT-X-ENDLIST\n" }))
        .route("/key.bin",get(|| async { "0123456789abcdef" }))
        .route("/init.mp4",get(|| async { "initialization" }))
        .route("/segment.ts",get(|headers:HeaderMap| async move {
            assert_eq!(headers.get(header::RANGE).unwrap(),"bytes=0-3");
            (StatusCode::PARTIAL_CONTENT,[(header::CONTENT_TYPE,"video/mp2t"),(header::CONTENT_RANGE,"bytes 0-3/188"),(header::ACCEPT_RANGES,"bytes")],"data")
        }))).await?;
    let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(64).await?).await?);
    let accounts =
        Arc::new(AccountStore::from_database(Database::in_memory(64).await?, [0x31; 32]).await?);
    let owner = accounts.select_household(None).await?;
    let profile = accounts
        .create_profile(
            &owner,
            Profile {
                id: "jellyfin-profile".into(),
                name: "Jellyfin".into(),
                color: "#123456".into(),
                preferences: Preferences::default(),
            },
        )
        .await?;
    let movie: Title = serde_json::from_value(
        json!({"type":"movie","id":27205,"title":"Inception","overview":"","rating":0.0,"details":true}),
    )?;
    accounts
        .put_title(&owner, &profile.id, ListKind::Library, movie)
        .await?;
    accounts.put_profile_metadata(&owner, &profile.id, "playback-settings", json!({"audio_language":"fr","subtitle_language":"en","subtitles_enabled":true,"max_height":720})).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let lookups = Arc::new(Mutex::new(Vec::new()));
    let mut state = AppState::new(
        catalog,
        accounts.clone(),
        Arc::new(FixtureEngine {
            url: format!("{upstream}/master.m3u8"),
            calls: calls.clone(),
            lookups: lookups.clone(),
        }),
        owner.clone(),
    );
    let output = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let key = "0123456789abcdef0123456789abcdef";
    state.jellyfin = Some(Arc::new(JellyfinBridge::new(JellyfinConfig::new(
        profile.id.clone(),
        output.path().to_owned(),
        base.clone(),
        key.into(),
    )?)));
    let server = tokio::spawn(async move {
        axum::serve(listener, routes::build_router(state))
            .await
            .expect("Mello fixture");
    });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let play = format!("{base}/integrations/jellyfin/play/movie/27205/0/0");
    let forbidden = client.get(&play).send().await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        forbidden.headers().get(header::CACHE_CONTROL).unwrap(),
        "private, no-store"
    );
    assert_eq!(
        forbidden.headers().get("referrer-policy").unwrap(),
        "no-referrer"
    );
    assert_eq!(
        client
            .get(format!("{play}?key=wrong"))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(format!(
                "{base}/integrations/jellyfin/play/movie/27205/1/1?key={key}"
            ))
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client
            .get(format!(
                "{base}/integrations/jellyfin/play/movie/999/0/0?key={key}"
            ))
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let (first, second) = tokio::join!(
        client.get(format!("{play}?key={key}")).send(),
        client.get(format!("{play}?key={key}")).send()
    );
    let first = first?;
    let second = second?;
    assert_eq!(first.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        first.headers().get(header::CACHE_CONTROL).unwrap(),
        "private, no-store"
    );
    let entry = first
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()?
        .to_owned();
    assert_eq!(
        Some(entry.as_str()),
        second
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    {
        let recorded = lookups.lock().expect("recorded lookups");
        assert_eq!(recorded[0].preferences.audio_languages, vec!["fr"]);
        assert_eq!(recorded[0].preferences.subtitle_languages, vec!["en"]);
        assert_eq!(recorded[0].preferences.max_height, 720);
        assert_eq!(recorded[0].device.max_height, 720);
        recorded[0].validate()?;
        recorded[0].validate_context()?;
    }
    let master = client
        .get(&entry)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(!master.contains(&upstream));
    let audio_url = master
        .lines()
        .find(|line| line.starts_with("#EXT-X-MEDIA:"))
        .unwrap()
        .split("URI=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(audio_url.starts_with(&base));
    assert!(reqwest::Url::parse(audio_url)?.path().ends_with(".m3u8"));
    let audio = client
        .get(audio_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(audio.lines().any(|line| line.starts_with(&base)));
    let video_url = master.lines().find(|line| line.starts_with(&base)).unwrap();
    let video = client
        .get(video_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    for tag in ["#EXT-X-KEY:", "#EXT-X-MAP:"] {
        let uri = video
            .lines()
            .find(|line| line.starts_with(tag))
            .unwrap()
            .split("URI=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(uri.starts_with(&base));
        assert!(client.get(uri).send().await?.status().is_success());
    }
    let segment = video.lines().find(|line| line.starts_with(&base)).unwrap();
    assert!(reqwest::Url::parse(segment)?.path().ends_with(".ts"));
    let response = client
        .get(segment)
        .header(header::RANGE, "bytes=0-3")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 0-3/188"
    );
    assert_eq!(response.text().await?, "data");
    assert_eq!(
        client.get(format!("{entry}wrong")).send().await?.status(),
        StatusCode::FORBIDDEN
    );
    accounts
        .put_profile_metadata(
            &owner,
            &profile.id,
            "playback-settings",
            json!({"max_height":0}),
        )
        .await?;
    assert_eq!(
        client
            .get(format!("{play}?key={key}"))
            .send()
            .await?
            .status(),
        StatusCode::TEMPORARY_REDIRECT
    );
    {
        let recorded = lookups.lock().expect("recorded Auto lookup");
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[1].preferences.max_height, 1080);
        assert_eq!(recorded[1].device.max_height, 1080);
        assert_eq!(recorded[1].startup_height, 1080);
    }
    accounts
        .put_profile_metadata(
            &owner,
            &profile.id,
            "playback-settings",
            json!({"max_height":-1}),
        )
        .await?;
    assert_eq!(
        client
            .get(format!("{play}?key={key}"))
            .send()
            .await?
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    accounts
        .remove_title(
            &owner,
            &profile.id,
            ListKind::Library,
            stream_core::catalog::TitleIdentity::new(
                stream_core::catalog::MediaType::Movie,
                27205,
            )?,
        )
        .await?;
    assert_eq!(
        client
            .get(format!("{play}?key={key}"))
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.abort();
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn disabled_route_does_not_serve_the_web_app() -> TestResult {
    let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(64).await?).await?);
    let accounts =
        Arc::new(AccountStore::from_database(Database::in_memory(64).await?, [0x42; 32]).await?);
    let owner = accounts.select_household(None).await?;
    let state = AppState::new(
        catalog,
        accounts,
        Arc::new(stream_server::state::NoopMediaEngine),
        owner,
    );
    let assets = tempfile::tempdir()?;
    tokio::fs::write(
        assets.path().join("index.html"),
        "<html>Mello web application</html>",
    )
    .await?;
    let (base, task) = spawn(routes::build_router_with_static(
        state,
        Some(assets.path().to_owned()),
    ))
    .await?;
    let response = reqwest::get(format!(
        "{base}/integrations/jellyfin/play/movie/27205/0/0?key=unconfigured"
    ))
    .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "private, no-store"
    );
    assert_eq!(
        response.headers().get("referrer-policy").unwrap(),
        "no-referrer"
    );
    assert!(!response.text().await?.contains("Mello web application"));
    task.abort();
    Ok(())
}
