pub mod vidcore;
pub mod vidfast;
pub mod vidlink;
pub mod vidsrc;

pub use vidcore::VidCoreResolver;
pub use vidfast::VidFastResolver;
pub use vidlink::VidLinkResolver;
pub use vidsrc::VidSrcResolver;

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use semver::Version;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, DeviceCapabilities, ErrorCategory, Lookup, ProviderId, ProviderTitle,
        QualityPreference, StreamFormat, TmdbId,
    },
};
use stream_provider_sdk::http::{Header, HostError, ProviderHost, Request, Response};
use tokio_util::sync::CancellationToken;

use crate::{
    discovery::{DiscoveryCoordinator, DiscoveryError, DiscoveryLimits, DiscoveryProvider, ProviderCatalog},
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
    let res = tokio::select! {
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

    let bytes_fut = res.bytes();
    let bytes = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(HostError::Cancelled),
        b = bytes_fut => b.map_err(|_| HostError::UpstreamFailure)?,
    };

    if req.max_response_bytes > 0 && bytes.len() > req.max_response_bytes as usize {
        return Err(HostError::LimitExceeded);
    }

    Ok(Response {
        url: req.url,
        status,
        headers,
        body: bytes.to_vec(),
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

    pub fn spawn_prewarm(&self) {
        let client = self.client.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let handle_clone = handle.clone();
            handle.spawn(async move {
                let urls = [
                    "https://api.wecollege.net",
                    "https://vidlink.pro",
                    "https://vidcore.io",
                    "https://vidsrc.sh",
                    "https://vidfast.vc",
                ];
                let futures = urls.iter().map(|url| {
                    let c = client.clone();
                    async move {
                        let _ = c.head(*url).timeout(Duration::from_secs(3)).send().await;
                    }
                });
                futures_util::future::join_all(futures).await;

                if let Ok(tmdb_id) = TmdbId::try_from(27205) {
                    let warmup_lookup = Lookup {
                        title: ProviderTitle {
                            media_type: MediaType::Movie,
                            tmdb_id,
                            imdb_id: "tt1375666".into(),
                            my_anime_list_id: None,
                            season: None,
                            episode: None,
                        },
                        preferences: QualityPreference::default(),
                        device: DeviceCapabilities {
                            formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
                            codecs: vec!["h264".into(), "aac".into()],
                            max_height: 1080,
                            allow_unknown_codecs: true,
                            allow_unknown_resolution: true,
                            supports_encrypted_hls: true,
                        },
                        context_key: String::new(),
                        startup_height: 1080,
                        wait_for_complete: false,
                        refresh: false,
                    };
                    let _ = tokio::task::spawn_blocking(move || {
                        let host = BlockingProviderHost {
                            handle: handle_clone,
                            client,
                            cancel: CancellationToken::new(),
                        };
                        let _ = stream_provider_adapters::resolve("vidcore", &warmup_lookup, &host);
                    })
                    .await;
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
    pub fn default_live() -> Result<Self, DiscoveryError> {
        let catalog = NativeProviders::new()?;
        catalog.spawn_prewarm();
        let transport = HttpClient::new().map_err(|_| DiscoveryError::Closed)?;
        let probe_limits = ProbeLimits {
            source_timeout: Duration::from_secs(4),
            max_variants: 3,
            ..ProbeLimits::default()
        };
        let prober = StreamProber::new(Arc::new(transport), probe_limits)
            .map_err(|_| DiscoveryError::Closed)?;
        Self::new(Arc::new(catalog), prober, DiscoveryLimits::default())
    }
}
