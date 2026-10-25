//! Synchronous capabilities available to pure provider implementations. Guest
//! ABI wrappers delegate these calls to the host; no ambient I/O is provided.

use std::fmt;
use thiserror::Error;

#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl fmt::Debug for Header {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Header")
            .field("name", &self.name)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub method: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    /// Zero inherits the host policy; positive values can only reduce it.
    pub max_response_bytes: u32,
    /// Zero inherits the module deadline; positive values can only shorten it.
    pub timeout_ms: u32,
}

impl fmt::Debug for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Request")
            .field("upstream", &"[REDACTED]")
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .field("max_response_bytes", &self.max_response_bytes)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    pub url: String,
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

impl fmt::Debug for Response {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Response")
            .field("status", &self.status)
            .field("upstream", &"[REDACTED]")
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum HostError {
    #[error("invalid provider host request")]
    InvalidRequest,
    #[error("provider host capability denied")]
    Denied,
    #[error("provider host budget exhausted")]
    LimitExceeded,
    #[error("provider host operation cancelled")]
    Cancelled,
    #[error("provider host operation timed out")]
    Timeout,
    #[error("provider upstream operation failed")]
    UpstreamFailure,
}

pub trait ProviderHost {
    fn fetch(&self, request: Request) -> Result<Response, HostError>;

    /// Results retain request order even when the native host completes the
    /// bounded requests concurrently. An outer error rejects the whole batch.
    fn fetch_batch(
        &self,
        requests: Vec<Request>,
    ) -> Result<Vec<Result<Response, HostError>>, HostError>;

    fn now_unix_ms(&self) -> Result<i64, HostError>;

    fn random_bytes(&self, length: u32) -> Result<Vec<u8>, HostError>;
}
