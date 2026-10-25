use std::{net::IpAddr, time::Duration};
use stream_media::network::{
    EgressMode, FetchRequest, HttpClient, ProxyEndpoint, TransportConfig, TransportError,
    parse_destination_url, public_ip,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn special_use_addresses_and_ambiguous_destination_spellings_are_denied() -> TestResult {
    for raw in [
        "0.0.0.0",
        "10.0.0.1",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.0.0.1",
        "192.0.2.1",
        "192.88.99.1",
        "192.168.0.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "::ffff:8.8.8.8",
        "64:ff9b::808:808",
        "2001:db8::1",
        "2002:808:808::1",
        "3fff::1",
        "fc00::1",
        "fe80::1",
        "ff00::1",
    ] {
        assert!(!public_ip(raw.parse::<IpAddr>()?), "accepted {raw}");
    }
    for raw in [
        "http://2130706433/a",
        "http://0x08080808/a",
        "http://010.010.010.010/a",
        "http://8.8.8/a",
        "http://127.0.0.1/a",
        "https://example.com:8443/a",
        "https://user:secret@example.com/a",
        "https://example.com/#fragment",
        "https://example.local/a",
        "http://localhost/a",
        "https://example.com\\@127.0.0.1/a",
        "https://%65xample.com/a",
        "https:////example.com/a",
        "https://example.com\n/a",
    ] {
        assert!(parse_destination_url(raw).is_err(), "accepted {raw}");
    }
    for raw in [
        "https://example.com/a?token=opaque",
        "http://8.8.8.8/a",
        "https://[2606:4700:4700::1111]/a",
    ] {
        parse_destination_url(raw)?;
    }
    Ok(())
}

#[test]
fn proxy_endpoints_and_request_debug_redact_credentials() -> TestResult {
    let endpoint = ProxyEndpoint::new("http://user:fixture-secret@127.0.0.1:8080")?;
    assert!(!format!("{endpoint:?}").contains("fixture-secret"));
    assert!(
        !format!(
            "{:?}",
            FetchRequest::get("https://example.com/a?token=fixture-secret")
        )
        .contains("fixture-secret")
    );
    ProxyEndpoint::new("socks5://127.0.0.1:1080")?;
    assert!(ProxyEndpoint::new("socks5h://127.0.0.1:1080").is_err());
    assert!(ProxyEndpoint::new("http://127.0.0.1:8080/path").is_err());
    assert!(
        HttpClient::with_config(TransportConfig {
            max_in_flight: 0,
            ..TransportConfig::default()
        })
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn public_transport_cannot_enable_private_destination_access() -> TestResult {
    let client = HttpClient::new()?;
    let mut restricted = FetchRequest::get("https://example.com/");
    restricted.allowed_origins = Some(Vec::new());
    assert!(matches!(
        client.fetch(restricted, &CancellationToken::new()).await,
        Err(TransportError::DestinationDenied)
    ));
    assert!(matches!(
        client
            .fetch(
                FetchRequest::get("http://127.0.0.1/"),
                &CancellationToken::new()
            )
            .await,
        Err(TransportError::DestinationDenied)
    ));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        client
            .fetch(FetchRequest::get("https://example.com/"), &cancellation)
            .await,
        Err(TransportError::Cancelled)
    ));
    Ok(())
}

async fn read_headers(socket: &mut tokio::net::TcpStream) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() > 16_384 {
            return Err(std::io::Error::other("fixture header overflow"));
        }
        bytes.push(socket.read_u8().await?);
    }
    String::from_utf8(bytes).map_err(std::io::Error::other)
}

#[tokio::test]
async fn explicit_http_proxy_carries_request_without_direct_egress() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let headers = read_headers(&mut socket).await?;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nproxy")
            .await?;
        Ok::<_, std::io::Error>(headers)
    });
    let client = HttpClient::with_config(TransportConfig {
        egress: EgressMode::HttpProxy(ProxyEndpoint::new(&format!("http://{address}"))?),
        ..TransportConfig::default()
    })?;
    // The public IP is sent only to our loopback proxy; no public server is dialed.
    let response = client
        .fetch(
            FetchRequest::get("http://8.8.8.8/media"),
            &CancellationToken::new(),
        )
        .await?;
    assert_eq!(response.body.as_ref(), b"proxy");
    assert!(
        server
            .await??
            .starts_with("GET http://8.8.8.8/media HTTP/1.1")
    );
    assert!(response.metrics.first_byte.is_some());
    Ok(())
}

#[tokio::test]
async fn socks5_handshake_uses_literal_target_ip() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut greeting = [0_u8; 3];
        socket.read_exact(&mut greeting).await?;
        assert_eq!(greeting, [5, 1, 0]);
        socket.write_all(&[5, 0]).await?;
        let mut destination = [0_u8; 10];
        socket.read_exact(&mut destination).await?;
        assert_eq!(destination, [5, 1, 0, 1, 8, 8, 8, 8, 0, 80]);
        socket.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80]).await?;
        let request = read_headers(&mut socket).await?;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nsocks")
            .await?;
        Ok::<_, std::io::Error>(request)
    });
    let client = HttpClient::with_config(TransportConfig {
        egress: EgressMode::Socks5Proxy(ProxyEndpoint::new(&format!("socks5://{address}"))?),
        ..TransportConfig::default()
    })?;
    let response = client
        .fetch(
            FetchRequest::get("http://8.8.8.8/media"),
            &CancellationToken::new(),
        )
        .await?;
    assert_eq!(response.body.as_ref(), b"socks");
    assert!(server.await??.starts_with("GET /media HTTP/1.1"));
    Ok(())
}

#[tokio::test]
async fn failed_explicit_proxy_returns_error_with_no_direct_fallback() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    drop(listener);
    let client = HttpClient::with_config(TransportConfig {
        egress: EgressMode::HttpProxy(ProxyEndpoint::new(&format!("http://{address}"))?),
        connect_timeout: Duration::from_millis(200),
        ..TransportConfig::default()
    })?;
    let error = client
        .fetch(
            FetchRequest::get("http://8.8.8.8/media?token=fixture-secret"),
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(
        error,
        Err(TransportError::Request(_)) | Err(TransportError::Timeout)
    ));
    assert!(!format!("{error:?}").contains("fixture-secret"));
    Ok(())
}
