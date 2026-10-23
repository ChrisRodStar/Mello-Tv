use crate::error::{Error, INVALID};
use std::net::IpAddr;
use url::{Host, Url};

/// Conservative public unicast policy, at least as strict as the Go provider.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let n = u32::from(ip);
            ![
                (0, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 3),
            ]
            .iter()
            .any(|&(p, b)| n >> (32 - b) == p >> (32 - b))
        }
        IpAddr::V6(ip) => {
            // Mapped/translated/transition addresses are deliberately denied.
            let n = u128::from(ip);
            n >> 125 == 1
                && ![
                    (0x2001_u128 << 112, 23),
                    (0x20010db8_u128 << 96, 32),
                    (0x2002_u128 << 112, 16),
                    (0x3fff_u128 << 112, 20),
                ]
                .iter()
                .any(|&(p, b)| n >> (128 - b) == p >> (128 - b))
        }
    }
}
pub fn destination(raw: &str) -> Result<Url, Error> {
    if raw.len() > 16384 || raw.bytes().any(|b| b.is_ascii_control() || b == b'\\') {
        return Err(INVALID);
    }
    let authority = raw
        .split_once("://")
        .ok_or(INVALID)?
        .1
        .split(['/', '?', '#'])
        .next()
        .ok_or(INVALID)?;
    if authority.contains(['%', '@']) {
        return Err(INVALID);
    }
    let url = Url::parse(raw).map_err(|_| INVALID)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(if url.scheme() == "https" { 443 } else { 80 })
    {
        return Err(INVALID);
    }
    match url.host().ok_or(INVALID)? {
        Host::Ipv4(ip) if public_ip(ip.into()) => {
            // Reject WHATWG integer/octal/hex/shortened address spellings.
            if authority.split(':').next() != Some(ip.to_string().as_str()) {
                return Err(INVALID);
            }
        }
        Host::Ipv6(ip) if public_ip(ip.into()) => {}
        Host::Domain(host) => {
            let host = host.trim_end_matches('.');
            if !host.contains('.')
                || host.split('.').any(|s| s.is_empty())
                || [
                    "localhost",
                    "local",
                    "internal",
                    "home",
                    "lan",
                    "test",
                    "invalid",
                    "onion",
                ]
                .iter()
                .any(|s| host == *s || host.ends_with(&format!(".{s}")))
            {
                return Err(INVALID);
            }
        }
        _ => return Err(INVALID),
    }
    Ok(url)
}
pub fn answers(ips: &[IpAddr]) -> Result<(), Error> {
    if ips.is_empty() || ips.len() > 64 || ips.iter().any(|ip| !public_ip(*ip)) {
        return Err(INVALID);
    }
    Ok(())
}
pub fn redirect(from: &Url, location: &str) -> Result<Url, Error> {
    if location.bytes().any(|b| b.is_ascii_control() || b == b'\\') {
        return Err(INVALID);
    }
    if location.contains("://") {
        destination(location)?;
    }
    if location.starts_with("//") {
        destination(&format!("{}:{location}", from.scheme()))?;
    }
    let joined = from.join(location).map_err(|_| INVALID)?;
    let target = destination(joined.as_str())?;
    if from.scheme() == "https" && target.scheme() != "https" {
        return Err(INVALID);
    }
    Ok(target)
}
