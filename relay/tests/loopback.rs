#![allow(clippy::unwrap_used)]
use family_stream_relay::{config::Config, control::App, runtime};
use std::{os::unix::fs::PermissionsExt, process::Command, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn importer_uses_real_control_route_and_supervisor_joins_shutdown() {
    let mut config = Config::parse(include_str!("../config/relay.example.toml")).unwrap();
    // The partial-header socket may be accepted before cancellation. Exercise a
    // configured one-second drain rather than racing accept against shutdown
    // while expecting the default longer drain to finish inside two seconds.
    config.lifecycle.drain_timeout_s = 1;
    let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = control.local_addr().unwrap();
    let stream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stream_address = stream.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let token = b"synthetic-bearer-01234567890123456789012345";
    let service = tokio::spawn(runtime::run(
        control,
        stream,
        App::new(config, token),
        shutdown.clone(),
    ));
    let directory = tempfile::tempdir().unwrap();
    let bearer = directory.path().join("bearer");
    std::fs::write(&bearer, token).unwrap();
    let selection = directory.path().join("selection");
    std::fs::write(
        &selection,
        include_bytes!("fixtures/providers/renditions.json"),
    )
    .unwrap();
    for path in [&bearer, &selection] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let expires = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_relayctl"))
            .arg("import-selection")
            .arg(address.to_string())
            .arg(bearer)
            .arg(selection)
            .arg("fixture-household-0001")
            .arg(expires)
            .arg("fixture-request-0001")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "control_status=503\n"
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "relayctl: session_import_rejected\n"
    );
    let mut client = TcpStream::connect(stream_address).await.unwrap();
    client
        .write_all(b"GET /internal/v1/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 404"));
    // A client that never finishes headers must not hold shutdown forever.
    let mut slow = TcpStream::connect(address).await.unwrap();
    slow.write_all(b"GET /health/livez HTTP/1.1\r\n")
        .await
        .unwrap();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn finite_connections_drop_excess_and_incomplete_headers_have_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(runtime::listener(
        listener,
        family_stream_relay::control::streaming(),
        shutdown.clone(),
        1,
        Duration::from_secs(1),
    ));
    let mut first = TcpStream::connect(address).await.unwrap();
    first
        .write_all(b"GET /stream/fixture/entry HTTP/1.1\r\n")
        .await
        .unwrap();
    // Wait until the owner has accepted the first socket, without depending on a fixed sleep.
    let mut rejected = false;
    for _ in 0..20 {
        let mut excess = TcpStream::connect(address).await.unwrap();
        let mut byte = [0; 1];
        if let Ok(result) =
            tokio::time::timeout(Duration::from_millis(20), excess.read(&mut byte)).await
        {
            rejected = matches!(result, Ok(0) | Err(_));
            if rejected {
                break;
            }
        }
    }
    assert!(rejected);
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(7), first.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    shutdown.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn drain_deadline_cancels_incomplete_control_body_and_joins_tasks() {
    let mut config = Config::parse(include_str!("../config/relay.example.toml")).unwrap();
    config.lifecycle.drain_timeout_s = 1;
    config.limits.max_control_concurrency_count = 1;
    let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = control.local_addr().unwrap();
    let stream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shutdown = CancellationToken::new();
    let service = tokio::spawn(runtime::run(
        control,
        stream,
        App::new(config, b"synthetic-bearer-01234567890123456789012345"),
        shutdown.clone(),
    ));
    let mut slow = TcpStream::connect(address).await.unwrap();
    slow.write_all(b"POST /internal/v1/sessions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer synthetic-bearer-01234567890123456789012345\r\nContent-Type: application/json\r\nIdempotency-Key: fixture\r\nContent-Length: 100\r\n\r\n").await.unwrap();
    // Confirm the slow body owns the only control slot before initiating drain.
    // Diagnostics now have independent admission, so probe another control route.
    let mut admitted = false;
    for _ in 0..20 {
        let mut other = TcpStream::connect(address).await.unwrap();
        other.write_all(b"GET /internal/v1/sessions/synthetic-session-001 HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer synthetic-bearer-01234567890123456789012345\r\n\r\n").await.unwrap();
        let mut response = Vec::new();
        other.read_to_end(&mut response).await.unwrap();
        if response.starts_with(b"HTTP/1.1 429") {
            admitted = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(admitted);
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut response = Vec::new();
    slow.read_to_end(&mut response).await.unwrap();
    assert!(response.is_empty());
}

#[test]
fn daemon_rejects_missing_verification_keys_before_network_startup() {
    let directory = tempfile::tempdir().unwrap();
    let bearer = directory.path().join("bearer");
    std::fs::write(&bearer, b"synthetic-bearer-01234567890123456789012345").unwrap();
    std::fs::set_permissions(&bearer, std::fs::Permissions::from_mode(0o600)).unwrap();
    let config = include_str!("../config/relay.example.toml")
        .replace(
            "/run/secrets/relay-control-bearer",
            bearer.to_str().unwrap(),
        )
        .replace(
            "/run/secrets/api-playback-public-keys.json",
            directory.path().join("missing").to_str().unwrap(),
        );
    let path = directory.path().join("relay.toml");
    std::fs::write(&path, config).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_family-stream-relay"))
        .arg(path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stderr, b"relay: invalid_configuration\n");
}
