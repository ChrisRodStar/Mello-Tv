#![allow(clippy::unwrap_used)]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use chrono::{Duration, Utc};
use family_stream_relay::{
    config::Config,
    contracts::{CreateSession, RenewSession, SessionReady, UpdateSource},
    control::{self, App},
    session::{self, AuthorizationStore, Registration, Resource, ResourceKind, Store},
};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

fn config(dir: &Path) -> Arc<Config> {
    let mut c = Config::parse(include_str!("../config/relay.example.toml")).unwrap();
    c.state.database_path = dir.join("state/relay.sqlite").to_str().unwrap().into();
    c.state.encryption_key_file = dir.join("key.json").to_str().unwrap().into();
    // Leases are integer Unix seconds: a one-second grace can have almost no
    // time left when a parallel test reaches its first assertion.
    c.state.old_generation_grace_s = 2;
    c.state.hot_index_bytes = 4096;
    key(Path::new(&c.state.encryption_key_file), 3);
    Arc::new(c)
}
fn key(path: &Path, byte: u8) {
    std::fs::write(path,json!({"active_key_id":"synthetic-key-001","keys":[{"kid":"synthetic-key-001","key":B64.encode([byte;32])}]}).to_string()).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
fn create() -> CreateSession {
    serde_json::from_value(json!({"contract_version":"relay.v1","owner_id":"synthetic-owner-001","expires_at":(Utc::now()+Duration::hours(1)).to_rfc3339(),"selection":serde_json::from_str::<Value>(include_str!("fixtures/providers/renditions.json")).unwrap()})).unwrap()
}
fn registration(uri: &str) -> Registration {
    Registration {
        manifest: None,
        encrypted: false,
        uri: uri.into(),
        kind: ResourceKind::Segment,
        context: create()
            .selection
            .selected
            .unwrap()
            .candidate
            .source
            .fetch_context,
        range: None,
        expires_at: None,
    }
}
fn start(
    c: Arc<Config>,
) -> (
    Store,
    CancellationToken,
    tokio::task::JoinHandle<Result<(), family_stream_relay::error::Error>>,
) {
    let (store, worker) = Store::open(c).unwrap();
    let stop = CancellationToken::new();
    (store, stop.clone(), tokio::spawn(worker.run(stop)))
}
async fn stop(
    stop: CancellationToken,
    worker: tokio::task::JoinHandle<Result<(), family_stream_relay::error::Error>>,
) {
    stop.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(3), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
async fn ready(store: &Store) -> SessionReady {
    store
        .create(create(), "synthetic-create-001".into())
        .await
        .unwrap()
}
fn db(c: &Config) -> rusqlite::Connection {
    rusqlite::Connection::open(&c.state.database_path).unwrap()
}

#[tokio::test]
async fn committed_sessions_resources_and_idempotency_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let request = create();
    let normalized = serde_json::to_vec(&request).unwrap();
    let first = s.create(request, "durable-retry".into()).await.unwrap();
    let r = registration("https://media.example.com/segment.ts?signature=synthetic-private");
    let ids = s
        .register(first.session_id.clone(), 1, vec![r.clone(), r.clone()])
        .await
        .unwrap();
    assert_eq!(ids[0], ids[1]);
    let root = s.lookup(&first.session_id, &Resource::Entry).await.unwrap();
    assert_eq!(root.auth_revision, 1);
    drop(root);
    let serialized = serde_json::to_value(&first).unwrap();
    stop(cancel, w).await;
    let raw = std::fs::read(&c.state.database_path).unwrap();
    for secret in [
        "media.example.com",
        "synthetic-private",
        "synthetic-owner-001",
        "fetch_context",
    ] {
        assert!(!raw.windows(secret.len()).any(|w| w == secret.as_bytes()));
    }
    let (s, cancel, w) = start(c);
    let retry = s
        .create(
            serde_json::from_slice(&normalized).unwrap(),
            "durable-retry".into(),
        )
        .await
        .unwrap();
    assert_eq!(serialized, serde_json::to_value(retry).unwrap());
    assert_eq!(
        s.register(first.session_id.clone(), 1, vec![r])
            .await
            .unwrap()[0],
        ids[0]
    );
    assert!(
        s.resolve(first.session_id.clone(), 1, ids[0].clone())
            .await
            .unwrap()
            .uri
            .contains("synthetic-private")
    );
    let reg = s
        .lookup(&first.session_id, &Resource::Registered(ids[0].clone()))
        .await
        .unwrap();
    assert_eq!(reg.resource_generation, 1);
    drop(reg);
    stop(cancel, w).await;
}

#[tokio::test]
async fn idempotency_is_owner_scoped_normalized_and_conflicting_payload_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (s, c, w) = start(config(dir.path()));
    let req = create();
    let value = serde_json::to_value(&req).unwrap();
    let a = s.create(req, "same-key".into()).await.unwrap();
    // Reorder the JSON object and alter equivalent UTC spelling.
    let mut b = value.clone();
    b["expires_at"] = json!(b["expires_at"].as_str().unwrap().replace("+00:00", "Z"));
    let retry = s
        .create(serde_json::from_value(b).unwrap(), "same-key".into())
        .await
        .unwrap();
    assert_eq!(a.session_id, retry.session_id);
    let mut b = value.clone();
    b["selection"]["extension"] = json!("different");
    assert_eq!(
        s.create(serde_json::from_value(b).unwrap(), "same-key".into())
            .await
            .err()
            .unwrap()
            .1,
        "idempotency_conflict"
    );
    let mut b = value;
    b["owner_id"] = json!("synthetic-owner-002");
    assert_ne!(
        s.create(serde_json::from_value(b).unwrap(), "same-key".into())
            .await
            .unwrap()
            .session_id,
        a.session_id
    );
    stop(c, w).await;
}

#[tokio::test]
async fn concurrent_revision_mutations_have_exactly_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let (s, c, w) = start(config(dir.path()));
    let a = ready(&s).await;
    let renew = || RenewSession {
        owner_id: "synthetic-owner-001".into(),
        expected_revision: 1,
        expires_at: Utc::now() + Duration::hours(2),
    };
    let (x, y) = tokio::join!(
        s.renew(a.session_id.clone(), renew()),
        s.renew(a.session_id.clone(), renew())
    );
    assert_eq!(usize::from(x.is_ok()) + usize::from(y.is_ok()), 1);
    let loser = if x.is_err() { x } else { y };
    assert_eq!(loser.err().unwrap().1, "revision_conflict");
    let read = s.read(a.session_id.clone()).await.unwrap();
    assert_eq!(read.revision, 2);
    assert_eq!(read.auth_revision, 1);
    assert_eq!(read.source_generation, 1);
    let wrong = RenewSession {
        owner_id: "synthetic-owner-002".into(),
        expected_revision: 2,
        expires_at: Utc::now() + Duration::hours(3),
    };
    assert_eq!(
        s.renew(a.session_id, wrong).await.err().unwrap().1,
        "owner_mismatch"
    );
    stop(c, w).await;
}

#[tokio::test]
async fn source_changes_keep_old_bytes_and_ids_bound_to_their_generation() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    let r = registration("https://media.example.com/a.ts");
    let old = s
        .register(a.session_id.clone(), 1, vec![r.clone()])
        .await
        .unwrap()
        .remove(0);
    let reader = s
        .lookup(&a.session_id, &Resource::Registered(old.clone()))
        .await
        .unwrap();
    let mut selection = create().selection;
    selection.selected.as_mut().unwrap().candidate.source.url =
        "https://media.example.com/new-master.m3u8".into();
    let b = s
        .source(
            a.session_id.clone(),
            UpdateSource {
                owner_id: "synthetic-owner-001".into(),
                expected_revision: 1,
                selection,
            },
        )
        .await
        .unwrap();
    assert_eq!(b.source_generation, 2);
    assert_eq!(b.auth_revision, 1);
    assert_eq!(b.resume_required, Some(true));
    let new = s
        .register(a.session_id.clone(), 2, vec![r])
        .await
        .unwrap()
        .remove(0);
    assert_ne!(old, new);
    assert_eq!(
        s.resolve(a.session_id.clone(), 1, old.clone())
            .await
            .unwrap()
            .uri,
        "https://media.example.com/a.ts"
    );
    assert!(
        s.resolve(a.session_id.clone(), 2, old.clone())
            .await
            .is_err()
    ); // even with old resource hot
    assert_eq!(
        s.register(
            a.session_id.clone(),
            1,
            vec![registration("https://media.example.com/b.ts")]
        )
        .await
        .err()
        .unwrap()
        .1,
        "revision_conflict"
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        reader.cancelled.cancelled(),
    )
    .await
    .unwrap();
    s.cleanup().await.unwrap();
    assert_eq!(
        db(&c)
            .query_row(
                "SELECT count(*) FROM resources WHERE generation=1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    ); // readers drain before deletion
    drop(reader);
    s.cleanup().await.unwrap();
    s.cleanup().await.unwrap();
    assert!(
        s.lookup(&a.session_id, &Resource::Registered(old))
            .await
            .is_err()
    );
    stop(cancel, w).await;
    let (s, cancel, w) = start(c);
    assert_eq!(s.read(a.session_id).await.unwrap().source_generation, 2);
    stop(cancel, w).await;
}

#[tokio::test]
async fn source_refresh_rejects_other_titles_and_cross_ids_and_stale_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let (s, c, w) = start(config(dir.path()));
    let a = ready(&s).await;
    for kind in ["title", "cross-id", "expired", "oversized"] {
        let mut selection = create().selection;
        let source = &mut selection.selected.as_mut().unwrap().candidate.source;
        match kind {
            "title" => source.title.tmdb_id += 1,
            "cross-id" => source.title.imdb_id = Some("tt0000001".into()),
            "expired" => source.expires_at = Some(Utc::now() - Duration::seconds(1)),
            _ => {
                selection
                    .extra
                    .insert("large".into(), json!("x".repeat(3_000_000)));
            }
        }
        assert!(
            s.source(
                a.session_id.clone(),
                UpdateSource {
                    owner_id: "synthetic-owner-001".into(),
                    expected_revision: 1,
                    selection
                }
            )
            .await
            .is_err(),
            "{kind}"
        );
    }
    assert_eq!(s.read(a.session_id).await.unwrap().revision, 1);
    stop(c, w).await;
}

#[tokio::test]
async fn revoke_is_durable_idempotent_cancels_readers_and_prevents_reactivation() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    let record = s.lookup(&a.session_id, &Resource::Entry).await.unwrap();
    s.revoke(a.session_id.clone()).await.unwrap();
    assert!(record.cancelled.is_cancelled());
    s.revoke(a.session_id.clone()).await.unwrap();
    s.revoke("unknown-session-id".into()).await.unwrap();
    let read = s.read(a.session_id.clone()).await.unwrap();
    assert_eq!(read.auth_revision, 2);
    assert_eq!(read.revision, 2);
    assert_eq!(serde_json::to_value(read).unwrap()["state"], "revoked");
    assert_eq!(
        s.lookup(&a.session_id, &Resource::Entry)
            .await
            .err()
            .unwrap()
            .1,
        "revoked_grant"
    );
    drop(record);
    stop(cancel, w).await;
    let (s, cancel, w) = start(c);
    assert_eq!(
        s.renew(
            a.session_id.clone(),
            RenewSession {
                owner_id: "synthetic-owner-001".into(),
                expected_revision: 2,
                expires_at: Utc::now() + Duration::hours(2)
            }
        )
        .await
        .err()
        .unwrap()
        .1,
        "revoked_grant"
    );
    assert_eq!(
        s.source(
            a.session_id,
            UpdateSource {
                owner_id: "synthetic-owner-001".into(),
                expected_revision: 2,
                selection: create().selection
            }
        )
        .await
        .err()
        .unwrap()
        .1,
        "revoked_grant"
    );
    stop(cancel, w).await;
}

#[tokio::test]
async fn session_and_source_expiry_bound_authorization_without_extending_provider_lease() {
    let dir = tempfile::tempdir().unwrap();
    let (s, c, w) = start(config(dir.path()));
    let mut req = create();
    req.selection
        .selected
        .as_mut()
        .unwrap()
        .candidate
        .source
        .expires_at = Some(Utc::now() + Duration::seconds(2));
    let a = s.create(req, "source-expiry".into()).await.unwrap();
    let record = s.lookup(&a.session_id, &Resource::Entry).await.unwrap();
    s.renew(
        a.session_id.clone(),
        RenewSession {
            owner_id: "synthetic-owner-001".into(),
            expected_revision: 1,
            expires_at: Utc::now() + Duration::hours(2),
        },
    )
    .await
    .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(4),
        record.cancelled.cancelled(),
    )
    .await
    .unwrap();
    assert_eq!(
        s.lookup(&a.session_id, &Resource::Entry)
            .await
            .err()
            .unwrap()
            .1,
        "source_expired"
    );
    drop(record);
    stop(c, w).await;
}

#[tokio::test]
async fn expiry_cleanup_is_batched_and_restart_accepts_partially_drained_expired_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Arc::try_unwrap(config(dir.path())).ok().unwrap();
    config.state.cleanup_batch_count = 2;
    let c = Arc::new(config);
    let (s, cancel, w) = start(c.clone());
    let mut req = create();
    req.expires_at = Utc::now() + Duration::seconds(2);
    let a = s.create(req, "short".into()).await.unwrap();
    for i in 0..3 {
        s.register(
            a.session_id.clone(),
            1,
            vec![registration(&format!("https://media.example.com/{i}.ts"))],
        )
        .await
        .unwrap();
    }
    let reader = s.lookup(&a.session_id, &Resource::Entry).await.unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(4),
        reader.cancelled.cancelled(),
    )
    .await
    .unwrap();
    assert_eq!(
        s.lookup(&a.session_id, &Resource::Entry)
            .await
            .err()
            .unwrap()
            .1,
        "session_expired"
    );
    s.cleanup().await.unwrap();
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM resources", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    drop(reader);
    s.cleanup().await.unwrap(); // cursor can wrap once
    s.cleanup().await.unwrap();
    let left = db(&c)
        .query_row("SELECT count(*) FROM resources", [], |r| r.get::<_, i64>(0))
        .unwrap();
    assert_eq!(left, 2);
    stop(cancel, w).await;
    let (s, cancel, w) = start(c.clone());
    for _ in 0..8 {
        s.cleanup().await.unwrap();
    }
    assert!(s.read(a.session_id).await.is_err());
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    stop(cancel, w).await;
}

#[tokio::test]
async fn resource_identity_includes_kind_context_range_expiry_and_session() {
    let dir = tempfile::tempdir().unwrap();
    let (s, c, w) = start(config(dir.path()));
    let a = ready(&s).await;
    let base = registration("https://media.example.com/media.ts");
    let mut values = vec![base.clone()];
    let mut r = base.clone();
    r.kind = ResourceKind::Init;
    values.push(r);
    let mut r = base.clone();
    r.range = Some(session::ByteRange {
        offset: 0,
        length: 5,
    });
    values.push(r);
    let mut r = base.clone();
    r.expires_at = Some(Utc::now().timestamp() + 50);
    values.push(r);
    let mut r = base.clone();
    r.context.extra.insert("future".into(), json!("different"));
    values.push(r);
    let ids = s.register(a.session_id.clone(), 1, values).await.unwrap();
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        5
    );
    let b = s.create(create(), "another-session".into()).await.unwrap();
    assert_ne!(
        s.register(b.session_id.clone(), 1, vec![base])
            .await
            .unwrap()[0],
        ids[0]
    );
    assert!(
        s.lookup(&b.session_id, &Resource::Registered(ids[0].clone()))
            .await
            .is_err()
    );
    stop(c, w).await;
}

#[tokio::test]
async fn resource_registration_limits_are_atomic_and_reads_do_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Arc::try_unwrap(config(dir.path())).ok().unwrap();
    c.state.max_resources_per_session_count = 3;
    c.state.hot_index_bytes = 1;
    let c = Arc::new(c);
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    let ids = s
        .register(
            a.session_id.clone(),
            1,
            vec![
                registration("https://media.example.com/a"),
                registration("https://media.example.com/b"),
            ],
        )
        .await
        .unwrap();
    assert!(
        s.register(
            a.session_id.clone(),
            1,
            vec![
                registration("https://media.example.com/c"),
                registration("https://media.example.com/d")
            ]
        )
        .await
        .is_err()
    );
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM resources", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    let before = std::fs::read(&c.state.database_path).unwrap();
    for _ in 0..10 {
        for rid in &ids {
            drop(
                s.lookup(&a.session_id, &Resource::Registered(rid.clone()))
                    .await
                    .unwrap(),
            );
        }
    }
    assert_eq!(before, std::fs::read(&c.state.database_path).unwrap());
    stop(cancel, w).await;
}

#[tokio::test]
async fn store_refuses_wrong_missing_keys_corruption_and_unknown_versions() {
    for mode in [
        "wrong-key",
        "missing-key",
        "corrupt-session",
        "corrupt-resource",
        "corrupt-generation",
        "corrupt-idempotency",
        "future-version",
        "wrong-row-aad",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let c = config(dir.path());
        let (s, cancel, w) = start(c.clone());
        let a = ready(&s).await;
        stop(cancel, w).await;
        match mode {
            "wrong-key" => key(Path::new(&c.state.encryption_key_file), 9),
            "missing-key" => {
                std::fs::remove_file(&c.state.encryption_key_file).unwrap();
            }
            "future-version" => {
                db(&c).execute_batch("PRAGMA user_version=99").unwrap();
            }
            "wrong-row-aad" => {
                db(&c)
                    .execute(
                        "UPDATE resources SET rid='synthetic-other-resource' WHERE sid=?1",
                        [a.session_id],
                    )
                    .unwrap();
            }
            other => {
                let table = other.trim_start_matches("corrupt-");
                let table = if table == "session" {
                    "sessions"
                } else if table == "resource" {
                    "resources"
                } else if table == "generation" {
                    "generations"
                } else {
                    "idempotency"
                };
                db(&c)
                    .execute(&format!("UPDATE {table} SET blob=x'00'"), [])
                    .unwrap();
            }
        }
        assert!(Store::open(c).is_err(), "{mode}");
    }
}

#[tokio::test]
async fn session_and_idempotency_limits_stop_admission() {
    for mode in ["sessions", "idempotency"] {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Arc::try_unwrap(config(dir.path())).ok().unwrap();
        if mode == "sessions" {
            c.state.max_sessions_count = 1;
        } else {
            c.state.max_idempotency_count = 1;
        }
        let c = Arc::new(c);
        let (s, cancel, w) = start(c.clone());
        let _a = ready(&s).await;
        assert!(s.create(create(), "extra".into()).await.is_err());
        stop(cancel, w).await;
    }
}
#[tokio::test]
async fn sqlite_full_rolls_back_unpublished_sessions_and_retains_committed_mappings() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Arc::try_unwrap(config(dir.path())).ok().unwrap();
    c.state.max_database_bytes = 1_048_576;
    let c = Arc::new(c);
    let (s, cancel, w) = start(c.clone());
    let mut successful = Vec::new();
    let mut failed = false;
    for i in 0..8 {
        let mut req = create();
        req.selection.extra.insert(
            "bounded-private-metadata".into(),
            json!("x".repeat(150_000)),
        );
        match s.create(req, format!("disk-quota-{i}")).await {
            Ok(a) => successful.push(a.session_id),
            Err(e) => {
                assert_eq!(e.1, "state_store_unavailable");
                failed = true;
                break;
            }
        }
    }
    assert!(failed);
    assert!(!successful.is_empty());
    assert!(std::fs::metadata(&c.state.database_path).unwrap().len() <= c.state.max_database_bytes);
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        successful.len() as i64
    );
    stop(cancel, w).await;
    let (s, cancel, w) = start(c);
    for sid in successful {
        assert!(s.lookup(&sid, &Resource::Entry).await.is_ok());
    }
    stop(cancel, w).await;
}

#[tokio::test]
async fn transaction_failure_rolls_back_every_mapping_and_poisoned_store_issues_no_urls() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    db(&c).execute_batch("CREATE TRIGGER synthetic_failure BEFORE INSERT ON idempotency BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert_eq!(
        s.create(create(), "failure".into()).await.err().unwrap().1,
        "state_store_unavailable"
    );
    for table in ["sessions", "generations", "resources", "idempotency"] {
        assert_eq!(
            db(&c)
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        s.create(create(), "retry".into()).await.err().unwrap().1,
        "state_store_unavailable"
    );
    stop(cancel, w).await;
}

#[tokio::test]
async fn exclusive_owner_and_shutdown_settle_durable_work_and_reject_further_requests() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    assert!(Store::open(c.clone()).is_err());
    let a = ready(&s).await;
    stop(cancel, w).await;
    assert_eq!(
        s.read(a.session_id.clone()).await.err().unwrap().1,
        "draining"
    );
    let (s, cancel, w) = start(c);
    assert_eq!(s.read(a.session_id).await.unwrap().revision, 1);
    stop(cancel, w).await;
}

fn request(path: &str, method: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(
            "authorization",
            "Bearer synthetic-bearer-01234567890123456789012345",
        )
        .header("content-type", "application/json")
        .header("idempotency-key", "http-create")
        .body(Body::from(value.to_string()))
        .unwrap()
}
#[tokio::test]
async fn real_control_paths_publish_only_redacted_committed_state_and_readiness_stays_false() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let mut app = App::new(
        Config::parse(include_str!("../config/relay.example.toml")).unwrap(),
        b"synthetic-bearer-01234567890123456789012345",
    );
    app.config = c;
    app.store = Some(s.clone());
    let router = control::control(app);
    let response = router
        .clone()
        .oneshot(request(
            "/internal/v1/sessions",
            "POST",
            serde_json::to_value(create()).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let sid = body["session_id"].as_str().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("https://"));
    assert!(!String::from_utf8_lossy(&bytes).contains("owner"));
    let path = format!("/internal/v1/sessions/{sid}");
    let response = router
        .clone()
        .oneshot(request(&path, "GET", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(s.lookup(sid, &Resource::Entry).await.is_ok());
    let response=router.clone().oneshot(request(&(path.clone()+"/renew"),"POST",json!({"owner_id":"synthetic-owner-001","expected_revision":1,"expires_at":(Utc::now()+Duration::hours(2)).to_rfc3339()}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(request("/health/readyz", "GET", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response = router
        .oneshot(request(&path, "DELETE", Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(s.lookup(sid, &Resource::Entry).await.is_err());
    stop(cancel, w).await;
}

#[test]
fn crash_writer_helper() {
    let Ok(root) = std::env::var("RELAY_CRASH_FIXTURE") else {
        return;
    };
    let c: Config =
        Config::parse(&std::fs::read_to_string(Path::new(&root).join("config.toml")).unwrap())
            .unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let c=Arc::new(c);
        let (s, _stop, _worker) = start(c.clone());
        let a = ready(&s).await;
        let ids = s
            .register(
                a.session_id.clone(),
                1,
                vec![registration("https://media.example.com/crash.ts")],
            )
            .await
            .unwrap();
        std::fs::write(
            Path::new(&root).join("result.json"),
            json!({"sid":a.session_id,"rid":ids[0]}).to_string(),
        )
        .unwrap();
        let connection=db(&c);
        connection.execute_batch("PRAGMA cache_size=1; BEGIN IMMEDIATE; INSERT INTO sessions VALUES('incomplete-creating',0,zeroblob(200000));").unwrap();
        std::process::exit(0); // bypasses transaction rollback, drops and store shutdown
    });
}
#[tokio::test]
async fn abrupt_process_exit_recovers_committed_mappings_and_ignores_rolled_back_work() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let config_text = include_str!("../config/relay.example.toml")
        .replace("state/relay.sqlite", &c.state.database_path)
        .replace(
            "/run/secrets/relay-session-encryption-key",
            &c.state.encryption_key_file,
        );
    std::fs::write(dir.path().join("config.toml"), config_text).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_writer_helper", "--nocapture"])
        .env("RELAY_CRASH_FIXTURE", dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("result.json")).unwrap()).unwrap();
    assert!(Path::new(&(c.state.database_path.clone() + "-journal")).exists());
    let (s, cancel, w) = start(c);
    assert!(
        s.lookup(
            result["sid"].as_str().unwrap(),
            &Resource::Registered(result["rid"].as_str().unwrap().into())
        )
        .await
        .is_ok()
    );
    assert!(s.read("incomplete-creating".into()).await.is_err());
    stop(cancel, w).await;
}

#[tokio::test]
async fn real_store_stream_authorization_enforces_grants_for_get_head_and_nested_resources() {
    use ed25519_dalek::{Signer, SigningKey};
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    let rid = s
        .register(
            a.session_id.clone(),
            1,
            vec![registration("https://media.example.com/audio.aac")],
        )
        .await
        .unwrap()
        .remove(0);
    let signing = SigningKey::from_bytes(&[7; 32]);
    let verifier=family_stream_relay::auth::Verifier::from_json(&c.auth,&serde_json::to_vec(&json!({"keys":[{"kid":"synthetic-jwt-key","public_key":B64.encode(signing.verifying_key().as_bytes())}]})).unwrap()).unwrap();
    let sign = |owner: &str| {
        let now = Utc::now().timestamp();
        let header = B64.encode(r#"{"alg":"EdDSA","kid":"synthetic-jwt-key"}"#);
        let body=B64.encode(json!({"iss":"family-stream-api","aud":"family-stream-relay","sub":owner,"sid":a.session_id,"arv":1,"iat":now,"nbf":now,"exp":now+100,"jti":"synthetic-grant-001"}).to_string());
        let data = format!("{header}.{body}");
        format!(
            "{data}.{}",
            B64.encode(signing.sign(data.as_bytes()).to_bytes())
        )
    };
    let token = sign("synthetic-owner-001");
    let wrong = sign("synthetic-owner-002");
    let router = control::streaming_with(Some(Arc::new(verifier)), Arc::new(s.clone()));
    for method in ["GET", "HEAD"] {
        for suffix in ["entry".to_owned(), format!("r/{rid}")] {
            for (grant, status, code) in [
                (&token, StatusCode::SERVICE_UNAVAILABLE, "media_unavailable"),
                (&wrong, StatusCode::FORBIDDEN, "owner_mismatch"),
            ] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(format!("/stream/{}/{suffix}?st={grant}", a.session_id))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), status);
                assert_eq!(response.headers()["referrer-policy"], "no-referrer");
                let body = to_bytes(response.into_body(), 16384).await.unwrap();
                if method == "HEAD" {
                    assert!(body.is_empty());
                } else {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&body).unwrap()["code"],
                        code
                    );
                }
            }
        }
    }
    s.revoke(a.session_id.clone()).await.unwrap();
    let response = router
        .oneshot(
            Request::get(format!("/stream/{}/entry?st={token}", a.session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    stop(cancel, w).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_relayctl_importer_reaches_durable_control_runtime() {
    use tokio::net::TcpListener;
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let mut app = App::new(
        Config::parse(include_str!("../config/relay.example.toml")).unwrap(),
        b"synthetic-bearer-01234567890123456789012345",
    );
    app.config = c.clone();
    app.store = Some(s);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = tokio::spawn(family_stream_relay::runtime::run(
        listener,
        stream,
        app,
        cancel.clone(),
    ));
    let bearer = dir.path().join("bearer");
    let selection = dir.path().join("selection");
    std::fs::write(&bearer, b"synthetic-bearer-01234567890123456789012345").unwrap();
    std::fs::write(
        &selection,
        include_bytes!("fixtures/providers/renditions.json"),
    )
    .unwrap();
    for path in [&bearer, &selection] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let receipt_path = dir.path().join("receipt.json");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_relayctl"))
        .arg("import-selection")
        .arg(address.to_string())
        .arg(bearer)
        .arg(selection)
        .arg("synthetic-owner-001")
        .arg((Utc::now() + Duration::hours(1)).to_rfc3339())
        .arg("real-importer")
        .arg(&receipt_path)
        .output()
        .await
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"control_status=201\n");
    assert!(output.stderr.is_empty());
    let receipt_bytes = std::fs::read(&receipt_path).unwrap();
    let receipt: SessionReady = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(receipt.contract_version, "relay.v1");
    assert_eq!(
        receipt.entry_path,
        format!("/stream/{}/entry", receipt.session_id)
    );
    assert_eq!(receipt.auth_revision, 1);
    assert_eq!(
        std::fs::metadata(&receipt_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let text = String::from_utf8(receipt_bytes).unwrap();
    assert!(!text.contains("https://"));
    assert!(!text.contains("fetch_context"));
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    cancel.cancel();
    service.await.unwrap().unwrap();
    stop(cancel, w).await;
}

#[tokio::test]
async fn authenticated_ciphertext_tampering_and_cross_row_substitution_fail_closed() {
    for mode in ["bit-flip", "row-swap"] {
        let dir = tempfile::tempdir().unwrap();
        let c = config(dir.path());
        let (s, cancel, w) = start(c.clone());
        let a = ready(&s).await;
        let ids = s
            .register(
                a.session_id.clone(),
                1,
                vec![
                    registration("https://media.example.com/a"),
                    registration("https://media.example.com/b"),
                ],
            )
            .await
            .unwrap();
        stop(cancel, w).await;
        let connection = db(&c);
        let blob: Vec<u8> = connection
            .query_row("SELECT blob FROM resources WHERE rid=?1", [&ids[0]], |r| {
                r.get(0)
            })
            .unwrap();
        let mut envelope: Value = serde_json::from_slice(&blob).unwrap();
        let target = if mode == "bit-flip" {
            let byte = envelope["ciphertext"][0].as_u64().unwrap();
            envelope["ciphertext"][0] = json!(byte ^ 1);
            &ids[0]
        } else {
            &ids[1]
        };
        connection
            .execute(
                "UPDATE resources SET blob=?1 WHERE rid=?2",
                rusqlite::params![serde_json::to_vec(&envelope).unwrap(), target],
            )
            .unwrap();
        drop(connection);
        assert!(Store::open(c).is_err());
    }
}

#[tokio::test]
async fn retained_encryption_keys_allow_rotation_but_missing_historical_keys_fail_startup() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    stop(cancel, w).await;
    let path = Path::new(&c.state.encryption_key_file);
    let keys = json!({"active_key_id":"synthetic-key-002","keys":[{"kid":"synthetic-key-001","key":B64.encode([3;32])},{"kid":"synthetic-key-002","key":B64.encode([4;32])}]});
    std::fs::write(path, keys.to_string()).unwrap();
    let (s, cancel, w) = start(c.clone());
    assert!(s.lookup(&a.session_id, &Resource::Entry).await.is_ok());
    let b = s.create(create(), "new-key".into()).await.unwrap();
    stop(cancel, w).await;
    let connection = db(&c);
    let old: Vec<u8> = connection
        .query_row(
            "SELECT blob FROM sessions WHERE sid=?1",
            [a.session_id],
            |r| r.get(0),
        )
        .unwrap();
    let new: Vec<u8> = connection
        .query_row(
            "SELECT blob FROM sessions WHERE sid=?1",
            [b.session_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(
        serde_json::from_slice::<Value>(&old).unwrap()["kid"],
        serde_json::from_slice::<Value>(&new).unwrap()["kid"]
    );
    drop(connection);
    let mut keys = keys;
    keys["keys"].as_array_mut().unwrap().remove(0);
    std::fs::write(path, keys.to_string()).unwrap();
    assert!(Store::open(c).is_err());
}

#[tokio::test]
async fn source_refresh_cannot_forget_previously_known_cross_ids() {
    let dir = tempfile::tempdir().unwrap();
    let c = config(dir.path());
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    let mut selection = create().selection;
    selection
        .selected
        .as_mut()
        .unwrap()
        .candidate
        .source
        .title
        .imdb_id = None;
    for source in selection.backups.iter_mut().flatten() {
        source.candidate.source.title.imdb_id = None;
    }
    s.source(
        a.session_id.clone(),
        UpdateSource {
            owner_id: "synthetic-owner-001".into(),
            expected_revision: 1,
            selection,
        },
    )
    .await
    .unwrap();
    stop(cancel, w).await;
    let (s, cancel, w) = start(c);
    let mut selection = create().selection;
    selection
        .selected
        .as_mut()
        .unwrap()
        .candidate
        .source
        .title
        .imdb_id = Some("tt0000001".into());
    assert!(
        s.source(
            a.session_id.clone(),
            UpdateSource {
                owner_id: "synthetic-owner-001".into(),
                expected_revision: 2,
                selection
            }
        )
        .await
        .is_err()
    );
    assert_eq!(s.read(a.session_id).await.unwrap().revision, 2);
    stop(cancel, w).await;
}

#[tokio::test]
async fn global_resource_budget_includes_all_sessions_and_rejects_whole_batches() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Arc::try_unwrap(config(dir.path())).ok().unwrap();
    c.state.max_resources_total_count = 3;
    c.state.max_resources_per_session_count = 3;
    let c = Arc::new(c);
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    let _b = s
        .create(create(), "second-owner-session".into())
        .await
        .unwrap();
    assert!(
        s.register(
            a.session_id.clone(),
            1,
            vec![
                registration("https://media.example.com/a"),
                registration("https://media.example.com/b")
            ]
        )
        .await
        .is_err()
    );
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM resources", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    s.register(
        a.session_id,
        1,
        vec![registration("https://media.example.com/a")],
    )
    .await
    .unwrap();
    assert!(s.create(create(), "third-session".into()).await.is_err());
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM resources", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    stop(cancel, w).await;
}

#[tokio::test]
async fn partially_cleaned_generations_cannot_exceed_recoverable_admission_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Arc::try_unwrap(config(dir.path())).ok().unwrap();
    c.state.max_resources_total_count = 2;
    c.state.max_resources_per_session_count = 2;
    c.state.cleanup_batch_count = 1;
    let c = Arc::new(c);
    let (s, cancel, w) = start(c.clone());
    let a = ready(&s).await;
    let old = s.lookup(&a.session_id, &Resource::Entry).await.unwrap();
    let update = |revision| UpdateSource {
        owner_id: "synthetic-owner-001".into(),
        expected_revision: revision,
        selection: create().selection,
    };
    s.source(a.session_id.clone(), update(1)).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), old.cancelled.cancelled())
        .await
        .unwrap();
    drop(old);
    s.cleanup().await.unwrap(); // last old resource removed, generation row still retained
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM generations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db(&c)
            .query_row("SELECT count(*) FROM resources", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        s.source(a.session_id.clone(), update(2))
            .await
            .err()
            .unwrap()
            .1,
        "state_limit"
    );
    assert_eq!(s.read(a.session_id.clone()).await.unwrap().revision, 2);
    stop(cancel, w).await;
    let (s, cancel, w) = start(c.clone()); // every successful commit is still recoverable
    for _ in 0..4 {
        s.cleanup().await.unwrap();
    }
    assert_eq!(
        s.source(a.session_id.clone(), update(2))
            .await
            .unwrap()
            .source_generation,
        3
    );
    stop(cancel, w).await;
    let (s, cancel, w) = start(c);
    assert_eq!(s.read(a.session_id).await.unwrap().source_generation, 3);
    stop(cancel, w).await;
}
