use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

/// Log paths without query strings, which can carry playback credentials.
pub async fn trace_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().path().to_owned();
    let start = Instant::now();

    tracing::debug!(%method, %uri, "incoming HTTP request");
    let response = next.run(req).await;
    let latency = start.elapsed();
    let status = response.status();

    if status.is_server_error() {
        tracing::error!(%method, %uri, %status, ?latency, "HTTP request failed with server error");
    } else if status.is_client_error() {
        tracing::warn!(%method, %uri, %status, ?latency, "HTTP request completed with client error");
    } else {
        tracing::info!(%method, %uri, %status, ?latency, "HTTP request completed");
    }

    response
}
