use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use semver::Version;
use stream_core::providers::{AdapterResult, ErrorCategory, Lookup, ProviderId};
use stream_provider_sdk::http::{Header, HostError, ProviderHost, Request, Response};
use tokio_util::sync::CancellationToken;

use crate::{
    discovery::{
        DiscoveryCoordinator, DiscoveryError, DiscoveryLimits, DiscoveryProvider, ProviderCatalog,
    },
    network::HttpClient,
    prober::{ProbeLimits, StreamProber},
};

struct BlockingProviderHost {
    handle: tokio::runtime::Handle,
    client: reqwest::Client,
    cancel: CancellationToken,
}

async fn execute_request(
    client: &reqwest::Client,
    req: Request,
    cancel: &CancellationToken,
) -> Result<Response, HostError> {
    if cancel.is_cancelled() {
        return Err(HostError::Cancelled);
    }
    let method = match req.method.as_str() {
        "POST" => reqwest::Method::POST,
        _ => reqwest::Method::GET,
    };
    let mut builder = client.request(method, &req.url);
    if req.timeout_ms > 0 {
        builder = builder.timeout(Duration::from_millis(u64::from(req.timeout_ms)));
    }
    for h in req.headers {
        builder = builder.header(&h.name, &h.value);
    }
    if !req.body.is_empty() {
        builder = builder.body(req.body);
    }

    let send_fut = builder.send();
    let mut res = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(HostError::Cancelled),
        r = send_fut => r.map_err(|e| {
            if e.is_timeout() {
                HostError::Timeout
            } else {
                HostError::UpstreamFailure
            }
        })?,
    };

    let status = res.status().as_u16();
    let mut headers = Vec::new();
    for (k, v) in res.headers() {
        if let Ok(v_str) = v.to_str() {
            headers.push(Header {
                name: k.as_str().to_string(),
                value: v_str.to_string(),
            });
        }
    }

    // Enforce the declared cap while reading, rather than allocating the entire
    // upstream body before rejecting it. Zero inherits the native host cap.
    let limit = if req.max_response_bytes == 0 {
        4 * 1024 * 1024
    } else {
        req.max_response_bytes as usize
    };
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(HostError::Cancelled),
            chunk = res.chunk() => chunk.map_err(|error| {
                if error.is_timeout() { HostError::Timeout } else { HostError::UpstreamFailure }
            })?,
        };
        let Some(chunk) = chunk else { break };
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(HostError::LimitExceeded);
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(Response {
        url: req.url,
        status,
        headers,
        body: bytes,
    })
}

impl ProviderHost for BlockingProviderHost {
    fn fetch(&self, request: Request) -> Result<Response, HostError> {
        let client = self.client.clone();
        let cancel = self.cancel.clone();
        self.handle
            .block_on(async move { execute_request(&client, request, &cancel).await })
    }

    fn fetch_batch(
        &self,
        requests: Vec<Request>,
    ) -> Result<Vec<Result<Response, HostError>>, HostError> {
        let client = self.client.clone();
        let cancel = self.cancel.clone();
        Ok(self.handle.block_on(async move {
            let futures = requests.into_iter().map(|req| {
                let client = client.clone();
                let cancel = cancel.clone();
                async move { execute_request(&client, req, &cancel).await }
            });
            futures_util::future::join_all(futures).await
        }))
    }

    fn now_unix_ms(&self) -> Result<i64, HostError> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Cancelled)?
            .as_millis();
        i64::try_from(millis).map_err(|_| HostError::Cancelled)
    }

    fn random_bytes(&self, length: u32) -> Result<Vec<u8>, HostError> {
        let mut buf = vec![0u8; length as usize];
        getrandom::fill(&mut buf).map_err(|_| HostError::Cancelled)?;
        Ok(buf)
    }
}

struct NativeAdapterProvider {
    id: ProviderId,
    version: Version,
    client: reqwest::Client,
}

#[async_trait]
impl DiscoveryProvider for NativeAdapterProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn version(&self) -> &Version {
        &self.version
    }

    fn generation(&self) -> u64 {
        1
    }

    async fn resolve(
        &self,
        lookup: &Lookup,
        cancel: CancellationToken,
    ) -> Result<AdapterResult, ErrorCategory> {
        let handle = tokio::runtime::Handle::current();
        let client = self.client.clone();
        let provider_name = self.id.as_str().to_owned();
        let lookup_owned = lookup.clone();
        let cancel_clone = cancel.clone();

        let join = tokio::task::spawn_blocking(move || {
            let host = BlockingProviderHost {
                handle,
                client,
                cancel: cancel_clone,
            };
            stream_provider_adapters::resolve(&provider_name, &lookup_owned, &host)
                .map_err(|e| e.category())
        });

        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ErrorCategory::Cancelled),
            res = join => match res {
                Ok(inner) => inner,
                Err(_) => Err(ErrorCategory::Unavailable),
            }
        }
    }
}

#[derive(Clone)]
pub struct NativeProviders {
    providers: Vec<Arc<dyn DiscoveryProvider>>,
    client: reqwest::Client,
}

impl NativeProviders {
    pub fn new() -> Result<Self, DiscoveryError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Duration::from_secs(90))
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| DiscoveryError::Closed)?;

        let mut providers: Vec<Arc<dyn DiscoveryProvider>> = Vec::new();
        for name in stream_provider_adapters::PROVIDERS {
            let id = ProviderId::new(name).map_err(|_| DiscoveryError::InvalidLookup)?;
            providers.push(Arc::new(NativeAdapterProvider {
                id,
                version: Version::new(1, 0, 0),
                client: client.clone(),
            }));
        }

        Ok(Self { providers, client })
    }

    pub fn spawn_prewarm(&self, cancel: CancellationToken) {
        let client = self.client.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let urls = [
                    "https://core.vidzee.wtf",
                    "https://streamdata.vaplayer.ru",
                    "https://vixsrc.to",
                    "https://fsharetv.cc",
                    "https://vidlink.pro",
                    "https://vidrock.net",
                    "https://vidcore.io",
                    "https://vidfast.vc",
                    "https://api.wecollege.net",
                    "https://vidsrc.sh",
                    "https://new.vidnest.fun",
                ];
                let futures = urls.iter().map(|url| {
                    let c = client.clone();
                    async move {
                        let _ = c.head(*url).timeout(Duration::from_secs(3)).send().await;
                    }
                });
                // Warm connections without consuming the upstream media request quota.
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {},
                    _ = futures_util::future::join_all(futures) => {}
                }
            });
        }
    }
}

impl ProviderCatalog for NativeProviders {
    fn snapshot(&self) -> Result<Vec<Arc<dyn DiscoveryProvider>>, DiscoveryError> {
        Ok(self.providers.clone())
    }
}

impl DiscoveryCoordinator {
    pub fn default_live(shutdown: CancellationToken) -> Result<Self, DiscoveryError> {
        let catalog = NativeProviders::new()?;
        catalog.spawn_prewarm(shutdown);
        let transport = HttpClient::new().map_err(|_| DiscoveryError::Closed)?;
        let probe_limits = ProbeLimits {
            total_sample_bytes: 16 * 1024 * 1024,
            max_variants: 3,
            ..ProbeLimits::default()
        };
        let prober = StreamProber::new(Arc::new(transport), probe_limits)
            .map_err(|_| DiscoveryError::Closed)?;
        Self::new(Arc::new(catalog), prober, DiscoveryLimits::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn native_host_enforces_body_cap_before_waiting_for_eof()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut request = [0u8; 2048];
            if socket.read(&mut request).await? == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "missing request",
                ));
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n12345678\r\n",
                )
                .await?;
            // Keep the body open: a cap violation must finish independently of EOF.
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok::<(), std::io::Error>(())
        });
        let request = Request {
            url: format!("http://{address}/"),
            method: "GET".into(),
            headers: vec![],
            body: vec![],
            max_response_bytes: 4,
            timeout_ms: 5000,
        };
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            execute_request(&reqwest::Client::new(), request, &CancellationToken::new()),
        )
        .await;
        server.abort();
        assert_eq!(result?, Err(HostError::LimitExceeded));
        Ok(())
    }
}
