#![allow(clippy::unwrap_used)]
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
fn setup(address: SocketAddr, answers: &[&str]) -> (Arc<Transport>, Worker, Arc<AtomicUsize>) {
    let config =
        crate::config::Config::parse(include_str!("../../../config/relay.example.toml")).unwrap();
    setup_network(address, answers, config.network)
}
fn setup_network(
    address: SocketAddr,
    answers: &[&str],
    config: crate::config::Network,
) -> (Arc<Transport>, Worker, Arc<AtomicUsize>) {
    let (mut transport, worker) = Transport::new(config).unwrap();
    let dials = Arc::new(AtomicUsize::new(0));
    Arc::get_mut(&mut transport).unwrap().mock = Some(Mock {
        address,
        answers: answers.iter().map(|s| s.parse().unwrap()).collect(),
        dials: dials.clone(),
        real_dns: false,
        rebind: false,
        resolutions: Arc::new(AtomicUsize::new(0)),
        pending_dns: false,
        pending_connect: false,
        first_timeout: false,
    });
    transport.guard.synthetic("exit-a");
    (transport, worker, dials)
}
fn context() -> FetchContext {
    serde_json::from_str(r#"{"scopes":[{"origin":"http://source.example.com","path_prefix":"/movie","headers":{"Authorization":"source-only","X-Csrf-Token":"csrf-only"},"cookies":"source-only=1","cookie_path":"/movie"}],"public_resources":{"referer":"https://root.example.com/"}}"#).unwrap()
}
fn fetch<'a>(context: &'a FetchContext, url: &'a str, cancel: CancellationToken) -> Fetch<'a> {
    Fetch {
        range: None,
        if_range: None,
        url,
        method: Method::GET,
        context,
        session_id: "synthetic-session",
        deadline: Instant::now() + Duration::from_secs(3),
        cancel,
    }
}
async fn read_request(socket: &mut (impl tokio::io::AsyncRead + Unpin)) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        socket.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 16384);
    }
    String::from_utf8(bytes).unwrap()
}
#[tokio::test]
async fn real_hyper_redirect_rebuilds_credentials_and_original_host() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (transport, worker, dials) = setup(
        listener.local_addr().unwrap(),
        &["8.8.8.8", "2606:4700:4700::1111"],
    );
    let shutdown = CancellationToken::new();
    let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        let first_headers = read_request(&mut first).await.to_ascii_lowercase();
        assert!(first_headers.contains("host: source.example.com\r\n"));
        assert!(first_headers.contains("authorization: source-only\r\n"));
        assert!(first_headers.contains("cookie: source-only=1\r\n"));
        first.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://cdn.example.com/video\r\nContent-Length: 999999\r\n\r\n").await.unwrap();
        let (mut second, _) = listener.accept().await.unwrap();
        let headers = read_request(&mut second).await.to_ascii_lowercase();
        assert!(headers.contains("host: cdn.example.com\r\n"));
        assert!(headers.contains("referer: https://root.example.com/\r\n"));
        for name in [
            "authorization:",
            "cookie:",
            "x-csrf-token:",
            "proxy-authorization:",
            "st=",
        ] {
            assert!(!headers.contains(name));
        }
        second.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Encoding: gzip\r\nSet-Cookie: ignored=1\r\n\r\nraw").await.unwrap();
    });
    let context = context();
    let mut response = transport
        .fetch(fetch(
            &context,
            "http://source.example.com/movie/file",
            CancellationToken::new(),
        ))
        .await
        .unwrap();
    assert_eq!(response.url.host_str(), Some("cdn.example.com"));
    assert_eq!(response.headers["content-encoding"], "gzip");
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, b"raw");
    assert_eq!(dials.load(Ordering::SeqCst), 2);
    drop(response);
    server.await.unwrap();
    shutdown.cancel();
    owner.await.unwrap();
}
#[tokio::test]
async fn mixed_dns_answers_never_dial() {
    let (transport, _worker, dials) =
        setup("127.0.0.1:1".parse().unwrap(), &["8.8.8.8", "127.0.0.1"]);
    assert!(matches!(
        transport
            .fetch(fetch(
                &context(),
                "http://source.example.com/movie",
                CancellationToken::new()
            ))
            .await,
        Err(NetError::Destination)
    ));
    assert_eq!(dials.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn redirect_to_private_is_blocked_before_second_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (transport, worker, dials) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
    let shutdown = CancellationToken::new();
    let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1/private\r\nContent-Length: 0\r\n\r\n").await.unwrap();
    });
    assert!(matches!(
        transport
            .fetch(fetch(
                &context(),
                "http://source.example.com/movie",
                CancellationToken::new()
            ))
            .await,
        Err(NetError::Destination)
    ));
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    server.await.unwrap();
    shutdown.cancel();
    owner.await.unwrap();
}
#[tokio::test]
async fn guard_loss_closes_unpolled_active_body_and_stale_epoch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (transport, worker, _) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
    let shutdown = CancellationToken::new();
    let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
    let (sent, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 99999\r\n\r\n")
            .await
            .unwrap();
        sent.send(()).unwrap();
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), socket.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let mut response = transport
        .fetch(fetch(
            &context(),
            "http://source.example.com/movie",
            CancellationToken::new(),
        ))
        .await
        .unwrap();
    received.await.unwrap();
    let old = transport.guard.lease().unwrap();
    transport.guard.synthetic("exit-b");
    assert!(old.cancel.is_cancelled());
    assert!(matches!(response.chunk().await, Err(NetError::Guard)));
    server.await.unwrap();
    drop(response);
    shutdown.cancel();
    owner.await.unwrap();
}
#[tokio::test]
async fn guard_and_caller_cancellation_interrupt_dns_and_connect() {
    for dns in [true, false] {
        for guard_loss in [true, false] {
            let (mut transport, _worker, _) = setup("127.0.0.1:1".parse().unwrap(), &["8.8.8.8"]);
            let mock = Arc::get_mut(&mut transport).unwrap().mock.as_mut().unwrap();
            mock.pending_dns = dns;
            mock.pending_connect = !dns;
            let context = context();
            let cancel = CancellationToken::new();
            let pending = transport.fetch(fetch(
                &context,
                "http://source.example.com/movie",
                cancel.clone(),
            ));
            tokio::pin!(pending);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut pending)
                    .await
                    .is_err()
            );
            if guard_loss {
                transport.guard.invalidate();
            } else {
                cancel.cancel();
            }
            let result = tokio::time::timeout(Duration::from_millis(100), pending)
                .await
                .unwrap();
            assert!(matches!(
                result,
                Err(NetError::Guard) | Err(NetError::Cancelled)
            ));
        }
    }
}
#[test]
fn closed_guard_and_public_policy_cover_alternate_forms() {
    for ip in [
        "0.0.0.0",
        "10.1.1.1",
        "100.64.0.1",
        "169.254.169.254",
        "192.0.0.9",
        "192.88.99.1",
        "198.18.0.1",
        "203.0.113.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "::ffff:8.8.8.8",
        "64:ff9b::808:808",
        "2001::1",
        "2001:db8::1",
        "2002:808:808::1",
        "3fff::1",
        "fc00::1",
        "fe80::1",
    ] {
        assert!(!destination::public_ip(ip.parse().unwrap()), "{ip}");
    }
    for url in [
        "http://localhost/",
        "http://a.local/",
        "http://127.1/",
        "http://2130706433/",
        "http://0x08080808/",
        "http://134744072/",
        "http://%38.8.8.8/",
        "https://user@public.example.com/",
        "http://public.example.com:443/",
        "https://public.example.com/#frag",
        "http://[fe80::1%25en0]/",
        "file:///tmp/file",
        "https://a\\@public.example.com/",
    ] {
        assert!(destination::destination(url).is_err(), "{url}");
    }
    assert!(destination::destination("https://example.com/").is_ok());
    let from = destination::destination("https://example.com/").unwrap();
    assert!(destination::redirect(&from, "http://example.com/").is_err());
    assert!(!Guard::default().healthy());
}
#[test]
fn context_scope_segment_cookie_and_encoded_paths() {
    let context = context();
    for url in [
        "http://source.example.com/moviefake",
        "http://source.example.com/movie%2ffile",
        "http://source.example.com/movie//file",
        "https://source.example.com/movie/file",
        "http://cdn.example.com/movie/file",
    ] {
        let headers = context::headers(&context, &destination::destination(url).unwrap()).unwrap();
        for name in ["authorization", "cookie", "x-csrf-token"] {
            assert!(!headers.contains_key(name), "{url}");
        }
    }
}

#[test]
fn proxy_environment_poisoning_does_not_change_transport() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "network::transport::tests::real_hyper_redirect_rebuilds_credentials_and_original_host",
        ])
        .env("HTTP_PROXY", "http://proxy-user:proxy-secret@127.0.0.1:1")
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("ALL_PROXY", "socks5h://127.0.0.1:1")
        .env("http_proxy", "http://127.0.0.1:1")
        .env("https_proxy", "http://127.0.0.1:1")
        .env("all_proxy", "socks5h://127.0.0.1:1")
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("1 passed")
    );
}
#[tokio::test]
async fn redirects_reresolve_and_stop_at_the_configured_limit() {
    for rebind in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut transport, worker, dials) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
        Arc::get_mut(&mut transport)
            .unwrap()
            .mock
            .as_mut()
            .unwrap()
            .rebind = rebind;
        let shutdown = CancellationToken::new();
        let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
        let server = tokio::spawn(async move {
            for _ in 0..if rebind { 1 } else { 4 } {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_request(&mut socket).await;
                socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: /movie/again\r\nContent-Length: 0\r\n\r\n").await.unwrap();
            }
        });
        let result = transport
            .fetch(fetch(
                &context(),
                "http://source.example.com/movie",
                CancellationToken::new(),
            ))
            .await;
        if rebind {
            assert!(matches!(result, Err(NetError::Destination)));
        } else {
            assert!(matches!(result, Err(NetError::Protocol)));
        }
        assert_eq!(dials.load(Ordering::SeqCst), if rebind { 1 } else { 4 });
        server.await.unwrap();
        shutdown.cancel();
        owner.await.unwrap();
    }
}
#[tokio::test]
async fn verified_tls_uses_original_sni_and_rejects_untrusted_or_wrong_host() {
    use tokio_rustls::{
        TlsAcceptor,
        rustls::{
            ServerConfig,
            pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        },
    };
    let ca = include_bytes!("../../../tests/fixtures/security/tls/ca.der");
    let cert = include_bytes!("../../../tests/fixtures/security/tls/leaf.der");
    let key = include_bytes!("../../../tests/fixtures/security/tls/leaf-key.der");
    for (trust, host, success) in [
        (true, "source.example.com", true),
        (false, "source.example.com", false),
        (true, "cdn.example.com", false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut transport, worker, _) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
        if trust {
            let mut roots = RootCertStore::empty();
            roots.add(CertificateDer::from(ca.to_vec())).unwrap();
            Arc::get_mut(&mut transport).unwrap().tls = TlsConnector::from(Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            ));
        }
        let tls = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert.to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec())),
            )
            .unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let tls = TlsAcceptor::from(Arc::new(tls)).accept(socket).await;
            if success {
                let mut tls = tls.unwrap();
                assert_eq!(tls.get_ref().1.server_name(), Some("source.example.com"));
                let headers = read_request(&mut tls).await;
                assert!(headers.contains("host: source.example.com\r\n"));
                tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .await
                    .unwrap();
            }
        });
        let shutdown = CancellationToken::new();
        let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
        let context = context();
        let url = format!("https://{host}/movie");
        let result = transport
            .fetch(fetch(&context, &url, CancellationToken::new()))
            .await;
        if success {
            let mut response = result.unwrap();
            assert_eq!(response.chunk().await.unwrap().unwrap(), "ok");
        } else {
            assert!(matches!(result, Err(NetError::Tls)));
        }
        server.await.unwrap();
        shutdown.cancel();
        owner.await.unwrap();
    }
}

#[tokio::test]
async fn production_doh_parser_and_dual_queries_precede_source_dial() {
    use tokio_rustls::{
        TlsAcceptor,
        rustls::{
            ServerConfig,
            pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        },
    };
    for mixed in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut transport, worker, dials) = setup(listener.local_addr().unwrap(), &[]);
        let inner = Arc::get_mut(&mut transport).unwrap();
        inner.mock.as_mut().unwrap().real_dns = true;
        inner.dns_url = destination::destination("https://source.example.com/dns-query").unwrap();
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(
                include_bytes!("../../../tests/fixtures/security/tls/ca.der").to_vec(),
            ))
            .unwrap();
        inner.tls = TlsConnector::from(Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ));
        let tls = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(
                    include_bytes!("../../../tests/fixtures/security/tls/leaf.der").to_vec(),
                )],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    include_bytes!("../../../tests/fixtures/security/tls/leaf-key.der").to_vec(),
                )),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(tls));
        let server = tokio::spawn(async move {
            let barrier = Arc::new(tokio::sync::Barrier::new(2));
            let mut queries = JoinSet::new();
            for _ in 0..2 {
                let (socket, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                let barrier = barrier.clone();
                queries.spawn(async move {
                    let mut tls = acceptor.accept(socket).await.unwrap();
                    let request = read_request(&mut tls).await;
                    let kind = if request.contains("type=28 HTTP") { 28 } else { 1 };
                    assert!(request.starts_with(&format!("GET /dns-query?name=actual.example.com&type={kind} HTTP/1.1")));
                    assert!(request.contains("accept: application/dns-json"));
                    assert!(!request.contains("authorization:"));
                    // A sequential resolver cannot receive the first answer.
                    barrier.wait().await;
                    let ip = if kind == 1 { "8.8.8.8" } else if mixed { "fe80::1" } else { "2606:4700:4700::1111" };
                    let body = serde_json::json!({"Status":0,"TC":false,"Question":[{"name":"actual.example.com.","type":kind}],"Answer":[{"type":kind,"TTL":30,"data":ip}]}).to_string();
                    tls.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/dns-json\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
                });
            }
            while let Some(result) = queries.join_next().await {
                result.unwrap();
            }
            if !mixed {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_request(&mut socket).await;
                assert!(request.contains("host: actual.example.com"));
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .await
                    .unwrap();
            }
        });
        let shutdown = CancellationToken::new();
        let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
        let context = context();
        let result = transport
            .fetch(fetch(
                &context,
                "http://actual.example.com/movie",
                CancellationToken::new(),
            ))
            .await;
        if mixed {
            assert!(matches!(result, Err(NetError::Destination)));
            assert_eq!(dials.load(Ordering::SeqCst), 2);
        } else {
            let mut response = result.unwrap();
            assert_eq!(response.chunk().await.unwrap().unwrap(), "ok");
            assert_eq!(dials.load(Ordering::SeqCst), 3);
        }
        server.await.unwrap();
        shutdown.cancel();
        owner.await.unwrap();
    }
    for bytes in [
        br#"{"Status":0,"TC":false,"Question":[{"name":"wrong.example.com","type":1}]}"#.as_slice(),
        br#"{"Status":0,"TC":true}"#,
        br#"{"Status":2,"TC":false}"#,
        br#"invalid"#,
    ] {
        assert!(parse_dns(bytes, "actual.example.com", 1).is_err());
    }
}
#[tokio::test]
async fn dropping_fetch_while_waiting_for_headers_closes_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (transport, worker, _) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
    let shutdown = CancellationToken::new();
    let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
    let (sent, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        sent.send(()).unwrap();
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), socket.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let context = context();
    let mut pending = Box::pin(transport.fetch(fetch(
        &context,
        "http://source.example.com/movie",
        CancellationToken::new(),
    )));
    tokio::select! {biased;_ = received => {},_ = &mut pending => unreachable!("server withholds headers")};
    drop(pending);
    server.await.unwrap();
    shutdown.cancel();
    owner.await.unwrap();
}

#[tokio::test]
async fn slow_guard_monitor_cannot_starve_jobs_and_is_joined_on_shutdown() {
    let (transport, worker, _) = setup("127.0.0.1:1".parse().unwrap(), &["8.8.8.8"]);
    let shutdown = CancellationToken::new();
    let (entered, started) = tokio::sync::oneshot::channel();
    let finished = Arc::new(AtomicUsize::new(0));
    struct MonitorOwner(Arc<AtomicUsize>);
    impl Drop for MonitorOwner {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let state = finished.clone();
    let monitor: Driver = Box::pin(async move {
        let _owned = MonitorOwner(state);
        entered.send(()).unwrap();
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    });
    let owner = tokio::spawn(worker.run_with_monitor(shutdown.clone(), Some(monitor)));
    started.await.unwrap();
    let (sent, received) = tokio::sync::oneshot::channel();
    transport
        .jobs
        .try_send(Box::pin(async move {
            sent.send(()).unwrap();
        }))
        .unwrap();
    tokio::time::timeout(Duration::from_millis(100), received)
        .await
        .unwrap()
        .unwrap();
    shutdown.cancel();
    owner.await.unwrap();
    assert_eq!(finished.load(Ordering::SeqCst), 1);
    assert!(!transport.guard.healthy());
}
#[tokio::test]
async fn connect_timeout_advances_to_next_validated_address_only_within_total_deadline() {
    for expired in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut transport, worker, dials) = setup(
            listener.local_addr().unwrap(),
            &["8.8.8.8", "2606:4700:4700::1111"],
        );
        let inner = Arc::get_mut(&mut transport).unwrap();
        inner.config.connect_timeout_s = 1;
        inner.mock.as_mut().unwrap().first_timeout = true;
        let shutdown = CancellationToken::new();
        let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
        let server = if !expired {
            Some(tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_request(&mut socket).await;
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .await
                    .unwrap();
            }))
        } else {
            None
        };
        let context = context();
        let mut request = fetch(
            &context,
            "http://source.example.com/movie",
            CancellationToken::new(),
        );
        if expired {
            request.deadline = Instant::now() + Duration::from_millis(30);
        }
        let result = transport.fetch(request).await;
        if expired {
            assert!(matches!(result, Err(NetError::Timeout)));
            assert_eq!(dials.load(Ordering::SeqCst), 1);
        } else {
            let mut response = result.unwrap();
            assert_eq!(response.chunk().await.unwrap().unwrap(), "ok");
            assert_eq!(dials.load(Ordering::SeqCst), 3);
        }
        if let Some(server) = server {
            server.await.unwrap();
        }
        shutdown.cancel();
        owner.await.unwrap();
    }
}

mod stage4;

#[tokio::test]
async fn exhausted_address_connect_timeout_remains_timeout_instead_of_source_rejection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut transport, worker, _) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
    let inner = Arc::get_mut(&mut transport).unwrap();
    inner.config.connect_timeout_s = 1;
    inner.mock.as_mut().unwrap().first_timeout = true;
    let stop = CancellationToken::new();
    let owner = tokio::spawn(worker.run_inner(stop.clone(), false));
    let context = context();
    let result = transport
        .fetch(fetch(
            &context,
            "http://source.example.com/movie",
            CancellationToken::new(),
        ))
        .await;
    assert!(matches!(result, Err(NetError::Timeout)));
    stop.cancel();
    owner.await.unwrap();
}

#[tokio::test]
async fn completed_bodies_reuse_one_connection_and_rebuild_scoped_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (transport, worker, dials) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
    let shutdown = CancellationToken::new();
    let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let first = read_request(&mut socket).await.to_ascii_lowercase();
        assert!(first.contains("authorization: source-only"));
        assert!(!first.contains("connection: close"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await
            .unwrap();
        let second = read_request(&mut socket).await.to_ascii_lowercase();
        assert!(second.starts_with("get /public/video"));
        assert!(!second.contains("authorization:") && !second.contains("cookie:"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await
            .unwrap();
    });
    let context = context();
    for path in ["movie/first", "public/video"] {
        let url = format!("http://source.example.com/{path}");
        let mut response = transport
            .fetch(fetch(&context, &url, CancellationToken::new()))
            .await
            .unwrap();
        while response.chunk().await.unwrap().is_some() {}
    }
    server.await.unwrap();
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    shutdown.cancel();
    owner.await.unwrap();
}

#[tokio::test]
async fn incomplete_response_and_changed_epoch_never_return_idle_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (transport, worker, dials) = setup(listener.local_addr().unwrap(), &["8.8.8.8"]);
    let shutdown = CancellationToken::new();
    let owner = tokio::spawn(worker.run_inner(shutdown.clone(), false));
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        read_request(&mut first).await;
        first
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9999\r\n\r\nx")
            .await
            .unwrap();
        let mut byte = [0];
        assert_eq!(first.read(&mut byte).await.unwrap(), 0);
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await
                .unwrap();
            assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
        }
    });
    let context = context();
    let response = transport
        .fetch(fetch(
            &context,
            "http://source.example.com/movie",
            CancellationToken::new(),
        ))
        .await
        .unwrap();
    drop(response);
    let mut response = transport
        .fetch(fetch(
            &context,
            "http://source.example.com/movie",
            CancellationToken::new(),
        ))
        .await
        .unwrap();
    while response.chunk().await.unwrap().is_some() {}
    drop(response);
    transport.guard.invalidate();
    transport.guard.synthetic("test-new-epoch");
    let mut response = transport
        .fetch(fetch(
            &context,
            "http://source.example.com/movie",
            CancellationToken::new(),
        ))
        .await
        .unwrap();
    while response.chunk().await.unwrap().is_some() {}
    drop(response);
    shutdown.cancel();
    owner.await.unwrap();
    server.await.unwrap();
    assert_eq!(dials.load(Ordering::SeqCst), 3);
}

#[test]
fn dns_positive_cache_respects_cname_ttl_and_missing_ttl_disables_reuse() {
    let body=serde_json::json!({"Status":0,"TC":false,"Question":[{"name":"example.com","type":1}],"Answer":[{"type":5,"TTL":3,"data":"cdn.example.com"},{"type":1,"TTL":120,"data":"8.8.8.8"}]}).to_string();
    assert_eq!(
        parse_dns_with_ttl(body.as_bytes(), "example.com", 1)
            .unwrap()
            .1,
        Duration::from_secs(3)
    );
    let body = body.replace("\"TTL\":3", "\"unused\":3");
    assert_eq!(
        parse_dns_with_ttl(body.as_bytes(), "example.com", 1)
            .unwrap()
            .1,
        Duration::ZERO
    );
}
