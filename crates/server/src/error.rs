use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use stream_core::{
    accounts::AccountValidationError,
    catalog::CatalogValidationError,
    error::DomainError,
};
use stream_storage::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("domain error: {0}")]
    Domain(#[from] DomainError),
    #[error("account validation: {0}")]
    AccountValidation(#[from] AccountValidationError),
    #[error("catalog validation: {0}")]
    CatalogValidation(#[from] CatalogValidationError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Conflict(msg) => {
                if msg == "revoked" || msg == "session_expired" {
                    (StatusCode::GONE, msg)
                } else {
                    (StatusCode::CONFLICT, msg)
                }
            }
            AppError::Storage(StorageError::NotFound) => {
                (StatusCode::NOT_FOUND, "not_found".to_string())
            }
            AppError::Storage(StorageError::Conflict(msg)) => {
                if msg == "revoked" || msg == "session_expired" {
                    (StatusCode::GONE, msg.to_string())
                } else {
                    (StatusCode::CONFLICT, msg.to_string())
                }
            }
            AppError::Storage(StorageError::InvalidInput(msg)) => {
                (StatusCode::BAD_REQUEST, msg.to_string())
            }
            AppError::Storage(e) => {
                tracing::error!(error = %e, "storage error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_storage_error".to_string())
            }
            AppError::Domain(e) => {
                tracing::error!(error = %e, "domain error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_domain_error".to_string())
            }
            AppError::AccountValidation(e) => {
                (StatusCode::BAD_REQUEST, e.to_string())
            }
            AppError::CatalogValidation(e) => {
                (StatusCode::BAD_REQUEST, e.to_string())
            }
            AppError::Json(e) => {
                (StatusCode::BAD_REQUEST, e.to_string())
            }
            AppError::Internal(msg) => {
                tracing::error!(msg = %msg, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, msg)
            }
        };
        (status, Json(json!({ "error": code }))).into_response()
    }
}
