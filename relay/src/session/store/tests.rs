#![allow(clippy::unwrap_used)]
use super::*;
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Store, Worker) {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    std::fs::write(&key,json!({"active_key_id":"synthetic-key-001","keys":[{"kid":"synthetic-key-001","key":B64.encode([3;32])}]}).to_string()).unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut config = Config::parse(include_str!("../../../config/relay.example.toml")).unwrap();
    config.state.encryption_key_file = key.to_str().unwrap().into();
    config.state.database_path = dir
        .path()
        .join("state/relay.sqlite")
        .to_str()
        .unwrap()
        .into();
    config.limits.max_store_queue_count = 2;
    let (store, worker) = Store::open(Arc::new(config)).unwrap();
    (dir, store, worker)
}
#[tokio::test]
async fn bounded_queue_rejects_excess_and_shutdown_discards_pending_jobs_then_joins() {
    let (_dir, store, worker) = fixture();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for _ in 0..2 {
        let count = count.clone();
        assert!(
            store
                .sender
                .try_send(Box::new(move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                }))
                .is_ok()
        );
    }
    assert!(store.sender.try_send(Box::new(|_| {})).is_err());
    assert_eq!(
        store
            .read("synthetic-session-001".into())
            .await
            .err()
            .unwrap()
            .1,
        STORE.1
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), worker.run(cancel))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(
        store
            .read("synthetic-session-001".into())
            .await
            .err()
            .unwrap()
            .1,
        "draining"
    );
}
#[tokio::test]
async fn exhausted_free_space_reserve_prevents_any_durable_publication() {
    let (_dir, store, mut worker) = fixture();
    let mut config = Config::parse(include_str!("../../../config/relay.example.toml")).unwrap();
    config.state.database_path = worker.engine.config.state.database_path.clone();
    // Inject an exhausted disk budget without filling the user's physical disk.
    config.state.min_free_space_bytes = u64::MAX - config.state.max_snapshot_bytes * 16 - 65536;
    worker.engine.config = Arc::new(config);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(worker.run(cancel.clone()));
    let request:CreateSession=serde_json::from_value(json!({"contract_version":"relay.v1","owner_id":"synthetic-owner-001","expires_at":(Utc::now()+chrono::Duration::hours(1)).to_rfc3339(),"selection":serde_json::from_str::<serde_json::Value>(include_str!("../../../tests/fixtures/providers/renditions.json")).unwrap()})).unwrap();
    assert_eq!(
        store
            .create(request, "budget".into())
            .await
            .err()
            .unwrap()
            .1,
        STORE.1
    );
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn prepared_activation_cancellation_epoch_and_deadline_roll_back_transaction() {
    for attack in 0..3 {
        let (_dir, store, worker) = fixture();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(worker.run(cancel.clone()));
        let request:CreateSession=serde_json::from_value(json!({"contract_version":"relay.v1","owner_id":"synthetic-owner-001","expires_at":(Utc::now()+chrono::Duration::hours(1)).to_rfc3339(),"selection":serde_json::from_str::<serde_json::Value>(include_str!("../../../tests/fixtures/providers/renditions.json")).unwrap()})).unwrap();
        let selection = request.selection.clone();
        let ready = store.create(request, "activation".into()).await.unwrap();
        let stop = CancellationToken::new();
        let epoch = CancellationToken::new();
        let deadline = if attack == 2 {
            tokio::time::Instant::now()
        } else {
            tokio::time::Instant::now() + Duration::from_secs(1)
        };
        if attack == 0 {
            stop.cancel();
        }
        if attack == 1 {
            epoch.cancel();
        }
        assert_eq!(
            store
                .activate(
                    ready.session_id.clone(),
                    UpdateSource {
                        owner_id: "synthetic-owner-001".into(),
                        expected_revision: 1,
                        selection
                    },
                    vec![],
                    stop,
                    epoch,
                    deadline
                )
                .await
                .err()
                .unwrap()
                .1,
            [
                "preparation_cancelled",
                "vpn_unavailable",
                "upstream_timeout"
            ][attack]
        );
        assert_eq!(
            store
                .read(ready.session_id)
                .await
                .unwrap()
                .source_generation,
            1
        );
        cancel.cancel();
        task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn late_backups_preserve_playing_source_grant_revision_and_generation() {
    let (_dir, store, worker) = fixture();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(worker.run(cancel.clone()));
    let selection: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/providers/renditions.json"
    ))
    .unwrap();
    let original_id = selection
        .selected
        .as_ref()
        .unwrap()
        .candidate
        .source
        .id
        .clone();
    let request = CreateSession {
        contract_version: "relay.v1".into(),
        owner_id: "synthetic-owner-001".into(),
        expires_at: Utc::now() + chrono::Duration::hours(1),
        selection: selection.clone(),
    };
    let ready = store.create(request, "late-backups".into()).await.unwrap();
    let mut full = selection;
    full.selected.as_mut().unwrap().candidate.source.id = "late-verified-backup".into();
    for _ in 0..2 {
        store
            .backups(
                ready.session_id.clone(),
                UpdateBackups {
                    owner_id: "synthetic-owner-001".into(),
                    expected_generation: ready.source_generation,
                    selection: full.clone(),
                },
            )
            .await
            .unwrap();
    }
    let after = store.read(ready.session_id.clone()).await.unwrap();
    assert_eq!(after.revision, ready.revision);
    assert_eq!(after.auth_revision, ready.auth_revision);
    assert_eq!(after.source_generation, ready.source_generation);
    let snapshot = store
        .call({
            let sid = ready.session_id.clone();
            move |engine| Ok(engine.generation(&sid, 1)?.selection)
        })
        .await
        .unwrap();
    assert_eq!(
        snapshot.selected.as_ref().unwrap().candidate.source.id,
        original_id
    );
    assert_eq!(
        snapshot
            .backups
            .as_ref()
            .unwrap()
            .iter()
            .filter(|ranked| ranked.candidate.source.id == "late-verified-backup")
            .count(),
        1
    );
    let error = store
        .backups(
            ready.session_id.clone(),
            UpdateBackups {
                owner_id: "synthetic-owner-001".into(),
                expected_generation: 2,
                selection: full.clone(),
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.1, "revision_conflict");
    let error = store
        .backups(
            ready.session_id.clone(),
            UpdateBackups {
                owner_id: "different-owner-001".into(),
                expected_generation: 1,
                selection: full,
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.1, "owner_mismatch");
    store.revoke(ready.session_id).await.unwrap();
    cancel.cancel();
    task.await.unwrap().unwrap();
}
