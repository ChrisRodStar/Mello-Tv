//! Application composition. Public routes are migrated after storage and media.

pub mod catalog_service;
pub mod discovery;
pub mod error;
pub mod extractors;
pub mod jellyfin;
pub mod middleware;
pub mod routes;
pub mod state;

pub use error::AppError;
pub use state::AppState;
