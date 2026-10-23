use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug)]
pub struct Error(pub StatusCode, pub &'static str);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}
impl std::error::Error for Error {}
#[derive(Serialize)]
struct Problem {
    r#type: &'static str,
    title: &'static str,
    status: u16,
    code: &'static str,
    detail: &'static str,
    request_id: String,
    retryable: bool,
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        crate::health::failure(self.1);
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let mut response = (
            self.0,
            Json(Problem {
                r#type: "about:blank",
                title: self.1,
                status: self.0.as_u16(),
                code: self.1,
                detail: self.1,
                request_id: NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string(),
                retryable: self.0 == StatusCode::SERVICE_UNAVAILABLE
                    || self.0 == StatusCode::TOO_MANY_REQUESTS,
            }),
        )
            .into_response();
        response.headers_mut().insert(
            "content-type",
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        response.headers_mut().insert(
            "cache-control",
            axum::http::HeaderValue::from_static("no-store"),
        );
        response
    }
}
pub const INVALID: Error = Error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_selection");
pub const CONFIG: Error = Error(StatusCode::INTERNAL_SERVER_ERROR, "invalid_configuration");
pub const STORE: Error = Error(StatusCode::SERVICE_UNAVAILABLE, "state_store_unavailable");
