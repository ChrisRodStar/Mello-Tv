//! Process-local diagnostics with a fixed vocabulary: no caller-controlled labels.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Instant;
#[derive(Default)]
pub struct Lifecycle {
    pub draining: AtomicBool,
    pub failed: AtomicBool,
}
const CAUSES: [&str; 12] = [
    "upstream",
    "timeout",
    "admission",
    "state",
    "cancelled",
    "range",
    "expired",
    "authorization",
    "manifest",
    "vpn",
    "refresh",
    "other",
];
static FAILURES: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];
const STAGES: [&str; 13] = [
    "authorization",
    "store",
    "preparation",
    "delivery",
    "fetch",
    "admission",
    "dns",
    "exchange",
    "connect",
    "tls",
    "headers",
    "manifest",
    "first_byte",
];
static COUNTS: [AtomicU64; 13] = [const { AtomicU64::new(0) }; 13];
static MICROS: [AtomicU64; 13] = [const { AtomicU64::new(0) }; 13];
pub static CONNECTION_REUSES: AtomicU64 = AtomicU64::new(0);
pub static DNS_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
pub static UPSTREAM_BYTES: AtomicU64 = AtomicU64::new(0);
pub static DELIVERED_BYTES: AtomicU64 = AtomicU64::new(0);
pub static GENERATIONS: AtomicU64 = AtomicU64::new(0);
pub static VPN_TRANSITIONS: AtomicU64 = AtomicU64::new(0);
pub static FORCED_DRAINS: AtomicU64 = AtomicU64::new(0);
fn add(counter: &AtomicU64, value: u64) {
    let _ = counter.try_update(Relaxed, Relaxed, |old| Some(old.saturating_add(value)));
}
pub fn bytes(counter: &AtomicU64, value: usize) {
    add(counter, value as u64);
}
pub fn event(counter: &AtomicU64) {
    add(counter, 1);
}
pub fn failure(code: &str) {
    let index = match code {
        "upstream_rejected" => 0,
        "upstream_timeout" | "control_timeout" | "downstream_timeout" => 1,
        "request_limit" | "state_limit" | "preparation_limit" | "manifest_limit" => 2,
        "state_store_unavailable" => 3,
        "stream_cancelled" | "preparation_cancelled" => 4,
        "range_mismatch" => 5,
        "session_expired" | "source_expired" | "expired_grant" => 6,
        "invalid_grant" | "revoked_grant" | "owner_mismatch" | "invalid_control_bearer" => 7,
        "invalid_manifest" | "unsupported_manifest" => 8,
        "vpn_unavailable" => 9,
        "source_refresh_required" => 10,
        _ => 11,
    };
    event(&FAILURES[index]);
}
pub enum Stage {
    Authorization,
    Store,
    Preparation,
    Delivery,
    Fetch,
    Admission,
    Dns,
    Exchange,
    Connect,
    Tls,
    Headers,
    Manifest,
    FirstByte,
}
pub fn observe(stage: Stage, duration: std::time::Duration) {
    let index = stage as usize;
    event(&COUNTS[index]);
    add(
        &MICROS[index],
        duration.as_micros().min(u64::MAX as u128) as u64,
    );
}
pub struct Timer(usize, Instant);
impl Timer {
    pub fn start(stage: Stage) -> Self {
        Self(stage as usize, Instant::now())
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        event(&COUNTS[self.0]);
        add(
            &MICROS[self.0],
            self.1.elapsed().as_micros().min(u64::MAX as u128) as u64,
        );
    }
}
pub fn metrics() -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(4096);
    for (index, cause) in CAUSES.iter().enumerate() {
        let _ = writeln!(
            output,
            "relay_failures_total{{cause=\"{cause}\"}} {}",
            FAILURES[index].load(Relaxed)
        );
    }
    for (index, stage) in STAGES.iter().enumerate() {
        let _ = writeln!(
            output,
            "relay_stage_count{{stage=\"{stage}\"}} {}\nrelay_stage_microseconds_total{{stage=\"{stage}\"}} {}",
            COUNTS[index].load(Relaxed),
            MICROS[index].load(Relaxed)
        );
    }
    for (name, value) in [
        ("connection_reuses_total", &CONNECTION_REUSES),
        ("dns_cache_hits_total", &DNS_CACHE_HITS),
        ("upstream_bytes_total", &UPSTREAM_BYTES),
        ("delivered_bytes_total", &DELIVERED_BYTES),
        ("generations_total", &GENERATIONS),
        ("vpn_transitions_total", &VPN_TRANSITIONS),
        ("forced_drains_total", &FORCED_DRAINS),
    ] {
        let _ = writeln!(output, "relay_{name} {}", value.load(Relaxed));
    }
    output
}
