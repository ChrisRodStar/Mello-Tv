use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("Entity not found: {0}")]
    NotFound(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Infrastructure operation failed")]
    Infrastructure(#[source] Box<dyn std::error::Error + Send + Sync>),
}
