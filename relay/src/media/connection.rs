//! Connection-level lease enforcement continues when downstream stops polling.
use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
    time::{Instant, Sleep},
};
use tokio_util::sync::CancellationToken;
#[derive(Clone, Default)]
pub struct ConnectionLease {
    inner: Arc<Mutex<Option<(CancellationToken, CancellationToken, Instant)>>>,
    changed: Arc<tokio::sync::Notify>,
}
impl ConnectionLease {
    pub fn set(&self, cancel: CancellationToken, guard: CancellationToken, deadline: Instant) {
        if let Ok(mut lease) = self.inner.lock() {
            *lease = Some((cancel, guard, deadline));
        }
        self.changed.notify_one();
    }
    pub async fn ended(&self) {
        loop {
            let notified = self.changed.notified();
            let lease = self.inner.lock().ok().and_then(|l| l.clone());
            if let Some((cancel, guard, deadline)) = lease {
                tokio::select! {_=cancel.cancelled()=>{},_=guard.cancelled()=>{},_=tokio::time::sleep_until(deadline)=>{}}
                crate::health::failure("stream_cancelled");
                return;
            }
            notified.await;
        }
    }
}
pub struct TimedSocket {
    socket: TcpStream,
    idle: Duration,
    blocked: Option<Pin<Box<Sleep>>>,
}
impl TimedSocket {
    pub fn new(socket: TcpStream, idle: Duration) -> Self {
        Self {
            socket,
            idle,
            blocked: None,
        }
    }
}
impl AsyncRead for TimedSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, buf)
    }
}
impl AsyncWrite for TimedSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.socket).poll_write(cx, buf) {
            Poll::Pending => {
                let idle = self.idle;
                let timer = self
                    .blocked
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(idle)));
                if timer.as_mut().poll(cx).is_ready() {
                    crate::health::failure("downstream_timeout");
                    Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "write_idle")))
                } else {
                    Poll::Pending
                }
            }
            result => {
                self.blocked = None;
                result
            }
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}
