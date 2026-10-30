use super::{ProbeFailure, ProbeResult};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(super) struct ByteBudget(Arc<Inner>);
struct Inner {
    state: Mutex<State>,
    changed: Notify,
}
struct State {
    available: usize,
    pending: usize,
}

impl ByteBudget {
    pub(super) fn new(bytes: usize) -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                available: bytes,
                pending: 0,
            }),
            changed: Notify::new(),
        }))
    }

    pub(super) async fn reserve(
        &self,
        wanted: usize,
        exact: bool,
        cancel: &CancellationToken,
    ) -> ProbeResult<Reservation> {
        if wanted == 0 {
            return Err(ProbeFailure::Budget);
        }
        loop {
            if cancel.is_cancelled() {
                return Err(ProbeFailure::Cancelled);
            }
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let mut state = self.0.state.lock().map_err(|_| ProbeFailure::Budget)?;
                // Pending reads may refund capacity. Wait instead of reporting
                // a permanently exhausted budget before those reads settle.
                if state.available >= wanted || state.pending == 0 {
                    if state.available == 0 || (exact && state.available < wanted) {
                        return Err(ProbeFailure::Budget);
                    }
                    let bytes = wanted.min(state.available);
                    state.available -= bytes;
                    state.pending += bytes;
                    return Ok(Reservation {
                        budget: self.clone(),
                        bytes,
                        settled: false,
                    });
                }
            }
            tokio::select! {biased; _=cancel.cancelled()=>return Err(ProbeFailure::Cancelled),_=changed=>{}}
        }
    }
}

pub(super) struct Reservation {
    budget: ByteBudget,
    pub(super) bytes: usize,
    settled: bool,
}
impl Reservation {
    pub(super) fn finish(mut self, spent: usize) -> ProbeResult<()> {
        if spent > self.bytes {
            return Err(ProbeFailure::Protocol);
        }
        self.settle(spent)?;
        self.settled = true;
        Ok(())
    }
    fn settle(&self, spent: usize) -> ProbeResult<()> {
        let mut state = self
            .budget
            .0
            .state
            .lock()
            .map_err(|_| ProbeFailure::Budget)?;
        state.available += self.bytes - spent;
        state.pending -= self.bytes;
        drop(state);
        self.budget.0.changed.notify_waiters();
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.settled {
            // A failed/cancelled HTTP future does not expose transferred byte
            // counts. Charge its complete reservation conservatively.
            let _ = self.settle(self.bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn late_rendition_can_reserve_samples_after_earlier_sources_use_four_mib()
    -> ProbeResult<()> {
        let budget = ByteBudget::new(16 * 1024 * 1024);
        let cancellation = CancellationToken::new();

        budget
            .reserve(4 * 1024 * 1024, true, &cancellation)
            .await?
            .finish(4 * 1024 * 1024)?;

        for _ in 0..3 {
            budget
                .reserve(128 * 1024, true, &cancellation)
                .await?
                .finish(128 * 1024)?;
        }
        Ok(())
    }

    struct NoTransport;

    #[async_trait::async_trait]
    impl super::super::ProbeTransport for NoTransport {
        async fn fetch(
            &self,
            _request: crate::network::FetchRequest,
            _cancellation: &CancellationToken,
        ) -> Result<crate::network::FetchResponse, crate::network::TransportError> {
            Err(crate::network::TransportError::Cancelled)
        }
    }

    #[test]
    fn probe_limits_accept_sixteen_mib_but_keep_an_explicit_upper_bound() {
        let transport = Arc::new(NoTransport);
        assert!(
            super::super::StreamProber::new(
                transport.clone(),
                super::super::ProbeLimits {
                    total_sample_bytes: 16 * 1024 * 1024,
                    ..super::super::ProbeLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            super::super::StreamProber::new(
                transport.clone(),
                super::super::ProbeLimits {
                    total_sample_bytes: 16 * 1024 * 1024 + 1,
                    ..super::super::ProbeLimits::default()
                }
            )
            .is_err()
        );
        let prober = super::super::StreamProber::new(
            transport,
            super::super::ProbeLimits::default(),
        )
        .unwrap();
        let health = prober.health_probe();
        assert_eq!(health.limits.total_playlist_bytes, 2 * 1024 * 1024);
    }
}
