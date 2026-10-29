use crate::{AppError, state::AppState};
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/provider-health", post(check))
}

async fn check(State(state): State<AppState>) -> Result<Response, AppError> {
    if !state.media.diagnostics().configured {
        return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    let providers = state.media.check_provider_health().await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "providers": providers })),
    )
        .into_response())
}
