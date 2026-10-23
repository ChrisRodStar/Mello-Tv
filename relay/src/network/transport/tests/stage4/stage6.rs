//! Stage 6 exercises the authenticated control and streaming paths over synthetic sockets.
use super::*;
use crate::contracts::{MutationBase, UpdateSource, providers::Selection};
const OWNER: &str = "synthetic-owner-001";
const BEARER: &str = "synthetic-control-01234567890123456789";
const VOD: &str = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n";
fn delivery(h: &Harness) -> Arc<Delivery> {
    Arc::new(Delivery {
        config: h.config.clone(),
        store: h.store.clone(),
        transport: h.transport.clone(),
    })
}
fn app(h: &Harness) -> control::App {
    let mut config =
        Config::parse(include_str!("../../../../../config/relay.example.toml")).unwrap();
    config.limits.max_control_concurrency_count = h.config.limits.max_control_concurrency_count;
    let mut app = control::App::new(config, BEARER.as_bytes());
    app.config = h.config.clone();
    let key = SigningKey::from_bytes(&[7; 32]);
    app.playback = Some(Arc::new(Verifier::from_json(&h.config.auth, &serde_json::to_vec(&json!({"keys":[{"kid":"test-key","public_key":B64.encode(key.verifying_key().as_bytes())}]})).unwrap()).unwrap()));
    app.store = Some(h.store.clone());
    app.delivery = Some(delivery(h));
    app
}
async fn call(
    router: Router,
    sid: &str,
    operation: &str,
    revision: u64,
    selection: Option<Selection>,
) -> axum::response::Response {
    let mut body = json!({"owner_id":OWNER,"expected_revision":revision});
    if let Some(selection) = selection {
        body["selection"] = serde_json::to_value(selection).unwrap();
    }
    router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/internal/v1/sessions/{sid}/{operation}"))
                .header("authorization", format!("Bearer {BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn code(response: axum::response::Response) -> String {
    serde_json::from_slice::<serde_json::Value>(
        &to_bytes(response.into_body(), 8192).await.unwrap(),
    )
    .unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}
async fn selection(h: &Harness) -> Selection {
    h.store
        .recovery(
            h.sid.clone(),
            OWNER.into(),
            h.store.read(h.sid.clone()).await.unwrap().revision,
            None,
        )
        .await
        .unwrap()
        .selection
}
async fn seed(h: &Harness, paths: &[&str], mp4: bool) -> Selection {
    let mut selection = selection(h).await;
    let mut candidate = selection.selected.clone().unwrap();
    candidate.candidate.source.selected_variant_url = None;
    if mp4 {
        candidate.candidate.source.format = "mp4".into();
        candidate.candidate.observation.format = "mp4".into();
        candidate.candidate.observation.encrypted = false;
    }
    selection.backups = Some(
        paths
            .iter()
            .map(|path| {
                let mut c = candidate.clone();
                c.candidate.source.url = format!("http://source.example.com{path}");
                // Scores and provider names must not affect the original array order.
                c.score = if path.contains("bad") { 999.0 } else { -999.0 };
                c
            })
            .collect(),
    );
    h.store
        .source(
            h.sid.clone(),
            UpdateSource {
                owner_id: OWNER.into(),
                expected_revision: 1,
                selection: selection.clone(),
            },
        )
        .await
        .unwrap();
    selection
}
fn mutation(revision: u64) -> MutationBase {
    MutationBase {
        owner_id: OWNER.into(),
        expected_revision: revision,
    }
}
async fn wait_for(h: &Harness, path: &str) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if h.requests.lock().unwrap().iter().any(|r| r.contains(path)) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn ordered_preparation_atomic_hls_graph_and_explicit_retry() {
    let h = Harness::start(|_| {}).await;
    seed(&h, &["/bad.m3u8", "/good.m3u8", "/later.m3u8"], false).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put("/bad.m3u8", Wire::body("<html>source unavailable</html>"));
    h.put("/good.m3u8", Wire::body(MASTER));
    for p in ["/v.m3u8", "/audio.m3u8", "/subs.m3u8"] {
        h.put(p, Wire::body(VOD));
    }
    let old_manifest = h.text(&h.entry()).await;
    let old_id = uris(&old_manifest)[2].clone();
    h.text(&old_id).await;
    let response = call(control::control(app(&h)), &h.sid, "failover", 2, None).await;
    assert_eq!(response.status(), 200);
    let ready: crate::contracts::SessionReady =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(
        (
            ready.revision,
            ready.source_generation,
            ready.auth_revision,
            ready.resume_required
        ),
        (3, 3, 1, Some(true))
    );
    let current = selection(&h).await;
    assert!(
        current
            .selected
            .unwrap()
            .candidate
            .source
            .url
            .ends_with("/good.m3u8")
    );
    assert_eq!(current.backups.unwrap().len(), 1);
    let requests = h.requests.lock().unwrap().clone();
    let failed = requests
        .iter()
        .position(|r| r.contains("GET /bad.m3u8 "))
        .unwrap();
    let good = requests
        .iter()
        .position(|r| r.contains("GET /good.m3u8 "))
        .unwrap();
    assert!(failed < good);
    assert!(!requests.iter().any(|r| r.contains("/later.m3u8")));
    // Existing old manifest references may be replayed during grace, but cannot
    // allocate new old-generation resources or reach the replacement root.
    assert_eq!(h.request(&old_id, "HEAD", None).await.status(), 200);
    let before = h.requests.lock().unwrap().len();
    assert_eq!(
        code(call(control::control(app(&h)), &h.sid, "failover", 2, None).await).await,
        "revision_conflict"
    );
    assert_eq!(h.requests.lock().unwrap().len(), before);
    let db = rusqlite::Connection::open(&h.config.state.database_path).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM resources WHERE generation=3",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(count >= 5); // root, full rendition graph and its leaf mappings
    let ids = db
        .prepare("SELECT rid FROM resources WHERE generation=3")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    drop(db);
    let mut mappings = Vec::new();
    for id in ids {
        let uri = h
            .store
            .resolve(h.sid.clone(), 3, id.clone())
            .await
            .unwrap()
            .uri;
        mappings.push((id, uri));
    }
    let config = h.config.clone();
    let sid = h.sid.clone();
    let directory = h._dir.clone();
    h.close().await;
    let (store, worker) = Store::open(config).unwrap();
    let stop = CancellationToken::new();
    let job = tokio::spawn(worker.run(stop.clone()));
    for (id, uri) in mappings {
        assert_eq!(store.resolve(sid.clone(), 3, id).await.unwrap().uri, uri);
    }
    assert_eq!(store.read(sid).await.unwrap().revision, 3);
    stop.cancel();
    job.await.unwrap().unwrap();
    drop(directory);
}
#[tokio::test]
async fn exhausted_alternatives_repeat_bounded_preparation_without_mutating_active_source() {
    let h = Harness::start(|_| {}).await;
    seed(&h, &["/bad-one.m3u8", "/bad-two.m3u8"], false).await;
    for _ in 0..2 {
        assert_eq!(
            code(call(control::control(app(&h)), &h.sid, "failover", 2, None).await).await,
            "source_refresh_required"
        );
        assert_eq!(h.store.read(h.sid.clone()).await.unwrap().revision, 2);
    }
    let paths: Vec<_> = h
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.split_whitespace().nth(1).unwrap().to_owned())
        .collect();
    assert_eq!(
        paths,
        [
            "/bad-one.m3u8",
            "/bad-two.m3u8",
            "/bad-one.m3u8",
            "/bad-two.m3u8"
        ]
    );
    h.close().await;
}
#[tokio::test]
async fn interrupted_guard_timeout_and_local_budgets_do_not_exhaust_backups() {
    for attack in 0..5 {
        let h = Harness::start(|c| {
            c.network.read_idle_timeout_s = 1;
            c.network.admission_wait_s = 1;
        })
        .await;
        seed(&h, &["/wait.m3u8", "/later.m3u8"], false).await;
        let mut wire = Wire::body("#EXTM3U\n");
        wire.length += 1;
        wire.stall = true;
        h.put("/wait.m3u8", wire);
        let d = delivery(&h);
        let sid = h.sid.clone();
        if attack == 4 {
            h.put(
                "/wait.m3u8",
                Wire::body(vec![b'x'; h.config.limits.max_playlist_bytes as usize + 1]),
            );
        }
        let permits = if attack == 3 {
            Some(
                h.transport
                    .slots
                    .clone()
                    .acquire_many_owned(h.config.network.max_streams_count as u32)
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        let job = tokio::spawn(async move { d.failover(sid, mutation(2)).await });
        if attack < 3 {
            wait_for(&h, "/wait.m3u8").await;
            if attack == 0 {
                job.abort();
            }
            if attack == 1 {
                h.transport.guard.invalidate();
            }
        }
        if attack == 0 {
            assert!(job.await.err().unwrap().is_cancelled());
        } else {
            let error = job.await.unwrap().err().unwrap();
            assert_eq!(
                error.1,
                [
                    "",
                    "vpn_unavailable",
                    "upstream_timeout",
                    "request_limit",
                    "preparation_limit"
                ][attack]
            );
        }
        drop(permits);
        assert_eq!(h.store.read(h.sid.clone()).await.unwrap().revision, 2);
        assert!(
            !h.requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.contains("/later.m3u8"))
        );
        h.close().await;
    }
}
#[tokio::test]
async fn concurrent_failover_and_refresh_have_one_cas_winner() {
    for refresh in [false, true] {
        let h = Harness::start(|_| {}).await;
        let selection = seed(&h, &["/good.m3u8"], false).await;
        h.put("/good.m3u8", Wire::body(VOD));
        h.put("/master.m3u8", Wire::body(MASTER));
        for p in ["/v.m3u8", "/audio.m3u8", "/subs.m3u8"] {
            h.put(p, Wire::body(VOD));
        }
        let router = control::control(app(&h));
        let (a, b) = tokio::join!(
            call(router.clone(), &h.sid, "failover", 2, None),
            call(
                router,
                &h.sid,
                if refresh { "source" } else { "failover" },
                2,
                refresh.then_some(selection)
            )
        );
        let (winner, loser) = if a.status() == 200 { (a, b) } else { (b, a) };
        assert_eq!(winner.status(), 200);
        assert_eq!(code(loser).await, "revision_conflict");
        assert_eq!(
            h.store.read(h.sid.clone()).await.unwrap().source_generation,
            3
        );
        h.close().await;
    }
}
#[tokio::test]
async fn owner_identity_expiry_and_revocation_are_enforced_before_activation() {
    let h = Harness::start(|_| {}).await;
    let mut selected = seed(&h, &["/wait.m3u8"], false).await;
    assert_eq!(
        delivery(&h)
            .failover(
                h.sid.clone(),
                MutationBase {
                    owner_id: "another-owner-001".into(),
                    expected_revision: 2
                }
            )
            .await
            .err()
            .unwrap()
            .1,
        "owner_mismatch"
    );
    selected
        .selected
        .as_mut()
        .unwrap()
        .candidate
        .source
        .title
        .tmdb_id += 1;
    assert!(
        delivery(&h)
            .replace(
                h.sid.clone(),
                UpdateSource {
                    owner_id: OWNER.into(),
                    expected_revision: 2,
                    selection: selected
                }
            )
            .await
            .is_err()
    );
    assert!(h.requests.lock().unwrap().is_empty());
    let mut wire = Wire::body("#EXTM3U\n");
    wire.length += 1;
    wire.stall = true;
    h.put("/wait.m3u8", wire);
    let d = delivery(&h);
    let sid = h.sid.clone();
    let job = tokio::spawn(async move { d.failover(sid, mutation(2)).await });
    wait_for(&h, "/wait.m3u8").await;
    h.store.revoke(h.sid.clone()).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), job)
            .await
            .unwrap()
            .unwrap()
            .err()
            .unwrap()
            .1,
        "preparation_cancelled"
    );
    assert_eq!(
        delivery(&h)
            .failover(h.sid.clone(), mutation(3))
            .await
            .err()
            .unwrap()
            .1,
        "revoked_grant"
    );
    h.close().await;
    let h = Harness::start(|_| {}).await;
    let mut s = seed(&h, &["/expired.m3u8", "/good.m3u8"], false).await;
    s.backups.as_mut().unwrap()[0].candidate.source.expires_at =
        Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    h.store
        .source(
            h.sid.clone(),
            UpdateSource {
                owner_id: OWNER.into(),
                expected_revision: 2,
                selection: s,
            },
        )
        .await
        .unwrap();
    h.put("/good.m3u8", Wire::body(VOD));
    assert!(
        delivery(&h)
            .failover(h.sid.clone(), mutation(3))
            .await
            .is_ok()
    );
    assert!(
        !h.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("expired.m3u8"))
    );
    h.close().await;
}
#[tokio::test]
async fn failed_store_publication_rolls_back_root_and_all_prepared_resources_on_restart() {
    let h = Harness::start(|_| {}).await;
    seed(&h, &["/good.m3u8"], false).await;
    h.put("/good.m3u8", Wire::body(VOD));
    let db = rusqlite::Connection::open(&h.config.state.database_path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_new BEFORE INSERT ON resources WHEN NEW.generation=3 AND (SELECT count(*) FROM resources WHERE sid=NEW.sid AND generation=3)>=1 BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert_eq!(
        code(call(control::control(app(&h)), &h.sid, "failover", 2, None).await).await,
        "state_store_unavailable"
    );
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM generations WHERE generation=3",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    db.execute_batch("DROP TRIGGER reject_new;").unwrap();
    drop(db);
    let config = h.config.clone();
    let sid = h.sid.clone();
    let directory = h._dir.clone();
    h.close().await;
    let (store, worker) = Store::open(config).unwrap();
    let stop = CancellationToken::new();
    let job = tokio::spawn(worker.run(stop.clone()));
    assert_eq!(store.read(sid.clone()).await.unwrap().source_generation, 2);
    assert!(
        store
            .lookup(&sid, &Resource::Entry)
            .await
            .unwrap()
            .registration
            .unwrap()
            .uri
            .ends_with("master.m3u8")
    );
    stop.cancel();
    job.await.unwrap().unwrap();
    drop(directory);
}
#[tokio::test]
async fn mp4_failover_generation_isolation_no_post_commit_substitution_and_restart_recovery() {
    let h = Harness::start_format(|_| {}, true).await;
    seed(&h, &["/backup.mp4"], true).await;
    let mut first = Wire::body(super::stage5::movie());
    first.length += 100;
    first.stall = true;
    h.put("/movie.mp4", first);
    h.put("/backup.mp4", Wire::body(super::stage5::movie()));
    let root = h.store.lookup(&h.sid, &Resource::Entry).await.unwrap();
    let old = h
        .store
        .register(h.sid.clone(), 2, vec![root.registration.unwrap()])
        .await
        .unwrap()
        .remove(0);
    let mut body = h.request(&h.entry(), "GET", None).await.into_body();
    assert!(body.frame().await.unwrap().is_ok()); // source A has committed
    assert!(
        delivery(&h)
            .failover(h.sid.clone(), mutation(2))
            .await
            .is_ok()
    );
    // Cancellation of A cannot turn its in-flight response into B or append JSON.
    h.transport.guard.invalidate();
    assert!(body.frame().await.unwrap().is_err());
    drop(body);
    let before = h
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.contains("backup.mp4"))
        .count();
    assert_eq!(before, 1); // preparation only
    let config = h.config.clone();
    let sid = h.sid.clone();
    let directory = h._dir.clone();
    h.close().await;
    let (store, worker) = Store::open(config.clone()).unwrap();
    let stop = CancellationToken::new();
    let job = tokio::spawn(worker.run(stop.clone()));
    let old_record = store
        .lookup(&sid, &Resource::Registered(old))
        .await
        .unwrap();
    assert_eq!(old_record.resource_generation, 2);
    assert!(old_record.registration.unwrap().uri.ends_with("movie.mp4"));
    let active = store.lookup(&sid, &Resource::Entry).await.unwrap();
    assert_eq!(active.resource_generation, 3);
    assert!(active.registration.unwrap().uri.ends_with("backup.mp4"));
    let (transport, _worker) = Transport::new(config.network.clone()).unwrap();
    assert!(!transport.guard().healthy());
    store.revoke(sid.clone()).await.unwrap();
    stop.cancel();
    job.await.unwrap().unwrap();
    let (store, worker) = Store::open(config).unwrap();
    let stop = CancellationToken::new();
    let job = tokio::spawn(worker.run(stop.clone()));
    assert_eq!(
        store.lookup(&sid, &Resource::Entry).await.err().unwrap().1,
        "revoked_grant"
    );
    stop.cancel();
    job.await.unwrap().unwrap();
    drop(directory);
}
#[tokio::test]
async fn private_health_reports_component_state_separately_from_provider_and_acceptance() {
    let h = Harness::start_format(|_| {}, true).await;
    let app = app(&h);
    let router = control::control(app.clone());
    async fn get(router: Router, path: &str) -> axum::response::Response {
        router
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {BEARER}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    assert_eq!(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        401
    );
    let response = get(router.clone(), "/health/readyz").await;
    assert_eq!(response.status(), 200);
    let state: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(state["provider_readiness"], "external");
    assert_eq!(state["overall_acceptance"], false);
    assert!(h.requests.lock().unwrap().is_empty());
    let permits = h
        .transport
        .slots
        .clone()
        .acquire_many_owned(h.config.network.max_streams_count as u32)
        .await
        .unwrap();
    assert_eq!(get(router.clone(), "/health/readyz").await.status(), 503);
    drop(permits);
    h.transport.guard.invalidate();
    assert_eq!(get(router.clone(), "/health/readyz").await.status(), 503);
    h.transport.guard.synthetic("restored");
    app.lifecycle.draining.store(true, Ordering::Release);
    assert_eq!(get(router.clone(), "/health/readyz").await.status(), 503);
    assert_eq!(get(router.clone(), "/health/livez").await.status(), 200);
    assert_eq!(
        code(call(router.clone(), &h.sid, "failover", 1, None).await).await,
        "draining"
    );
    app.lifecycle.failed.store(true, Ordering::Release);
    assert_eq!(get(router.clone(), "/health/livez").await.status(), 503);
    let metrics = to_bytes(get(router, "/metrics").await.into_body(), 8192)
        .await
        .unwrap();
    let metrics = std::str::from_utf8(&metrics).unwrap();
    assert!(metrics.len() < 8192);
    assert!(metrics.lines().count() < 64);
    for secret in [
        &h.sid,
        &h.grant,
        OWNER,
        BEARER,
        "source.example.com",
        "restored",
    ] {
        assert!(!metrics.contains(secret));
    }
    assert!(h.requests.lock().unwrap().is_empty());
    h.close().await;
}

#[tokio::test]
async fn active_drain_completes_accepted_transfer_and_forced_deadline_cleans_unpolled_sockets() {
    for forced in [false, true] {
        let h = Harness::start_format(
            |c| {
                c.lifecycle.drain_timeout_s = 1;
                c.network.write_idle_timeout_s = 10;
                c.network.read_idle_timeout_s = 10;
            },
            true,
        )
        .await;
        let bytes = super::stage5::movie();
        let gate = Arc::new(tokio::sync::Notify::new());
        let mut wire = Wire::body(&bytes[..24]);
        wire.length = bytes.len();
        wire.delayed_tail = Some((gate.clone(), bytes[24..].to_vec()));
        if forced {
            wire.delayed_tail = None;
            wire.logical_total = Some(1 << 40);
        }
        let closed = wire.closed.clone();
        let written = wire.written.clone();
        h.put("/movie.mp4", wire);
        let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let application = app(&h);
        let lifecycle = application.lifecycle.clone();
        let job = tokio::spawn(crate::runtime::run(
            control,
            listener,
            application,
            shutdown.clone(),
        ));
        let mut client = TcpStream::connect(address).await.unwrap();
        client
            .write_all(
                format!("GET {} HTTP/1.1\r\nHost: relay.example\r\n\r\n", h.entry()).as_bytes(),
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        while !received.windows(4).any(|v| v == b"\r\n\r\n") || received.len() < 32 {
            let mut chunk = [0; 32];
            let n = client.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            received.extend_from_slice(&chunk[..n]);
        }
        let began = tokio::time::Instant::now();
        shutdown.cancel();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(lifecycle.draining.load(Ordering::Acquire));
        assert!(!job.is_finished());
        assert!(h.transport.guard().healthy());
        assert!(h.store.health().await.is_ok());
        if !forced {
            gate.notify_one();
            client.read_to_end(&mut received).await.unwrap();
            let offset = received.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
            assert_eq!(&received[offset..], bytes);
        }
        tokio::time::timeout(Duration::from_secs(2), job)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let elapsed = began.elapsed();
        if forced {
            assert!(elapsed >= Duration::from_millis(900));
        }
        assert!(elapsed < Duration::from_secs(2));
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if closed.load(Ordering::SeqCst) > 0
                    && h.transport.available() == h.config.network.max_streams_count as usize
                    && h.transport.counts.lock().unwrap().is_empty()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        eprintln!(
            "stage6 drain forced={forced} elapsed_ms={} source_bytes_written={} permits={} source_closed={}",
            elapsed.as_millis(),
            written.load(Ordering::SeqCst),
            h.transport.available(),
            closed.load(Ordering::SeqCst)
        );
        assert!(written.load(Ordering::SeqCst) < 32 * 1024 * 1024);
        drop(client);
        h.close().await;
    }
}
#[tokio::test]
async fn published_resource_failure_never_probes_or_serves_backup_for_get_or_head() {
    let h = Harness::start_format(|_| {}, true).await;
    seed(&h, &["/backup.mp4"], true).await;
    let root = h.store.lookup(&h.sid, &Resource::Entry).await.unwrap();
    let rid = h
        .store
        .register(h.sid.clone(), 2, vec![root.registration.unwrap()])
        .await
        .unwrap()
        .remove(0);
    let path = format!("/stream/{}/r/{rid}?st={}", h.sid, h.grant);
    h.put("/movie.mp4", Wire::body(super::stage5::movie()));
    drop(h.request(&path, "HEAD", None).await);
    h.put(
        "/movie.mp4",
        Wire {
            status: 503,
            ..Wire::body("source failure")
        },
    );
    h.put("/backup.mp4", Wire::body(super::stage5::movie()));
    for method in ["GET", "HEAD"] {
        assert_eq!(h.request(&path, method, None).await.status(), 502);
    }
    assert_eq!(
        h.store.read(h.sid.clone()).await.unwrap().source_generation,
        2
    );
    assert!(
        !h.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("backup.mp4"))
    );
    h.close().await;
}

#[tokio::test]
async fn fresh_selection_preparation_can_change_format_and_publishes_resume_contract() {
    let h = Harness::start(|_| {}).await;
    let mut s = selection(&h).await;
    let candidate = s.selected.as_mut().unwrap();
    candidate.candidate.source.url = "http://source.example.com/fresh.mp4".into();
    candidate.candidate.source.selected_variant_url = None;
    candidate.candidate.source.format = "mp4".into();
    candidate.candidate.observation.format = "mp4".into();
    candidate.candidate.observation.encrypted = false;
    s.backups = None;
    h.put("/fresh.mp4", Wire::body("<html>not a movie</html>"));
    assert_eq!(
        code(
            call(
                control::control(app(&h)),
                &h.sid,
                "source",
                1,
                Some(s.clone())
            )
            .await
        )
        .await,
        "upstream_rejected"
    );
    assert_eq!(
        h.store.read(h.sid.clone()).await.unwrap().source_generation,
        1
    );
    h.put("/fresh.mp4", Wire::body(super::stage5::movie()));
    let response = call(control::control(app(&h)), &h.sid, "source", 1, Some(s)).await;
    assert_eq!(response.status(), 200);
    let ready: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(ready["format"], "mp4");
    assert_eq!(ready["resume_required"], true);
    assert_eq!(ready["auth_revision"], 1);
    let response = h.request(&h.entry(), "GET", None).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "video/mp4");
    assert_eq!(
        to_bytes(response.into_body(), 8192).await.unwrap(),
        super::stage5::movie()
    );
    h.close().await;
}

#[tokio::test]
async fn expired_active_source_can_recover_without_generation_lease_cancelling_preparation() {
    let h = Harness::start(|_| {}).await;
    let mut s = seed(&h, &["/good.m3u8"], false).await;
    s.selected.as_mut().unwrap().candidate.source.expires_at =
        Some(chrono::Utc::now() + chrono::Duration::seconds(1));
    h.store
        .source(
            h.sid.clone(),
            UpdateSource {
                owner_id: OWNER.into(),
                expected_revision: 2,
                selection: s,
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        h.store
            .lookup(&h.sid, &Resource::Entry)
            .await
            .err()
            .unwrap()
            .1,
        "source_expired"
    );
    h.put("/good.m3u8", Wire::body(VOD));
    assert!(
        delivery(&h)
            .failover(h.sid.clone(), mutation(3))
            .await
            .is_ok()
    );
    assert_eq!(
        h.store.read(h.sid.clone()).await.unwrap().source_generation,
        4
    );
    h.close().await;
}
#[tokio::test]
async fn publication_state_limit_is_local_exhaustion_and_preserves_active_generation() {
    let h = Harness::start(|c| {
        c.state.max_resources_per_session_count = 4;
    })
    .await;
    seed(&h, &["/good.m3u8", "/later.m3u8"], false).await;
    h.put("/good.m3u8", Wire::body(MASTER));
    for p in ["/v.m3u8", "/audio.m3u8", "/subs.m3u8"] {
        h.put(p, Wire::body(VOD));
    }
    assert_eq!(
        code(call(control::control(app(&h)), &h.sid, "failover", 2, None).await).await,
        "state_limit"
    );
    assert_eq!(
        h.store.read(h.sid.clone()).await.unwrap().source_generation,
        2
    );
    assert!(
        !h.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("later.m3u8"))
    );
    h.close().await;
}

#[tokio::test]
async fn diagnostic_admission_is_independent_of_single_control_permit_and_remains_bounded() {
    let h = Harness::start(|c| {
        c.limits.max_control_concurrency_count = 1;
    })
    .await;
    let application = app(&h);
    let router = control::control(application.clone());
    async fn ready(router: Router) -> axum::response::Response {
        router
            .oneshot(
                Request::builder()
                    .uri("/health/readyz")
                    .header("authorization", format!("Bearer {BEARER}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    assert_eq!(ready(router.clone()).await.status(), 200);
    // Hold the actual only control permit with an incomplete accepted request.
    let (sender, receiver) = tokio::sync::mpsc::channel::<
        std::result::Result<hyper::body::Frame<Bytes>, std::io::Error>,
    >(1);
    struct WaitingBody(
        tokio::sync::mpsc::Receiver<std::result::Result<hyper::body::Frame<Bytes>, std::io::Error>>,
        Arc<tokio::sync::Notify>,
    );
    impl hyper::body::Body for WaitingBody {
        type Data = Bytes;
        type Error = std::io::Error;
        fn poll_frame(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<std::result::Result<hyper::body::Frame<Bytes>, std::io::Error>>>
        {
            self.1.notify_one();
            self.0.poll_recv(cx)
        }
    }
    let admitted = Arc::new(tokio::sync::Notify::new());
    let request = router.clone().oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/internal/v1/sessions/{}/failover", h.sid))
            .header("authorization", format!("Bearer {BEARER}"))
            .header("content-type", "application/json")
            .body(Body::new(WaitingBody(receiver, admitted.clone())))
            .unwrap(),
    );
    let request = tokio::spawn(request);
    tokio::time::timeout(Duration::from_secs(1), admitted.notified())
        .await
        .unwrap();
    assert_eq!(ready(router.clone()).await.status(), 503);
    request.abort();
    let _ = request.await;
    drop(sender);
    assert_eq!(ready(router.clone()).await.status(), 200);
    h.close().await;
}
#[tokio::test]
async fn recovery_reserves_maximum_supported_grant_before_publishing_hls_resources() {
    let h = Harness::start(|_| {}).await;
    seed(&h, &["/large.m3u8", "/later.m3u8"], false).await;
    let mut manifest = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:6\n");
    for i in 0..300 {
        manifest.push_str(&format!("#EXTINF:6,\n{i}.ts\n"));
    }
    manifest.push_str("#EXT-X-ENDLIST\n");
    h.put("/large.m3u8", Wire::body(&manifest));
    let root = Store::root(&{
        let mut s = selection(&h).await;
        s.selected = Some(s.backups.as_ref().unwrap()[0].clone());
        s
    })
    .unwrap();
    let parse = || {
        crate::hls::parse(
            manifest.as_bytes(),
            &Url::parse(&root.uri).unwrap(),
            &root,
            6,
            4096,
            h.config.state.max_snapshot_bytes as usize,
        )
        .unwrap()
    };
    let ids = vec!["x".repeat(43); 300];
    assert!(
        parse()
            .publish(
                &ids,
                &h.sid,
                "",
                h.config.limits.max_playlist_bytes as usize
            )
            .is_ok()
    );
    assert!(
        parse()
            .publish(
                &ids,
                &h.sid,
                &"x".repeat(h.config.auth.max_grant_bytes as usize),
                h.config.limits.max_playlist_bytes as usize
            )
            .is_err()
    );
    assert_eq!(
        code(call(control::control(app(&h)), &h.sid, "failover", 2, None).await).await,
        "manifest_limit"
    );
    assert_eq!(
        h.store.read(h.sid.clone()).await.unwrap().source_generation,
        2
    );
    assert!(
        !h.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("later.m3u8"))
    );
    h.close().await;
}

/// Exercises the opt-in operator tool against the real Rust listener, verifier,
/// encrypted store and delivery path. Only the existing test connector is fake.
#[tokio::test]
#[ignore = "opt-in cross-language tooling check requires python3"]
async fn acceptance_probe_uses_production_listener_for_hls_and_mp4() {
    for mp4 in [false, true] {
        let h = Harness::start_format_gate(|_| {}, mp4, Some(4 * 1024 * 1024)).await;
        if mp4 {
            h.put("/movie.mp4", Wire::ranged(super::stage5::movie()));
        } else {
            h.put("/master.m3u8", Wire::body(MASTER));
            h.put("/v.m3u8", Wire::body(VOD));
            h.put("/audio.m3u8", Wire::body(VOD));
            h.put(
                "/subs.m3u8",
                Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.vtt\n#EXT-X-ENDLIST\n"),
            );
            let mut ts = vec![0; 188 * 3];
            for i in [0, 188, 376] {
                ts[i] = 0x47;
                ts[i + 3] = 0x10;
            }
            h.put("/a.ts", Wire::body(ts));
            h.put(
                "/a.vtt",
                Wire::body("WEBVTT\n\n00:00.000 --> 00:01.000\nSynthetic\n"),
            );
        }
        let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = stream.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let job = tokio::spawn(crate::runtime::run(
            control,
            stream,
            app(&h),
            shutdown.clone(),
        ));
        let receipt = h._dir.path().join("receipt.json");
        let grant = h._dir.path().join("grant");
        std::fs::write(
            &receipt,
            serde_json::to_vec(&json!({
                "contract_version":"relay.v1", "session_id":h.sid,
                "entry_path":format!("/stream/{}/entry",h.sid),
                "format":if mp4 {"mp4"} else {"hls"}
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(&grant, &h.grant).unwrap();
        for path in [&receipt, &grant] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut command = tokio::process::Command::new("python3");
        command
            .kill_on_drop(true)
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tools/acceptance_probe.py"
            ))
            .arg("--opt-in")
            .arg("--port")
            .arg(address.port().to_string())
            .arg("--receipt")
            .arg(receipt)
            .arg("--grant-file")
            .arg(grant);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output())
            .await
            .unwrap()
            .unwrap();
        shutdown.cancel();
        job.await.unwrap().unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["evidence"], "bounded_relay_byte_access");
        assert_eq!(report["decoded"], false);
        assert_eq!(report["player_verified"], false);
        assert!(report["media_prefixes_or_ranges"].as_u64().unwrap() >= 2);
        let report = String::from_utf8(output.stdout).unwrap();
        assert!(!report.contains(&h.grant));
        assert!(!report.contains(&h.sid));
        assert!(!report.contains("source.example"));
        let remaining: i64 = rusqlite::Connection::open(h._dir.path().join("acceptance.sqlite"))
            .unwrap()
            .query_row("SELECT remaining FROM acceptance", [], |r| r.get(0))
            .unwrap();
        assert!(remaining > 0 && remaining < 4 * 1024 * 1024);
        h.close().await;
    }
}

#[tokio::test]
async fn acceptance_gate_cancels_concurrent_mp4_ranges_retries_and_network_owner() {
    let h = Harness::start_format_gate(|_| {}, true, Some(256 * 1024)).await;
    let mut movie = super::stage5::movie();
    movie.resize(1024 * 1024, 1);
    h.put("/movie.mp4", Wire::ranged(movie));
    let mut clients = JoinSet::new();
    for _ in 0..4 {
        let router = h.router.clone();
        let path = h.entry();
        clients.spawn(async move {
            for _ in 0..4 {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(&path)
                            .header("range", "bytes=0-65535")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let _ = to_bytes(response.into_body(), 128 * 1024).await;
            }
        });
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(client) = clients.join_next().await {
            client.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(h.transport.acceptance.as_ref().unwrap().stop.is_cancelled());
    let remaining: i64 = rusqlite::Connection::open(h._dir.path().join("acceptance.sqlite"))
        .unwrap()
        .query_row("SELECT remaining FROM acceptance", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 0);
    assert!(!h.transport.guard.healthy());
    h.close().await;
}

#[path = "stage6_service.rs"]
mod service;
