use crate::{
    config::Config,
    contracts::CreateSession,
    error::{Error, INVALID, STORE},
    validation,
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct App {
    pub config: Arc<Config>,
    pub playback: Option<Arc<crate::auth::Verifier>>,
    pub store: Option<crate::session::Store>,
    pub delivery: Option<Arc<crate::media::Delivery>>,
    pub lifecycle: Arc<crate::health::Lifecycle>,
    bearer_digest: [u8; 32],
    slots: Arc<Semaphore>,
    diagnostic_slots: Arc<Semaphore>,
}
impl App {
    pub fn new(config: Config, bearer: &[u8]) -> Self {
        let slots = Arc::new(Semaphore::new(
            config.limits.max_control_concurrency_count as usize,
        ));
        Self {
            config: Arc::new(config),
            playback: None,
            store: None,
            delivery: None,
            lifecycle: Arc::new(crate::health::Lifecycle::default()),
            bearer_digest: Sha256::digest(bearer).into(),
            slots,
            diagnostic_slots: Arc::new(Semaphore::new(1)),
        }
    }
}
pub fn control(app: App) -> Router {
    Router::new()
        .route("/internal/v1/sessions", post(create))
        .route("/internal/v1/sessions/{sid}", get(read).delete(revoke))
        .route("/internal/v1/sessions/{sid}/renew", post(renew))
        .route("/internal/v1/sessions/{sid}/source", post(source))
        .route("/internal/v1/sessions/{sid}/backups", post(backups))
        .route("/internal/v1/sessions/{sid}/failover", post(failover))
        .route("/health/livez", get(livez))
        .route("/health/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .fallback(|| async { STORE })
        .layer(middleware::from_fn_with_state(app.clone(), boundary))
        .with_state(app)
}
pub fn streaming() -> Router {
    streaming_with(None, Arc::new(crate::session::Unavailable))
}
#[derive(Clone)]
struct StreamState {
    verifier: Option<Arc<crate::auth::Verifier>>,
    store: Arc<dyn crate::session::AuthorizationStore>,
    delivery: Option<Arc<crate::media::Delivery>>,
}
pub fn streaming_with(
    verifier: Option<Arc<crate::auth::Verifier>>,
    store: Arc<dyn crate::session::AuthorizationStore>,
) -> Router {
    streaming_delivery(verifier, store, None)
}
pub fn streaming_delivery(
    verifier: Option<Arc<crate::auth::Verifier>>,
    store: Arc<dyn crate::session::AuthorizationStore>,
    delivery: Option<Arc<crate::media::Delivery>>,
) -> Router {
    Router::new()
        .route("/stream/{session_id}/entry", get(entry))
        .route("/stream/{session_id}/r/{resource_id}", get(resource))
        .fallback(|| async { Error(StatusCode::NOT_FOUND, "unknown_resource") })
        .layer(middleware::from_fn(stream_privacy))
        .with_state(StreamState {
            verifier,
            store,
            delivery,
        })
}
async fn stream_privacy(request: Request, next: Next) -> Response {
    let head = request.method() == axum::http::Method::HEAD;
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "cache-control",
        header::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        "referrer-policy",
        header::HeaderValue::from_static("no-referrer"),
    );
    if head {
        *response.body_mut() = axum::body::Body::empty();
    }
    response
}
async fn entry(
    State(state): State<StreamState>,
    axum::extract::Path(sid): axum::extract::Path<String>,
    request: Request,
) -> Result<Response, Error> {
    stream_boundary(state, sid, crate::session::Resource::Entry, request).await
}
async fn resource(
    State(state): State<StreamState>,
    axum::extract::Path((sid, rid)): axum::extract::Path<(String, String)>,
    request: Request,
) -> Result<Response, Error> {
    stream_boundary(
        state,
        sid,
        crate::session::Resource::Registered(rid),
        request,
    )
    .await
}
async fn stream_boundary(
    state: StreamState,
    sid: String,
    resource: crate::session::Resource,
    request: Request,
) -> Result<Response, Error> {
    let auth_time = crate::health::Timer::start(crate::health::Stage::Authorization);
    let verifier = state.verifier.ok_or(crate::auth::INVALID_GRANT)?;
    let token = verifier.query(request.uri().query())?;
    let now = Utc::now().timestamp();
    let claims = verifier.verify(&token, now)?;
    if claims.sid != sid || !crate::auth::opaque(&sid) {
        return Err(crate::auth::INVALID_GRANT);
    }
    let record = tokio::time::timeout(Duration::from_secs(2), state.store.lookup(&sid, &resource))
        .await
        .map_err(|_| STORE)??;
    let authorization =
        crate::session::authorize(&claims, &sid, &resource, record, Utc::now().timestamp())?;
    drop(auth_time);
    let delivery = state
        .delivery
        .ok_or(Error(StatusCode::SERVICE_UNAVAILABLE, "media_unavailable"))?;
    if let Some(lease) = request
        .extensions()
        .get::<crate::media::connection::ConnectionLease>()
    {
        lease.set(
            authorization.cancelled.clone(),
            delivery
                .transport
                .guard()
                .lease()
                .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "vpn_unavailable"))?
                .cancel,
            crate::media::lease_deadline(authorization.expires_at)?.min(
                tokio::time::Instant::now()
                    + Duration::from_secs(
                        match authorization.registration.as_ref().map(|r| &r.kind) {
                            Some(crate::session::ResourceKind::Mp4) => {
                                delivery.config.network.mp4_max_transfer_s
                            }
                            Some(crate::session::ResourceKind::Manifest) => {
                                delivery.config.network.playlist_timeout_s
                            }
                            _ => delivery.config.network.segment_timeout_s,
                        },
                    ),
            ),
        );
    }
    delivery
        .serve(
            &sid,
            &token,
            authorization,
            request.method().clone(),
            request.headers(),
        )
        .await
}

async fn boundary(State(app): State<App>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if headers.len() > 64
        || headers
            .iter()
            .map(|(k, v)| k.as_str().len() + v.len())
            .sum::<usize>()
            > 16384
    {
        return Error(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "headers_too_large",
        )
        .into_response();
    }
    let mut auth = headers.get_all(header::AUTHORIZATION).iter();
    let supplied = auth
        .next()
        .and_then(|v| v.as_bytes().strip_prefix(b"Bearer "));
    if auth.next().is_some()
        || !supplied
            .is_some_and(|v| bool::from(Sha256::digest(v).as_slice().ct_eq(&app.bearer_digest)))
    {
        return Error(StatusCode::UNAUTHORIZED, "invalid_control_bearer").into_response();
    }
    let diagnostic = matches!(
        request.uri().path(),
        "/health/livez" | "/health/readyz" | "/metrics"
    );
    if app
        .lifecycle
        .draining
        .load(std::sync::atomic::Ordering::Acquire)
        && !diagnostic
    {
        return Error(StatusCode::SERVICE_UNAVAILABLE, "draining").into_response();
    }
    // Diagnostics have one independently bounded slot, so a readiness request
    // neither consumes the last control permit nor disappears under control load.
    let slots = if diagnostic {
        &app.diagnostic_slots
    } else {
        &app.slots
    };
    let Ok(_permit) = slots.clone().try_acquire_owned() else {
        return Error(StatusCode::TOO_MANY_REQUESTS, "request_limit").into_response();
    };
    match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
        Ok(mut response) => {
            response.headers_mut().insert(
                "cache-control",
                header::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(_) => Error(StatusCode::REQUEST_TIMEOUT, "control_timeout").into_response(),
    }
}
async fn create(State(app): State<App>, request: Request) -> Result<Response, Error> {
    let keys: Vec<_> = request
        .headers()
        .get_all("idempotency-key")
        .iter()
        .collect();
    if keys.len() != 1
        || keys[0].is_empty()
        || keys[0].len() > 128
        || !keys[0].as_bytes().iter().all(|b| b.is_ascii_graphic())
    {
        return Err(Error(StatusCode::BAD_REQUEST, "invalid_idempotency_key"));
    }
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        != Some("application/json")
    {
        return Err(Error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
        ));
    }
    let key = keys[0].to_str().map_err(|_| INVALID)?.to_owned();
    let body = to_bytes(
        request.into_body(),
        app.config.limits.max_control_body_bytes as usize,
    )
    .await
    .map_err(|_| Error(StatusCode::PAYLOAD_TOO_LARGE, "import_too_large"))?;
    let request: CreateSession = serde_json::from_slice(&body).map_err(|_| INVALID)?;
    // Validation and idempotency run together at the durable serialization point.
    if app.store.is_none() {
        validation::create(&request, &app.config, Utc::now())?;
    }
    let ready = app
        .store
        .as_ref()
        .ok_or(STORE)?
        .create(request, key)
        .await?;
    Ok((StatusCode::CREATED, Json(ready)).into_response())
}

async fn read(
    State(app): State<App>,
    axum::extract::Path(sid): axum::extract::Path<String>,
) -> Result<Response, Error> {
    Ok(Json(app.store.as_ref().ok_or(STORE)?.read(sid).await?).into_response())
}
async fn revoke(
    State(app): State<App>,
    axum::extract::Path(sid): axum::extract::Path<String>,
) -> Result<Response, Error> {
    app.store.as_ref().ok_or(STORE)?.revoke(sid).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn body<T: serde::de::DeserializeOwned>(app: &App, request: Request) -> Result<T, Error> {
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        != Some("application/json")
    {
        return Err(Error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
        ));
    }
    let bytes = to_bytes(
        request.into_body(),
        app.config.limits.max_control_body_bytes as usize,
    )
    .await
    .map_err(|_| Error(StatusCode::PAYLOAD_TOO_LARGE, "import_too_large"))?;
    serde_json::from_slice(&bytes).map_err(|_| INVALID)
}
async fn renew(
    State(app): State<App>,
    axum::extract::Path(sid): axum::extract::Path<String>,
    request: Request,
) -> Result<Response, Error> {
    let request = body(&app, request).await?;
    Ok(Json(app.store.as_ref().ok_or(STORE)?.renew(sid, request).await?).into_response())
}
async fn source(
    State(app): State<App>,
    axum::extract::Path(sid): axum::extract::Path<String>,
    request: Request,
) -> Result<Response, Error> {
    let request = body(&app, request).await?;
    Ok(Json(
        app.delivery
            .as_ref()
            .ok_or(STORE)?
            .replace(sid, request)
            .await?,
    )
    .into_response())
}
async fn backups(
    State(app): State<App>,
    axum::extract::Path(sid): axum::extract::Path<String>,
    request: Request,
) -> Result<Response, Error> {
    let request = body(&app, request).await?;
    app.store
        .as_ref()
        .ok_or(STORE)?
        .backups(sid, request)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn failover(
    State(app): State<App>,
    axum::extract::Path(sid): axum::extract::Path<String>,
    request: Request,
) -> Result<Response, Error> {
    let request = body(&app, request).await?;
    Ok(Json(
        app.delivery
            .as_ref()
            .ok_or(STORE)?
            .failover(sid, request)
            .await?,
    )
    .into_response())
}

async fn livez(State(app): State<App>) -> Response {
    let failed = app
        .lifecycle
        .failed
        .load(std::sync::atomic::Ordering::Acquire);
    (if failed { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK },
     Json(serde_json::json!({"status":if failed {"not_ready"} else {"alive"},"contract_version":"relay.v1"}))).into_response()
}
async fn readyz(State(app): State<App>) -> Response {
    use std::sync::atomic::Ordering::Acquire;
    let store = if let Some(store) = &app.store {
        tokio::time::timeout(Duration::from_millis(500), store.health())
            .await
            .is_ok_and(|r| r.is_ok())
    } else {
        false
    };
    let transport = app
        .delivery
        .as_ref()
        .is_some_and(|d| d.transport.guard().healthy());
    let admission = app.slots.available_permits() > 0
        && app
            .delivery
            .as_ref()
            .is_some_and(|d| d.transport.available() > 0);
    let draining = app.lifecycle.draining.load(Acquire);
    let tasks = !app.lifecycle.failed.load(Acquire);
    let ready = store && transport && admission && tasks && app.playback.is_some() && !draining;
    (if ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE }, Json(serde_json::json!({
        "contract_version":"relay.v1", "status":if draining {"draining"} else if ready {"ready"} else {"not_ready"},
        "store_keys":store,"transport_epoch":transport,"admission":admission,"owned_tasks":tasks,
        "provider_readiness":"external", "overall_acceptance":false
    }))).into_response()
}
async fn metrics(State(app): State<App>) -> Response {
    let output = format!(
        "{}relay_draining {}\nrelay_owned_task_failure {}\nrelay_control_available {}\nrelay_transport_available {}\n",
        crate::health::metrics(),
        u8::from(
            app.lifecycle
                .draining
                .load(std::sync::atomic::Ordering::Acquire)
        ),
        u8::from(
            app.lifecycle
                .failed
                .load(std::sync::atomic::Ordering::Acquire)
        ),
        app.slots.available_permits(),
        app.delivery.as_ref().map_or(0, |d| d.transport.available())
    );
    ([("content-type", "text/plain; version=0.0.4")], output).into_response()
}

#[cfg(test)]
mod diagnostic_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn diagnostic_capacity_is_finite_and_rejects_excess_without_using_control_slots() {
        let config = Config::parse(include_str!("../../config/relay.example.toml")).unwrap();
        let app = App::new(config, b"synthetic-control-bearer");
        let permit = app.diagnostic_slots.clone().try_acquire_owned().unwrap();
        let slots = app.slots.available_permits();
        let router = control(app.clone());
        let request = || {
            Request::builder()
                .uri("/health/livez")
                .header("authorization", "Bearer synthetic-control-bearer")
                .body(axum::body::Body::empty())
                .unwrap()
        };
        assert_eq!(
            router.clone().oneshot(request()).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(app.slots.available_permits(), slots);
        drop(permit);
        assert_eq!(
            router.oneshot(request()).await.unwrap().status(),
            StatusCode::OK
        );
    }
}
