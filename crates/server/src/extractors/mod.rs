use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde_json::json;
use stream_core::accounts::is_safe_identifier;

pub struct IdempotencyKey(pub String);

impl<S> FromRequestParts<S> for IdempotencyKey
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get("Idempotency-Key")
            .and_then(|val| val.to_str().ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        if key.is_empty() || !is_safe_identifier(&key) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_idempotency_key" })),
            )
                .into_response());
        }
        Ok(IdempotencyKey(key))
    }
}
