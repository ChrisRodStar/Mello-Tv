use std::io::{self, Read};

/// Only the native app opts in. A terminal, container, or service may legitimately
/// launch with closed stdin and must continue running.
pub fn watch_stdin(enabled: bool) -> io::Result<impl Future<Output = ()>> {
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    if enabled {
        // Tokio's stdin uses a blocking runtime task which can prevent shutdown
        // while the owner is still alive. A dedicated OS thread does not keep the
        // runtime or process alive during ordinary SIGTERM shutdown.
        std::thread::Builder::new()
            .name("app-lifetime".into())
            .spawn(move || {
                let mut stdin = io::stdin().lock();
                let mut buffer = [0; 64];
                loop {
                    match stdin.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            tracing::warn!(%error, "app lifetime pipe failed");
                            break;
                        }
                    }
                }
                let _ = closed_tx.send(());
            })?;
    }
    Ok(async move {
        if enabled {
            let _ = closed_rx.await;
        } else {
            std::future::pending::<()>().await;
        }
    })
}
