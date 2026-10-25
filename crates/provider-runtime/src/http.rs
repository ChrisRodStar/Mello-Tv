//! Capability-checked host HTTP interface. Implementations must enforce the
//! supplied origin policy at every redirect and connection, including DNS/IP
//! checks. Post-response validation cannot undo leaked redirect credentials.

use std::{
    collections::HashSet,
    fmt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use futures_util::{StreamExt, stream::FuturesUnordered};
use http::{HeaderName, HeaderValue, Method};
use stream_core::packages::AllowedOrigin;
use thiserror::Error;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use url::Url;
use wasmtime::StoreLimits;

use crate::bindings::stream_tv::provider::http as wit;

const MAX_BATCH_REQUESTS: usize = 16;
const MAX_RESPONSE_METADATA_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_METADATA_PER_REQUEST: usize = 8192 + 16 * 1024;
const MAX_CLOCK_CALLS: u16 = 128;
const MAX_RANDOM_CALLS: u16 = 64;
const MAX_RANDOM_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &"[REDACTED]")
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[derive(Clone)]
pub struct HttpResponse {
    pub url: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("url", &"[REDACTED]")
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[derive(Clone)]
pub struct HttpPolicy {
    pub allowed_origins: Arc<[AllowedOrigin]>,
    /// Trusted ingress partition, never supplied by guest JSON.
    pub context_key: Arc<str>,
    pub deadline: Instant,
    /// Bound bytes while reading/decompressing, before allocating the full body.
    pub max_response_bytes: usize,
}

impl fmt::Debug for HttpPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpPolicy")
            .field("origins", &self.allowed_origins.len())
            .field("context_key", &"[REDACTED]")
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HttpError {
    #[error("invalid provider HTTP request")]
    InvalidRequest,
    #[error("provider HTTP capability denied")]
    Denied,
    #[error("provider HTTP budget exhausted")]
    LimitExceeded,
    #[error("provider HTTP request cancelled")]
    Cancelled,
    #[error("provider HTTP request timed out")]
    Timeout,
    #[error("provider HTTP upstream request failed")]
    UpstreamFailure,
}

#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// Do not detach request tasks: dropping this future must cancel owned I/O.
    /// Apply policy on every redirect; never fall back from an explicit proxy.
    async fn fetch(
        &self,
        request: HttpRequest,
        policy: HttpPolicy,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError>;
}

pub(crate) struct HostState {
    pub limits: StoreLimits,
    pub transport: Arc<dyn HttpTransport>,
    pub policy: HttpPolicy,
    pub cancellation: CancellationToken,
    pub max_requests: u16,
    pub requests: u16,
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub response_metadata_bytes: usize,
    pub max_request_bytes: usize,
    pub clock_calls: u16,
    pub random_calls: u16,
    pub random_bytes: usize,
}

impl wit::Host for HostState {
    async fn fetch(&mut self, input: wit::Request) -> Result<wit::Response, wit::Failure> {
        self.fetch_checked(input).await.map_err(wit::Failure::from)
    }

    async fn fetch_batch(
        &mut self,
        inputs: Vec<wit::Request>,
    ) -> Result<Vec<Result<wit::Response, wit::Failure>>, wit::Failure> {
        self.fetch_batch_checked(inputs)
            .await
            .map_err(wit::Failure::from)
    }

    async fn now_unix_ms(&mut self) -> Result<i64, wit::Failure> {
        self.now_unix_ms_checked().map_err(wit::Failure::from)
    }

    async fn random_bytes(&mut self, length: u32) -> Result<Vec<u8>, wit::Failure> {
        self.random_bytes_checked(length)
            .map_err(wit::Failure::from)
    }
}

impl HostState {
    fn check_active(&self) -> Result<(), HttpError> {
        if self.cancellation.is_cancelled() {
            return Err(HttpError::Cancelled);
        }
        if Instant::now() >= self.policy.deadline {
            return Err(HttpError::Timeout);
        }
        Ok(())
    }

    fn reserve_request_count(&mut self, count: usize) -> Result<(), HttpError> {
        let count = u16::try_from(count).map_err(|_| HttpError::LimitExceeded)?;
        self.requests = self
            .requests
            .checked_add(count)
            .ok_or(HttpError::LimitExceeded)?;
        if self.requests > self.max_requests {
            return Err(HttpError::LimitExceeded);
        }
        Ok(())
    }

    fn prepare_request(
        &self,
        input: wit::Request,
    ) -> Result<(HttpRequest, usize, usize, u32), HttpError> {
        validate_url(&input.url, &self.policy.allowed_origins)?;
        let method =
            Method::from_bytes(input.method.as_bytes()).map_err(|_| HttpError::InvalidRequest)?;
        if method != Method::GET && method != Method::POST && method != Method::HEAD {
            return Err(HttpError::Denied);
        }
        if method != Method::POST && !input.body.is_empty() {
            return Err(HttpError::InvalidRequest);
        }
        let headers: Vec<_> = input
            .headers
            .into_iter()
            .map(|header| (header.name, header.value))
            .collect();
        let header_bytes = validate_headers(&headers, true)?;
        let bytes = input
            .url
            .len()
            .checked_add(input.method.len())
            .and_then(|size| size.checked_add(header_bytes))
            .and_then(|size| size.checked_add(input.body.len()))
            .ok_or(HttpError::LimitExceeded)?;
        if bytes > self.max_request_bytes {
            return Err(HttpError::LimitExceeded);
        }
        Ok((
            HttpRequest {
                url: input.url,
                method: method.as_str().to_owned(),
                headers,
                body: input.body,
            },
            input.max_response_bytes as usize,
            bytes,
            input.timeout_ms,
        ))
    }

    fn charge_request_bytes(&mut self, bytes: usize) -> Result<(), HttpError> {
        self.request_bytes = self
            .request_bytes
            .checked_add(bytes)
            .ok_or(HttpError::LimitExceeded)?;
        if self.request_bytes > self.max_request_bytes {
            return Err(HttpError::LimitExceeded);
        }
        Ok(())
    }

    fn remaining_response_bytes(&self) -> Result<usize, HttpError> {
        let remaining = self
            .policy
            .max_response_bytes
            .saturating_sub(self.response_bytes);
        if remaining == 0 || self.response_metadata_bytes >= MAX_RESPONSE_METADATA_BYTES {
            return Err(HttpError::LimitExceeded);
        }
        Ok(remaining)
    }

    fn reserve_response_metadata(&self, count: usize) -> Result<(), HttpError> {
        let reservation = count
            .checked_mul(MAX_RESPONSE_METADATA_PER_REQUEST)
            .ok_or(HttpError::LimitExceeded)?;
        if reservation > MAX_RESPONSE_METADATA_BYTES.saturating_sub(self.response_metadata_bytes) {
            return Err(HttpError::LimitExceeded);
        }
        Ok(())
    }

    async fn fetch_checked(&mut self, input: wit::Request) -> Result<wit::Response, HttpError> {
        self.check_active()?;
        self.reserve_request_count(1)?;
        let (request, requested, bytes, timeout_ms) = self.prepare_request(input)?;
        self.charge_request_bytes(bytes)?;
        let remaining = self.remaining_response_bytes()?;
        self.reserve_response_metadata(1)?;
        let allocation = if requested == 0 {
            remaining
        } else {
            remaining.min(requested)
        };
        let req_deadline = if timeout_ms > 0 {
            Instant::now() + Duration::from_millis(timeout_ms as u64)
        } else {
            self.policy.deadline
        };
        let policy = HttpPolicy {
            max_response_bytes: allocation,
            deadline: self.policy.deadline.min(req_deadline),
            ..self.policy.clone()
        };
        let result = perform_request(
            self.transport.clone(),
            request,
            policy,
            self.cancellation.child_token(),
        )
        .await;
        self.finish_response(result, allocation)
    }

    async fn fetch_batch_checked(
        &mut self,
        inputs: Vec<wit::Request>,
    ) -> Result<Vec<Result<wit::Response, wit::Failure>>, HttpError> {
        self.check_active()?;
        if inputs.len() > MAX_BATCH_REQUESTS {
            return Err(HttpError::LimitExceeded);
        }
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        self.reserve_request_count(inputs.len())?;
        let mut requests = Vec::with_capacity(inputs.len());
        let mut caps = Vec::with_capacity(inputs.len());
        let mut request_bytes = 0usize;
        // Reject the entire batch before any I/O if its shape or requested
        // capabilities are invalid. Transport failures remain per-request.
        for input in inputs {
            let (request, cap, bytes, timeout_ms) = self.prepare_request(input)?;
            request_bytes = request_bytes
                .checked_add(bytes)
                .ok_or(HttpError::LimitExceeded)?;
            requests.push((request, timeout_ms));
            caps.push(cap);
        }
        self.charge_request_bytes(request_bytes)?;
        let allocations = fair_allocations(&caps, self.remaining_response_bytes()?)?;
        let count = requests.len();
        self.reserve_response_metadata(count)?;
        let mut pending = FuturesUnordered::new();
        for (index, ((request, timeout_ms), allocation)) in requests.into_iter().zip(allocations).enumerate() {
            let transport = self.transport.clone();
            let req_deadline = if timeout_ms > 0 {
                Instant::now() + Duration::from_millis(timeout_ms as u64)
            } else {
                self.policy.deadline
            };
            let policy = HttpPolicy {
                max_response_bytes: allocation,
                deadline: self.policy.deadline.min(req_deadline),
                ..self.policy.clone()
            };
            let cancellation = self.cancellation.child_token();
            pending.push(async move {
                let result = perform_request(transport, request, policy, cancellation).await;
                (index, allocation, result)
            });
        }
        let mut results: Vec<Option<Result<wit::Response, wit::Failure>>> =
            std::iter::repeat_with(|| None).take(count).collect();
        // Each future owns a fixed response reservation. Total body capacity
        // cannot exceed the remaining manifest budget while requests overlap.
        while !pending.is_empty() {
            let completed = tokio::select! {
                biased;
                _=self.cancellation.cancelled()=>return Err(HttpError::Cancelled),
                result=timeout_at(self.policy.deadline,pending.next())=>result.map_err(|_|HttpError::Timeout)?,
            };
            if let Some((index, allocation, response)) = completed {
                let response = self
                    .finish_response(response, allocation)
                    .map_err(wit::Failure::from);
                if self.response_metadata_bytes > MAX_RESPONSE_METADATA_BYTES {
                    return Err(HttpError::LimitExceeded);
                }
                let output = results.get_mut(index).ok_or(HttpError::UpstreamFailure)?;
                *output = Some(response);
            }
        }
        self.check_active()?;
        results
            .into_iter()
            .map(|result| result.ok_or(HttpError::UpstreamFailure))
            .collect()
    }

    fn finish_response(
        &mut self,
        result: Result<HttpResponse, HttpError>,
        allocation: usize,
    ) -> Result<wit::Response, HttpError> {
        // Failed transports do not expose their partial download count. Charge
        // their whole reservation conservatively instead of allowing retries to
        // exceed the aggregate downloaded-body quota.
        let spent = match &result {
            Ok(response) => response.body.len().min(allocation),
            Err(_) => allocation,
        };
        self.response_bytes = self
            .response_bytes
            .checked_add(spent)
            .ok_or(HttpError::LimitExceeded)?;
        if self.response_bytes > self.policy.max_response_bytes {
            return Err(HttpError::LimitExceeded);
        }
        let metadata = match &result {
            Ok(response) => response
                .headers
                .iter()
                .try_fold(response.url.len(), |bytes, (name, value)| {
                    bytes
                        .checked_add(name.len())
                        .and_then(|bytes| bytes.checked_add(value.len()))
                })
                .unwrap_or(MAX_RESPONSE_METADATA_PER_REQUEST)
                .min(MAX_RESPONSE_METADATA_PER_REQUEST),
            Err(_) => MAX_RESPONSE_METADATA_PER_REQUEST,
        };
        self.response_metadata_bytes = self
            .response_metadata_bytes
            .checked_add(metadata)
            .ok_or(HttpError::LimitExceeded)?;
        if self.response_metadata_bytes > MAX_RESPONSE_METADATA_BYTES {
            return Err(HttpError::LimitExceeded);
        }
        let response = result?;
        if response.body.len() > allocation {
            return Err(HttpError::LimitExceeded);
        }
        validate_url(&response.url, &self.policy.allowed_origins)?;
        if !(200..=599).contains(&response.status) {
            return Err(HttpError::UpstreamFailure);
        }
        validate_headers(&response.headers, false)?;
        self.check_active()?;
        Ok(wit::Response {
            url: response.url,
            status: response.status,
            headers: response
                .headers
                .into_iter()
                .map(|(name, value)| wit::Header { name, value })
                .collect(),
            body: response.body,
        })
    }

    fn now_unix_ms_checked(&mut self) -> Result<i64, HttpError> {
        self.check_active()?;
        self.clock_calls = self
            .clock_calls
            .checked_add(1)
            .ok_or(HttpError::LimitExceeded)?;
        if self.clock_calls > MAX_CLOCK_CALLS {
            return Err(HttpError::LimitExceeded);
        }
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HttpError::UpstreamFailure)?;
        i64::try_from(elapsed.as_millis()).map_err(|_| HttpError::UpstreamFailure)
    }

    fn random_bytes_checked(&mut self, length: u32) -> Result<Vec<u8>, HttpError> {
        self.check_active()?;
        self.random_calls = self
            .random_calls
            .checked_add(1)
            .ok_or(HttpError::LimitExceeded)?;
        if self.random_calls > MAX_RANDOM_CALLS {
            return Err(HttpError::LimitExceeded);
        }
        let length = usize::try_from(length).map_err(|_| HttpError::LimitExceeded)?;
        self.random_bytes = self
            .random_bytes
            .checked_add(length)
            .ok_or(HttpError::LimitExceeded)?;
        if self.random_bytes > MAX_RANDOM_BYTES {
            return Err(HttpError::LimitExceeded);
        }
        let mut bytes = vec![0u8; length];
        getrandom::fill(&mut bytes).map_err(|_| HttpError::UpstreamFailure)?;
        Ok(bytes)
    }
}

/// Equal shares among unconstrained requests, after satisfying smaller explicit
/// caps. Input order does not let a large player download starve a tiny seed.
fn fair_allocations(requested: &[usize], budget: usize) -> Result<Vec<usize>, HttpError> {
    let mut order: Vec<_> = (0..requested.len()).collect();
    order.sort_by_key(|&index| {
        if requested[index] == 0 {
            usize::MAX
        } else {
            requested[index]
        }
    });
    let mut remaining = budget;
    let mut slots = requested.len();
    let mut allocations = vec![0; requested.len()];
    for index in order {
        let fair = remaining
            .checked_div(slots)
            .ok_or(HttpError::LimitExceeded)?;
        let allocation = if requested[index] == 0 {
            fair
        } else {
            fair.min(requested[index])
        };
        if allocation == 0 {
            return Err(HttpError::LimitExceeded);
        }
        allocations[index] = allocation;
        remaining -= allocation;
        slots -= 1;
    }
    Ok(allocations)
}

async fn perform_request(
    transport: Arc<dyn HttpTransport>,
    request: HttpRequest,
    policy: HttpPolicy,
    cancellation: CancellationToken,
) -> Result<HttpResponse, HttpError> {
    let _cancel_on_drop = cancellation.clone().drop_guard();
    tokio::select! {
        biased;
        _=cancellation.cancelled()=>Err(HttpError::Cancelled),
        result=timeout_at(policy.deadline,transport.fetch(request,policy.clone(),cancellation.clone()))=>result.map_err(|_|HttpError::Timeout)?,
    }
}

fn validate_url(value: &str, origins: &[AllowedOrigin]) -> Result<(), HttpError> {
    if value.is_empty()
        || value.len() > 8192
        || value.trim() != value
        || value.contains('\\')
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(HttpError::InvalidRequest);
    }
    let url = Url::parse(value).map_err(|_| HttpError::InvalidRequest)?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(HttpError::InvalidRequest);
    }
    let origin = url.origin().ascii_serialization();
    if !origins.iter().any(|allowed| allowed.as_str() == origin) {
        return Err(HttpError::Denied);
    }
    Ok(())
}

fn validate_headers(headers: &[(String, String)], request: bool) -> Result<usize, HttpError> {
    if headers.len() > 64 {
        return Err(HttpError::LimitExceeded);
    }
    let mut total = 0usize;
    let mut seen = HashSet::new();
    for (name, value) in headers {
        let parsed =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| HttpError::InvalidRequest)?;
        if request && !seen.insert(parsed.clone()) {
            return Err(HttpError::InvalidRequest);
        }
        HeaderValue::from_str(value).map_err(|_| HttpError::InvalidRequest)?;
        if value
            .bytes()
            .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n')
        {
            return Err(HttpError::InvalidRequest);
        }
        if request
            && matches!(
                parsed.as_str(),
                "host"
                    | "connection"
                    | "content-length"
                    | "transfer-encoding"
                    | "te"
                    | "trailer"
                    | "upgrade"
                    | "proxy-authorization"
                    | "proxy-connection"
            )
        {
            return Err(HttpError::Denied);
        }
        total = total
            .checked_add(name.len())
            .and_then(|size| size.checked_add(value.len()))
            .ok_or(HttpError::LimitExceeded)?;
        if total > 16 * 1024 {
            return Err(HttpError::LimitExceeded);
        }
    }
    Ok(total)
}

impl From<HttpError> for wit::Failure {
    fn from(value: HttpError) -> Self {
        match value {
            HttpError::InvalidRequest => Self::InvalidRequest,
            HttpError::Denied => Self::Denied,
            HttpError::LimitExceeded => Self::LimitExceeded,
            HttpError::Cancelled => Self::Cancelled,
            HttpError::Timeout => Self::Timeout,
            HttpError::UpstreamFailure => Self::UpstreamFailure,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wasmtime::StoreLimitsBuilder;

    struct FixedResponse {
        url: String,
        body: Vec<u8>,
    }
    #[async_trait]
    impl HttpTransport for FixedResponse {
        async fn fetch(
            &self,
            _: HttpRequest,
            _: HttpPolicy,
            _: CancellationToken,
        ) -> Result<HttpResponse, HttpError> {
            Ok(HttpResponse {
                url: self.url.clone(),
                status: 200,
                headers: Vec::new(),
                body: self.body.clone(),
            })
        }
    }

    fn state(url: &str, body: Vec<u8>) -> Result<HostState, Box<dyn std::error::Error>> {
        Ok(HostState {
            limits: StoreLimitsBuilder::new().build(),
            transport: Arc::new(FixedResponse {
                url: url.into(),
                body,
            }),
            policy: HttpPolicy {
                allowed_origins: vec![AllowedOrigin::new("https://fixture.example")?].into(),
                context_key: "secret-context".into(),
                deadline: Instant::now() + Duration::from_secs(1),
                max_response_bytes: 100,
            },
            cancellation: CancellationToken::new(),
            max_requests: 4,
            requests: 0,
            request_bytes: 0,
            response_bytes: 0,
            response_metadata_bytes: 0,
            max_request_bytes: 100,
            clock_calls: 0,
            random_calls: 0,
            random_bytes: 0,
        })
    }

    fn request() -> wit::Request {
        wit::Request {
            url: "https://fixture.example/resolve".into(),
            method: "GET".into(),
            headers: Vec::new(),
            body: Vec::new(),
            max_response_bytes: 0,
            timeout_ms: 0,
        }
    }

    #[test]
    fn request_headers_reject_smuggling_and_ambiguous_duplicates() {
        for name in [
            "host",
            "Content-Length",
            "transfer-encoding",
            "Proxy-Authorization",
            "Connection",
        ] {
            assert!(matches!(
                validate_headers(&[(name.into(), "value".into())], true),
                Err(HttpError::Denied)
            ));
        }
        assert!(
            validate_headers(
                &[
                    ("Referer".into(), "one".into()),
                    ("referer".into(), "two".into())
                ],
                true
            )
            .is_err()
        );
        assert!(
            validate_headers(
                &[(
                    "Authorization".into(),
                    "Bearer secret\r\nInjected: true".into()
                )],
                true
            )
            .is_err()
        );
        assert!(
            validate_headers(
                &[
                    ("Set-Cookie".into(), "one".into()),
                    ("Set-Cookie".into(), "two".into())
                ],
                false
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn host_rechecks_final_origin_and_cumulative_response_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut host = state("https://unapproved.example/", vec![1])?;
        assert!(matches!(
            host.fetch_checked(request()).await,
            Err(HttpError::Denied)
        ));
        let mut host = state("https://fixture.example/", vec![1; 101])?;
        assert!(matches!(
            host.fetch_checked(request()).await,
            Err(HttpError::LimitExceeded)
        ));
        let mut host = state("https://fixture.example/", vec![1; 40])?;
        host.fetch_checked(request()).await?;
        host.fetch_checked(request()).await?;
        assert!(matches!(
            host.fetch_checked(request()).await,
            Err(HttpError::LimitExceeded)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn host_request_bytes_and_sensitive_debug_are_bounded()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut host = state("https://fixture.example/", vec![])?;
        let mut input = request();
        input.method = "POST".into();
        input.body = vec![1; 101];
        assert!(matches!(
            host.fetch_checked(input).await,
            Err(HttpError::LimitExceeded)
        ));
        let request = HttpRequest {
            url: "https://secret.example/token".into(),
            method: "GET".into(),
            headers: vec![("Cookie".into(), "secret-cookie".into())],
            body: b"secret-body".to_vec(),
        };
        let debug = format!("{request:?} {:?}", host.policy);
        for secret in [
            "secret.example",
            "secret-cookie",
            "secret-body",
            "secret-context",
        ] {
            assert!(!debug.contains(secret));
        }
        Ok(())
    }

    #[test]
    fn malformed_urls_cannot_expand_capabilities() -> Result<(), Box<dyn std::error::Error>> {
        let origins = vec![AllowedOrigin::new("https://fixture.example")?];
        for url in [
            "https://fixture.example@evil.example/",
            "https://fixture.example/#secret",
            "file:///etc/passwd",
            "https://fixture.example\\@evil.example/",
            " https://fixture.example/",
            "https://fixture.example:444/",
        ] {
            assert!(validate_url(url, &origins).is_err());
        }
        validate_url("https://fixture.example:443/resolve", &origins)?;
        Ok(())
    }
}
