use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket as StdUdpSocket},
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

/// Checks whether an IPv4 address is in the Tailscale CGNAT subnet `100.64.0.0/10`
/// (100.64.0.0 - 100.127.255.255).
pub fn is_tailscale_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

/// Checks whether an IPv6 address is in the Tailscale ULA range `fd7a:...`
/// (`fd7a:115c:a1e0::/48`).
pub fn is_tailscale_ipv6(ip: &Ipv6Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 0xfd && octets[1] == 0x7a
}

/// Checks whether an IPv6 address is a non-loopback, non-link-local LAN or global IPv6 address.
pub fn is_lan_ipv6(ip: &Ipv6Addr) -> bool {
    let octets = ip.octets();
    if ip.is_loopback() || ip.is_unspecified() {
        return false;
    }
    // Skip link-local fe80::/10
    if octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80 {
        return false;
    }
    if is_tailscale_ipv6(ip) {
        return false;
    }
    // Global unicast 2000::/3 or private ULA fc00::/7
    (octets[0] & 0xe0) == 0x20 || (octets[0] & 0xfe) == 0xfc
}

const TAILSCALE_CANDIDATE_PATHS: &[&str] = &[
    "tailscale",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    "/usr/local/bin/tailscale",
    "/opt/homebrew/bin/tailscale",
    "/usr/bin/tailscale",
];

/// Queries the local Tailscale daemon status JSON if available.
pub fn query_tailscale_status() -> Option<serde_json::Value> {
    for path in TAILSCALE_CANDIDATE_PATHS {
        if let Ok(out) = std::process::Command::new(path)
            .args(["status", "--json"])
            .output()
            && out.status.success()
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout)
        {
            return Some(json);
        }
    }
    None
}

/// Extracts Tailscale IP strings from the `tailscale status --json` output.
pub fn extract_tailscale_ips_from_json(status: &serde_json::Value) -> Vec<String> {
    let mut ips = Vec::new();

    let check_array = |arr_opt: Option<&Vec<serde_json::Value>>, ips: &mut Vec<String>| {
        if let Some(arr) = arr_opt {
            for v in arr {
                if let Some(s) = v.as_str() {
                    ips.push(s.to_string());
                }
            }
        }
    };

    check_array(
        status.get("TailscaleIPs").and_then(|v| v.as_array()),
        &mut ips,
    );
    if let Some(self_node) = status.get("Self") {
        check_array(
            self_node.get("TailscaleIPs").and_then(|v| v.as_array()),
            &mut ips,
        );
    }

    ips
}

/// Extracts the MagicDNS FQDN (`*.ts.net`) from the `tailscale status --json` output.
pub fn extract_magicdns_fqdn(status: &serde_json::Value) -> Option<String> {
    if let Some(self_node) = status.get("Self")
        && let Some(dns_name) = self_node.get("DNSName").and_then(|v| v.as_str())
    {
        let clean = dns_name.trim_end_matches('.');
        if clean.contains(".ts.net") {
            return Some(clean.to_string());
        }
    }
    if let Some(cert_domains) = status.get("CertDomains").and_then(|v| v.as_array()) {
        for domain in cert_domains {
            if let Some(s) = domain.as_str() {
                let clean = s.trim_end_matches('.');
                if clean.contains(".ts.net") {
                    return Some(clean.to_string());
                }
            }
        }
    }
    None
}

/// Fallback detection for Tailscale MagicDNS FQDN when `tailscale status --json` is not reachable.
pub fn detect_magicdns_fallback() -> Option<String> {
    if let Ok(out) = std::process::Command::new("hostname").arg("-f").output()
        && out.status.success()
    {
        let name = String::from_utf8_lossy(&out.stdout)
            .trim()
            .trim_end_matches('.')
            .to_string();
        if name.contains(".ts.net") {
            return Some(name);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("scutil").arg("--dns").output()
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(domain) = trimmed.strip_prefix("domain   : ") {
                    let domain = domain.trim().trim_end_matches('.');
                    if domain.ends_with(".ts.net") {
                        let server_name = detect_server_name();
                        let clean_prefix =
                            server_name.to_lowercase().replace([' ', '’', '\''], "-");
                        return Some(format!("{clean_prefix}.{domain}"));
                    }
                }
            }
        }
    }

    None
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

/// Dynamically enumerates all reachable endpoints on this machine:
/// 1. LAN Wi-Fi / Ethernet IPv4 addresses
/// 2. Tailscale IPv4 addresses (`100.64.0.0/10`)
/// 3. Tailscale IPv6 addresses (`fd7a:...`)
/// 4. Tailscale MagicDNS FQDN (`*.ts.net`)
/// 5. Bonjour / mDNS local hostname (`.local`)
/// 6. Any other routable LAN IPv6 addresses
pub fn enumerate_reachable_endpoints(port: u16) -> Vec<String> {
    let mut lan_v4_ips = BTreeSet::new();
    let mut lan_v6_ips = BTreeSet::new();
    let mut tailscale_v4_ips = BTreeSet::new();
    let mut tailscale_v6_ips = BTreeSet::new();
    let mut magicdns_fqdn = None;

    // 1. Query tailscale status --json for authoritative Tailscale IPs and MagicDNS FQDN
    if let Some(status) = query_tailscale_status() {
        if let Some(fqdn) = extract_magicdns_fqdn(&status) {
            magicdns_fqdn = Some(fqdn);
        }
        for ip_str in extract_tailscale_ips_from_json(&status) {
            if let Ok(ip4) = ip_str.parse::<Ipv4Addr>()
                && is_tailscale_ipv4(&ip4)
            {
                tailscale_v4_ips.insert(ip4);
            } else if let Ok(ip6) = ip_str.parse::<Ipv6Addr>()
                && is_tailscale_ipv6(&ip6)
            {
                tailscale_v6_ips.insert(ip6);
            }
        }
    }

    // 2. Fallback / supplement from network interfaces and OS routing table probes
    let (v4s, v6s) = collect_interface_ips();

    for ip in v4s {
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
            continue;
        }
        if is_tailscale_ipv4(&ip) {
            tailscale_v4_ips.insert(ip);
        } else if ip.is_private() {
            lan_v4_ips.insert(ip);
        }
    }

    for ip in v6s {
        if ip.is_loopback() || ip.is_unspecified() {
            continue;
        }
        let octets = ip.octets();
        // Skip link-local fe80::/10
        if octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80 {
            continue;
        }
        if is_tailscale_ipv6(&ip) {
            tailscale_v6_ips.insert(ip);
        } else if is_lan_ipv6(&ip) {
            lan_v6_ips.insert(ip);
        }
    }

    // 3. Fallback for MagicDNS FQDN if not discovered via tailscale status --json
    if magicdns_fqdn.is_none() {
        magicdns_fqdn = detect_magicdns_fallback();
    }

    // 4. Assemble endpoints with clean deduplication
    let mut endpoints = Vec::new();
    let mut seen = BTreeSet::new();

    let mut push_endpoint = |ep: String| {
        if seen.insert(ep.clone()) {
            endpoints.push(ep);
        }
    };

    // Primary LAN IPv4 endpoints
    for ip in lan_v4_ips {
        push_endpoint(format!("http://{ip}:{port}"));
    }

    // Tailscale IPv4 endpoints (100.64.0.0/10)
    for ip in tailscale_v4_ips {
        push_endpoint(format!("http://{ip}:{port}"));
    }

    // Tailscale IPv6 endpoints (fd7a:...)
    for ip in tailscale_v6_ips {
        push_endpoint(format!("http://[{ip}]:{port}"));
    }

    // Tailscale MagicDNS FQDN (*.ts.net)
    if let Some(fqdn) = magicdns_fqdn {
        push_endpoint(format!("http://{fqdn}:{port}"));
    }

    // Local mDNS (.local)
    if let Some(local_host) = detect_local_hostname() {
        push_endpoint(format!("http://{local_host}:{port}"));
    }

    // Other routable LAN IPv6 endpoints
    for ip in lan_v6_ips {
        push_endpoint(format!("http://[{ip}]:{port}"));
    }

    endpoints
}

fn collect_interface_ips() -> (Vec<Ipv4Addr>, Vec<Ipv6Addr>) {
    let mut v4_ips = Vec::new();
    let mut v6_ips = Vec::new();

    // 1. OS routing table probes (zero network packets sent) for LAN & Tailscale interfaces
    for probe_target in [
        "1.1.1.1:80",
        "192.168.1.1:80",
        "10.0.0.1:80",
        "100.100.100.100:80",
    ] {
        if let Ok(sock) = StdUdpSocket::bind("0.0.0.0:0")
            && sock.connect(probe_target).is_ok()
            && let Ok(SocketAddr::V4(local)) = sock.local_addr()
        {
            v4_ips.push(*local.ip());
        }
    }

    for probe_target in [
        "[2606:4700:4700::1111]:80",
        "[2001:4860:4860::8888]:80",
        "[fd7a:115c:a1e0::1]:80",
    ] {
        if let Ok(sock) = StdUdpSocket::bind("[::]:0")
            && sock.connect(probe_target).is_ok()
            && let Ok(SocketAddr::V6(local)) = sock.local_addr()
        {
            v6_ips.push(*local.ip());
        }
    }

    // 2. Parse `ifconfig` output for all active interfaces
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
                v4_ips.push(ip);
            } else if let Some(rest) = trimmed.strip_prefix("inet6 ")
                && let Some(token) = rest.split_whitespace().next()
            {
                let addr_str = token.split('%').next().unwrap_or(token);
                if let Ok(ip) = addr_str.parse::<Ipv6Addr>() {
                    v6_ips.push(ip);
                }
            }
        }
    }

    (v4_ips, v6_ips)
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

async fn run_udp_discovery_responder(server_id: String, http_port: u16) -> std::io::Result<()> {
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

        // Prioritize endpoint based on requesting peer's network
        match peer.ip() {
            IpAddr::V4(peer_v4) => {
                if is_tailscale_ipv4(&peer_v4) {
                    announcement.endpoints.sort_by_key(|ep| {
                        if ep.contains(".ts.net") || ep.starts_with("http://100.") {
                            0
                        } else {
                            1
                        }
                    });
                } else {
                    let peer_octets = peer_v4.octets();
                    let prefix = format!(
                        "http://{}.{}.{}.",
                        peer_octets[0], peer_octets[1], peer_octets[2]
                    );
                    announcement.endpoints.sort_by_key(
                        |ep| {
                            if ep.starts_with(&prefix) { 0 } else { 1 }
                        },
                    );
                }
            }
            IpAddr::V6(peer_v6) => {
                if is_tailscale_ipv6(&peer_v6) {
                    announcement.endpoints.sort_by_key(|ep| {
                        if ep.contains(".ts.net") || ep.starts_with("http://[fd7a:") {
                            0
                        } else {
                            1
                        }
                    });
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tailscale_ip_classification() {
        let ts_v4: Ipv4Addr = "100.99.24.1".parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
        assert!(is_tailscale_ipv4(&ts_v4));

        let non_ts_v4: Ipv4Addr = "192.168.1.1".parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
        assert!(!is_tailscale_ipv4(&non_ts_v4));

        let ts_v6: Ipv6Addr = "fd7a:115c:a1e0::252a:1802"
            .parse()
            .unwrap_or(Ipv6Addr::UNSPECIFIED);
        assert!(is_tailscale_ipv6(&ts_v6));

        let non_ts_v6: Ipv6Addr = "2603:6080:9005:f881::1"
            .parse()
            .unwrap_or(Ipv6Addr::UNSPECIFIED);
        assert!(!is_tailscale_ipv6(&non_ts_v6));
    }

    #[test]
    fn test_extract_tailscale_json() {
        let sample = serde_json::json!({
            "TailscaleIPs": ["100.99.24.1", "fd7a:115c:a1e0::252a:1802"],
            "Self": {
                "DNSName": "chriss-mac-mini.tail113fae.ts.net.",
                "TailscaleIPs": ["100.99.24.1", "fd7a:115c:a1e0::252a:1802"]
            }
        });

        let ips = extract_tailscale_ips_from_json(&sample);
        assert!(ips.contains(&"100.99.24.1".to_string()));
        assert!(ips.contains(&"fd7a:115c:a1e0::252a:1802".to_string()));

        let fqdn = extract_magicdns_fqdn(&sample);
        assert_eq!(fqdn.as_deref(), Some("chriss-mac-mini.tail113fae.ts.net"));
    }
}
