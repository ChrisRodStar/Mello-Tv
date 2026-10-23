//! Opt-in conservative receive quota. Normal production traffic has no ledger.
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{
    io,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::sync::CancellationToken;
pub const READ_RESERVATION: usize = 16 * 1024;
#[derive(Clone)]
pub struct Gate {
    database: Arc<Mutex<Connection>>,
    pub(crate) stop: CancellationToken,
    deadline: tokio::time::Instant,
    request_quota: bool,
}
fn rejected() -> io::Error {
    io::Error::other("acceptance_limit")
}
fn now_ms() -> io::Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| rejected())?
            .as_millis(),
    )
    .map_err(|_| rejected())
}
impl Gate {
    /// Existing ledger only: startup must never recreate or replenish a run.
    pub fn open(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path)?;
        let parent = std::fs::metadata(path.parent().ok_or_else(rejected)?)?;
        if !metadata.is_file()
            || metadata.mode() & 0o777 != 0o600
            || !parent.is_dir()
            || parent.mode() & 0o777 != 0o700
        {
            return Err(rejected());
        }
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_| rejected())?;
        db.busy_timeout(Duration::from_millis(100))
            .map_err(|_| rejected())?;
        db.pragma_update(None, "synchronous", "FULL")
            .map_err(|_| rejected())?;
        let (limit, remaining, deadline, stopped): (i64, i64, i64, i64) = db
            .query_row(
                "SELECT byte_limit, remaining, deadline_ms, stopped FROM acceptance WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|_| rejected())?;
        let delta = deadline
            .checked_sub(now_ms()?)
            .filter(|n| *n > 0)
            .ok_or_else(rejected)?;
        if !(1..=9_007_199_254_740_992).contains(&limit)
            || remaining <= 0
            || remaining > limit
            || stopped != 0
            || delta > 86_400_000
        {
            return Err(rejected());
        }
        let request_quota: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='acceptance_requests')",
            [], |row| row.get(0)).map_err(|_| rejected())?;
        if request_quota {
            let (limit, remaining): (i64, i64) = db
                .query_row(
                    "SELECT request_limit, remaining FROM acceptance_requests WHERE id=1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| rejected())?;
            if !(1..=100_000).contains(&limit) || !(1..=limit).contains(&remaining) {
                return Err(rejected());
            }
        }
        Ok(Self {
            request_quota,
            database: Arc::new(Mutex::new(db)),
            stop: CancellationToken::new(),
            deadline: tokio::time::Instant::now() + Duration::from_millis(delta as u64),
        })
    }
    pub(crate) async fn stopped(&self) {
        tokio::select! { _ = self.stop.cancelled() => {}, _ = tokio::time::sleep_until(self.deadline) => { self.stop.cancel(); } }
    }
    fn reserve(&self, wanted: usize) -> io::Result<usize> {
        let result = self.reserve_inner(wanted);
        if result.is_err() {
            self.stop.cancel();
        }
        result
    }
    fn reserve_inner(&self, wanted: usize) -> io::Result<usize> {
        if self.stop.is_cancelled() || tokio::time::Instant::now() >= self.deadline {
            return Err(rejected());
        }
        let mut db = self.database.lock().map_err(|_| rejected())?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| rejected())?;
        let (limit, remaining, deadline, stopped): (i64, i64, i64, i64) = tx
            .query_row(
                "SELECT byte_limit, remaining, deadline_ms, stopped FROM acceptance WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|_| rejected())?;
        if !(1..=9_007_199_254_740_992).contains(&limit)
            || remaining < 0
            || remaining > limit
            || !matches!(stopped, 0 | 1)
        {
            return Err(rejected());
        }
        let n = remaining.min(wanted.min(READ_RESERVATION) as i64).max(0);
        let expired = now_ms()? >= deadline || tokio::time::Instant::now() >= self.deadline;
        let terminal = stopped != 0 || expired || n == remaining || n == 0;
        tx.execute(
            "UPDATE acceptance SET remaining=?1, stopped=?2 WHERE id=1",
            rusqlite::params![
                if expired || stopped != 0 {
                    remaining
                } else {
                    remaining - n
                },
                i64::from(terminal)
            ],
        )
        .map_err(|_| rejected())?;
        tx.commit().map_err(|_| rejected())?;
        if terminal {
            return Err(rejected());
        }
        Ok(n as usize)
    }
    /// Independently bound HTTP exchanges (including DoH/redirects/retries).
    /// Old byte-only ledgers remain compatible but make no request-count claim.
    pub(crate) fn request(&self) -> io::Result<()> {
        if !self.request_quota {
            return Ok(());
        }
        let result = (|| {
            if self.stop.is_cancelled() || tokio::time::Instant::now() >= self.deadline {
                return Err(rejected());
            }
            let mut db = self.database.lock().map_err(|_| rejected())?;
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| rejected())?;
            let (remaining, deadline, stopped): (i64, i64, i64) = tx.query_row(
                "SELECT r.remaining, a.deadline_ms, a.stopped FROM acceptance_requests r JOIN acceptance a ON a.id=r.id WHERE r.id=1",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(|_| rejected())?;
            if remaining <= 0 || stopped != 0 || now_ms()? >= deadline {
                return Err(rejected());
            }
            tx.execute(
                "UPDATE acceptance_requests SET remaining=remaining-1 WHERE id=1",
                [],
            )
            .map_err(|_| rejected())?;
            tx.commit().map_err(|_| rejected())?;
            Ok(())
        })();
        if result.is_err() {
            self.stop.cancel();
        }
        result
    }
    /// Charge discarded kernel read-ahead on every connection attempt, including
    /// retries. A concurrency-only allowance would miss buffers lost on close.
    pub(crate) fn reserve_connection(&self) -> io::Result<()> {
        self.reserve(READ_RESERVATION)?;
        self.reserve(READ_RESERVATION)?;
        Ok(())
    }
    pub(crate) fn wrap<T>(&self, inner: T) -> Metered<T> {
        let gate = self.clone();
        Metered {
            inner,
            gate: self.clone(),
            credit: 0,
            stopped: false,
            stop: Box::pin(async move { gate.stopped().await }),
        }
    }
}
pub(crate) struct Metered<T> {
    inner: T,
    gate: Gate,
    credit: usize,
    stopped: bool,
    stop: Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
}
impl<T> Metered<T> {
    fn shut_off(&mut self, cx: &mut Context<'_>) -> bool {
        if !self.stopped {
            self.stopped = self.stop.as_mut().poll(cx).is_ready();
        }
        self.stopped
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for Metered<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.shut_off(cx) {
            return Poll::Ready(Err(rejected()));
        }
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if this.credit == 0 {
            match this.gate.reserve(buf.remaining()) {
                Ok(n) => this.credit = n,
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
        // Reserve before touching the socket; keep credit across Pending. Never refund
        // on close/crash: no restart or concurrent reader can reuse those bytes.
        if this.gate.stop.is_cancelled() || tokio::time::Instant::now() >= this.gate.deadline {
            return Poll::Ready(Err(rejected()));
        }
        let limit = this.credit.min(buf.remaining());
        let mut bounded = ReadBuf::new(buf.initialize_unfilled_to(limit));
        match Pin::new(&mut this.inner).poll_read(cx, &mut bounded) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {
                let n = bounded.filled().len();
                this.credit -= n;
                buf.advance(n);
                Poll::Ready(Ok(()))
            }
        }
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for Metered<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.shut_off(cx) {
            return Poll::Ready(Err(rejected()));
        }
        Pin::new(&mut this.inner).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
#[cfg(test)]
pub(crate) mod tests;
