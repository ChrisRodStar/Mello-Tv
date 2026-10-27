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

async fn spawn_test_server()
-> Result<(String, Arc<AccountStore>, Arc<CatalogStore>, String), Box<dyn std::error::Error>> {
    let catalog_db = Database::in_memory(64).await?;
    let catalog = Arc::new(CatalogStore::from_database(catalog_db).await?);

    let accounts_db = Database::in_memory(64).await?;
    let key = [0x5cu8; 32];
    let accounts = Arc::new(AccountStore::from_database(accounts_db, key).await?);
    let household_id = accounts.select_household(None).await?;

    let media = Arc::new(NoopMediaEngine);
    let state = AppState::new(
        catalog.clone(),
        accounts.clone(),
        media,
        household_id.clone(),
    );

    let router = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok((base_url, accounts, catalog, household_id))
}

#[tokio::test]
async fn profiles_crud_and_preferences_parity() -> TestResult {
    let (base, _, _, _) = spawn_test_server().await?;
    let client = reqwest::Client::new();

    // 1. Create Profile
    let res = client
        .post(format!("{base}/api/v1/profiles"))
        .json(&json!({
            "name": "Alice",
            "color": "#446e88",
            "preferences": {
                "completed": false,
                "format": "both",
                "genres": ["action", "scifi"]
            }
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::CREATED);
    let profile: Value = res.json().await?;
    let profile_id = profile["id"].as_str().unwrap().to_string();
    assert_eq!(profile["name"], "Alice");
    assert_eq!(profile["color"], "#446e88");
    assert_eq!(profile["preferences"]["format"], "both");

    // 2. List Profiles
    let res = client.get(format!("{base}/api/v1/profiles")).send().await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let list: Value = res.json().await?;
    let profiles = list["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["id"], profile_id);

    // 3. Get Profile by ID
    let res = client
        .get(format!("{base}/api/v1/profiles/{profile_id}"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let fetched: Value = res.json().await?;
    assert_eq!(fetched["id"], profile_id);

    // 4. Update Profile Name/Color
    let res = client
        .put(format!("{base}/api/v1/profiles/{profile_id}"))
        .json(&json!({
            "name": "Alice Cooper",
            "color": "#916050"
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let updated: Value = res.json().await?;
    assert_eq!(updated["name"], "Alice Cooper");
    assert_eq!(updated["color"], "#916050");

    // 5. Update Preferences
    let res = client
        .put(format!("{base}/api/v1/profiles/{profile_id}/preferences"))
        .json(&json!({
            "completed": true,
            "format": "movie",
            "genres": ["comedy", "drama"]
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let with_prefs: Value = res.json().await?;
    assert_eq!(with_prefs["preferences"]["completed"], true);
    assert_eq!(with_prefs["preferences"]["format"], "movie");

    // 6. Record Activity
    let res = client
        .post(format!("{base}/api/v1/profiles/{profile_id}/activity"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

    Ok(())
}

#[tokio::test]
async fn custom_lists_and_recommendation_feedback_parity() -> TestResult {
    let (base, _, catalog, _) = spawn_test_server().await?;
    let client = reqwest::Client::new();
    let instant = now();

    // Populate a sample movie in catalog
    catalog
        .put(
            CatalogWrite {
                kind: "title".into(),
                key: "movie:101".into(),
                data: json!({
                    "type": "movie",
                    "id": 101,
                    "title": "Inception",
                    "overview": "Dream within a dream",
                    "rating": 8.8,
                    "poster": "https://image.tmdb.org/t/p/w500/inception.jpg",
                    "details": true
                }),
                ttl: Duration::days(7),
            },
            instant,
        )
        .await?;

    // Create Profile
    let res = client
        .post(format!("{base}/api/v1/profiles"))
        .json(&json!({ "name": "Bob", "color": "#616b4b" }))
        .send()
        .await?;
    let profile: Value = res.json().await?;
    let pid = profile["id"].as_str().unwrap();

    // Test Favorites
    let res = client
        .put(format!("{base}/api/v1/profiles/{pid}/favorites/movie/101"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

    let res = client
        .get(format!("{base}/api/v1/profiles/{pid}/favorites"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let fav_reply: Value = res.json().await?;
    assert_eq!(fav_reply["titles"].as_array().unwrap().len(), 1);
    assert_eq!(fav_reply["titles"][0]["id"], 101);

    // Test Library (Watchlist)
    let res = client
        .put(format!("{base}/api/v1/profiles/{pid}/library/movie/101"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

    let res = client
        .get(format!("{base}/api/v1/profiles/{pid}/library"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let lib_reply: Value = res.json().await?;
    assert_eq!(lib_reply["titles"].as_array().unwrap().len(), 1);

    // Test Recommendation Hidden, Watched, Ignored
    for choice in [
        "recommendation-hidden",
        "recommendation-watched",
        "recommendation-ignored",
    ] {
        let res = client
            .put(format!("{base}/api/v1/profiles/{pid}/{choice}/movie/101"))
            .send()
            .await?;
        assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

        let res = client
            .get(format!("{base}/api/v1/profiles/{pid}/{choice}"))
            .send()
            .await?;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        let reply: Value = res.json().await?;
        assert_eq!(reply["titles"].as_array().unwrap().len(), 1);
    }

    // Test Recommendation Topics
    let res = client
        .put(format!(
            "{base}/api/v1/profiles/{pid}/recommendation-topics/topic-action"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

    let res = client
        .get(format!(
            "{base}/api/v1/profiles/{pid}/recommendation-topics"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let topics_reply: Value = res.json().await?;
    assert_eq!(
        topics_reply["topics"].as_array().unwrap(),
        &vec![json!("topic-action")]
    );

    // Test Recommendation Feedback
    let res = client
        .get(format!(
            "{base}/api/v1/profiles/{pid}/recommendation-feedback"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let feedback: Value = res.json().await?;
    assert_eq!(feedback["favorites"].as_array().unwrap().len(), 1);
    assert_eq!(feedback["hidden"].as_array().unwrap().len(), 1);
    assert_eq!(feedback["watched"].as_array().unwrap().len(), 1);
    assert_eq!(feedback["ignored"].as_array().unwrap().len(), 1);
    assert_eq!(
        feedback["topics"].as_array().unwrap(),
        &vec![json!("topic-action")]
    );

    // Delete topic
    let res = client
        .delete(format!(
            "{base}/api/v1/profiles/{pid}/recommendation-topics/topic-action"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

    // Delete from favorites
    let res = client
        .delete(format!("{base}/api/v1/profiles/{pid}/favorites/movie/101"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

    Ok(())
}

#[tokio::test]
async fn home_and_recommendations_parity() -> TestResult {
    let (base, _, catalog, _) = spawn_test_server().await?;
    let client = reqwest::Client::new();
    let instant = now();

    // Populate a shelf in catalog
    catalog
        .put(
            CatalogWrite {
                kind: "shelf".into(),
                key: "shelf-trending".into(),
                data: json!({
                    "id": "shelf-trending",
                    "heading": "Trending Now",
                    "titles": [
                        {
                            "type": "movie",
                            "id": 201,
                            "title": "Interstellar",
                            "overview": "Space exploration",
                            "rating": 8.7,
                            "poster": "https://image.tmdb.org/t/p/w500/interstellar.jpg",
                            "details": true
                        }
                    ]
                }),
                ttl: Duration::days(7),
            },
            instant,
        )
        .await?;

    let res = client
        .post(format!("{base}/api/v1/profiles"))
        .json(&json!({ "name": "Charlie", "color": "#76618e" }))
        .send()
        .await?;
    let profile: Value = res.json().await?;
    let pid = profile["id"].as_str().unwrap();

    // GET home
    let res = client
        .get(format!(
            "{base}/api/v1/profiles/{pid}/home?type=all&topic=all"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let home: Value = res.json().await?;
    let sections = home["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0]["id"], "shelf-trending");
    assert!(home.get("page_version").is_some());

    // GET recommendations
    let res = client
        .get(format!(
            "{base}/api/v1/profiles/{pid}/recommendations?type=movie"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let recs: Value = res.json().await?;
    let rec_sections = recs["data"]["sections"].as_array().unwrap();
    assert_eq!(rec_sections.len(), 1);
    assert_eq!(rec_sections[0]["id"], "shelf-trending");

    Ok(())
}

#[tokio::test]
async fn playback_lifecycle_and_progress_parity() -> TestResult {
    let (base, _, _, _) = spawn_test_server().await?;
    let client = reqwest::Client::new();

    let res = client
        .post(format!("{base}/api/v1/profiles"))
        .json(&json!({ "name": "Dave", "color": "#93773c" }))
        .send()
        .await?;
    let profile: Value = res.json().await?;
    let pid = profile["id"].as_str().unwrap();

    // 1. Resolve / Create Playback
    let res = client
        .post(format!("{base}/api/v1/profiles/{pid}/playback"))
        .header("Idempotency-Key", "test_key_001")
        .json(&json!({
            "type": "movie",
            "id": 501,
            "device": {
                "formats": ["hls", "mp4"],
                "codecs": ["avc1", "mp4a"],
                "supports_encrypted_hls": true
            }
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let pb: Value = res.json().await?;

    let pbid = pb["id"].as_str().unwrap();
    assert!(pb["url"].as_str().unwrap().starts_with("/stream/"));
    assert_eq!(pb["format"], "hls");
    assert_eq!(pb["revision"], 1);
    assert_eq!(pb["source_generation"], 1);
    assert_eq!(pb["reload_required"], false);
    assert_eq!(pb["title"]["type"], "movie");
    assert_eq!(pb["title"]["id"], 501);

    // 2. Idempotent replay with same key and body returns identical session
    let res = client
        .post(format!("{base}/api/v1/profiles/{pid}/playback"))
        .header("Idempotency-Key", "test_key_001")
        .json(&json!({
            "type": "movie",
            "id": 501,
            "device": {
                "formats": ["hls", "mp4"],
                "codecs": ["avc1", "mp4a"],
                "supports_encrypted_hls": true
            }
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let replay: Value = res.json().await?;
    assert_eq!(replay["id"], pbid);

    // 2b. Same key with different payload returns 409 idempotency conflict
    let res = client
        .post(format!("{base}/api/v1/profiles/{pid}/playback"))
        .header("Idempotency-Key", "test_key_001")
        .json(&json!({
            "type": "movie",
            "id": 502
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::CONFLICT);

    // 3. GET Playback
    let res = client
        .get(format!("{base}/api/v1/profiles/{pid}/playback/{pbid}"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let get_pb: Value = res.json().await?;
    assert_eq!(get_pb["id"], pbid);
    assert_eq!(get_pb["title"]["id"], 501);

    // 4. Update Viewing Progress
    // Sequence 1: 10s watched out of 30s duration
    let res = client
        .put(format!("{base}/api/v1/profiles/{pid}/progress"))
        .json(&json!({
            "playback_id": pbid,
            "sequence": 1,
            "position_seconds": 10.0,
            "duration_seconds": 30.0,
            "watched_seconds": 10.0
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let prog1: Value = res.json().await?;
    assert_eq!(prog1["position_seconds"], 10.0);
    assert_eq!(prog1["duration_seconds"], 30.0);
    assert_eq!(prog1["watched_seconds"], 10.0);
    assert_eq!(prog1["completed"], false);

    // Counter regression rejection: watched < 10.0
    let res = client
        .put(format!("{base}/api/v1/profiles/{pid}/progress"))
        .json(&json!({
            "playback_id": pbid,
            "sequence": 2,
            "position_seconds": 12.0,
            "duration_seconds": 30.0,
            "watched_seconds": 5.0
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::CONFLICT);

    // Sequence 2: seek backwards to 5s, total watched increases to 18s
    let res = client
        .put(format!("{base}/api/v1/profiles/{pid}/progress"))
        .json(&json!({
            "playback_id": pbid,
            "sequence": 2,
            "position_seconds": 5.0,
            "duration_seconds": 30.0,
            "watched_seconds": 18.0
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let prog2: Value = res.json().await?;
    assert_eq!(prog2["position_seconds"], 5.0);
    assert_eq!(prog2["watched_seconds"], 18.0);

    // Sequence 3: reach completion threshold (>= 90% position and >= 60% watched)
    let res = client
        .put(format!("{base}/api/v1/profiles/{pid}/progress"))
        .json(&json!({
            "playback_id": pbid,
            "sequence": 3,
            "position_seconds": 28.0,
            "duration_seconds": 30.0,
            "watched_seconds": 25.0
        }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let prog3: Value = res.json().await?;
    assert_eq!(prog3["completed"], true);

    // 5. Resume Coordinates
    let res = client
        .get(format!(
            "{base}/api/v1/profiles/{pid}/resume?type=movie&id=501"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let resume: Value = res.json().await?;
    assert_eq!(resume["position_seconds"], 28.0);

    // Resume non-existent -> 404
    let res = client
        .get(format!(
            "{base}/api/v1/profiles/{pid}/resume?type=movie&id=99999"
        ))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);

    // 6. History Pagination
    let res = client
        .get(format!("{base}/api/v1/profiles/{pid}/history?offset=0"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let hist: Value = res.json().await?;
    assert_eq!(hist["records"].as_array().unwrap().len(), 1);
    assert_eq!(hist["offset"], 0);
    assert_eq!(hist["limit"], 100);

    // 7. Lease Renewal (renew)
    let res = client
        .post(format!(
            "{base}/api/v1/profiles/{pid}/playback/{pbid}/renew"
        ))
        .header("Idempotency-Key", "renew_key_01")
        .json(&json!({ "expected_revision": 1 }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let renewed: Value = res.json().await?;
    assert_eq!(renewed["revision"], 2);
    assert_eq!(renewed["source_generation"], 1);
    assert_eq!(renewed["reload_required"], false);

    // Revision conflict (expected 1 when revision is now 2) -> 409
    let res = client
        .post(format!(
            "{base}/api/v1/profiles/{pid}/playback/{pbid}/renew"
        ))
        .header("Idempotency-Key", "renew_key_02")
        .json(&json!({ "expected_revision": 1 }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::CONFLICT);

    // 8. Failover (backup)
    let res = client
        .post(format!(
            "{base}/api/v1/profiles/{pid}/playback/{pbid}/backup"
        ))
        .header("Idempotency-Key", "backup_key_01")
        .json(&json!({ "expected_revision": 2 }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let backed_up: Value = res.json().await?;
    assert_eq!(backed_up["revision"], 3);
    assert_eq!(backed_up["source_generation"], 2);
    assert_eq!(backed_up["reload_required"], true);

    // 9. Source Refresh (refresh)
    let res = client
        .post(format!(
            "{base}/api/v1/profiles/{pid}/playback/{pbid}/refresh"
        ))
        .header("Idempotency-Key", "refresh_key_01")
        .json(&json!({ "expected_revision": 3 }))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let refreshed: Value = res.json().await?;
    assert_eq!(refreshed["revision"], 4);
    assert_eq!(refreshed["source_generation"], 3);
    assert_eq!(refreshed["reload_required"], true);

    // 10. Revoke Session
    let res = client
        .delete(format!("{base}/api/v1/profiles/{pid}/playback/{pbid}"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::NO_CONTENT);

    // After revoke, GET returns 410 Gone
    let res = client
        .get(format!("{base}/api/v1/profiles/{pid}/playback/{pbid}"))
        .send()
        .await?;
    assert_eq!(res.status(), reqwest::StatusCode::GONE);

    Ok(())
}
