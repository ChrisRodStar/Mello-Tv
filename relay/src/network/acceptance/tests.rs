#![allow(clippy::unwrap_used)]
use super::*;
use std::os::unix::fs::PermissionsExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) fn ledger(path: &Path, bytes: i64, milliseconds: i64) {
    std::fs::set_permissions(
        path.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let db = Connection::open(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    db.execute_batch("CREATE TABLE acceptance (id INTEGER PRIMARY KEY, byte_limit INTEGER, remaining INTEGER, deadline_ms INTEGER, stopped INTEGER);").unwrap();
    db.execute(
        "INSERT INTO acceptance VALUES (1,?1,?1,?2,0)",
        rusqlite::params![bytes, now_ms().unwrap() + milliseconds],
    )
    .unwrap();
}
fn remaining(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT remaining FROM acceptance", [], |r| r.get(0))
        .unwrap()
}
#[tokio::test]
async fn fast_producer_never_reads_unreserved_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget");
    ledger(&path, 100_000, 5000);
    let gate = Gate::open(&path).unwrap();
    let (reader, mut writer) = tokio::io::duplex(256 * 1024);
    let producer = tokio::spawn(async move {
        let _ = writer.write_all(&vec![1; 256 * 1024]).await;
    });
    let mut reader = gate.wrap(reader);
    let mut total = 0;
    let mut buf = [0; 32768];
    while let Ok(n) = reader.read(&mut buf).await {
        if n == 0 {
            break;
        }
        total += n;
    }
    assert_eq!(remaining(&path), 0);
    assert!(total <= 100_000);
    assert_eq!(total, 98_304);
    assert!(gate.stop.is_cancelled());
    drop(reader);
    producer.await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_independent_connections_retries_and_restart_share_durable_quota() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget");
    ledger(&path, 100_000, 5000);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let path = path.clone();
        tasks.spawn(async move {
            // Independent instances model separate processes/SQLite connections.
            let gate = Gate::open(&path);
            let mut received = 0;
            if let Ok(gate) = gate {
                let mut r = gate.wrap(std::io::Cursor::new(vec![1; 100_000]));
                let mut b = [0; 1024];
                while let Ok(n) = r.read(&mut b).await {
                    if n == 0 {
                        break;
                    }
                    received += n;
                }
            }
            received
        });
    }
    let mut received = 0;
    while let Some(n) = tasks.join_next().await {
        received += n.unwrap();
    }
    assert!(received <= 100_000);
    assert_eq!(remaining(&path), 0);
    assert!(Gate::open(&path).is_err());
}
#[tokio::test]
async fn restart_and_cancelled_pending_read_never_refund_credit_or_extend_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget");
    ledger(&path, 100_000, 250);
    let gate = Gate::open(&path).unwrap();
    let (r, _w) = tokio::io::duplex(100);
    let mut r = gate.wrap(r);
    let mut b = [0; 1000];
    assert!(
        tokio::time::timeout(Duration::from_millis(20), r.read(&mut b))
            .await
            .is_err()
    );
    assert_eq!(remaining(&path), 99_000);
    drop(r);
    drop(gate);
    let gate = Gate::open(&path).unwrap();
    let (r, _w) = tokio::io::duplex(100);
    let mut r = gate.wrap(r);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), r.read(&mut b))
            .await
            .unwrap()
            .is_err()
    );
    assert!(Gate::open(&path).is_err());
}
#[tokio::test]
async fn exhaustion_wakes_stalled_socket_and_global_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget");
    ledger(&path, 2000, 5000);
    let gate = Gate::open(&path).unwrap();
    let (r, _w) = tokio::io::duplex(100);
    let mut r = gate.wrap(r);
    let mut b = [0; 1000];
    assert!(
        tokio::time::timeout(Duration::from_millis(20), r.read(&mut b))
            .await
            .is_err()
    );
    assert!(gate.reserve(1000).is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), r.read(&mut b))
            .await
            .unwrap()
            .is_err()
    );
    tokio::time::timeout(Duration::from_millis(100), gate.stopped())
        .await
        .unwrap();
}

#[tokio::test]
async fn ledger_lock_or_corruption_fails_closed_without_replenishing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget");
    ledger(&path, 100_000, 5000);
    let gate = Gate::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(gate.reserve(1000).is_err());
    assert!(gate.stop.is_cancelled());
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(remaining(&path), 100_000);
    db.execute("UPDATE acceptance SET remaining=100001", [])
        .unwrap();
    assert!(Gate::open(&path).is_err());
    let alias = dir.path().join("link");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(Gate::open(&alias).is_err());
}

#[tokio::test]
async fn sequential_reconnects_charge_kernel_headroom_even_without_body_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget");
    ledger(&path, 100_000, 5000);
    for attempt in 1..=3 {
        let gate = Gate::open(&path).unwrap();
        gate.reserve_connection().unwrap();
        assert_eq!(remaining(&path), 100_000 - attempt * 32_768);
        // Restarting or retrying never refunds a closed connection's buffer.
    }
    let gate = Gate::open(&path).unwrap();
    assert!(gate.reserve_connection().is_err());
    assert_eq!(remaining(&path), 0);
    assert!(Gate::open(&path).is_err());
}

#[tokio::test]
async fn repeated_reads_and_writes_after_byte_or_time_shutoff_keep_returning_errors() {
    for exhausted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget");
        ledger(&path, 100_000, if exhausted { 5000 } else { 20 });
        let gate = Gate::open(&path).unwrap();
        let (r, _w) = tokio::io::duplex(100);
        let mut r = gate.wrap(r);
        if exhausted {
            gate.stop.cancel();
        } else {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        let mut b = [0; 100];
        for _ in 0..3 {
            assert!(r.read(&mut b).await.is_err());
            assert!(r.write(b"data").await.is_err());
        }
    }
}

#[tokio::test]
async fn http_request_quota_is_durable_shared_and_separate_from_receive_allowance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget");
    ledger(&path, 100_000, 5000);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE acceptance_requests (id INTEGER PRIMARY KEY, request_limit INTEGER, remaining INTEGER); INSERT INTO acceptance_requests VALUES (1,3,3);").unwrap();
    let first = Gate::open(&path).unwrap();
    let second = Gate::open(&path).unwrap();
    first.request().unwrap();
    second.request().unwrap();
    drop(first);
    let reopened = Gate::open(&path).unwrap();
    reopened.request().unwrap();
    assert!(second.request().is_err());
    assert!(second.stop.is_cancelled());
    assert!(Gate::open(&path).is_err());
    assert_eq!(remaining(&path), 100_000);
}
