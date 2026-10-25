use super::{context::request_headers, destination, resolver::GuardedResolver};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::{Method, StatusCode, header::HeaderMap};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};
use stream_core::{packages::AllowedOrigin, providers::FetchContext};
use thiserror::Error;
use tokio::{sync::Semaphore, time::Instant};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Error)]
pub enum TransportError {
    #[error("upstream destination denied")]
    DestinationDenied,
    #[error("invalid outbound transport configuration")]
    InvalidConfig,
    #[error("invalid outbound request or credential context")]
    InvalidContext,
    #[error("outbound DNS resolution failed")]
    Dns,
    #[error("outbound request capacity exhausted")]
    Capacity,
    #[error("outbound request cancelled")]
    Cancelled,
    #[error("outbound request deadline exceeded")]
    Timeout,
    #[error("upstream redirect denied")]
    RedirectDenied,
    #[error("upstream response exceeds byte limit")]
    ResponseLimit,
    #[error("upstream response headers exceed limits")]
    HeaderLimit,
    #[error("upstream request failed")]
    Request(#[source] reqwest::Error),
}
impl fmt::Debug for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

/// Explicitly trusted proxy endpoint, which may be local. Credentials are never
/// displayed. Remote-DNS SOCKS5h is intentionally unsupported.
#[derive(Clone)]
pub struct ProxyEndpoint(Url);
impl ProxyEndpoint {
    pub fn new(raw: &str) -> Result<Self, TransportError> {
        if raw.len() > 4096
            || raw.trim() != raw
            || raw
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b'\\')
        {
            return Err(TransportError::InvalidConfig);
        }
        let url = Url::parse(raw).map_err(|_| TransportError::InvalidConfig)?;
        if !matches!(url.scheme(), "http" | "https" | "socks5")
            || url.host_str().is_none()
            || url.port_or_known_default() == Some(0)
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.path(), "" | "/")
        {
            return Err(TransportError::InvalidConfig);
        }
        Ok(Self(url))
    }
}
impl fmt::Debug for ProxyEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProxyEndpoint { [REDACTED] }")
    }
}

#[derive(Clone, Debug, Default)]
pub enum EgressMode {
    #[default]
    Direct,
    /// The configured proxy is trusted to enforce its remote DNS/destination
    /// policy. Local preflight does not prove where the proxy connects.
    HttpProxy(ProxyEndpoint),
    /// Uses the guarded local resolver and sends a literal target IP.
    Socks5Proxy(ProxyEndpoint),
}
#[derive(Clone, Debug)]
pub struct TransportConfig {
    pub egress: EgressMode,
    pub max_in_flight: usize,
    pub max_queued: usize,
    /// Bound distinct pool keys while connections may remain active or idle.
    pub max_origins: usize,
    pub max_response_bytes: usize,
    pub max_redirects: u8,
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
}
impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            egress: EgressMode::Direct,
            max_in_flight: 16,
            max_queued: 64,
            max_origins: 128,
            max_response_bytes: 4 * 1024 * 1024,
            max_redirects: 3,
            request_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(3),
            read_timeout: Duration::from_secs(6),
        }
    }
}
#[derive(Clone)]
pub struct FetchRequest {
    pub url: String,
    pub method: Method,
    pub body: Bytes,
    pub context: FetchContext,
    /// Trusted host controls only; credentials belong in the scoped context.
    pub headers: BTreeMap<String, String>,
    /// Zero inherits the transport cap; positive values may only reduce it.
    pub max_response_bytes: usize,
    /// A probe may intentionally take a prefix instead of rejecting a large body.
    pub sample: bool,
    /// None is unrestricted public egress; Some(empty) grants no origins.
    pub allowed_origins: Option<Vec<AllowedOrigin>>,
}
impl FetchRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: Method::GET,
            body: Bytes::new(),
            context: FetchContext::default(),
            headers: BTreeMap::new(),
            max_response_bytes: 0,
            sample: false,
            allowed_origins: None,
        }
    }
}
impl fmt::Debug for FetchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FetchRequest")
            .field("method", &self.method)
            .field("upstream", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FetchMetrics {
    pub admission_wait: Duration,
    /// DNS, connection/TLS, request and server headers combined; never present
    /// this as an isolated TCP-connect measurement.
    pub response_headers: Duration,
    /// First response body byte, including admission and prior redirects.
    pub first_byte: Option<Duration>,
    pub transfer: Duration,
    pub total: Duration,
    pub redirects: u8,
}
pub struct FetchResponse {
    pub url: Url,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub metrics: FetchMetrics,
}

pub type ResponseBody=std::pin::Pin<Box<dyn futures_util::Stream<Item=Result<Bytes,TransportError>>+Send+Sync>>;
/// Streaming reads retain admission/connection permits until EOF or drop.
pub struct StreamingResponse {pub url:Url,pub status:StatusCode,pub headers:HeaderMap,pub body:ResponseBody}
impl fmt::Debug for StreamingResponse {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.debug_struct("StreamingResponse").field("status",&self.status).finish_non_exhaustive()}}
impl fmt::Debug for FetchResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FetchResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}
#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    config: Arc<TransportConfig>,
    pub(super) active: Arc<Semaphore>,
    admitted: Arc<Semaphore>,
    origins: Arc<Mutex<BTreeMap<String, Instant>>>,
    proxy_host: Option<Arc<str>>,
    #[cfg(test)]
    pub(super) fixture_origins: Arc<std::collections::BTreeSet<String>>,
}
impl fmt::Debug for HttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HttpClient { [REDACTED] }")
    }
}
impl HttpClient {
    pub fn new() -> Result<Self, TransportError> {
        Self::with_config(TransportConfig::default())
    }
    pub fn max_response_bytes(&self) -> usize {
        self.config.max_response_bytes
    }
    pub fn with_config(config: TransportConfig) -> Result<Self, TransportError> {
        if !(1..=256).contains(&config.max_in_flight)
            || config.max_queued > 1024
            || !(1..=1024).contains(&config.max_origins)
            || !(1..=256 * 1024 * 1024).contains(&config.max_response_bytes)
            || config.max_redirects > 5
            || config.request_timeout.is_zero()
            || config.request_timeout > Duration::from_secs(120)
            || config.connect_timeout.is_zero()
            || config.read_timeout.is_zero()
            || config.connect_timeout > config.request_timeout
            || config.read_timeout > config.request_timeout
        {
            return Err(TransportError::InvalidConfig);
        }
        let proxy = match &config.egress {
            EgressMode::Direct => None,
            EgressMode::HttpProxy(endpoint) if matches!(endpoint.0.scheme(), "http" | "https") => {
                Some(endpoint)
            }
            EgressMode::Socks5Proxy(endpoint) if endpoint.0.scheme() == "socks5" => Some(endpoint),
            _ => return Err(TransportError::InvalidConfig),
        };
        let proxy_host = proxy
            .and_then(|endpoint| endpoint.0.host_str())
            .map(str::to_owned);
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .dns_resolver(GuardedResolver::new(proxy_host.clone()))
            .retry(reqwest::retry::never())
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Duration::from_secs(90))
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.read_timeout)
            .timeout(config.request_timeout);
        if let Some(endpoint) = proxy {
            builder = builder.proxy(
                reqwest::Proxy::all(endpoint.0.as_str())
                    .map_err(request_error)?
                    .no_proxy(None),
            );
        }
        let client = builder.build().map_err(request_error)?;
        Ok(Self {
            client,
            active: Arc::new(Semaphore::new(config.max_in_flight)),
            admitted: Arc::new(Semaphore::new(config.max_in_flight + config.max_queued)),
            proxy_host: proxy_host.map(Arc::from),
            config: Arc::new(config),
            origins: Arc::default(),
            #[cfg(test)]
            fixture_origins: Arc::default(),
        })
    }
    pub async fn fetch(
        &self,
        request: FetchRequest,
        cancellation: &CancellationToken,
    ) -> Result<FetchResponse, TransportError> {
        let start = Instant::now();
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(TransportError::Cancelled),
            result = tokio::time::timeout_at(start+self.config.request_timeout,self.execute(request,start)) => result.map_err(|_|TransportError::Timeout)?,
        }
    }

    /// Bounded GET/HEAD streaming for large signed update targets. The same
    /// proxy, DNS, origin, redirect and credential policy applies to every hop.
    pub async fn fetch_stream(&self,request:FetchRequest,cancellation:&CancellationToken)->Result<StreamingResponse,TransportError>{
        let deadline=Instant::now()+self.config.request_timeout;
        let cancel=cancellation.child_token();
        let operation=async {
            if !matches!(request.method,Method::GET|Method::HEAD)||!request.body.is_empty()||request.sample
                ||request.allowed_origins.as_ref().is_some_and(|origins|origins.len()>32){return Err(TransportError::InvalidContext);}
            let limit=if request.max_response_bytes==0{self.config.max_response_bytes}else{request.max_response_bytes};
            if limit>self.config.max_response_bytes{return Err(TransportError::InvalidContext);}
            let admitted=self.admitted.clone().try_acquire_owned().map_err(|_|TransportError::Capacity)?;
            let active=self.active.clone().acquire_owned().await.map_err(|_|TransportError::Capacity)?;
            let mut url=self.destination(&request.url)?;
            for redirects in 0..=self.config.max_redirects {
                if self.proxy_host.as_deref().is_some_and(|host|url.host_str()==Some(host))
                    ||request.allowed_origins.as_ref().is_some_and(|origins|!origins.iter().any(|origin|origin.as_str()==url.origin().ascii_serialization())) {return Err(TransportError::DestinationDenied);}
                self.admit_origin(&url)?;
                if matches!(self.config.egress,EgressMode::HttpProxy(_)) && let Some(url::Host::Domain(host))=url.host(){GuardedResolver::destination(host).await?;}
                let headers=request_headers(&request.context,&url,&request.headers)?;
                let response=self.client.request(request.method.clone(),url.clone()).headers(headers).send().await.map_err(request_error)?;
                if response.headers().iter().map(|(name,value)|name.as_str().len()+value.len()).sum::<usize>()>16384{return Err(TransportError::HeaderLimit);}
                if matches!(response.status().as_u16(),301|302|303|307|308){
                    if request.method!=Method::GET||redirects>=self.config.max_redirects||response.headers().get_all("location").iter().count()!=1{return Err(TransportError::RedirectDenied);}
                    let location=response.headers().get("location").and_then(|value|value.to_str().ok()).ok_or(TransportError::RedirectDenied)?;
                    url=self.redirect(&url,location)?;continue;
                }
                if request.method!=Method::HEAD && response.content_length().is_some_and(|bytes|bytes>limit as u64){return Err(TransportError::ResponseLimit);}
                let status=response.status();let headers=response.headers().clone();
                let stream=response.bytes_stream();
                let state=(stream,admitted,active,cancel.clone(),0usize,false);
                let body=futures_util::stream::unfold(state,move|(mut stream,admitted,active,cancel,mut bytes,done)|async move {
                    if done{return None;}
                    let next=tokio::select!{biased;
                        _=cancel.cancelled()=>Some(Err(TransportError::Cancelled)),
                        next=tokio::time::timeout_at(deadline,stream.next())=>match next {Ok(Some(value))=>Some(value.map_err(request_error)),Ok(None)=>None,Err(_)=>Some(Err(TransportError::Timeout))},
                    };
                    let mut done=false;
                    let next=match next? {
                        Ok(chunk)=>match bytes.checked_add(chunk.len()){Some(total) if total<=limit=>{bytes=total;Ok(chunk)},_=>{done=true;Err(TransportError::ResponseLimit)}},
                        Err(error)=>{done=true;Err(error)},
                    };
                    Some((next,(stream,admitted,active,cancel,bytes,done)))
                });
                return Ok(StreamingResponse{url,status,headers,body:Box::pin(body)});
            }
            Err(TransportError::RedirectDenied)
        };
        tokio::select!{biased;_=cancel.cancelled()=>Err(TransportError::Cancelled),result=tokio::time::timeout_at(deadline,operation)=>result.map_err(|_|TransportError::Timeout)?,}
    }
    async fn execute(
        &self,
        request: FetchRequest,
        start: Instant,
    ) -> Result<FetchResponse, TransportError> {
        if !matches!(request.method, Method::GET | Method::HEAD | Method::POST)
            || request.body.len() > 512 * 1024
            || (request.method != Method::POST && !request.body.is_empty())
            || request
                .allowed_origins
                .as_ref()
                .is_some_and(|origins| origins.len() > 32)
        {
            return Err(TransportError::InvalidContext);
        }
        let limit = if request.max_response_bytes == 0 {
            self.config.max_response_bytes
        } else {
            request.max_response_bytes
        };
        if limit > self.config.max_response_bytes {
            return Err(TransportError::InvalidContext);
        }
        let _admitted = self
            .admitted
            .try_acquire()
            .map_err(|_| TransportError::Capacity)?;
        let _active = self
            .active
            .acquire()
            .await
            .map_err(|_| TransportError::Capacity)?;
        let admission_wait = start.elapsed();
        let mut url = self.destination(&request.url)?;
        let mut redirects = 0_u8;
        loop {
            if self
                .proxy_host
                .as_deref()
                .is_some_and(|host| url.host_str() == Some(host))
            {
                return Err(TransportError::DestinationDenied);
            }
            if request.allowed_origins.as_ref().is_some_and(|origins| {
                !origins
                    .iter()
                    .any(|origin| origin.as_str() == url.origin().ascii_serialization())
            }) {
                return Err(TransportError::DestinationDenied);
            }
            let headers = request_headers(&request.context, &url, &request.headers)?;
            self.admit_origin(&url)?;
            if matches!(self.config.egress, EgressMode::HttpProxy(_))
                && let Some(url::Host::Domain(host)) = url.host()
            {
                GuardedResolver::destination(host).await?;
            }
            let mut outgoing = self
                .client
                .request(request.method.clone(), url.clone())
                .headers(headers);
            if !request.body.is_empty() {
                outgoing = outgoing.body(request.body.clone());
            }
            let response = outgoing.send().await.map_err(request_error)?;
            let response_headers = start.elapsed();
            if response
                .headers()
                .iter()
                .map(|(name, value)| name.as_str().len() + value.len())
                .sum::<usize>()
                > 16_384
            {
                return Err(TransportError::HeaderLimit);
            }
            if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
                if request.method != Method::GET || redirects >= self.config.max_redirects {
                    return Err(TransportError::RedirectDenied);
                }
                let locations = response.headers().get_all("location");
                if locations.iter().count() != 1 {
                    return Err(TransportError::RedirectDenied);
                }
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or(TransportError::RedirectDenied)?;
                let next = self.redirect(&url, location)?;
                drop(response);
                url = next;
                redirects += 1;
                continue;
            }
            let status = response.status();
            let headers = response.headers().clone();
            if !request.sample
                && request.method != Method::HEAD
                && response
                    .content_length()
                    .is_some_and(|bytes| bytes > limit as u64)
            {
                return Err(TransportError::ResponseLimit);
            }
            let mut stream = response.bytes_stream();
            let mut body = BytesMut::with_capacity(limit.min(16 * 1024));
            let mut first_byte = None;
            let mut transfer_start = None;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(request_error)?;
                if chunk.is_empty() {
                    continue;
                }
                if first_byte.is_none() {
                    first_byte = Some(start.elapsed());
                    transfer_start = Some(Instant::now());
                }
                let available = limit.saturating_sub(body.len());
                if chunk.len() > available && !request.sample {
                    return Err(TransportError::ResponseLimit);
                }
                let take = available.min(chunk.len());
                body.extend_from_slice(&chunk[..take]);
                if request.sample && body.len() == limit {
                    break;
                }
            }
            return Ok(FetchResponse {
                url,
                status,
                headers,
                body: body.freeze(),
                metrics: FetchMetrics {
                    admission_wait,
                    response_headers,
                    first_byte,
                    transfer: transfer_start
                        .map(|instant| instant.elapsed())
                        .unwrap_or_default(),
                    total: start.elapsed(),
                    redirects,
                },
            });
        }
    }
    fn admit_origin(&self, url: &Url) -> Result<(), TransportError> {
        let now = Instant::now();
        let mut origins = self.origins.lock().map_err(|_| TransportError::Capacity)?;
        origins.retain(|_, expiry| *expiry > now);
        let key = url.origin().ascii_serialization();
        if !origins.contains_key(&key) && origins.len() >= self.config.max_origins {
            return Err(TransportError::Capacity);
        }
        // A request lasts at most request_timeout, then its idle pool connection
        // lasts at most 90s. Retain longer to avoid expiring a live pool key.
        origins.insert(
            key,
            now + self.config.request_timeout + Duration::from_secs(95),
        );
        Ok(())
    }
    fn destination(&self, raw: &str) -> Result<Url, TransportError> {
        #[cfg(test)]
        if let Ok(url) = Url::parse(raw)
            && self
                .fixture_origins
                .contains(&url.origin().ascii_serialization())
            && url.fragment().is_none()
            && url.username().is_empty()
            && url.password().is_none()
        {
            return Ok(url);
        }
        destination::parse_destination_url(raw)
    }
    fn redirect(&self, from: &Url, raw: &str) -> Result<Url, TransportError> {
        #[cfg(test)]
        if self
            .fixture_origins
            .contains(&from.origin().ascii_serialization())
        {
            if raw
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b'\\')
            {
                return Err(TransportError::RedirectDenied);
            }
            let next = from.join(raw).map_err(|_| TransportError::RedirectDenied)?;
            return self.destination(next.as_str());
        }
        destination::redirect(from, raw)
    }
}
fn request_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Request(error.without_url())
    }
}
