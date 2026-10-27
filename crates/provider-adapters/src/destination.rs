//! Pure declaration checks. The native host repeats these checks and validates
//! and pins DNS at connection time; guest checks cannot approve DNS answers.
use crate::AdapterError;
use std::net::IpAddr;
use url::{Host, Url};

fn public_ip(ip: IpAddr) -> bool {
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
                    (0x2001u128 << 112, 23),
                    (0x20010db8u128 << 96, 32),
                    (0x2002u128 << 112, 16),
                    (0x3fffu128 << 112, 20),
                ]
                .iter()
                .any(|&(prefix, bits)| value >> (128 - bits) == prefix >> (128 - bits))
        }
    }
}

pub fn public_url(raw: &str) -> Result<Url, AdapterError> {
    if raw.is_empty()
        || raw.len() > 8192
        || raw.trim() != raw
        || raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return Err(AdapterError::Protocol("invalid destination"));
    }
    let authority = raw
        .split_once("://")
        .and_then(|(_, rest)| rest.split(['/', '?', '#']).next())
        .filter(|value| !value.is_empty())
        .ok_or(AdapterError::Protocol("destination authority"))?;
    if authority.contains(['@', '%']) {
        return Err(AdapterError::Protocol("destination credentials"));
    }
    let url = Url::parse(raw).map_err(|_| AdapterError::Protocol("destination URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(if url.scheme() == "https" { 443 } else { 80 })
    {
        return Err(AdapterError::Protocol("destination transport"));
    }
    match url
        .host()
        .ok_or(AdapterError::Protocol("destination host"))?
    {
        Host::Ipv4(ip) if public_ip(ip.into()) => {
            if authority.split(':').next() != Some(ip.to_string().as_str()) {
                return Err(AdapterError::Protocol("ambiguous address"));
            }
        }
        Host::Ipv6(ip) if public_ip(ip.into()) => {}
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
                return Err(AdapterError::Protocol("nonpublic hostname"));
            }
        }
        _ => return Err(AdapterError::Protocol("nonpublic address")),
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_url_validation_and_ssrf_rejection() {
        // Valid public URLs
        assert!(public_url("https://example.com/api").is_ok());
        assert!(public_url("http://8.8.8.8/").is_ok());
        assert!(public_url("https://[2606:4700:4700::1111]/").is_ok());

        // Private / Loopback IPv4
        assert!(public_url("http://127.0.0.1/").is_err());
        assert!(public_url("http://10.0.0.1/").is_err());
        assert!(public_url("http://192.168.1.1/").is_err());
        assert!(public_url("http://172.16.0.1/").is_err());
        assert!(public_url("http://169.254.169.254/").is_err());

        // Hex, octal, and single dword IPv4 representations
        assert!(public_url("http://0x7f000001/").is_err());
        assert!(public_url("http://0x08080808/").is_err());
        assert!(public_url("http://0177.0.0.1/").is_err());
        assert!(public_url("http://010.010.010.010/").is_err());
        assert!(public_url("http://2130706433/").is_err());
        assert!(public_url("http://8.8.8/").is_err());

        // IPv6 loopback, private, and IPv4-mapped IPv6
        assert!(public_url("http://[::1]/").is_err());
        assert!(public_url("http://[fc00::1]/").is_err());
        assert!(public_url("http://[fe80::1]/").is_err());
        assert!(public_url("http://[::ffff:127.0.0.1]/").is_err());
        assert!(public_url("http://[::ffff:8.8.8.8]/").is_err());
        assert!(public_url("http://[2002:7f00:1::]/").is_err());

        // Nonpublic domains and credentials
        assert!(public_url("http://localhost/").is_err());
        assert!(public_url("http://internal.lan/").is_err());
        assert!(public_url("http://user:pass@example.com/").is_err());
        assert!(public_url("http://example.com:8080/").is_err());
    }
}
