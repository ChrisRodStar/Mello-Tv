//! Idle HTTP/1 connections are returned only after complete verified body EOF.
use super::*;
use tokio::sync::watch;
const IDLE: Duration = Duration::from_secs(15);

pub(super) struct Connection {
    pub sender: http1::SendRequest<Full<Bytes>>,
    pub authority: String,
    pub peer: IpAddr,
    pub epoch: Epoch,
    pub stop: CancellationToken,
    pub activity: watch::Sender<Instant>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
struct IdleConnection {
    connection: Connection,
    expires: Instant,
}
#[derive(Default)]
pub(super) struct Pool(Mutex<Vec<IdleConnection>>);
impl Pool {
    pub fn take(
        &self,
        authority: &str,
        addresses: &[IpAddr],
        epoch: &Epoch,
        deadline: Instant,
    ) -> Result<Option<Connection>> {
        let mut entries = self.0.lock().map_err(|_| NetError::Limit)?;
        entries.retain(|idle| {
            idle.expires > Instant::now()
                && !idle.connection.epoch.cancel.is_cancelled()
                && !idle.connection.stop.is_cancelled()
                && !idle.connection.sender.is_closed()
        });
        let position = entries.iter().position(|idle| {
            idle.connection.authority == authority
                && idle.connection.epoch.index == epoch.index
                && idle.connection.epoch.identity == epoch.identity
                && addresses.contains(&idle.connection.peer)
        });
        Ok(position.map(|position| {
            let connection = entries.swap_remove(position).connection;
            let _ = connection.activity.send(deadline);
            connection
        }))
    }
    fn put(&self, connection: Connection) {
        if connection.epoch.cancel.is_cancelled()
            || connection.stop.is_cancelled()
            || connection.sender.is_closed()
        {
            return;
        }
        if let Ok(mut entries) = self.0.lock() {
            entries.retain(|idle| {
                idle.expires > Instant::now()
                    && !idle.connection.stop.is_cancelled()
                    && !idle.connection.epoch.cancel.is_cancelled()
            });
            if entries.len() < 16
                && entries
                    .iter()
                    .filter(|idle| idle.connection.authority == connection.authority)
                    .count()
                    < 4
            {
                let _ = connection.activity.send(Instant::now() + IDLE);
                entries.push(IdleConnection {
                    connection,
                    expires: Instant::now() + IDLE,
                });
            }
        }
    }
}
pub(super) struct Lease {
    pub connection: Option<Connection>,
    pub pool: Arc<Pool>,
    pub reusable: bool,
}
impl Lease {
    pub fn complete(&mut self) {
        if let Some(connection) = self.connection.take()
            && self.reusable
        {
            self.pool.put(connection);
        }
    }
}

pub(super) async fn drive(
    connection: impl Future<Output = std::result::Result<(), hyper::Error>>,
    stop: CancellationToken,
    epoch: CancellationToken,
    mut activity: watch::Receiver<Instant>,
) {
    tokio::pin!(connection);
    let mut expiry = *activity.borrow_and_update();
    loop {
        tokio::select! { biased;
            _ = stop.cancelled() => break,
            _ = epoch.cancelled() => break,
            _ = tokio::time::sleep_until(expiry) => break,
            _ = &mut connection => break,
            changed = activity.changed() => {
                if changed.is_err() { break; }
                expiry = *activity.borrow_and_update();
            }
        }
    }
    stop.cancel();
}
