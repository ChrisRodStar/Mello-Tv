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
