#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{sync::Arc, time::SystemTime};

use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use stream_server::{
    routes,
    state::{AppState, NoopMediaEngine},
};
use stream_storage::{
    AccountStore,
    catalog::{CatalogStore, CatalogWrite},
    db::Database,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn now() -> DateTime<Utc> {
    SystemTime::now().into()
}

fn sample_title(id: i64, name: &str) -> Value {
    json!({
        "type": "movie",
        "id": id,
        "title": name,
        "overview": "A test movie overview",
        "rating": 8.5,
        "poster": "/internal/v1/artwork/w500/poster.jpg",
        "backdrop": "/internal/v1/artwork/w1280/backdrop.jpg",
        "details": true
    })
}

fn sample_season(show_id: i64, season_num: i32) -> Value {
    json!({
        "show_id": show_id,
        "season": season_num,
        "name": "Season 1",
        "episodes": [
            {
                "tmdb_episode_id": 1001,
                "season": season_num,
                "episode": 1,
                "name": "Pilot",
                "overview": "The first episode",
                "runtime": 55,
                "still": "/internal/v1/artwork/w300/still1.jpg",
                "air_date": "2024-01-01"
            },
            {
                "tmdb_episode_id": 1002,
                "season": season_num,
                "episode": 2,
                "name": "Second Chapter",
                "overview": "The second episode",
                "runtime": 52,
                "still": "/internal/v1/artwork/w300/still2.jpg",
                "air_date": "2024-01-08"
            }
        ]
    })
}

async fn spawn_test_server() -> Result<(String, Arc<CatalogStore>), Box<dyn std::error::Error>> {
    let (base, catalog, _) = spawn_test_server_with_relay().await?;
    Ok((base, catalog))
}

async fn spawn_test_server_with_relay()
-> Result<(String, Arc<CatalogStore>, Arc<stream_media::MediaRelay>), Box<dyn std::error::Error>> {
    let catalog_db = Database::in_memory(64).await?;
    let catalog = Arc::new(CatalogStore::from_database(catalog_db).await?);

    let accounts_db = Database::in_memory(64).await?;
    let key = [0x5cu8; 32];
    let accounts = Arc::new(AccountStore::from_database(accounts_db, key).await?);
    let household_id = accounts.select_household(None).await?;

    let media = Arc::new(NoopMediaEngine);
    let state = AppState::new(catalog.clone(), accounts, media, household_id);

    let relay = state.relay.clone();
    let router = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok((base_url, catalog, relay))
}

#[tokio::test]
async fn health_endpoints_parity() -> TestResult {
    let (base, _) = spawn_test_server().await?;
    let client = reqwest::Client::new();

    for path in ["/health/livez", "/internal/v1/health/live"] {
        let res = client.get(format!("{base}{path}")).send().await?;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        let body: Value = res.json().await?;
        assert!(body.get("version").is_some());
        assert!(body.get("status").is_some());
    }

    for path in ["/health/readyz", "/internal/v1/health/ready"] {
        let res = client.get(format!("{base}{path}")).send().await?;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        let body: Value = res.json().await?;
        assert_eq!(body["status"], "ready");
    }

    Ok(())
}

#[tokio::test]
async fn title_and_cross_ids_parity() -> TestResult {
    let (base, catalog) = spawn_test_server().await?;
    let client = reqwest::Client::new();
    let instant = now();

    catalog
        .put(
            CatalogWrite {
                kind: "title".into(),
                key: "movie:550".into(),
                data: sample_title(550, "Fight Club"),
                ttl: Duration::hours(24),
            },
            instant,
        )
        .await?;

    // Public title route: artwork normalized
    let res = client
        .get(format!("{base}/api/v1/titles/movie/550"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["kind"], "title");
    assert_eq!(body["key"], "movie:550");
    assert_eq!(body["data"]["title"], "Fight Club");
    // Verify artwork normalization to tmdb CDN
    assert_eq!(
        body["data"]["poster"],
        "https://image.tmdb.org/t/p/w500/poster.jpg"
    );
    assert_eq!(
        body["data"]["backdrop"],
        "https://image.tmdb.org/t/p/w1280/backdrop.jpg"
    );

    // Internal title route
    let res = client
        .get(format!("{base}/internal/v1/titles/movie/550"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // PUT cross-ids
    let cross_payload = json!({
        "imdb_id": "tt0137523",
        "mal_id": 123
    });
    let res = client
        .put(format!("{base}/internal/v1/titles/movie/550/cross-ids"))
        .json(&cross_payload)
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let put_res: Value = res.json().await?;
    assert_eq!(put_res["status"], "saved");

    // Re-read title: cross-ids are merged into Title
    let res = client
        .get(format!("{base}/api/v1/titles/movie/550"))
        .send()
        .await?;
    let body: Value = res.json().await?;
    assert_eq!(body["data"]["imdb_id"], "tt0137523");
    assert_eq!(body["data"]["mal_id"], 123);

    // A missing title with no profile is explicitly unavailable and does not fetch metadata.
    let res = client
        .get(format!("{base}/api/v1/titles/movie/999999"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let missing: Value = res.json().await?;
    assert_eq!(missing["status"], "unavailable");

    Ok(())
}

#[tokio::test]
async fn season_and_episode_coordinates_parity() -> TestResult {
    let (base, catalog) = spawn_test_server().await?;
    let client = reqwest::Client::new();
    let instant = now();

    catalog
        .put(
            CatalogWrite {
                kind: "season".into(),
                key: "tv:100:1".into(),
                data: sample_season(100, 1),
                ttl: Duration::hours(24),
            },
            instant,
        )
        .await?;

    // GET season
    let res = client
        .get(format!("{base}/api/v1/tv/100/seasons/1"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["data"]["show_id"], 100);
    assert_eq!(body["data"]["season"], 1);
    // Artwork normalized in episodes
    assert_eq!(
        body["data"]["episodes"][0]["still"],
        "https://image.tmdb.org/t/p/w300/still1.jpg"
    );

    // GET public episode (shape: { show_id, data: Episode, state, updated_at, expires_at })
    let res = client
        .get(format!("{base}/api/v1/tv/100/seasons/1/episodes/2"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["show_id"], 100);
    assert_eq!(body["data"]["episode"], 2);
    assert_eq!(body["data"]["name"], "Second Chapter");
    assert_eq!(
        body["data"]["still"],
        "https://image.tmdb.org/t/p/w300/still2.jpg"
    );

    // GET internal catalog episode (shape: CatalogRecord with show_id in data)
    let res = client
        .get(format!("{base}/internal/v1/tv/100/seasons/1/episodes/2"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["kind"], "season");
    assert_eq!(body["key"], "tv:100:1");
    assert_eq!(body["data"]["show_id"], 100);
    assert_eq!(body["data"]["episode"], 2);

    // A missing episode is explicit when no profile exists to authorize catalog generation.
    let res = client
        .get(format!("{base}/api/v1/tv/100/seasons/1/episodes/99"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["status"], "unavailable");

    Ok(())
}

#[tokio::test]
async fn search_and_browse_parity() -> TestResult {
    let (base, catalog) = spawn_test_server().await?;
    let client = reqwest::Client::new();
    let instant = now();

    catalog
        .put_batch(
            vec![
                CatalogWrite {
                    kind: "title".into(),
                    key: "movie:1".into(),
                    data: sample_title(1, "The Dark Knight"),
                    ttl: Duration::hours(24),
                },
                CatalogWrite {
                    kind: "title".into(),
                    key: "movie:2".into(),
                    data: sample_title(2, "Knight and Day"),
                    ttl: Duration::hours(24),
                },
                CatalogWrite {
                    kind: "shelf".into(),
                    key: "trending".into(),
                    data: json!({
                        "id": "trending",
                        "heading": "Trending",
                        "titles": [sample_title(1, "The Dark Knight")]
                    }),
                    ttl: Duration::hours(6),
                },
            ],
            instant,
        )
        .await?;

    // Search case-insensitive
    let res = client
        .get(format!("{base}/api/v1/search?q=KNIGHT"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["records"].as_array().unwrap().len(), 2);
    assert_eq!(body["offset"], 0);
    assert_eq!(body["limit"], 50);

    // Search with alias `query`
    let res = client
        .get(format!("{base}/internal/v1/search?query=dark"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["records"].as_array().unwrap().len(), 1);

    // Browse shelves
    let res = client.get(format!("{base}/api/v1/browse")).send().await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["records"].as_array().unwrap().len(), 1);

    // Shelf by ID
    let res = client
        .get(format!("{base}/api/v1/shelves/trending"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body: Value = res.json().await?;
    assert_eq!(body["key"], "trending");

    // Internal shelves route
    let res = client
        .get(format!("{base}/internal/v1/shelves"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    Ok(())
}

#[tokio::test]
async fn snapshot_export_and_artwork_redirect_parity() -> TestResult {
    let (base, catalog) = spawn_test_server().await?;
    let instant = now();

    catalog
        .put(
            CatalogWrite {
                kind: "title".into(),
                key: "movie:1".into(),
                data: sample_title(1, "Test"),
                ttl: Duration::hours(24),
            },
            instant,
        )
        .await?;

    // Test snapshot export with etag header
    let client = reqwest::Client::new();
    let res = client
        .get(format!("{base}/internal/v1/snapshot"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let etag = res
        .headers()
        .get("x-catalog-revision")
        .expect("etag present");
    assert!(etag.to_str()?.starts_with("\"catalog-v1-"));
    let body: Value = res.json().await?;
    assert_eq!(body["version"], "1");
    assert!(!body["records"].as_array().unwrap().is_empty());

    // Test artwork redirect (without following redirect)
    let no_redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let res = no_redirect_client
        .get(format!("{base}/internal/v1/artwork/w500/sample.jpg"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    let location = res.headers().get("location").expect("location present");
    assert_eq!(
        location.to_str()?,
        "https://image.tmdb.org/t/p/w500/sample.jpg"
    );
    let cache_control = res
        .headers()
        .get("cache-control")
        .expect("cache-control present");
    assert_eq!(cache_control.to_str()?, "private, max-age=86400");

    Ok(())
}

#[tokio::test]
async fn update_readiness_follows_playback_activity_without_caching() -> TestResult {
    let (base, _, relay) = spawn_test_server_with_relay().await?;
    let response = reqwest::get(format!("{base}/api/v1/update-readiness")).await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: Value = response.json().await?;
    assert_eq!(body, json!({"safe_to_restart": true, "active_streams": 0}));
    relay.note_playback_activity("playing");
    let body: Value = reqwest::get(format!("{base}/api/v1/update-readiness"))
        .await?
        .json()
        .await?;
    assert_eq!(body, json!({"safe_to_restart": false, "active_streams": 1}));
    relay.revoke_session("playing");
    let body: Value = reqwest::get(format!("{base}/api/v1/update-readiness"))
        .await?
        .json()
        .await?;
    assert_eq!(body, json!({"safe_to_restart": true, "active_streams": 0}));
    Ok(())
}
