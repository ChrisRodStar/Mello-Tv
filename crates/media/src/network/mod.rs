//! Portable, bounded outbound HTTP for provider discovery and media probes.
//! Ambient proxies are disabled. HTTP proxies are trusted to resolve their
//! remote destinations; local validation cannot attest the proxy's remote DNS.
mod client;
mod context;
mod destination;
mod resolver;

pub use client::{
    BROWSER_USER_AGENT, EgressMode, FetchMetrics, FetchRequest, FetchResponse, HttpClient,
    ProxyEndpoint, ResponseBody, StreamingResponse, TransportConfig, TransportError,
};
pub use destination::{parse_destination_url, public_ip, validate_destination_url};
pub use reqwest::{Method, StatusCode};

#[cfg(test)]
mod tests;
