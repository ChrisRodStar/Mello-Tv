pub mod browse;
pub mod profiles;
pub mod providers;
pub mod stream;

use crate::state::AppState;
use axum::Router;
use std::path::PathBuf;

pub fn build_router(state: AppState) -> Router {
    build_router_with_static(state, None)
}

pub fn build_router_with_static(state: AppState, static_dir: Option<PathBuf>) -> Router {
    let mut router = Router::new()
        .merge(browse::routes())
        .merge(profiles::routes())
        .merge(providers::routes())
        .merge(stream::routes())
        .layer(axum::middleware::from_fn(crate::middleware::trace_request))
        .with_state(state);

    if let Some(dir) = static_dir.filter(|d| d.is_dir()) {
        let index_file = dir.join("index.html");
        let serve_dir = tower_http::services::ServeDir::new(&dir)
            .fallback(tower_http::services::ServeFile::new(index_file));
        router = router.fallback_service(serve_dir);
    }

    router
}
