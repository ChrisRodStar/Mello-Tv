pub mod browse;
pub mod profiles;
pub mod stream;

use crate::state::AppState;
use axum::Router;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(browse::routes())
        .merge(profiles::routes())
        .merge(stream::routes())
        .layer(axum::middleware::from_fn(crate::middleware::trace_request))
        .with_state(state)
}
