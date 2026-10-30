//! mello-jellyfin library: HTTP app, auth, configuration, library sync and
//! the TMDB client. `main.rs` wires these together.

pub mod app;
pub mod auth;
pub mod config;
pub mod library;
pub mod tmdb;

use std::time::Duration;

use stream_media::network::{EgressMode, TransportConfig};

/// Transport limits for the stream relay. Bodies are unbounded (films), so
/// only response headers share the request deadline; reads time out when idle.
pub fn relay_transport(egress: EgressMode) -> TransportConfig {
    TransportConfig {
        egress,
        max_in_flight: 128,
        max_queued: 512,
        max_origins: 512,
        max_response_bytes: 64 * 1024 * 1024,
        max_redirects: 5,
        request_timeout: Duration::from_secs(30),
        connect_timeout: Duration::from_secs(6),
        read_timeout: Duration::from_secs(30),
    }
}
