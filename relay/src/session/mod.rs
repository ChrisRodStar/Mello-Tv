mod crypto;
mod store;
pub use store::{ByteRange, ManifestContext, Registration, ResourceKind, Store, Worker};
// Atomic durable authorization boundary.
use crate::{
    auth::{Claims, opaque},
    error::{Error, STORE},
};
use axum::http::StatusCode;
use std::{future::Future, pin::Pin};
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
pub enum Resource {
    Entry,
    Registered(String),
}
/// Must come from a single consistent durable read. Resource ownership includes
/// generation retention and source validity; arbitrary URLs are never lookups.
pub struct AuthorizationRecord {
    pub registration: Option<Registration>,
    pub session_id: String,
    pub owner_id: String,
    pub auth_revision: u64,
    pub expires_at: i64,
    pub source_expires_at: Option<i64>,
    pub revoked: bool,
    pub resource: Resource,
    pub resource_session_id: String,
    pub source_generation: u64,
    pub resource_generation: u64,
    pub generation_valid_until: Option<i64>,
    /// Store cancels this on revocation, expiry or generation removal.
    pub cancelled: CancellationToken,
    pub reader: Option<std::sync::Arc<()>>,
}
pub trait AuthorizationStore: Send + Sync {
    fn lookup<'a>(
        &'a self,
        sid: &'a str,
        resource: &'a Resource,
    ) -> Pin<Box<dyn Future<Output = Result<AuthorizationRecord, Error>> + Send + 'a>>;
}
pub struct Unavailable;
impl AuthorizationStore for Unavailable {
    fn lookup<'a>(
        &'a self,
        _: &'a str,
        _: &'a Resource,
    ) -> Pin<Box<dyn Future<Output = Result<AuthorizationRecord, Error>> + Send + 'a>> {
        Box::pin(async { Err(STORE) })
    }
}
pub struct Authorized {
    pub registration: Option<Registration>,
    pub generation: u64,
    pub expires_at: i64,
    pub cancelled: CancellationToken,
    pub reader: Option<std::sync::Arc<()>>,
}
pub fn authorize(
    claims: &Claims,
    sid: &str,
    resource: &Resource,
    record: AuthorizationRecord,
    now: i64,
) -> Result<Authorized, Error> {
    let denied = Error(StatusCode::FORBIDDEN, "owner_mismatch");
    if !opaque(sid)
        || claims.sid != sid
        || record.session_id != sid
        || claims.sub != record.owner_id
        || claims.arv != record.auth_revision
    {
        return Err(denied);
    }
    if record.revoked || record.cancelled.is_cancelled() {
        return Err(Error(StatusCode::FORBIDDEN, "revoked_grant"));
    }
    let matches = match (resource, &record.resource) {
        (Resource::Entry, Resource::Entry) => true,
        (Resource::Registered(a), Resource::Registered(b)) => opaque(a) && a == b,
        _ => false,
    };
    if !matches
        || record.resource_session_id != sid
        || (matches!(resource, Resource::Entry)
            && record.resource_generation != record.source_generation)
        || record.source_generation == 0
        || record.resource_generation == 0
        || (record.resource_generation != record.source_generation
            && record
                .generation_valid_until
                .is_none_or(|expiry| expiry <= now))
    {
        return Err(Error(StatusCode::NOT_FOUND, "unknown_resource"));
    }
    let expires_at = claims
        .exp
        .min(record.expires_at)
        .min(record.source_expires_at.unwrap_or(i64::MAX))
        .min(record.generation_valid_until.unwrap_or(i64::MAX));
    // Never extend a source/session/active body lease using JWT clock leeway.
    if expires_at <= now {
        return Err(Error(StatusCode::GONE, "session_expired"));
    }
    Ok(Authorized {
        registration: record.registration,
        generation: record.resource_generation,
        expires_at,
        cancelled: record.cancelled,
        reader: record.reader,
    })
}
