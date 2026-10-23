//! Bounded positive DNS cache and caller-owned lookup coalescing. No I/O here.
use super::{NetError, Result, guard::Epoch};
use std::{
    collections::BTreeMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, time::Instant};

#[derive(Default)]
pub(super) struct Cache(Arc<Mutex<BTreeMap<String, Entry>>>);
struct Entry {
    epoch: Epoch,
    state: State,
}
enum State {
    Pending(Arc<watch::Sender<bool>>),
    Ready {
        addresses: Vec<IpAddr>,
        expires: Instant,
    },
}
pub(super) enum Claim {
    Cached(Vec<IpAddr>),
    Wait(watch::Receiver<bool>),
    Owner(Owner),
}
pub(super) struct Owner {
    cache: Arc<Mutex<BTreeMap<String, Entry>>>,
    host: String,
    signal: Arc<watch::Sender<bool>>,
}
impl Cache {
    pub(super) fn claim(&self, host: &str, epoch: &Epoch) -> Result<Claim> {
        let mut entries = self.0.lock().map_err(|_| NetError::Limit)?;
        entries.retain(|_, entry| {
            !entry.epoch.cancel.is_cancelled()
                && match &entry.state {
                    State::Pending(_) => true,
                    State::Ready { expires, .. } => *expires > Instant::now(),
                }
        });
        if let Some(entry) = entries.get(host)
            && entry.epoch.identity == epoch.identity
            && entry.epoch.index == epoch.index
        {
            return Ok(match &entry.state {
                State::Ready { addresses, .. } => Claim::Cached(addresses.clone()),
                State::Pending(signal) => Claim::Wait(signal.subscribe()),
            });
        }
        if entries.len() >= 128 {
            let oldest = entries
                .iter()
                .filter_map(|(host, entry)| match entry.state {
                    State::Ready { expires, .. } => Some((host.clone(), expires)),
                    _ => None,
                })
                .min_by_key(|(_, expires)| *expires)
                .map(|(host, _)| host);
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            } else {
                return Err(NetError::Limit);
            }
        }
        let (signal, _) = watch::channel(false);
        let signal = Arc::new(signal);
        entries.insert(
            host.to_owned(),
            Entry {
                epoch: epoch.clone(),
                state: State::Pending(signal.clone()),
            },
        );
        Ok(Claim::Owner(Owner {
            cache: self.0.clone(),
            host: host.to_owned(),
            signal,
        }))
    }
}
impl Owner {
    pub(super) fn complete(&self, addresses: Vec<IpAddr>, ttl: Duration) -> Result<()> {
        let mut entries = self.cache.lock().map_err(|_| NetError::Limit)?;
        if let Some(entry) = entries.get_mut(&self.host)
            && !entry.epoch.cancel.is_cancelled()
            && matches!(&entry.state, State::Pending(signal) if Arc::ptr_eq(signal, &self.signal))
            && !ttl.is_zero()
        {
            entry.state = State::Ready {
                addresses,
                expires: Instant::now() + ttl.min(Duration::from_secs(60)),
            };
        }
        Ok(())
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.cache.lock()
            && entries.get(&self.host).is_some_and(|entry| matches!(&entry.state, State::Pending(signal) if Arc::ptr_eq(signal, &self.signal))) {
            entries.remove(&self.host);
        }
        // Receiver subscribes under the same state mutex; completion cannot be lost.
        let _ = self.signal.send(true);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::network::Guard;
    #[tokio::test]
    async fn ownership_ttl_and_epoch_invalidation() {
        let cache = Cache::default();
        let guard = Guard::default();
        guard.synthetic("first");
        let epoch = guard.lease().unwrap();
        let Claim::Owner(owner) = cache.claim("example.com", &epoch).unwrap() else {
            unreachable!()
        };
        let Claim::Wait(mut waiter) = cache.claim("example.com", &epoch).unwrap() else {
            unreachable!()
        };
        owner
            .complete(vec!["8.8.8.8".parse().unwrap()], Duration::from_millis(2))
            .unwrap();
        drop(owner);
        waiter.changed().await.unwrap();
        assert!(matches!(
            cache.claim("example.com", &epoch).unwrap(),
            Claim::Cached(_)
        ));
        tokio::time::sleep(Duration::from_millis(4)).await;
        let Claim::Owner(owner) = cache.claim("example.com", &epoch).unwrap() else {
            unreachable!()
        };
        let Claim::Wait(mut waiter) = cache.claim("example.com", &epoch).unwrap() else {
            unreachable!()
        };
        drop(owner);
        waiter.changed().await.unwrap();
        let Claim::Owner(owner) = cache.claim("example.com", &epoch).unwrap() else {
            unreachable!()
        };
        owner
            .complete(vec!["8.8.8.8".parse().unwrap()], Duration::from_secs(60))
            .unwrap();
        drop(owner);
        guard.invalidate();
        guard.synthetic("first");
        assert!(matches!(
            cache.claim("example.com", &guard.lease().unwrap()).unwrap(),
            Claim::Owner(_)
        ));
    }
    #[test]
    fn bounded_inflight_admission_and_no_zero_ttl_reuse() {
        let cache = Cache::default();
        let guard = Guard::default();
        guard.synthetic("test");
        let epoch = guard.lease().unwrap();
        let mut owners = Vec::new();
        for index in 0..128 {
            let Claim::Owner(owner) = cache
                .claim(&format!("host-{index}.example"), &epoch)
                .unwrap()
            else {
                unreachable!()
            };
            owners.push(owner);
        }
        assert!(matches!(
            cache.claim("overflow.example", &epoch),
            Err(NetError::Limit)
        ));
        drop(owners);
        let Claim::Owner(owner) = cache.claim("overflow.example", &epoch).unwrap() else {
            unreachable!()
        };
        owner
            .complete(vec!["8.8.8.8".parse().unwrap()], Duration::ZERO)
            .unwrap();
        drop(owner);
        assert!(matches!(
            cache.claim("overflow.example", &epoch).unwrap(),
            Claim::Owner(_)
        ));
    }
}
