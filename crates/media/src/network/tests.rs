use super::{
    FetchRequest, HttpClient, TransportConfig, TransportError,
    context::request_headers,
    destination::{redirect, validate_answers},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use stream_core::providers::{FetchContext, FetchScope, PublicResourceContext};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use url::Url;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn context(origin: &str) -> FetchContext {
    FetchContext {
        scopes: vec![FetchScope {
            origin: origin.into(),
            path_prefix: "/api".into(),
            headers: BTreeMap::from([("Authorization".into(), "Bearer fixture-secret".into())]),
            cookies: "session=fixture-secret".into(),
            cookie_path: "/api/media".into(),
        }],
        public_resources: Some(PublicResourceContext {
            origin: "https://player.example.com".into(),
            referer: "https://player.example.com/".into(),
        }),
    }
}

#[test]
fn scoped_credentials_never_cross_origins_paths_or_ambiguous_encodings() -> TestResult {
    let context = context("https://media.example.com");
    let headers = request_headers(
        &context,
        &Url::parse("https://media.example.com/api/media/file")?,
        &BTreeMap::new(),
    )?;
    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer fixture-secret")
    );
    assert_eq!(
        headers.get("cookie").and_then(|value| value.to_str().ok()),
        Some("session=fixture-secret")
    );
    for raw in [
        "https://cdn.example.com/api/media/file",
        "https://media.example.com/apix/media",
        "https://media.example.com/api/%2fmedia",
        "https://media.example.com/api//media",
    ] {
        let headers = request_headers(&context, &Url::parse(raw)?, &BTreeMap::new())?;
        assert!(headers.get("authorization").is_none());
        assert!(headers.get("cookie").is_none());
        assert!(headers.get("origin").is_some());
    }
    let headers = request_headers(
        &context,
        &Url::parse("https://media.example.com/api/other")?,
        &BTreeMap::new(),
    )?;
    assert!(headers.get("authorization").is_some());
    assert!(headers.get("cookie").is_none());
    Ok(())
}

#[test]
fn conflicting_scopes_and_internal_credential_overrides_are_rejected() -> TestResult {
    let mut context = context("https://media.example.com");
    let mut other = context.scopes[0].clone();
    other
        .headers
        .insert("Authorization".into(), "different-secret".into());
    context.scopes.push(other);
    let target = Url::parse("https://media.example.com/api/media")?;
    assert!(matches!(
        request_headers(&context, &target, &BTreeMap::new()),
        Err(TransportError::InvalidContext)
    ));
    assert!(matches!(
        request_headers(
            &FetchContext::default(),
            &target,
            &BTreeMap::from([("Cookie".into(), "secret=value".into())])
        ),
        Err(TransportError::InvalidContext)
    ));
    Ok(())
}

#[test]
fn all_dns_answers_and_redirect_targets_must_be_approved() -> TestResult {
    validate_answers(&["8.8.8.8:0".parse()?, "1.1.1.1:0".parse()?])?;
    assert!(validate_answers(&["8.8.8.8:0".parse()?, "127.0.0.1:0".parse()?]).is_err());
    assert!(validate_answers(&[]).is_err());
    assert!(validate_answers(&vec!["8.8.8.8:0".parse()?; 65]).is_err());
    let origin = Url::parse("https://media.example.com/api/master.m3u8")?;
    assert!(redirect(&origin, "http://media.example.com/api/media").is_err());
    assert!(redirect(&origin, "//127.0.0.1/api").is_err());
    assert!(redirect(&origin, "https:////example.com/api").is_err());
    assert_eq!(
        redirect(&origin, "../segment.ts")?.as_str(),
        "https://media.example.com/segment.ts"
    );
    Ok(())
}

struct Fixture {
    url: Url,
    task: tokio::task::JoinHandle<std::io::Result<String>>,
}

async fn fixture(response: String, delay: Duration) -> Result<Fixture, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let url = Url::parse(&format!("http://{address}/api/media")).map_err(std::io::Error::other)?;
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            if request.len() > 16_384 {
                return Err(std::io::Error::other("fixture header overflow"));
            }
            request.push(socket.read_u8().await?);
        }
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        socket.write_all(response.as_bytes()).await?;
        String::from_utf8(request).map_err(std::io::Error::other)
    });
    Ok(Fixture { url, task })
}

fn fixture_client(urls: &[&Url], config: TransportConfig) -> Result<HttpClient, TransportError> {
    let mut client = HttpClient::with_config(config)?;
    client.fixture_origins = Arc::new(
        urls.iter()
            .map(|url| url.origin().ascii_serialization())
            .collect(),
    );
    Ok(client)
}

#[tokio::test]
async fn streamed_body_limits_and_prefix_sampling_are_enforced() -> TestResult {
    for (sample, chunked) in [(false, false), (false, true), (true, true)] {
        let reply = if chunked {
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nA\r\nabcdefghij\r\n0\r\n\r\n"
        } else {
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabcdefghij"
        };
        let fixture = fixture(reply.into(), Duration::ZERO).await?;
        let client = fixture_client(&[&fixture.url], TransportConfig::default())?;
        let mut request = FetchRequest::get(fixture.url.as_str());
        request.max_response_bytes = 8;
        request.sample = sample;
        let result = client.fetch(request, &CancellationToken::new()).await;
        if sample {
            assert_eq!(result?.body.as_ref(), b"abcdefgh");
        } else {
            assert!(matches!(result, Err(TransportError::ResponseLimit)));
        }
        fixture.task.await??;
    }
    Ok(())
}

#[tokio::test]
async fn redirect_reapplies_credentials_for_the_new_origin() -> TestResult {
    let second = fixture(
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".into(),
        Duration::ZERO,
    )
    .await?;
    let first = fixture(
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            second.url
        ),
        Duration::ZERO,
    )
    .await?;
    let client = fixture_client(&[&first.url, &second.url], TransportConfig::default())?;
    let mut request = FetchRequest::get(first.url.as_str());
    request.context = context(&first.url.origin().ascii_serialization());
    let result = client.fetch(request, &CancellationToken::new()).await?;
    assert_eq!(result.body.as_ref(), b"ok");
    assert_eq!(result.metrics.redirects, 1);
    let first_request = first.task.await??;
    let second_request = second.task.await??;
    assert!(first_request.contains("fixture-secret"));
    assert!(!second_request.contains("fixture-secret"));
    assert!(
        second_request
            .to_ascii_lowercase()
            .contains("origin: https://player.example.com")
    );
    Ok(())
}

#[tokio::test]
async fn deadlines_and_cancellation_cover_admission_wait() -> TestResult {
    let client = HttpClient::with_config(TransportConfig {
        max_in_flight: 1,
        max_queued: 1,
        request_timeout: Duration::from_millis(40),
        connect_timeout: Duration::from_millis(10),
        read_timeout: Duration::from_millis(20),
        ..TransportConfig::default()
    })?;
    let _held = client.active.acquire().await?;
    // Holding admission prevents any socket/DNS activity in these tests.
    assert!(matches!(
        client
            .fetch(
                FetchRequest::get("https://example.com/"),
                &CancellationToken::new()
            )
            .await,
        Err(TransportError::Timeout)
    ));
    let cancellation = CancellationToken::new();
    let canceller = cancellation.clone();
    let pending = client.fetch(FetchRequest::get("https://example.com/"), &cancellation);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        canceller.cancel();
    };
    let (result, ()) = tokio::join!(pending, cancel);
    assert!(matches!(result, Err(TransportError::Cancelled)));
    Ok(())
}

#[tokio::test]
async fn response_header_delay_is_bounded_and_does_not_claim_first_frame_timing() -> TestResult {
    let fixture = fixture(
        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".into(),
        Duration::from_secs(1),
    )
    .await?;
    let client = fixture_client(
        &[&fixture.url],
        TransportConfig {
            request_timeout: Duration::from_millis(50),
            connect_timeout: Duration::from_millis(10),
            read_timeout: Duration::from_millis(20),
            ..TransportConfig::default()
        },
    )?;
    assert!(matches!(
        client
            .fetch(
                FetchRequest::get(fixture.url.as_str()),
                &CancellationToken::new()
            )
            .await,
        Err(TransportError::Timeout)
    ));
    fixture.task.abort();
    Ok(())
}

#[tokio::test]
async fn shared_client_reuses_a_completed_origin_connection() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let url = Url::parse(&format!("http://{address}/api/media"))?;
    let server = tokio::spawn(async move {
        // Exactly one accepted socket must carry both requests.
        let (mut socket, _) = listener.accept().await?;
        for _ in 0..2 {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await?);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await?;
        }
        Ok::<_, std::io::Error>(())
    });
    let client = fixture_client(
        &[&url],
        TransportConfig {
            request_timeout: Duration::from_millis(500),
            read_timeout: Duration::from_millis(200),
            connect_timeout: Duration::from_millis(100),
            ..TransportConfig::default()
        },
    )?;
    for _ in 0..2 {
        assert_eq!(
            client
                .fetch(FetchRequest::get(url.as_str()), &CancellationToken::new())
                .await?
                .body
                .as_ref(),
            b"ok"
        );
    }
    server.await??;
    Ok(())
}

#[tokio::test]
async fn new_origin_capacity_is_bounded_before_connecting() -> TestResult {
    let fixture = fixture(
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".into(),
        Duration::ZERO,
    )
    .await?;
    let client = fixture_client(
        &[&fixture.url],
        TransportConfig {
            max_origins: 1,
            ..TransportConfig::default()
        },
    )?;
    client
        .fetch(
            FetchRequest::get(fixture.url.as_str()),
            &CancellationToken::new(),
        )
        .await?;
    // The origin lease cap rejects this before any public socket or DNS lookup.
    assert!(matches!(
        client
            .fetch(
                FetchRequest::get("http://8.8.8.8/"),
                &CancellationToken::new()
            )
            .await,
        Err(TransportError::Capacity)
    ));
    fixture.task.await??;
    Ok(())
}

#[tokio::test]
async fn custom_dial_resolver_returns_only_its_validated_ip_addresses() -> TestResult {
    use reqwest::dns::Resolve;
    let resolver = super::resolver::GuardedResolver::new(None);
    // Numeric names are handled locally by the OS, without a DNS server request.
    assert!(resolver.resolve("127.0.0.1".parse()?).await.is_err());
    let addresses: Vec<_> = resolver.resolve("8.8.8.8".parse()?).await?.collect();
    assert!(!addresses.is_empty());
    assert!(
        addresses
            .iter()
            .all(|address| address.ip().to_string() == "8.8.8.8")
    );
    Ok(())
}
