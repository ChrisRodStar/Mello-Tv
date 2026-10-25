//! A bounded, ordered SQLite actor. Each connection belongs to a dedicated
//! blocking thread, never to an async executor worker.

mod files;

use crate::StorageError;
use rusqlite::{Connection, OpenFlags};
use std::{path::Path, sync::Arc, thread, time::Duration};
use tokio::sync::{Mutex, mpsc, oneshot};

type Operation = Box<dyn FnOnce(&mut Connection) + Send + 'static>;
enum Message {
    Call(Operation),
    Shutdown(oneshot::Sender<()>),
}
struct Inner {
    admission: Mutex<Option<mpsc::Sender<Message>>>,
    closed: tokio::sync::watch::Receiver<bool>,
}

#[derive(Clone)]
pub struct Database {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub async fn open(path: impl AsRef<Path>, queue_capacity: usize) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(StorageError::InvalidInput(
                "use in_memory for ephemeral databases",
            ));
        }
        Self::start(queue_capacity, move || {
            files::prepare(&path)?;
            // Resolve normal OS aliases such as macOS /var -> /private/var
            // before asking SQLite to reject symlinks in the opened path.
            let path = std::fs::canonicalize(path)?;
            Ok(Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?)
        })
        .await
    }

    pub async fn in_memory(queue_capacity: usize) -> Result<Self, StorageError> {
        Self::start(queue_capacity, || Ok(Connection::open_in_memory()?)).await
    }

    async fn start(
        queue_capacity: usize,
        open: impl FnOnce() -> Result<Connection, StorageError> + Send + 'static,
    ) -> Result<Self, StorageError> {
        if !(1..=1024).contains(&queue_capacity) {
            return Err(StorageError::InvalidInput(
                "database queue capacity must be 1..=1024",
            ));
        }
        let (sender, mut receiver) = mpsc::channel::<Message>(queue_capacity);
        let (ready_tx, ready_rx) = oneshot::channel();
        let (closed_tx, closed_rx) = tokio::sync::watch::channel(false);
        thread::Builder::new()
            .name("stream-sqlite".into())
            .spawn(move || {
                let opened = open().and_then(|connection| {
                    connection.busy_timeout(Duration::from_secs(5))?;
                    connection.pragma_update(None, "foreign_keys", true)?;
                    connection.set_prepared_statement_cache_capacity(64);
                    Ok(connection)
                });
                let mut connection = match opened {
                    Ok(connection) => connection,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        closed_tx.send_replace(true);
                        return;
                    }
                };
                if ready_tx.send(Ok(())).is_err() {
                    closed_tx.send_replace(true);
                    return;
                }
                while let Some(message) = receiver.blocking_recv() {
                    match message {
                        Message::Call(operation) => operation(&mut connection),
                        Message::Shutdown(acknowledged) => {
                            drop(connection);
                            closed_tx.send_replace(true);
                            let _ = acknowledged.send(());
                            return;
                        }
                    }
                }
                drop(connection);
                closed_tx.send_replace(true);
            })?;
        ready_rx.await.map_err(|_| StorageError::Closed)??;
        Ok(Self {
            inner: Arc::new(Inner {
                admission: Mutex::new(Some(sender)),
                closed: closed_rx,
            }),
        })
    }

    /// Once admitted, work completes even if the caller disconnects. Durable
    /// mutations must recover their outcomes through repository idempotency.
    pub async fn call<T, F>(&self, operation: F) -> Result<T, StorageError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let message = Message::Call(Box::new(move |connection| {
            let result = operation(connection);
            let _ = result_tx.send(result);
        }));
        {
            let guard = self.inner.admission.lock().await;
            let sender = guard.as_ref().ok_or(StorageError::Closed)?;
            sender
                .send(message)
                .await
                .map_err(|_| StorageError::Closed)?;
        }
        result_rx.await.map_err(|_| StorageError::Closed)?
    }

    /// Closes admission for every clone, drains accepted work, and waits until
    /// SQLite releases file handles. Repeated/concurrent shutdown is supported.
    pub async fn shutdown(&self) -> Result<(), StorageError> {
        let acknowledgement = {
            let mut guard = self.inner.admission.lock().await;
            if let Some(sender) = guard.as_ref() {
                // Keep the sender until a slot is reserved. Cancellation while
                // waiting cannot strand the database in a half-closed state.
                let permit = sender.reserve().await.map_err(|_| StorageError::Closed)?;
                let (acknowledged, acknowledgement) = oneshot::channel();
                permit.send(Message::Shutdown(acknowledged));
                *guard = None;
                Some(acknowledgement)
            } else {
                None
            }
        };
        if let Some(acknowledgement) = acknowledgement {
            acknowledgement.await.map_err(|_| StorageError::Closed)?;
        } else {
            let mut closed = self.inner.closed.clone();
            if !*closed.borrow_and_update() {
                closed
                    .wait_for(|closed| *closed)
                    .await
                    .map_err(|_| StorageError::Closed)?;
            }
        }
        Ok(())
    }
}
