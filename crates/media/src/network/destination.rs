use super::TransportError;
use std::net::IpAddr;
use url::{Host, Url};

/// Conservative globally routable unicast policy retained from the relay.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
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
            .any(|&(prefix, bits)| value >> (32 - bits) == prefix >> (32 - bits))
        }
        IpAddr::V6(ip) => {
            let value = u128::from(ip);
            value >> 125 == 1
                && ![
                    (0x2001_u128 << 112, 23),
                    (0x20010db8_u128 << 96, 32),
                    (0x2002_u128 << 112, 16),
                    (0x3fff_u128 << 112, 20),
                ]
                .iter()
                .any(|&(prefix, bits)| value >> (128 - bits) == prefix >> (128 - bits))
        }
    }
}

/// Validate before WHATWG parsing can normalize ambiguous IP spellings.
pub fn parse_destination_url(raw: &str) -> Result<Url, TransportError> {
    if raw.is_empty()
        || raw.len() > 16_384
        || raw.trim() != raw
        || raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return Err(TransportError::DestinationDenied);
    }
    let authority = raw
        .split_once("://")
        .ok_or(TransportError::DestinationDenied)?
        .1
        .split(['/', '?', '#'])
        .next()
        .ok_or(TransportError::DestinationDenied)?;
    if authority.is_empty() || authority.contains(['%', '@']) {
        return Err(TransportError::DestinationDenied);
    }
    let parsed = Url::parse(raw).map_err(|_| TransportError::DestinationDenied)?;
    if let Some(Host::Ipv4(address)) = parsed.host()
        && authority.split(':').next() != Some(address.to_string().as_str())
    {
        return Err(TransportError::DestinationDenied);
    }
    validate_destination_url(&parsed)?;
    Ok(parsed)
}

/// Pure syntax/literal-address check; actual DNS validation happens at dial.
pub fn validate_destination_url(url: &Url) -> Result<(), TransportError> {
    if !matches!(url.scheme(), "https" | "http")
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(if url.scheme() == "https" { 443 } else { 80 })
    {
        return Err(TransportError::DestinationDenied);
    }
    match url.host().ok_or(TransportError::DestinationDenied)? {
        Host::Ipv4(ip) if public_ip(ip.into()) => Ok(()),
        Host::Ipv6(ip) if public_ip(ip.into()) => Ok(()),
        Host::Domain(host) => {
            let host = host.trim_end_matches('.');
            if !host.contains('.')
                || host.split('.').any(str::is_empty)
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
                .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
            {
                return Err(TransportError::DestinationDenied);
            }
            Ok(())
        }
        _ => Err(TransportError::DestinationDenied),
    }
}

pub(super) fn validate_answers(addresses: &[std::net::SocketAddr]) -> Result<(), TransportError> {
    if addresses.is_empty()
        || addresses.len() > 64
        || addresses.iter().any(|address| !public_ip(address.ip()))
    {
        return Err(TransportError::DestinationDenied);
    }
    Ok(())
}

pub(super) fn redirect(from: &Url, raw: &str) -> Result<Url, TransportError> {
    if raw.is_empty()
        || raw.len() > 16_384
        || raw.trim() != raw
        || raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return Err(TransportError::RedirectDenied);
    }
    if raw.contains("://") {
        parse_destination_url(raw)?;
    }
    if raw.starts_with("//") {
        parse_destination_url(&format!("{}:{raw}", from.scheme()))?;
    }
    let target = from.join(raw).map_err(|_| TransportError::RedirectDenied)?;
    validate_destination_url(&target)?;
    if from.scheme() == "https" && target.scheme() != "https" {
        return Err(TransportError::RedirectDenied);
    }
    Ok(target)
}
