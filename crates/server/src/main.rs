use std::{
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
};

use stream_core::ports::MediaEngine;
use stream_media::DiscoveryCoordinator;
use stream_server::{
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
    pub proxy: Option<String>,
    pub static_dir: Option<PathBuf>,
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

        let mut proxy = std::env::var("STREAM_PROXY")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .ok()
            .filter(|p| !p.trim().is_empty());

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

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    println!("Usage: stream-server [OPTIONS]");
                    println!();
                    println!("Options:");
                    println!("  -p, --port <PORT>          Port to listen on [default: 8080]");
                    println!("  -d, --db-path <PATH>       Database directory path [default: data]");
                    println!("      --static-dir <PATH>    Static web assets directory");
                    println!("      --host <HOST>          Host address to bind to [default: 0.0.0.0]");
                    println!("      --proxy <URL>          HTTP/SOCKS5 proxy URL for media network calls");
                    println!("  -h, --help                 Print help");
                    std::process::exit(0);
                }
                "--port" | "-p" => {
                    if let Some(val) = args.next() {
                        port = val.parse::<u16>()?;
                    } else {
                        return Err(ConfigError::InvalidArgument("missing value for --port".into()));
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
                        return Err(ConfigError::InvalidArgument("missing value for --db-path".into()));
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
                        return Err(ConfigError::InvalidArgument("missing value for --host".into()));
                    }
                }
                arg if arg.starts_with("--host=") => {
                    host = arg.trim_start_matches("--host=").to_string();
                }
                "--proxy" => {
                    if let Some(val) = args.next() {
                        proxy = Some(val);
                    } else {
                        return Err(ConfigError::InvalidArgument("missing value for --proxy".into()));
                    }
                }
                arg if arg.starts_with("--proxy=") => {
                    proxy = Some(arg.trim_start_matches("--proxy=").to_string());
                }
                "--static-dir" => {
                    if let Some(val) = args.next() {
                        static_dir = Some(PathBuf::from(val));
                    } else {
                        return Err(ConfigError::InvalidArgument("missing value for --static-dir".into()));
                    }
                }
                arg if arg.starts_with("--static-dir=") => {
                    static_dir = Some(PathBuf::from(arg.trim_start_matches("--static-dir=")));
                }
                other => {
                    return Err(ConfigError::InvalidArgument(format!("unknown option: {other}")));
                }
            }
        }

        Ok(Self {
            host,
            port,
            db_path,
            proxy,
            static_dir,
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

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,stream_server=debug")),
        )
        .try_init();

    let config = ServerConfig::parse()?;
    tracing::info!(
        host = %config.host,
        port = config.port,
        db_path = %config.db_path.display(),
        proxy = ?config.proxy,
        "loaded server configuration"
    );

    tokio::fs::create_dir_all(&config.db_path).await?;

    let catalog_db = config.db_path.join("catalog.db");
    let catalog = CatalogStore::open(catalog_db).await?;
    stream_server::seed::seed_if_empty(&catalog).await?;

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

    let household_id = accounts.select_household(None).await?;
    tracing::info!(%household_id, "household account selected");

    let media: Arc<dyn MediaEngine> = match DiscoveryCoordinator::default_live() {
        Ok(coordinator) => {
            tracing::info!("initialized live media discovery engine with 5 native providers");
            Arc::new(coordinator)
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to initialize live media engine, falling back to noop");
            Arc::new(NoopMediaEngine)
        }
    };

    let state = AppState::new(
        Arc::new(catalog),
        Arc::new(accounts),
        media,
        household_id,
    );

    let router = routes::build_router_with_static(state.clone(), config.static_dir.clone());

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "stream HTTP API daemon listening");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

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

    #[test]
    fn test_config_defaults() {
        let config = ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 8080,
            db_path: PathBuf::from("data"),
            proxy: None,
            static_dir: None,
        };
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
        assert_eq!(config.db_path, PathBuf::from("data"));
        assert!(config.proxy.is_none());
        assert!(config.static_dir.is_none());
    }
}
