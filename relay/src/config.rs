use crate::error::{CONFIG, Error};
use serde::Deserialize;
use std::net::SocketAddr;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub http: Http,
    pub auth: Auth,
    pub state: State,
    pub network: Network,
    pub limits: Limits,
    pub lifecycle: Lifecycle,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    #[serde(default)]
    pub trusted_listen_ip: Option<std::net::Ipv4Addr>,
    pub control_listen: SocketAddr,
    pub stream_listen: SocketAddr,
    pub public_prefix: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    pub control_bearer_file: String,
    pub api_verification_keys_file: String,
    pub issuer: String,
    pub audience: String,
    pub algorithm: String,
    pub clock_skew_s: u64,
    pub max_grant_bytes: u64,
    pub max_grant_lifetime_s: u64,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    #[serde(default = "default_database_bytes")]
    pub max_database_bytes: u64,
    #[serde(default = "default_free_space_bytes")]
    pub min_free_space_bytes: u64,
    #[serde(default = "default_idempotency_count")]
    pub max_idempotency_count: u64,
    pub database_path: String,
    pub encryption_key_file: String,
    pub max_sessions_count: u64,
    pub max_sources_per_session_count: u64,
    pub max_resources_per_session_count: u64,
    pub max_resources_total_count: u64,
    pub hot_index_bytes: u64,
    pub max_snapshot_bytes: u64,
    pub max_session_lifetime_s: u64,
    pub old_generation_grace_s: u64,
    pub cleanup_batch_count: u64,
    pub cleanup_period_s: u64,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub guard_mode: String,
    pub dns_endpoint: String,
    pub dns_endpoint_ip: String,
    pub dns_timeout_s: u64,
    pub connect_timeout_s: u64,
    pub tls_timeout_s: u64,
    pub response_header_timeout_s: u64,
    pub read_idle_timeout_s: u64,
    pub write_idle_timeout_s: u64,
    pub playlist_timeout_s: u64,
    pub segment_timeout_s: u64,
    pub mp4_max_transfer_s: u64,
    pub max_redirects_count: u64,
    pub max_streams_count: u64,
    pub max_streams_per_session_count: u64,
    pub max_streams_per_authority_count: u64,
    pub admission_wait_s: u64,
    pub body_chunk_bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default = "default_manifest_registrations")]
    pub max_manifest_registrations_count: u64,
    pub max_control_body_bytes: u64,
    pub max_control_concurrency_count: u64,
    pub max_store_queue_count: u64,
    pub max_playlist_bytes: u64,
    pub max_playlist_depth_count: u64,
    pub max_key_bytes: u64,
    pub max_sniff_bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    pub drain_timeout_s: u64,
}
fn default_manifest_registrations() -> u64 {
    4096
}
impl Config {
    pub fn parse(input: &str) -> Result<Self, Error> {
        let config: Self = toml::from_str(input).map_err(|_| CONFIG)?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.limits.max_manifest_registrations_count == 0
            || self.limits.max_manifest_registrations_count > 20000
        {
            return Err(CONFIG);
        }
        if !(1_048_576..=68_719_476_736).contains(&self.state.max_database_bytes)
            || self.state.min_free_space_bytes == 0
            || self.state.min_free_space_bytes > 68_719_476_736
            || self.state.max_idempotency_count == 0
            || self.state.max_idempotency_count > 65536
        {
            return Err(CONFIG);
        }
        if self.auth.clock_skew_s == 0 || self.auth.clock_skew_s > 30 {
            return Err(CONFIG);
        }
        if self.auth.max_grant_bytes == 0 || self.auth.max_grant_bytes > 4096 {
            return Err(CONFIG);
        }
        if self.auth.max_grant_lifetime_s == 0 || self.auth.max_grant_lifetime_s > 345600 {
            return Err(CONFIG);
        }
        if self.state.max_sessions_count == 0 || self.state.max_sessions_count > 2048 {
            return Err(CONFIG);
        }
        if self.state.max_sources_per_session_count == 0
            || self.state.max_sources_per_session_count > 2048
        {
            return Err(CONFIG);
        }
        if self.state.max_resources_per_session_count == 0
            || self.state.max_resources_per_session_count > 320000
        {
            return Err(CONFIG);
        }
        if self.state.max_resources_total_count == 0
            || self.state.max_resources_total_count > 1600000
        {
            return Err(CONFIG);
        }
        if self.state.hot_index_bytes == 0 || self.state.hot_index_bytes > 268435456 {
            return Err(CONFIG);
        }
        if self.state.max_snapshot_bytes == 0 || self.state.max_snapshot_bytes > 33554432 {
            return Err(CONFIG);
        }
        if self.state.max_session_lifetime_s == 0 || self.state.max_session_lifetime_s > 345600 {
            return Err(CONFIG);
        }
        if self.state.old_generation_grace_s == 0 || self.state.old_generation_grace_s > 1920 {
            return Err(CONFIG);
        }
        if self.state.cleanup_batch_count == 0 || self.state.cleanup_batch_count > 2048 {
            return Err(CONFIG);
        }
        if self.state.cleanup_period_s == 0 || self.state.cleanup_period_s > 480 {
            return Err(CONFIG);
        }
        if self.network.dns_timeout_s == 0 || self.network.dns_timeout_s > 256 {
            return Err(CONFIG);
        }
        if self.network.connect_timeout_s == 0 || self.network.connect_timeout_s > 256 {
            return Err(CONFIG);
        }
        if self.network.tls_timeout_s == 0 || self.network.tls_timeout_s > 256 {
            return Err(CONFIG);
        }
        if self.network.response_header_timeout_s == 0
            || self.network.response_header_timeout_s > 256
        {
            return Err(CONFIG);
        }
        if self.network.read_idle_timeout_s == 0 || self.network.read_idle_timeout_s > 320 {
            return Err(CONFIG);
        }
        if self.network.write_idle_timeout_s == 0 || self.network.write_idle_timeout_s > 480 {
            return Err(CONFIG);
        }
        if self.network.playlist_timeout_s == 0 || self.network.playlist_timeout_s > 320 {
            return Err(CONFIG);
        }
        if self.network.segment_timeout_s == 0 || self.network.segment_timeout_s > 1920 {
            return Err(CONFIG);
        }
        if self.network.mp4_max_transfer_s == 0 || self.network.mp4_max_transfer_s > 345600 {
            return Err(CONFIG);
        }
        if self.network.max_redirects_count == 0 || self.network.max_redirects_count > 256 {
            return Err(CONFIG);
        }
        if self.network.max_streams_count == 0 || self.network.max_streams_count > 512 {
            return Err(CONFIG);
        }
        if self.network.max_streams_per_session_count == 0
            || self.network.max_streams_per_session_count > 256
        {
            return Err(CONFIG);
        }
        if self.network.max_streams_per_authority_count == 0
            || self.network.max_streams_per_authority_count > 256
        {
            return Err(CONFIG);
        }
        if self.network.admission_wait_s == 0 || self.network.admission_wait_s > 256 {
            return Err(CONFIG);
        }
        if self.network.body_chunk_bytes == 0 || self.network.body_chunk_bytes > 1048576 {
            return Err(CONFIG);
        }
        if self.limits.max_control_body_bytes == 0 || self.limits.max_control_body_bytes > 67108864
        {
            return Err(CONFIG);
        }
        if self.limits.max_control_concurrency_count == 0
            || self.limits.max_control_concurrency_count > 256
        {
            return Err(CONFIG);
        }
        if self.limits.max_store_queue_count == 0 || self.limits.max_store_queue_count > 512 {
            return Err(CONFIG);
        }
        if self.limits.max_playlist_bytes == 0 || self.limits.max_playlist_bytes > 16777216 {
            return Err(CONFIG);
        }
        if self.limits.max_playlist_depth_count == 0 || self.limits.max_playlist_depth_count > 256 {
            return Err(CONFIG);
        }
        if self.limits.max_key_bytes == 0 || self.limits.max_key_bytes > 65536 {
            return Err(CONFIG);
        }
        if self.limits.max_sniff_bytes == 0 || self.limits.max_sniff_bytes > 65536 {
            return Err(CONFIG);
        }
        if self.lifecycle.drain_timeout_s == 0 || self.lifecycle.drain_timeout_s > 480 {
            return Err(CONFIG);
        }

        let trusted_bind = |address: SocketAddr| {
            address.ip().is_loopback()
                || self
                    .http
                    .trusted_listen_ip
                    .is_some_and(|ip| ip.is_private() && address.ip() == std::net::IpAddr::V4(ip))
        };
        if !trusted_bind(self.http.control_listen)
            || !trusted_bind(self.http.stream_listen)
            || self.http.control_listen.port() == 0
            || self.http.stream_listen.port() == 0
            || self.http.control_listen == self.http.stream_listen
            || self.http.public_prefix != "/stream"
            || self.network.guard_mode != "local-mullvad"
            || self.auth.algorithm != "EdDSA"
            || self.auth.issuer != "family-stream-api"
            || self.auth.audience != "family-stream-relay"
            || self.auth.max_grant_lifetime_s > self.state.max_session_lifetime_s
            || self.auth.clock_skew_s >= self.auth.max_grant_lifetime_s
            || self.state.max_sources_per_session_count > 128
            || self.state.max_resources_per_session_count > self.state.max_resources_total_count
            || self.state.max_snapshot_bytes > self.limits.max_control_body_bytes
            || self.network.max_streams_per_session_count > self.network.max_streams_count
            || self.network.max_streams_per_authority_count > self.network.max_streams_count
            || self.network.body_chunk_bytes > 65536
            || self.network.max_redirects_count > 3
            || self.state.database_path.is_empty()
        {
            return Err(CONFIG);
        }
        Ok(())
    }
}

const fn default_database_bytes() -> u64 {
    1_073_741_824
}
const fn default_free_space_bytes() -> u64 {
    67_108_864
}
const fn default_idempotency_count() -> u64 {
    256
}
