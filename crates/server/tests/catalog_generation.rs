#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use stream_server::{
    catalog_service::{CatalogWorker, CatalogWorkerConfig},
    routes,
    state::{AppState, NoopMediaEngine},
};
use stream_storage::{
    AccountStore,
    catalog::{CatalogStore, CatalogWrite},
    db::Database,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn assert_recommendations_record(response: &Value, profile_id: &str, state: &str) {
    assert_eq!(response["kind"], "recommendations");
    assert_eq!(response["key"], profile_id);
    assert_eq!(response["state"], state);
    assert!(
        response["revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
    );
    assert!(response["updated_at"].as_str().is_some());
    assert!(response["expires_at"].as_str().is_some());
    assert!(
        response["data"]["fingerprint"]
            .as_str()
            .is_some_and(|fingerprint| !fingerprint.is_empty())
    );
    assert!(response["data"]["pool"].is_array());
    assert!(!response["data"]["pool"].as_array().unwrap().is_empty());
    assert!(response["data"]["sections"].is_array());
}

#[derive(Clone)]
struct MockState {
    calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
    discover_delay_ms: u64,
}

fn tmdb_title(id: i64, name: &str, media_type: &str) -> Value {
    if media_type == "tv" {
        json!({"id":id,"media_type":"tv","name":name,"overview":"Mock synopsis","first_air_date":"2022-01-01","vote_average":8.0,"poster_path":"/poster.jpg","backdrop_path":"/backdrop.jpg","external_ids":{"imdb_id":"tt1234567"},"seasons":[{"season_number":1,"name":"Season 1","episode_count":3}]})
    } else {
        json!({"id":id,"media_type":"movie","title":name,"overview":"Mock synopsis","release_date":"2022-01-01","vote_average":8.0,"poster_path":"/poster.jpg","backdrop_path":"/backdrop.jpg","external_ids":{"imdb_id":"tt1234567"}})
    }
}

async fn mock_tmdb(State(state): State<MockState>, uri: Uri) -> Response {
    state.calls.fetch_add(1, Ordering::SeqCst);
    let path = uri.path();
    let query = uri.query().unwrap_or_default();
    state.requests.lock().unwrap().push(if query.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{query}")
    });
    if path == "/movie/99999" {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"injected failure"})),
        )
            .into_response();
    }
    if path.starts_with("/search/keyword") {
        let results = if query.to_ascii_lowercase().contains("query=horror") {
            vec![
                json!({"id":986,"name":"Supernatural Horror"}),
                json!({"id":987,"name":"Horror"}),
            ]
        } else {
            vec![]
        };
        return Json(
            json!({"page":1,"results":results,"total_pages":1,"total_results":results.len()}),
        )
        .into_response();
    }
    if path == "/tv/999/season/1" {
        return Json(json!({"id":901,"name":"Season 1","episodes":[
            {"id":1,"season_number":1,"episode_number":1,"name":"Episode 1","overview":"One","still_path":"/one.jpg","air_date":"2022-01-01"},
            {"id":2,"season_number":1,"episode_number":2,"name":"Episode 2","overview":"Two","still_path":"/two.jpg","air_date":"2022-01-02"},
            {"id":3,"season_number":1,"episode_number":3,"name":"Episode 3","overview":"Three","still_path":"/three.jpg","air_date":"2022-01-03"}
        ]})).into_response();
    }
    if path.starts_with("/search/multi") {
        let query = uri.query().unwrap_or_default();
        let results = if query.contains("query=zero") {
            vec![]
        } else {
            vec![tmdb_title(2000, "Search Result", "movie")]
        };
        return Json(
            json!({"page":1,"results":results,"total_pages":1,"total_results":results.len()}),
        )
        .into_response();
    }
    if path.ends_with("/recommendations") {
        tokio::time::sleep(Duration::from_millis(120)).await;
        return Json(json!({"page":1,"results":[tmdb_title(5000, "Recommended", "movie")]}))
            .into_response();
    }
    if path.starts_with("/discover/") {
        if state.discover_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(state.discover_delay_ms)).await;
        }
        let is_tv = path.starts_with("/discover/tv");
        if is_tv && query.contains("with_genres=27") {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status_code":22,"status_message":"Invalid TV genre ID"})),
            )
                .into_response();
        }
        if query.contains("with_genres=16") && !query.contains("with_original_language=ja") {
            return (StatusCode::BAD_REQUEST, Json(json!({"status_code":22,"status_message":"Anime discovery must use Japanese original language"}))).into_response();
        }
        if is_tv && query.contains("with_keywords=986") {
            return (StatusCode::BAD_REQUEST, Json(json!({"status_code":22,"status_message":"Keyword lookup must select the exact Horror name"}))).into_response();
        }
        let (id, name, media) = if is_tv {
            (3001, "Discover TV", "tv")
        } else {
            (3000, "Discover Movie", "movie")
        };
        return Json(json!({"page":1,"results":[tmdb_title(id,name,media)]})).into_response();
    }
    if path == "/movie/popular" {
        return Json(json!({"page":1,"results":[tmdb_title(1100,"Popular Movie","movie")]}))
            .into_response();
    }
    if path == "/tv/popular" {
        return Json(json!({"page":1,"results":[tmdb_title(1200,"Popular TV","tv")]}))
            .into_response();
    }
    if path == "/trending/all/week" {
        return Json(json!({"page":1,"results":[tmdb_title(1000,"Trending","movie")]}))
            .into_response();
    }
    if path.starts_with("/tv/") {
        let id = path
            .split('/')
            .nth(2)
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(999);
        return Json(tmdb_title(id, "Mock TV", "tv")).into_response();
    }
    if path.starts_with("/movie/") {
        let id = path
            .split('/')
            .nth(2)
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(1);
        return Json(tmdb_title(id, "Hydrated Movie", "movie")).into_response();
    }
    Json(json!({"results":[]})).into_response()
}

async fn start_mock()
-> Result<(String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>), Box<dyn std::error::Error>> {
    start_mock_with_delay(0).await
}

async fn start_mock_with_delay(
    delay_ms: u64,
) -> Result<(String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>), Box<dyn std::error::Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback(mock_tmdb).with_state(MockState {
        calls: calls.clone(),
        requests: requests.clone(),
        discover_delay_ms: delay_ms,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((base, calls, requests))
}

async fn start_server(
    catalog: Arc<CatalogStore>,
    accounts: Arc<AccountStore>,
    household: String,
    worker: Option<Arc<CatalogWorker>>,
) -> Result<String, Box<dyn std::error::Error>> {
    let state = AppState::new(catalog, accounts, Arc::new(NoopMediaEngine), household)
        .with_catalog_worker(worker);
    let router = routes::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Ok(base)
}

async fn wait_status(
    client: &reqwest::Client,
    url: &str,
    wanted: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    for _ in 0..300 {
        let value: Value = client.get(url).send().await?.json().await?;
        if value["status"] == wanted {
            return Ok(value);
        }
        if value["status"] == "failed" || value["status"] == "unavailable" {
            return Err(format!(
                "generation ended as {}: {}",
                value["status"], value["reason"]
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("timed out waiting for {wanted} at {url}").into())
}

async fn wait_http_ok(
    client: &reqwest::Client,
    url: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    for _ in 0..300 {
        let response = client.get(url).send().await?;
        if response.status() == StatusCode::OK {
            return Ok(response.json().await?);
        }
        if response.status() != StatusCode::ACCEPTED {
            return Err(format!("unexpected response {}", response.status()).into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("timed out polling {url}").into())
}

async fn make_playback(
    client: &reqwest::Client,
    base: &str,
    profile_id: &str,
    episode: i64,
) -> Result<String, Box<dyn std::error::Error>> {
    let response: Value = client.post(format!("{base}/api/v1/profiles/{profile_id}/playback"))
        .header("Idempotency-Key", format!("series_episode_{episode}"))
        .json(&json!({"type":"tv","id":700,"season":1,"episode":episode,"device":{"formats":["hls"],"codecs":["avc1"],"supports_encrypted_hls":true}}))
        .send().await?.error_for_status()?.json().await?;
    Ok(response["id"].as_str().unwrap().to_owned())
}

async fn complete_playback(
    client: &reqwest::Client,
    base: &str,
    profile_id: &str,
    playback_id: &str,
) -> TestResult {
    let response = client.put(format!("{base}/api/v1/profiles/{profile_id}/progress"))
        .json(&json!({"playback_id":playback_id,"sequence":1,"position_seconds":30.0,"duration_seconds":30.0,"watched_seconds":30.0}))
        .send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn cold_install_onboarding_profile_recommendations_and_restart_cache() -> TestResult {
    let (tmdb, calls, requests) = start_mock().await?;
    let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(64).await?).await?);
    let accounts =
        Arc::new(AccountStore::from_database(Database::in_memory(64).await?, [0x5c; 32]).await?);
    let household = accounts.select_household(None).await?;
    let worker = Arc::new(CatalogWorker::with_config(
        catalog.clone(),
        CatalogWorkerConfig::new(None, Some("test-key".into())).with_base_url(tmdb.clone()),
    )?);
    let base = start_server(
        catalog.clone(),
        accounts.clone(),
        household.clone(),
        Some(worker.clone()),
    )
    .await?;
    let client = reqwest::Client::new();

    let browse: Value = client
        .get(format!("{base}/api/v1/browse"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(browse["status"], "unavailable");
    client.get(format!("{base}/api/v1/status")).send().await?;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "cold server must not contact TMDB before profile creation"
    );

    let profile: Value = client.post(format!("{base}/api/v1/profiles"))
        .json(&json!({"name":"First","color":"#446e88","preferences":{"completed":false,"format":"both","genres":["comedy"]}}))
        .send().await?.error_for_status()?.json().await?;
    let first = profile["id"].as_str().unwrap().to_owned();
    wait_status(
        &client,
        &format!("{base}/api/v1/profiles/{first}/catalog-status"),
        "ready",
    )
    .await?;
    assert!(calls.load(Ordering::SeqCst) > 0);
    let browse: Value = client
        .get(format!("{base}/api/v1/browse"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(browse["status"], "ready");
    let shelves = browse["records"].as_array().unwrap();
    assert!(!shelves.is_empty());
    let general_titles: Vec<_> = shelves
        .iter()
        .flat_map(|record| record["data"]["titles"].as_array().into_iter().flatten())
        .filter_map(|title| title["title"].as_str())
        .collect();
    for title in ["Trending", "Popular Movie", "Popular TV"] {
        assert_eq!(
            general_titles
                .iter()
                .filter(|actual| **actual == title)
                .count(),
            1,
            "public browse must include one real {title} result: {general_titles:?}"
        );
    }
    assert!(
        !general_titles
            .iter()
            .any(|title| matches!(*title, "Avengers: Infinity War" | "Barbie" | "The Batman")),
        "public browse must not expose hardcoded starter fixtures: {general_titles:?}"
    );

    let p1 = make_playback(&client, &base, &first, 1).await?;
    complete_playback(&client, &base, &first, &p1).await?;
    let p2 = make_playback(&client, &base, &first, 2).await?;
    complete_playback(&client, &base, &first, &p2).await?;
    for id in [101, 102, 103] {
        let response = client
            .put(format!(
                "{base}/api/v1/profiles/{first}/favorites/movie/{id}"
            ))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    let prefs = client
        .put(format!("{base}/api/v1/profiles/{first}/preferences"))
        .json(&json!({"completed":true,"format":"movie","genres":["comedy"]}))
        .send()
        .await?;
    assert_eq!(prefs.status(), StatusCode::OK);
    // Change taste while the initial recommendation request is still running;
    // the final persisted snapshot must reflect this latest favorite.
    let concurrent_favorite = client
        .put(format!(
            "{base}/api/v1/profiles/{first}/favorites/movie/104"
        ))
        .send()
        .await?;
    assert_eq!(concurrent_favorite.status(), StatusCode::NO_CONTENT);
    wait_status(
        &client,
        &format!("{base}/api/v1/profiles/{first}/catalog-status"),
        "ready",
    )
    .await?;
    let favorites: Value = client
        .get(format!("{base}/api/v1/profiles/{first}/favorites"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(favorites["titles"].as_array().unwrap().len(), 4);
    let recs: Value = client
        .get(format!("{base}/api/v1/profiles/{first}/recommendations"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(recs["status"], "ready");
    assert_recommendations_record(&recs, &first, "fresh");
    assert!(
        !recs["data"]["sections"].as_array().unwrap().is_empty(),
        "series history must aggregate by show before recommendation validation"
    );
    let old_fingerprint = catalog
        .get(
            "recommendations",
            &first,
            std::time::SystemTime::now().into(),
        )
        .await?
        .data["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    let favorite = client
        .put(format!(
            "{base}/api/v1/profiles/{first}/favorites/movie/105"
        ))
        .send()
        .await?;
    assert_eq!(favorite.status(), StatusCode::NO_CONTENT);
    wait_status(
        &client,
        &format!("{base}/api/v1/profiles/{first}/catalog-status"),
        "ready",
    )
    .await?;
    let favorite_fingerprint = catalog
        .get(
            "recommendations",
            &first,
            std::time::SystemTime::now().into(),
        )
        .await?
        .data["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(
        old_fingerprint, favorite_fingerprint,
        "favorite changes must invalidate the profile recommendation snapshot"
    );

    let second_profile: Value = client
        .post(format!("{base}/api/v1/profiles"))
        .json(&json!({"name":"Second","color":"#76618e"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let second = second_profile["id"].as_str().unwrap();
    wait_status(
        &client,
        &format!("{base}/api/v1/profiles/{second}/catalog-status"),
        "ready",
    )
    .await?;
    let seen = requests.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|request| request.starts_with("/search/keyword?")
                && request.to_ascii_lowercase().contains("query=horror")),
        "horror TV discovery must resolve a keyword by name: {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|request| request.starts_with("/discover/tv?")
                && request.contains("with_keywords=987")
                && !request.contains("with_genres=27")),
        "horror TV must use the exact-name keyword ID, never genre 27: {seen:?}"
    );
    for media in ["movie", "tv"] {
        assert!(
            seen.iter().any(
                |request| request.starts_with(&format!("/discover/{media}?"))
                    && request.contains("with_genres=16")
                    && request.contains("with_original_language=ja")
            ),
            "Japanese anime discovery must cover {media}: {seen:?}"
        );
    }
    let second_recs: Value = client
        .get(format!("{base}/api/v1/profiles/{second}/recommendations"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(second_recs["status"], "unavailable");
    assert!(
        second_recs["data"]["sections"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let changed = client
        .put(format!("{base}/api/v1/profiles/{first}/preferences"))
        .json(&json!({"completed":true,"format":"tv","genres":["comedy"]}))
        .send()
        .await?;
    assert_eq!(changed.status(), StatusCode::OK);
    wait_status(
        &client,
        &format!("{base}/api/v1/profiles/{first}/catalog-status"),
        "ready",
    )
    .await?;
    let new_fingerprint = catalog
        .get(
            "recommendations",
            &first,
            std::time::SystemTime::now().into(),
        )
        .await?
        .data["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(
        favorite_fingerprint, new_fingerprint,
        "format changes must invalidate the profile recommendation snapshot"
    );

    let calls_before_restart = calls.load(Ordering::SeqCst);
    let restarted =
        start_server(catalog.clone(), accounts.clone(), household, Some(worker)).await?;
    let restarted_status = wait_status(
        &client,
        &format!("{restarted}/api/v1/profiles/{first}/catalog-status"),
        "ready",
    )
    .await?;
    assert_eq!(restarted_status["status"], "ready");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        calls_before_restart,
        "fresh matching persisted generation must recover without an upstream call"
    );

    let fresh = catalog
        .get(
            "recommendations",
            &first,
            std::time::SystemTime::now().into(),
        )
        .await?;
    catalog
        .put(
            CatalogWrite {
                kind: "recommendations".into(),
                key: first.clone(),
                data: fresh.data,
                ttl: chrono::Duration::seconds(-1),
            },
            std::time::SystemTime::now().into(),
        )
        .await?;
    let offline = start_server(
        catalog.clone(),
        accounts.clone(),
        // Use a second household lookup through the same account store.
        accounts.select_household(None).await?,
        None,
    )
    .await?;
    let offline_status: Value = client
        .get(format!("{offline}/api/v1/profiles/{first}/catalog-status"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(offline_status["status"], "ready");
    assert_eq!(
        offline_status["stale"], true,
        "matching expired data remains usable when no key is configured"
    );
    let offline_recs: Value = client
        .get(format!("{offline}/api/v1/profiles/{first}/recommendations"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(offline_recs["status"], "ready");
    assert_eq!(offline_recs["stale"], true);
    assert_recommendations_record(&offline_recs, &first, "stale");
    assert!(
        !offline_recs["data"]["sections"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let snapshot: Value = client
        .get(format!("{offline}/api/v1/snapshot"))
        .send()
        .await?
        .json()
        .await?;
    assert!(
        snapshot["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["kind"] != "recommendations"
                && record["kind"] != "profile_generation")
    );
    assert!(
        !snapshot.to_string().contains(&first),
        "public snapshot must not include profile IDs or fingerprints"
    );
    let deleted = client
        .delete(format!("{offline}/api/v1/profiles/{first}"))
        .send()
        .await?;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(
        catalog
            .get(
                "recommendations",
                &first,
                std::time::SystemTime::now().into()
            )
            .await
            .is_err()
    );
    assert!(
        catalog
            .get(
                "profile_generation",
                &first,
                std::time::SystemTime::now().into()
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn lazy_hydration_zero_results_and_upstream_failure_are_explicit() -> TestResult {
    let (tmdb, calls, _) = start_mock().await?;
    let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(64).await?).await?);
    let accounts =
        Arc::new(AccountStore::from_database(Database::in_memory(64).await?, [0x5c; 32]).await?);
    let household = accounts.select_household(None).await?;
    let worker = Arc::new(CatalogWorker::with_config(
        catalog.clone(),
        CatalogWorkerConfig::new(None, Some("test-key".into())).with_base_url(tmdb.clone()),
    )?);
    let base = start_server(catalog.clone(), accounts.clone(), household, Some(worker)).await?;
    let client = reqwest::Client::new();

    let profile: Value = client.post(format!("{base}/api/v1/profiles"))
        .json(&json!({"name":"Hydration","color":"#123456","preferences":{"completed":false,"format":"both","genres":["comedy"]}}))
        .send().await?.error_for_status()?.json().await?;
    let profile_id = profile["id"].as_str().unwrap();
    wait_status(
        &client,
        &format!("{base}/api/v1/profiles/{profile_id}/catalog-status"),
        "ready",
    )
    .await?;

    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    catalog.put(CatalogWrite { kind:"title".into(), key:"movie:444".into(), data:json!({"type":"movie","id":444,"title":"Old Fixture","overview":"Old","date":"2020-01-01","rating":7.0,"poster":"","backdrop":"","imdb_id":"","mal_id":0,"runtime":0,"genres":[],"seasons":[],"details":true}), ttl:chrono::Duration::days(365) }, now).await?;
    let before_legacy_refresh = calls.load(Ordering::SeqCst);
    let legacy_url = format!("{base}/api/v1/titles/movie/444");
    let legacy = client.get(&legacy_url).send().await?;
    assert_eq!(
        legacy.status(),
        StatusCode::OK,
        "legacy cached metadata remains immediately usable"
    );
    let legacy_body: Value = legacy.json().await?;
    assert_eq!(legacy_body["status"], "pending");
    for _ in 0..200 {
        let cached = catalog
            .title("movie:444", std::time::SystemTime::now().into())
            .await?;
        if cached.data["title"] == "Hydrated Movie" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        catalog
            .title("movie:444", std::time::SystemTime::now().into())
            .await?
            .data["title"],
        "Hydrated Movie",
        "legacy metadata refresh did not finish before the polling deadline"
    );
    assert!(
        calls.load(Ordering::SeqCst) > before_legacy_refresh,
        "365-day legacy fixture metadata must be refreshed in place"
    );

    let title_url = format!("{base}/api/v1/titles/movie/3000");
    let first = client.get(&title_url).send().await?;
    assert_eq!(
        first.status(),
        StatusCode::ACCEPTED,
        "browse summary titles must hydrate details before satisfying title detail reads"
    );
    let hydrated = wait_http_ok(&client, &title_url).await?;
    assert_eq!(hydrated["data"]["details"], true);
    assert_eq!(hydrated["data"]["imdb_id"], "tt1234567");

    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    catalog
        .put(
            CatalogWrite {
                kind: "season".into(),
                key: "tv:999:1".into(),
                data: json!({"show_id":999,"season":1,"name":"Season 1","episodes":[
                    {"tmdb_episode_id":1,"season":1,"episode":1,"name":"Episode 1"},
                    {"tmdb_episode_id":2,"season":1,"episode":2,"name":"Episode 2"}
                ]}),
                ttl: chrono::Duration::days(1),
            },
            now,
        )
        .await?;
    let episode_url = format!("{base}/api/v1/tv/999/seasons/1/episodes/3");
    let first = client.get(&episode_url).send().await?;
    assert_eq!(
        first.status(),
        StatusCode::ACCEPTED,
        "a missing episode in a cached partial season must force season hydration"
    );
    let episode = wait_http_ok(&client, &episode_url).await?;
    assert_eq!(episode["data"]["episode"], 3);
    let season: Value = client
        .get(format!("{base}/api/v1/tv/999/seasons/1"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(season["data"]["episodes"].as_array().unwrap().len(), 3);

    let search_url = format!("{base}/api/v1/search-results?q=zero");
    let first = client.get(&search_url).send().await?;
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let empty = wait_http_ok(&client, &search_url).await?;
    assert_eq!(empty["kind"], "search");
    assert_eq!(
        empty["data"],
        json!([]),
        "zero upstream results should be a completed cached result"
    );
    let search: Value = client
        .get(format!("{base}/api/v1/search?q=zero"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(search["status"], "ready");
    assert!(search["records"].as_array().unwrap().is_empty());

    // A completed job must not suppress a new lookup if its cached result disappears.
    let calls_before = calls.load(Ordering::SeqCst);
    catalog.delete("search", "zero").await?;
    let search_url = format!("{base}/api/v1/search?q=zero");
    assert_eq!(
        client.get(&search_url).send().await?.status(),
        StatusCode::ACCEPTED
    );
    wait_status(&client, &search_url, "ready").await?;
    assert!(calls.load(Ordering::SeqCst) > calls_before);
    // An expired but valid search snapshot remains readable during background refresh.
    catalog
        .put(
            stream_storage::catalog::CatalogWrite {
                kind: "search".into(),
                key: "zero".into(),
                data: json!([]),
                ttl: chrono::Duration::seconds(-1),
            },
            std::time::SystemTime::now().into(),
        )
        .await?;
    let stale_search: Value = client.get(&search_url).send().await?.json().await?;
    assert_eq!(stale_search["status"], "ready");
    assert_eq!(stale_search["stale"], true);

    let failure_url = format!("{base}/api/v1/titles/movie/99999");
    let first = client.get(&failure_url).send().await?;
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let failure = wait_http_ok(&client, &failure_url).await?;
    assert_eq!(failure["status"], "failed");
    assert!(
        failure["reason"]
            .as_str()
            .unwrap()
            .contains("lookup failed")
    );
    Ok(())
}

#[tokio::test]
async fn unconfigured_profile_is_explicit_and_never_fetches() -> TestResult {
    let (tmdb, calls, requests) = start_mock().await?;
    let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(64).await?).await?);
    let accounts =
        Arc::new(AccountStore::from_database(Database::in_memory(64).await?, [0x5c; 32]).await?);
    let household = accounts.select_household(None).await?;
    let worker = Arc::new(CatalogWorker::with_config(
        catalog.clone(),
        CatalogWorkerConfig::new(None, None).with_base_url(tmdb),
    )?);
    let base = start_server(catalog.clone(), accounts, household, Some(worker)).await?;
    let client = reqwest::Client::new();
    let profile: Value = client.post(format!("{base}/api/v1/profiles"))
        .json(&json!({"name":"Needs a key","color":"#123456","preferences":{"completed":false,"format":"both","genres":["horror"]}}))
        .send().await?.error_for_status()?.json().await?;
    let profile_id = profile["id"].as_str().unwrap();
    let status: Value = client
        .get(format!(
            "{base}/api/v1/profiles/{profile_id}/catalog-status"
        ))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(status["status"], "unavailable");
    assert!(
        status["reason"]
            .as_str()
            .unwrap()
            .contains("personal TMDB key")
    );
    let recs: Value = client
        .get(format!(
            "{base}/api/v1/profiles/{profile_id}/recommendations"
        ))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(recs["status"], "unavailable");
    assert!(recs["data"]["sections"].as_array().unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(requests.lock().unwrap().is_empty());
    Ok(())
}

#[tokio::test]
async fn deleting_profile_during_generation_does_not_restore_profile_metadata() -> TestResult {
    let (tmdb, _, _) = start_mock_with_delay(180).await?;
    let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(64).await?).await?);
    let accounts =
        Arc::new(AccountStore::from_database(Database::in_memory(64).await?, [0x5c; 32]).await?);
    let household = accounts.select_household(None).await?;
    let worker = Arc::new(CatalogWorker::with_config(
        catalog.clone(),
        CatalogWorkerConfig::new(None, Some("test-key".into())).with_base_url(tmdb.clone()),
    )?);
    let base = start_server(catalog.clone(), accounts.clone(), household, Some(worker)).await?;
    let client = reqwest::Client::new();
    let profile: Value = client.post(format!("{base}/api/v1/profiles"))
        .json(&json!({"name":"Delete during warmup","color":"#123456","preferences":{"completed":false,"format":"both","genres":["comedy"]}}))
        .send().await?.error_for_status()?.json().await?;
    let profile_id = profile["id"].as_str().unwrap();
    for _ in 0..200 {
        if catalog
            .list("shelf", 100, std::time::SystemTime::now().into())
            .await?
            .iter()
            .any(|record| record.key.starts_with("favorites-movie-comedy"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let deleted = client
        .delete(format!("{base}/api/v1/profiles/{profile_id}"))
        .send()
        .await?;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(
        catalog
            .get(
                "profile_generation",
                profile_id,
                std::time::SystemTime::now().into()
            )
            .await
            .is_err()
    );
    assert!(
        catalog
            .get(
                "recommendations",
                profile_id,
                std::time::SystemTime::now().into()
            )
            .await
            .is_err()
    );
    let get = client
        .get(format!(
            "{base}/api/v1/profiles/{profile_id}/catalog-status"
        ))
        .send()
        .await?;
    assert_eq!(get.status(), StatusCode::NOT_FOUND);
    Ok(())
}
