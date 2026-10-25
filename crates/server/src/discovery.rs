use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket as StdUdpSocket},
    process::Stdio,
    sync::Arc,
};

use serde::Serialize;
use tokio::net::UdpSocket;

/// Default UDP port used for fast LAN broadcast discovery (`MELLO_TV_DISCOVER`).
pub const DISCOVERY_UDP_PORT: u16 = 7359;

/// Bonjour / mDNS service type registered on the local network.
pub const BONJOUR_SERVICE_TYPE: &str = "_mellotv._tcp";

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryAnnouncement {
    pub service: &'static str,
    pub version: &'static str,
    pub app_version: &'static str,
    pub server_id: String,
    pub server_name: String,
    pub port: u16,
    pub endpoints: Vec<String>,
}

/// Returns the machine's human-friendly hostname.
pub fn detect_server_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
            && out.status.success()
        {
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }

    if let Ok(out) = std::process::Command::new("hostname").output()
        && out.status.success()
    {
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let clean = raw.trim_end_matches(".local").to_string();
        if !clean.is_empty() {
            return clean;
        }
    }

    "Mello TV Server".to_string()
}

/// Returns the machine's mDNS `.local` hostname if available.
pub fn detect_local_hostname() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("scutil")
            .args(["--get", "LocalHostName"])
            .output()
            && out.status.success()
        {
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !name.is_empty() {
                return Some(format!("{name}.local"));
            }
        }
    }
    None
}

/// Dynamically enumerates all non-loopback IPv4 addresses on this machine
/// (LAN Wi-Fi/Ethernet first, then Tailscale CGNAT 100.64.0.0/10, then `.local` mDNS).
pub fn enumerate_reachable_endpoints(port: u16) -> Vec<String> {
    let mut lan_ips = BTreeSet::new();
    let mut tailscale_ips = BTreeSet::new();

    for ip in collect_interface_ipv4s() {
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
            continue;
        }
        let octets = ip.octets();
        // Tailscale / CGNAT range: 100.64.0.0/10 (100.64.0.0 - 100.127.255.255)
        if octets[0] == 100 && (64..=127).contains(&octets[1]) {
            tailscale_ips.insert(ip);
        } else if ip.is_private() {
            lan_ips.insert(ip);
        }
    }

    let mut endpoints = Vec::new();
    for ip in lan_ips {
        endpoints.push(format!("http://{ip}:{port}"));
    }
    for ip in tailscale_ips {
        endpoints.push(format!("http://{ip}:{port}"));
    }
    if let Some(local_host) = detect_local_hostname() {
        endpoints.push(format!("http://{local_host}:{port}"));
    }

    endpoints
}

fn collect_interface_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();

    // 1. OS routing table probes (zero network packets sent) for primary LAN & Tailscale interfaces
    for probe_target in ["1.1.1.1:80", "192.168.1.1:80", "10.0.0.1:80", "100.100.100.100:80"] {
        if let Ok(sock) = StdUdpSocket::bind("0.0.0.0:0")
            && sock.connect(probe_target).is_ok()
            && let Ok(SocketAddr::V4(local)) = sock.local_addr()
        {
            ips.push(*local.ip());
        }
    }

    // 2. Parse `ifconfig` output for all active multi-homed IPv4 interfaces
    if let Ok(out) = std::process::Command::new("ifconfig").output()
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("inet ")
                && let Some(addr_str) = rest.split_whitespace().next()
                && let Ok(ip) = addr_str.parse::<Ipv4Addr>()
            {
                ips.push(ip);
            }
        }
    }

    ips
}

/// Builds the current discovery announcement for this server instance.
pub fn build_announcement(server_id: &str, port: u16) -> DiscoveryAnnouncement {
    DiscoveryAnnouncement {
        service: "mello-tv",
        version: "api.v1",
        app_version: env!("CARGO_PKG_VERSION"),
        server_id: server_id.to_string(),
        server_name: detect_server_name(),
        port,
        endpoints: enumerate_reachable_endpoints(port),
    }
}

/// Spawns background tasks for:
/// 1. UDP Broadcast Discovery Responder on `0.0.0.0:7359`
/// 2. Native Bonjour / mDNS (`_mellotv._tcp`) service advertisement
pub fn spawn_discovery_services(server_id: String, http_port: u16) {
    let udp_server_id = server_id.clone();
    tokio::spawn(async move {
        if let Err(err) = run_udp_discovery_responder(udp_server_id, http_port).await {
            tracing::warn!(error = %err, "UDP discovery responder exited");
        }
    });

    tokio::spawn(async move {
        run_bonjour_registration(server_id, http_port).await;
    });
}

async fn run_udp_discovery_responder(
    server_id: String,
    http_port: u16,
) -> std::io::Result<()> {
    let bind_addr = SocketAddr::from(([0, 0, 0, 0], DISCOVERY_UDP_PORT));
    let socket = UdpSocket::bind(bind_addr).await?;
    let _ = socket.set_broadcast(true);
    let socket = Arc::new(socket);

    tracing::info!(
        udp_port = DISCOVERY_UDP_PORT,
        http_port,
        "Mello TV UDP discovery responder listening"
    );

    let mut buf = [0u8; 1024];
    loop {
        let (len, peer) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(error = %e, "UDP discovery recv error");
                continue;
            }
        };

        let msg = std::str::from_utf8(&buf[..len]).unwrap_or("").trim();
        if !msg.starts_with("MELLO_TV_DISCOVER") && !msg.starts_with("WHO_IS_MELLO_TV") {
            continue;
        }

        let mut announcement = build_announcement(&server_id, http_port);
        // Prioritize the endpoint on the same subnet as the requesting peer
        if let IpAddr::V4(peer_v4) = peer.ip() {
            let peer_octets = peer_v4.octets();
            let prefix = format!(
                "http://{}.{}.{}.",
                peer_octets[0], peer_octets[1], peer_octets[2]
            );
            announcement.endpoints.sort_by_key(|ep| {
                if ep.starts_with(&prefix) { 0 } else { 1 }
            });
        }

        if let Ok(payload) = serde_json::to_vec(&announcement) {
            let _ = socket.send_to(&payload, peer).await;
        }
    }
}

async fn run_bonjour_registration(server_id: String, http_port: u16) {
    let server_name = detect_server_name();
    #[cfg(target_os = "macos")]
    {
        let mut child = match tokio::process::Command::new("/usr/bin/dns-sd")
            .args([
                "-R",
                &server_name,
                BONJOUR_SERVICE_TYPE,
                "local",
                &http_port.to_string(),
                "service=mello-tv",
                "version=api.v1",
                &format!("server_id={server_id}"),
                "path=/api/v1/status",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => {
                tracing::info!(
                    service_type = BONJOUR_SERVICE_TYPE,
                    %server_name,
                    http_port,
                    "registered Bonjour mDNS service via dns-sd"
                );
                c
            }
            Err(e) => {
                tracing::debug!(error = %e, "failed to spawn dns-sd for Bonjour registration");
                return;
            }
        };
        let _ = child.wait().await;
    }
}
