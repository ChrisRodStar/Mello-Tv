//! mello-jellyfin: resolves streams on demand for Jellyfin `.strm` files and
//! keeps a Jellyfin library folder in sync with a list of wanted titles.

mod app;
mod auth;
mod config;
mod library;
mod tmdb;

use std::{process::ExitCode, sync::Arc, time::Duration};

use stream_core::ports::MediaEngine;
use stream_media::{
    DiscoveryCoordinator, HttpClient, MediaRelay,
    network::{EgressMode, TransportConfig},
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::{
    app::{AppState, AppStateInit},
    auth::Keys,
    config::Config,
    library::Library,
    tmdb::Tmdb,
};

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutdown signal received");
}

fn relay_client(egress: EgressMode) -> Result<HttpClient, Box<dyn std::error::Error>> {
    Ok(HttpClient::with_config(TransportConfig {
        egress,
        max_in_flight: 128,
        max_queued: 512,
        max_origins: 512,
        max_response_bytes: 64 * 1024 * 1024,
        max_redirects: 5,
        request_timeout: Duration::from_secs(30),
        connect_timeout: Duration::from_secs(6),
        read_timeout: Duration::from_secs(30),
    })?)
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();

    let config = Config::load()?;
    tokio::fs::create_dir_all(&config.data_dir).await?;
    let keys = Keys::load(config.api_key.clone(), &config.data_dir).await?;

    if !config.public_url_configured {
        tracing::warn!(
            public_url = %config.public_url,
            "MELLO_PUBLIC_URL is not set; .strm files will point at localhost, which only works \
             when Jellyfin runs on this same host network"
        );
    }
    let tmdb = Tmdb::new(config.tmdb_api_key.clone(), config.tmdb_read_token.clone())?;
    if !tmdb.configured() {
        tracing::warn!("TMDB_API_KEY / TMDB_READ_ACCESS_TOKEN not set; library sync is disabled");
    }

    let egress = match &config.proxy {
        Some(raw) => {
            tracing::info!("routing provider and stream traffic through MELLO_PROXY");
            EgressMode::from_proxy_url(raw)
                .map_err(|_| "MELLO_PROXY must be http://, https:// or socks5://host:port")?
        }
        None => EgressMode::Direct,
    };

    let shutdown = CancellationToken::new();
    let media: Arc<dyn MediaEngine> = Arc::new(
        DiscoveryCoordinator::default_live(shutdown.clone(), egress.clone())
            .map_err(|e| format!("failed to start the provider engine: {e}"))?,
    );
    let relay = Arc::new(MediaRelay::new(relay_client(egress)?));
    let library = Library::open(&config, tmdb.clone(), keys.play_key()).await?;

    tokio::spawn(library.clone().run_sync_loop(shutdown.clone()));

    let state = AppState::new(AppStateInit {
        keys,
        media,
        relay,
        tmdb,
        library,
        public_url: config.public_url.clone(),
        public_url_configured: config.public_url_configured,
        max_height: config.max_height,
        audio_languages: config.audio_languages.clone(),
    });

    let address = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(
        %address,
        library = %config.library_dir.display(),
        "mello-jellyfin listening"
    );
    axum::serve(listener, app::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    shutdown.cancel();
    Ok(())
}

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("failed to start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mello-jellyfin: {e}");
            ExitCode::FAILURE
        }
    }
}
