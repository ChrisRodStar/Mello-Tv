#![allow(clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use stream_core::{
    error::DomainError,
    ports::MediaEngine,
    providers::{Lookup, Selection},
};
use stream_server::{routes, state::AppState};
use stream_storage::{AccountStore, catalog::CatalogStore, db::Database};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct ScriptedMedia {
    selections: Mutex<VecDeque<Selection>>,
    lookups: Mutex<Vec<Lookup>>,
    fail_infrastructure: AtomicBool,
}

#[async_trait]
impl MediaEngine for ScriptedMedia {
    async fn resolve_stream(&self, lookup: &Lookup) -> Result<Selection, DomainError> {
        self.lookups.lock().unwrap().push(lookup.clone());
        if self.fail_infrastructure.load(Ordering::SeqCst) {
            return Err(DomainError::Infrastructure(
                std::io::Error::other("synthetic failure").into(),
            ));
        }
        self.selections
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| DomainError::NotFound("no source".into()))
    }
}

fn selection(height: i64) -> Selection {
    serde_json::from_value(json!({
        "version":"providers.v1", "rejected":null, "backups":[], "providers":null,
        "selected":{
            "score":50.0, "reasons":null,
            "candidate":{
                "source":{
                    "id":format!("source-{height}"), "provider":"vidfast",
                    "url":format!("https://media.example/{height}/master.m3u8"),
                    "format":"hls", "title":{"type":"tv","tmdb_id":1622,"season":1,"episode":16},
                    "fetch_context":{}, "resolver_metadata":{"tracks_known":true}, "content_verified":false
                },
                "observation":{
                    "at":chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()), "delivery_bps":30_000_000.0,
                    "variant":{"url":format!("https://media.example/{height}/index.m3u8"),
                        "width":height*16/9,"height":height,"bitrate":4_000_000,"codecs":["avc1.4d401f"]},
                    "sample_bytes":393216,"format":"hls","playlist_metadata":{"tracks_known":true},
                    "samples_completed":3,"accessible":true,"encrypted":false,"protection_known":true
                }
            }
        }
    })).unwrap()
}

async fn server(
    heights: &[i64],
) -> Result<(String, Arc<ScriptedMedia>, Arc<stream_media::MediaRelay>), Box<dyn std::error::Error>>
{
    server_with_selections(heights.iter().copied().map(selection).collect()).await
}

async fn server_with_selections(
    selections: VecDeque<Selection>,
) -> Result<(String, Arc<ScriptedMedia>, Arc<stream_media::MediaRelay>), Box<dyn std::error::Error>>
{
    let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(32).await?).await?);
    let accounts =
        Arc::new(AccountStore::from_database(Database::in_memory(32).await?, [0x42; 32]).await?);
    let household = accounts.select_household(None).await?;
    let media = Arc::new(ScriptedMedia {
        selections: Mutex::new(selections),
        lookups: Mutex::new(Vec::new()),
        fail_infrastructure: AtomicBool::new(false),
    });
    let state = AppState::new(catalog, accounts, media.clone(), household);
    let relay = state.relay.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move {
        axum::serve(listener, routes::build_router(state))
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    client
        .post(format!("{base}/api/profiles"))
        .json(&json!({"name":"Quality check","color":"#536e70"}))
        .send()
        .await?
        .error_for_status()?;
    Ok((base, media, relay))
}

#[tokio::test]
async fn higher_mp4_source_returns_reload_even_without_hls_choices() -> TestResult {
    let mut higher = selection(1080);
    let candidate = &mut higher.selected.as_mut().unwrap().candidate;
    candidate.source.format = stream_core::providers::StreamFormat::Mp4;
    candidate.source.url = "https://media.example/1080.mp4".into();
    candidate.observation.format = stream_core::providers::StreamFormat::Mp4;
    let (base, _, relay) = server_with_selections(VecDeque::from([selection(720), higher])).await?;
    let initial = create(&base).await?;
    let id = initial["id"].as_str().unwrap();
    let response: Value = reqwest::Client::new()
        .post(format!("{base}/api/playback/{id}/upgrade"))
        .json(&json!({"expected_revision":initial["revision"]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(response["format"], "mp4");
    assert_eq!(response["available_heights"], json!([]));
    assert_eq!(response["reload_required"], true);
    assert_eq!(relay.active_height(id), Some(1080));
    assert_ne!(response["url"], initial["url"]);
    Ok(())
}

async fn create(base: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(reqwest::Client::new()
        .post(format!("{base}/api/playback"))
        .json(&json!({"type":"tv","id":1622,"season":1,"episode":16,"imdb_id":"tt0460681"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

#[tokio::test]
async fn late_1080_upgrade_changes_generation_and_uses_complete_cached_discovery() -> TestResult {
    let (base, media, relay) = server(&[720, 1080]).await?;
    let initial = create(&base).await?;
    assert_eq!(initial["available_heights"], json!([720]));
    let id = initial["id"].as_str().unwrap();
    let upgraded: Value = reqwest::Client::new()
        .post(format!("{base}/api/playback/{id}/upgrade"))
        .header("Idempotency-Key", "quality-upgrade")
        .json(&json!({"expected_revision":initial["revision"]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(upgraded["available_heights"], json!([1080]));
    assert_eq!(
        upgraded["source_generation"].as_u64(),
        Some(initial["source_generation"].as_u64().unwrap() + 1)
    );
    assert_eq!(upgraded["reload_required"], true);
    assert_eq!(relay.active_height(id), Some(1080));
    let token = relay.token_for(id);
    let replayed: Value = reqwest::Client::new()
        .post(format!("{base}/api/playback/{id}/upgrade"))
        .header("Idempotency-Key", "quality-upgrade")
        .json(&json!({"expected_revision":initial["revision"]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(replayed["url"], upgraded["url"]);
    assert_eq!(replayed["revision"], upgraded["revision"]);
    assert_eq!(relay.token_for(id), token);
    let conflict = reqwest::Client::new()
        .post(format!("{base}/api/playback/{id}/upgrade"))
        .header("Idempotency-Key", "different-operation")
        .json(&json!({"expected_revision":initial["revision"]}))
        .send()
        .await?;
    assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(relay.token_for(id), token);
    let lookups = media.lookups.lock().unwrap();
    assert_eq!(lookups.len(), 2);
    assert!(lookups[1].wait_for_complete);
    assert!(!lookups[1].refresh);
    Ok(())
}

#[tokio::test]
async fn equal_or_lower_quality_is_a_noop_without_rotating_token() -> TestResult {
    for next in [480, 720] {
        let (base, _, relay) = server(&[720, next]).await?;
        let initial = create(&base).await?;
        let id = initial["id"].as_str().unwrap();
        let token = relay.token_for(id);
        let response: Value = reqwest::Client::new()
            .post(format!("{base}/api/playback/{id}/upgrade"))
            .json(&json!({"expected_revision":initial["revision"]}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(response["revision"], initial["revision"]);
        assert_eq!(response["source_generation"], initial["source_generation"]);
        assert_eq!(response["reload_required"], false);
        assert_eq!(relay.active_height(id), Some(720));
        assert_eq!(relay.token_for(id), token);
    }
    Ok(())
}

#[tokio::test]
async fn stale_upgrade_cannot_replace_the_active_source() -> TestResult {
    let (base, _, relay) = server(&[720, 1080]).await?;
    let initial = create(&base).await?;
    let id = initial["id"].as_str().unwrap();
    let token = relay.token_for(id);
    let response = reqwest::Client::new()
        .post(format!("{base}/api/playback/{id}/upgrade"))
        .json(&json!({"expected_revision":99}))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(relay.active_height(id), Some(720));
    assert_eq!(relay.token_for(id), token);
    Ok(())
}

#[tokio::test]
async fn concurrent_upgrade_requests_publish_only_one_source_generation() -> TestResult {
    let (base, media, relay) = server(&[720, 1080]).await?;
    let initial = create(&base).await?;
    let id = initial["id"].as_str().unwrap();
    let client = reqwest::Client::new();
    let request = |key| {
        client
            .post(format!("{base}/api/playback/{id}/upgrade"))
            .header("Idempotency-Key", key)
            .json(&json!({"expected_revision":initial["revision"]}))
            .send()
    };
    let (first, second) = tokio::join!(request("first-upgrade"), request("second-upgrade"));
    let mut statuses = [first?.status().as_u16(), second?.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    assert_eq!(media.lookups.lock().unwrap().len(), 2);
    assert_eq!(relay.active_height(id), Some(1080));
    let current: Value = client
        .get(format!("{base}/api/playback/{id}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(current["source_generation"], 2);
    Ok(())
}

#[tokio::test]
async fn backup_skips_variants_with_the_same_master_and_reports_when_exhausted() -> TestResult {
    let mut initial_selection = selection(720);
    let first = initial_selection.selected.as_ref().unwrap().clone();
    let mut same_master = first.clone();
    same_master.candidate.source.id = "another-variant".into();
    same_master
        .candidate
        .observation
        .variant
        .as_mut()
        .unwrap()
        .height = Some(480);
    let distinct = selection(1080).selected.unwrap();
    initial_selection.backups = Some(vec![same_master.clone(), distinct, same_master]);
    let (base, _, relay) = server_with_selections(VecDeque::from([initial_selection])).await?;
    let initial = create(&base).await?;
    let id = initial["id"].as_str().unwrap();
    let client = reqwest::Client::new();
    let changed: Value = client
        .post(format!("{base}/api/playback/{id}/backup"))
        .json(&json!({"expected_revision":initial["revision"]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(relay.active_height(id), Some(1080));
    assert_ne!(changed["url"], initial["url"]);
    let token = relay.token_for(id);
    let exhausted = client
        .post(format!("{base}/api/playback/{id}/backup"))
        .json(&json!({"expected_revision":changed["revision"]}))
        .send()
        .await?;
    assert_eq!(
        exhausted.status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );
    let error: Value = exhausted.json().await?;
    assert_eq!(error["error"], "source_refresh_required");
    assert_eq!(relay.token_for(id), token);
    Ok(())
}

#[tokio::test]
async fn failed_refresh_does_not_rotate_token_or_advance_revision() -> TestResult {
    let (base, media, relay) = server(&[720]).await?;
    let initial = create(&base).await?;
    let id = initial["id"].as_str().unwrap();
    let token = relay.token_for(id);
    media.fail_infrastructure.store(true, Ordering::SeqCst);
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/api/playback/{id}/refresh"))
        .json(&json!({"expected_revision":initial["revision"]}))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );
    let current: Value = client
        .get(format!("{base}/api/playback/{id}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(current["revision"], initial["revision"]);
    assert_eq!(current["source_generation"], initial["source_generation"]);
    assert_eq!(relay.token_for(id), token);
    assert!(media.lookups.lock().unwrap().last().unwrap().refresh);
    Ok(())
}

#[tokio::test]
async fn backup_can_use_same_url_with_a_different_fetch_context() -> TestResult {
    let mut initial_selection = selection(720);
    let mut different_context = initial_selection.selected.as_ref().unwrap().clone();
    different_context.candidate.source.id = "alternate-context".into();
    different_context
        .candidate
        .source
        .fetch_context
        .public_resources = Some(stream_core::providers::PublicResourceContext {
        origin: "https://other.example".into(),
        referer: "https://other.example/".into(),
    });
    initial_selection.backups = Some(vec![different_context]);
    let (base, _, relay) = server_with_selections(VecDeque::from([initial_selection])).await?;
    let initial = create(&base).await?;
    let id = initial["id"].as_str().unwrap();
    let previous = relay.token_for(id);
    reqwest::Client::new()
        .post(format!("{base}/api/playback/{id}/backup"))
        .json(&json!({"expected_revision": initial["revision"]}))
        .send()
        .await?
        .error_for_status()?;
    assert_ne!(relay.token_for(id), previous);
    Ok(())
}
