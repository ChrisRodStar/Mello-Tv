//! Finite HTTP/1 connection owners, explicit header deadlines and joined drain.
use axum::Router;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{io, time::Duration};
use tokio::{net::TcpListener, task::JoinSet};
use tokio_util::sync::CancellationToken;

pub async fn listener(
    listener: TcpListener,
    router: Router,
    cancel: CancellationToken,
    max_connections: usize,
    drain: Duration,
) -> io::Result<()> {
    listener_with_idle(
        listener,
        router,
        cancel,
        max_connections,
        drain,
        Duration::from_secs(30),
    )
    .await
}
pub(crate) async fn listener_with_idle(
    listener: TcpListener,
    router: Router,
    cancel: CancellationToken,
    max_connections: usize,
    drain: Duration,
    write_idle: Duration,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    let outcome = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Ok(()),
            result = connections.join_next(), if !connections.is_empty() => {
                if result.is_some_and(|r| r.is_err()) { break Err(io::Error::other("connection_task_failed")); }
            }
            accepted = listener.accept() => {
                let (socket, _) = match accepted { Ok(socket) => socket, Err(error) => break Err(error) };
                if connections.len() >= max_connections { drop(socket); continue; }
                let lease = crate::media::connection::ConnectionLease::default();
                let service = TowerToHyperService::new(router.clone().layer(axum::Extension(lease.clone())));
                let shutdown = cancel.clone();
                connections.spawn(async move {
                    let mut builder = hyper::server::conn::http1::Builder::new();
                    builder.timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(5))
                        .max_headers(64).max_buf_size(32768).keep_alive(false);
                    let connection = builder.serve_connection(TokioIo::new(crate::media::connection::TimedSocket::new(socket, write_idle)), service);
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {},
                        _ = lease.ended() => {},
                        _ = shutdown.cancelled() => {
                            connection.as_mut().graceful_shutdown();
                            tokio::select! { _ = &mut connection => {}, _ = lease.ended() => {} }
                        }
                    }
                });
            }
        }
    };
    cancel.cancel();
    if tokio::time::timeout(drain, async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        crate::health::event(&crate::health::FORCED_DRAINS);
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
    outcome
}

pub async fn run(
    control: TcpListener,
    stream: TcpListener,
    app: crate::control::App,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let drain = Duration::from_secs(app.config.lifecycle.drain_timeout_s);
    let control_limit = app.config.limits.max_control_concurrency_count as usize * 2;
    let stream_limit = app.config.network.max_streams_count as usize;
    let mut listeners = JoinSet::new();
    listeners.spawn(listener(
        control,
        crate::control::control(app.clone()),
        shutdown.clone(),
        control_limit,
        drain,
    ));
    listeners.spawn(listener_with_idle(
        stream,
        crate::control::streaming_delivery(
            app.playback.clone(),
            app.store
                .clone()
                .map(|s| {
                    std::sync::Arc::new(s) as std::sync::Arc<dyn crate::session::AuthorizationStore>
                })
                .unwrap_or_else(|| std::sync::Arc::new(crate::session::Unavailable)),
            app.delivery.clone(),
        ),
        shutdown.clone(),
        stream_limit,
        drain,
        Duration::from_secs(app.config.network.write_idle_timeout_s),
    ));
    let lifecycle = app.lifecycle.clone();
    let outcome = supervise(listeners, shutdown, Some(lifecycle.clone())).await;
    if outcome.is_err() {
        lifecycle
            .failed
            .store(true, std::sync::atomic::Ordering::Release);
    }
    outcome
}

async fn supervise(
    mut listeners: JoinSet<io::Result<()>>,
    shutdown: CancellationToken,
    lifecycle: Option<std::sync::Arc<crate::health::Lifecycle>>,
) -> io::Result<()> {
    let mut failure = tokio::select! {
        biased;
        _ = shutdown.cancelled() => false,
        _ = listeners.join_next() => true,
    };
    if let Some(state) = lifecycle {
        if failure {
            state
                .failed
                .store(true, std::sync::atomic::Ordering::Release);
        }
        state
            .draining
            .store(true, std::sync::atomic::Ordering::Release);
    }
    shutdown.cancel();
    while let Some(result) = listeners.join_next().await {
        if !matches!(result, Ok(Ok(()))) {
            failure = true;
        }
    }
    if failure {
        Err(io::Error::other("listener_stopped"))
    } else {
        Ok(())
    }
}

/// Tokens for the ordered listener -> transport -> store shutdown phases.
pub struct Infrastructure {
    pub listeners: CancellationToken,
    pub network: CancellationToken,
    pub storage: CancellationToken,
    pub lifecycle: std::sync::Arc<crate::health::Lifecycle>,
}
/// The daemon's owner also watches infrastructure failures throughout active drain.
/// A signal stops listener admission first; infrastructure is retained until all
/// accepted connections finish or the listener's finite drain deadline expires.
pub async fn own(
    service: impl std::future::Future<Output = io::Result<()>>,
    network: impl std::future::Future<Output = ()>,
    storage: impl std::future::Future<Output = Result<(), crate::error::Error>>,
    signal: impl std::future::Future<Output = Result<(), crate::error::Error>>,
    state: Infrastructure,
) -> Result<(), crate::error::Error> {
    use crate::error::CONFIG;
    use std::sync::atomic::Ordering::Release;
    tokio::pin!(service, network, storage, signal);
    let mut network_done = false;
    let mut storage_done = false;
    let mut draining = false;
    let mut failure = false;
    let result = loop {
        tokio::select! {
            result = &mut service => break result,
            _ = &mut network, if !network_done => {
                network_done = true; failure = true; draining = true;
                state.lifecycle.failed.store(true, Release);
                state.lifecycle.draining.store(true, Release);
                state.listeners.cancel();
            },
            _ = &mut storage, if !storage_done => {
                storage_done = true; failure = true; draining = true;
                state.lifecycle.failed.store(true, Release);
                state.lifecycle.draining.store(true, Release);
                state.network.cancel(); state.listeners.cancel();
            },
            signal = &mut signal, if !draining => {
                failure |= signal.is_err(); draining = true;
                state.lifecycle.draining.store(true, Release);
                state.listeners.cancel();
            },
        }
    };
    // A listener completion and an infrastructure failure can become ready on
    // the same tick. Inspect both owners before issuing their stop signals so
    // ordinary shutdown cannot hide an already-completed failed task.
    if !network_done {
        tokio::select! { biased;
            _ = &mut network => { network_done = true; failure = true; },
            _ = std::future::ready(()) => {},
        }
    }
    if !storage_done {
        tokio::select! { biased;
            _ = &mut storage => { storage_done = true; failure = true; },
            _ = std::future::ready(()) => {},
        }
    }
    state.lifecycle.draining.store(true, Release);
    state.listeners.cancel();
    state.network.cancel();
    if !network_done {
        network.await;
    }
    state.storage.cancel();
    if !storage_done && storage.await.is_err() {
        failure = true;
    }
    if failure || result.is_err() {
        state.lifecycle.failed.store(true, Release);
        return Err(CONFIG);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn simultaneous_cancellation_and_success_are_clean() {
        let shutdown = CancellationToken::new();
        let mut listeners = JoinSet::new();
        listeners.spawn(async { Ok(()) });
        tokio::task::yield_now().await;
        shutdown.cancel();
        assert!(supervise(listeners, shutdown, None).await.is_ok());
    }

    #[tokio::test]
    async fn simultaneous_cancellation_does_not_hide_listener_errors() {
        let shutdown = CancellationToken::new();
        let mut listeners = JoinSet::new();
        listeners.spawn(async { Err(io::Error::other("synthetic_listener_failure")) });
        tokio::task::yield_now().await;
        shutdown.cancel();
        assert!(supervise(listeners, shutdown, None).await.is_err());
    }
}

#[cfg(test)]
mod infrastructure_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::SeqCst},
    };
    #[tokio::test]
    async fn owner_preserves_infrastructure_until_listener_drain_then_joins_in_order() {
        let stop = CancellationToken::new();
        let net = CancellationToken::new();
        let storage = CancellationToken::new();
        let phase = Arc::new(AtomicUsize::new(0));
        let lifecycle = Arc::new(crate::health::Lifecycle::default());
        let service = async {
            stop.cancelled().await;
            assert!(!net.is_cancelled());
            assert!(!storage.is_cancelled());
            tokio::time::sleep(Duration::from_millis(30)).await;
            assert!(!net.is_cancelled());
            assert!(!storage.is_cancelled());
            phase.store(1, SeqCst);
            Ok(())
        };
        let network = async {
            net.cancelled().await;
            assert_eq!(phase.load(SeqCst), 1);
            assert!(!storage.is_cancelled());
            phase.store(2, SeqCst);
        };
        let store = async {
            storage.cancelled().await;
            assert_eq!(phase.load(SeqCst), 2);
            phase.store(3, SeqCst);
            Ok(())
        };
        own(
            service,
            network,
            store,
            async { Ok(()) },
            Infrastructure {
                listeners: stop.clone(),
                network: net.clone(),
                storage: storage.clone(),
                lifecycle: lifecycle.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(phase.load(SeqCst), 3);
        assert!(lifecycle.draining.load(SeqCst));
        assert!(!lifecycle.failed.load(SeqCst));
    }
    #[tokio::test]
    async fn owner_detects_infrastructure_failure_during_drain_and_marks_liveness_failed() {
        for store_fails in [false, true] {
            let stop = CancellationToken::new();
            let net = CancellationToken::new();
            let storage = CancellationToken::new();
            let lifecycle = Arc::new(crate::health::Lifecycle::default());
            let service = async {
                stop.cancelled().await;
                tokio::time::sleep(Duration::from_millis(60)).await;
                Ok(())
            };
            let network = async {
                if store_fails {
                    net.cancelled().await;
                } else {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            };
            let store = async {
                if store_fails {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    Err(crate::error::STORE)
                } else {
                    storage.cancelled().await;
                    Ok(())
                }
            };
            assert!(
                own(
                    service,
                    network,
                    store,
                    async { Ok(()) },
                    Infrastructure {
                        listeners: stop.clone(),
                        network: net.clone(),
                        storage: storage.clone(),
                        lifecycle: lifecycle.clone()
                    }
                )
                .await
                .is_err()
            );
            assert!(lifecycle.failed.load(SeqCst));
        }
    }
}

#[cfg(test)]
mod simultaneous_owner_tests {
    use super::*;
    #[tokio::test]
    async fn simultaneous_service_completion_cannot_hide_an_infrastructure_exit() {
        let lifecycle = std::sync::Arc::new(crate::health::Lifecycle::default());
        let storage = CancellationToken::new();
        assert!(
            own(
                async { Ok(()) },
                async {},
                async {
                    storage.cancelled().await;
                    Ok(())
                },
                std::future::pending(),
                Infrastructure {
                    listeners: CancellationToken::new(),
                    network: CancellationToken::new(),
                    storage: storage.clone(),
                    lifecycle: lifecycle.clone()
                }
            )
            .await
            .is_err()
        );
        assert!(lifecycle.failed.load(std::sync::atomic::Ordering::Acquire));
    }
}
