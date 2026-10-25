use super::{TransportError, destination::validate_answers};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, OnceLock, RwLock},
    time::{Duration, Instant},
};

const DNS_CACHE_TTL: Duration = Duration::from_secs(120);
const DNS_CACHE_MAX_ENTRIES: usize = 256;

#[derive(Clone)]
struct CacheEntry {
    addresses: Vec<SocketAddr>,
    expires_at: Instant,
}

fn dns_cache() -> &'static RwLock<BTreeMap<String, CacheEntry>> {
    static CACHE: OnceLock<RwLock<BTreeMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Returns only the addresses validated by this same lookup to the connector.
#[derive(Clone)]
pub(super) struct GuardedResolver {
    proxy_host: Option<Arc<str>>,
}
impl GuardedResolver {
    pub(super) fn new(proxy_host: Option<String>) -> Self {
        Self {
            proxy_host: proxy_host.map(Arc::from),
        }
    }
    pub(super) async fn destination(host: &str) -> Result<Vec<SocketAddr>, TransportError> {
        let answers = resolve_cached(host).await?;
        validate_answers(&answers)?;
        Ok(answers)
    }
}
async fn resolve_cached(host: &str) -> Result<Vec<SocketAddr>, TransportError> {
    let now = Instant::now();
    let cached = dns_cache()
        .read()
        .ok()
        .and_then(|c| c.get(host).filter(|e| e.expires_at > now).map(|e| e.addresses.clone()));
    if let Some(addresses) = cached {
        return Ok(addresses);
    }

    let mut addresses = Vec::new();
    for address in tokio::net::lookup_host((host, 0))
        .await
        .map_err(|_| TransportError::Dns)?
    {
        if addresses.len() >= 64 {
            return Err(TransportError::DestinationDenied);
        }
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    if addresses.is_empty() {
        return Err(TransportError::Dns);
    }
    validate_answers(&addresses)?;

    if let Ok(mut cache) = dns_cache().write() {
        if cache.len() >= DNS_CACHE_MAX_ENTRIES {
            cache.retain(|_, v| v.expires_at > now);
        }
        if cache.len() < DNS_CACHE_MAX_ENTRIES {
            cache.insert(
                host.to_owned(),
                CacheEntry {
                    addresses: addresses.clone(),
                    expires_at: now + DNS_CACHE_TTL,
                },
            );
        }
    }

    Ok(addresses)
}
impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let name = name.as_str().to_owned();
        let proxy = self.proxy_host.clone();
        Box::pin(async move {
            let addresses = resolve_cached(&name)
                .await
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)?;
            // Proxy endpoints are explicit operator configuration and may be
            // local. The client forbids this hostname as an upstream target.
            if proxy.as_deref() != Some(name.as_str()) {
                validate_answers(&addresses)
                    .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)?;
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}
