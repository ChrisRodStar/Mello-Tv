use family_stream_relay::{
    config::Config,
    control::App,
    error::{CONFIG, Error},
    runtime, secret,
};
use std::{io::Read, path::Path, process::ExitCode};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> ExitCode {
    match start().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("relay: {error}");
            ExitCode::FAILURE
        }
    }
}
async fn start() -> Result<(), Error> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 2 && !(args.len() == 4 && args[2] == "--acceptance-budget") {
        return Err(CONFIG);
    }
    let mut input = String::new();
    std::fs::File::open(&args[1])
        .map_err(|_| CONFIG)?
        .take(65537)
        .read_to_string(&mut input)
        .map_err(|_| CONFIG)?;
    if input.len() > 65536 {
        return Err(CONFIG);
    }
    let config = Config::parse(&input)?;
    let bearer = secret::bearer(Path::new(&config.auth.control_bearer_file))?;
    let verifier = family_stream_relay::auth::Verifier::load(&config.auth)?;
    let (transport, worker) = if args.len() == 4 {
        let gate = family_stream_relay::network::acceptance::Gate::open(Path::new(&args[3]))
            .map_err(|_| CONFIG)?;
        family_stream_relay::network::Transport::with_acceptance(config.network.clone(), gate)
    } else {
        family_stream_relay::network::Transport::new(config.network.clone())
    }
    .map_err(|_| CONFIG)?;
    let mut app = App::new(config, &bearer);
    app.playback = Some(std::sync::Arc::new(verifier));
    let store_config = app.config.clone();
    let (store, store_worker) = tokio::task::spawn_blocking(move || {
        family_stream_relay::session::Store::open(store_config)
    })
    .await
    .map_err(|_| CONFIG)??;
    app.delivery = Some(std::sync::Arc::new(family_stream_relay::media::Delivery {
        config: app.config.clone(),
        store: store.clone(),
        transport,
    }));
    app.store = Some(store);
    let control = TcpListener::bind(app.config.http.control_listen)
        .await
        .map_err(|_| CONFIG)?;
    let stream = TcpListener::bind(app.config.http.stream_listen)
        .await
        .map_err(|_| CONFIG)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| CONFIG)?;
    let shutdown = CancellationToken::new();
    // Listener drain owns the first shutdown phase. Accepted transfers still
    // need the guard monitor, connection drivers and store cancellation leases.
    let network_stop = CancellationToken::new();
    let storage_stop = CancellationToken::new();
    let lifecycle = app.lifecycle.clone();
    let service = runtime::run(control, stream, app, shutdown.clone());
    let network = worker.run(network_stop.clone());
    let storage = store_worker.run(storage_stop.clone());
    eprintln!(
        "relay: started; authenticated HLS/MP4 enabled; runtime readiness on private control listener; overall acceptance pending"
    );
    let signal = async move {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal.map_err(|_| CONFIG),
            _ = terminate.recv() => Ok(()),
        }
    };
    runtime::own(
        service,
        network,
        storage,
        signal,
        runtime::Infrastructure {
            listeners: shutdown,
            network: network_stop,
            storage: storage_stop,
            lifecycle,
        },
    )
    .await
}
