use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

mod owner_lifetime;

use stream_core::ports::MediaEngine;
use stream_media::DiscoveryCoordinator;
use stream_server::{
    catalog_service::CatalogWorker,
    jellyfin::{JellyfinBridge, JellyfinConfig},
    routes,
    state::{AppState, NoopMediaEngine},
};
use stream_storage::{AccountStore, CatalogStore};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub db_path: PathBuf,
    pub static_dir: Option<PathBuf>,
    pub exit_on_stdin_close: bool,
    pub validate_data: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("failed to parse port number: {0}")]
    InvalidPort(#[from] std::num::ParseIntError),
}

impl ServerConfig {
    pub fn parse() -> Result<Self, ConfigError> {
        let mut host = std::env::var("STREAM_HOST")
            .or_else(|_| std::env::var("HOST"))
            .unwrap_or_else(|_| "0.0.0.0".to_string());

        let mut port = std::env::var("STREAM_PORT")
            .or_else(|_| std::env::var("PORT"))
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(8080);

        let mut db_path = std::env::var("STREAM_DB_PATH")
            .or_else(|_| std::env::var("DB_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data"));

        let mut static_dir = std::env::var("STREAM_STATIC_DIR")
            .or_else(|_| std::env::var("STATIC_DIR"))
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                let default_web = PathBuf::from("web/dist");
                if default_web.is_dir() {
                    Some(default_web)
                } else {
                    None
                }
            });

        let mut exit_on_stdin_close = false;
        let mut validate_data = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    println!("Usage: stream-server [OPTIONS]");
                    println!();
                    println!("Options:");
                    println!("  -p, --port <PORT>          Port to listen on [default: 8080]");
                    println!(
                        "  -d, --db-path <PATH>       Database directory path [default: data]"
                    );
                    println!("      --static-dir <PATH>    Static web assets directory");
                    println!(
                        "      --host <HOST>          Host address to bind to [default: 0.0.0.0]"
                    );
                    println!("      --exit-on-stdin-close  Shut down when the launching app exits");
                    println!(
                        "      --validate-data        Validate databases and encryption key, then exit"
                    );
                    println!("  -h, --help                 Print help");
                    std::process::exit(0);
                }
                "--exit-on-stdin-close" => exit_on_stdin_close = true,
                "--validate-data" => validate_data = true,
                "--port" | "-p" => {
                    if let Some(val) = args.next() {
                        port = val.parse::<u16>()?;
                    } else {
                        return Err(ConfigError::InvalidArgument(
                            "missing value for --port".into(),
                        ));
                    }
                }
                arg if arg.starts_with("--port=") => {
                    let val = arg.trim_start_matches("--port=");
                    port = val.parse::<u16>()?;
                }
                "--db-path" | "-d" => {
                    if let Some(val) = args.next() {
                        db_path = PathBuf::from(val);
                    } else {
                        return Err(ConfigError::InvalidArgument(
                            "missing value for --db-path".into(),
                        ));
                    }
                }
                arg if arg.starts_with("--db-path=") => {
                    let val = arg.trim_start_matches("--db-path=");
                    db_path = PathBuf::from(val);
                }
                "--host" => {
                    if let Some(val) = args.next() {
                        host = val;
                    } else {
                        return Err(ConfigError::InvalidArgument(
                            "missing value for --host".into(),
                        ));
                    }
                }
                arg if arg.starts_with("--host=") => {
                    host = arg.trim_start_matches("--host=").to_string();
                }
                "--static-dir" => {
                    if let Some(val) = args.next() {
                        static_dir = Some(PathBuf::from(val));
                    } else {
                        return Err(ConfigError::InvalidArgument(
                            "missing value for --static-dir".into(),
                        ));
                    }
                }
                arg if arg.starts_with("--static-dir=") => {
                    static_dir = Some(PathBuf::from(arg.trim_start_matches("--static-dir=")));
                }
                other => {
                    return Err(ConfigError::InvalidArgument(format!(
                        "unknown option: {other}"
                    )));
                }
            }
        }

        Ok(Self {
            host,
            port,
            db_path,
            static_dir,
            exit_on_stdin_close,
            validate_data,
        })
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut stream) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            stream.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received SIGINT/Ctrl+C, initiating graceful shutdown");
        }
        _ = terminate => {
            tracing::info!("received SIGTERM, initiating graceful shutdown");
        }
    }
}

fn bind_ipv4_listener(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    let _ = socket.set_reuse_address(true);
    socket.set_nonblocking(true)?;
    let addr: std::net::SocketAddr = (std::net::Ipv4Addr::UNSPECIFIED, port).into();
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    let std_listener: std::net::TcpListener = socket.into();
    tokio::net::TcpListener::from_std(std_listener)
}

fn bind_ipv6_listener(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    let _ = socket.set_only_v6(true);
    let _ = socket.set_reuse_address(true);
    socket.set_nonblocking(true)?;
    let addr: std::net::SocketAddr = (std::net::Ipv6Addr::UNSPECIFIED, port).into();
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    let std_listener: std::net::TcpListener = socket.into();
    tokio::net::TcpListener::from_std(std_listener)
}

/// Shared with the Mac restore coordinator's exclusive flock. The handle stays
/// alive until all server work has shut down; a restore never replaces live files.
fn lock_household_directory(directory: &std::path::Path) -> std::io::Result<std::fs::File> {
    let path = directory.join(".household.lock");
    if let Ok(metadata) = std::fs::symlink_metadata(&path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::other(
            "Household lock must be a regular file",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.try_lock_shared().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => std::io::Error::other(
            "Household data is being restored. Try again after restore finishes.",
        ),
        std::fs::TryLockError::Error(error) => error,
    })?;
    if directory.join(".restore-in-progress").exists() {
        return Err(std::io::Error::other(
            "An interrupted household restore needs recovery in the Mac app before the server can start.",
        ));
    }
    Ok(file)
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,stream_server=debug")),
        )
        .try_init();

    let config = ServerConfig::parse()?;
    let jellyfin_config = if config.validate_data {
        None
    } else {
        JellyfinConfig::from_env().map_err(std::io::Error::other)?
    };
    let owner_closed = owner_lifetime::watch_stdin(config.exit_on_stdin_close)?;
    tracing::info!(
        host = %config.host,
        port = config.port,
        db_path = %config.db_path.display(),
        "loaded server configuration"
    );

    if config.validate_data
        && (!config.db_path.join("accounts.db").is_file()
            || !config.db_path.join("catalog.db").is_file()
            || std::fs::metadata(config.db_path.join("api-encryption"))?.len() != 32)
    {
        return Err(std::io::Error::other(
            "A complete household backup and matching encryption key are required",
        )
        .into());
    }
    tokio::fs::create_dir_all(&config.db_path).await?;
    let _household_lock = lock_household_directory(&config.db_path)?;

    let catalog_db = config.db_path.join("catalog.db");
    let catalog = CatalogStore::open(catalog_db).await?;

    let key = if let Ok(bytes) = tokio::fs::read(config.db_path.join("api-encryption")).await {
        if bytes.len() == 32 {
            let mut k = [0u8; 32];
            k.copy_from_slice(&bytes);
            tracing::info!("loaded custom encryption key from api-encryption");
            k
        } else {
            [0x5cu8; 32]
        }
    } else {
        [0x5cu8; 32]
    };

    let accounts_db = config.db_path.join("accounts.db");
    let accounts = AccountStore::open(accounts_db, key).await?;

    if config.validate_data {
        accounts.shutdown().await?;
        catalog.shutdown().await?;
        return Ok(());
    }

    let household_id = accounts.select_household(None).await?;
    tracing::info!(%household_id, "household account selected");

    let media_shutdown = tokio_util::sync::CancellationToken::new();
    let _cancel_media_on_exit = media_shutdown.clone().drop_guard();
    let media: Arc<dyn MediaEngine> = match DiscoveryCoordinator::default_live(
        media_shutdown.clone(),
    ) {
        Ok(coordinator) => {
            tracing::info!("initialized live media discovery engine with 5 native providers");
            Arc::new(coordinator)
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to initialize live media engine, falling back to noop");
            Arc::new(NoopMediaEngine)
        }
    };

    let catalog = Arc::new(catalog);
    let catalog_worker = match CatalogWorker::from_env(catalog.clone()) {
        Ok(worker) => Some(Arc::new(worker)),
        Err(error) => {
            tracing::warn!(error = %error, "catalog worker unavailable");
            None
        }
    };
    let jellyfin = jellyfin_config.map(|config| Arc::new(JellyfinBridge::new(config)));
    let state = AppState::new(catalog, Arc::new(accounts), media, household_id)
        .with_catalog_worker(catalog_worker)
        .with_port(config.port)
        .with_jellyfin(jellyfin);

    stream_server::discovery::spawn_discovery_services(
        state.household_account_id.clone(),
        config.port,
    );

    let router = routes::build_router_with_static(state.clone(), config.static_dir.clone());

    let is_dual_stack = config.host == "0.0.0.0" || config.host == "::";
    let mut listeners = Vec::new();

    if is_dual_stack {
        match bind_ipv4_listener(config.port) {
            Ok(listener) => {
                tracing::info!(
                    port = config.port,
                    "bound dual-stack IPv4 listener on 0.0.0.0:{}",
                    config.port
                );
                listeners.push(listener);
            }
            Err(err) => {
                tracing::warn!(
                    port = config.port,
                    error = %err,
                    "failed to bind IPv4 listener on 0.0.0.0"
                );
            }
        }

        match bind_ipv6_listener(config.port) {
            Ok(listener) => {
                tracing::info!(
                    port = config.port,
                    "bound dual-stack IPv6 listener on [::]:{}",
                    config.port
                );
                listeners.push(listener);
            }
            Err(err) => {
                tracing::warn!(
                    port = config.port,
                    error = %err,
                    "failed to bind IPv6 listener on [::]"
                );
            }
        }

        if listeners.is_empty() {
            return Err(format!(
                "failed to bind either IPv4 or IPv6 listener on port {}",
                config.port
            )
            .into());
        }
    } else {
        let addr = format!("{}:{}", config.host, config.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!(%addr, "stream HTTP API daemon listening");
        listeners.push(listener);
    }

    let jellyfin_task = state.jellyfin.as_ref().map(|bridge| {
        tracing::info!(profile_id = %bridge.config.profile_id, library_dir = %bridge.config.library_dir.display(), "Jellyfin library export enabled");
        let bridge = bridge.clone();
        let state = state.clone();
        let shutdown = media_shutdown.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        match bridge.sync(&state).await {
                            Ok(report) if report.written > 0 || report.removed > 0 => {
                                tracing::info!(files = report.files, written = report.written, removed = report.removed, "Jellyfin library export synchronized");
                            }
                            Ok(_) => tracing::debug!("Jellyfin library export is current"),
                            Err(error) => tracing::warn!(%error, "Jellyfin library export failed; retrying in 60 seconds"),
                        }
                    }
                }
            }
        })
    });

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
    let mut serve_handles = Vec::new();

    for listener in listeners {
        let local_addr = listener.local_addr().ok();
        let router = router.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        serve_handles.push(tokio::spawn(async move {
            let res = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.recv().await;
                })
                .await;
            (local_addr, res)
        }));
    }

    tokio::select! {
        _ = shutdown_signal() => {
            tracing::info!("shutdown signal received, initiating graceful shutdown");
        }
        _ = owner_closed => {
            tracing::info!("launching app disconnected, initiating graceful shutdown");
        }
    }
    media_shutdown.cancel();
    if let Some(task) = jellyfin_task {
        task.abort();
        let _ = task.await;
    }
    let _ = shutdown_tx.send(());

    // An active request must not keep the app-owned server alive indefinitely.
    let drain_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    for mut handle in serve_handles {
        let result = if config.exit_on_stdin_close {
            match tokio::time::timeout_at(drain_deadline, &mut handle).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::info!("closing remaining requests after app-owned shutdown deadline");
                    handle.abort();
                    let _ = handle.await;
                    continue;
                }
            }
        } else {
            handle.await
        };
        match result {
            Ok((addr, Ok(()))) => {
                tracing::debug!(?addr, "listener shutdown cleanly");
            }
            Ok((addr, Err(e))) => {
                tracing::error!(?addr, error = %e, "listener error");
            }
            Err(e) => {
                tracing::error!(error = %e, "serve task join error");
            }
        }
    }

    tracing::info!("flushing storage repositories on shutdown");
    let _ = state.catalog.shutdown().await;
    let _ = state.accounts.shutdown().await;
    tracing::info!("shutdown complete");

    Ok(())
}

fn main() -> ExitCode {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to build tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    match rt.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("server error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ipv4_and_ipv6_listeners_share_a_port() -> Result<(), Box<dyn std::error::Error>> {
        let ipv4 = bind_ipv4_listener(0)?;
        let port = ipv4.local_addr()?.port();
        let ipv6 = bind_ipv6_listener(port)?;
        assert_eq!(ipv6.local_addr()?.port(), port);
        assert!(ipv4.local_addr()?.is_ipv4());
        assert!(ipv6.local_addr()?.is_ipv6());
        Ok(())
    }
}
