use crate::error::DomainError;
use crate::providers::{Lookup, Selection};
use async_trait::async_trait;

#[async_trait]
pub trait MediaEngine: Send + Sync {
    /// Resolve using title coordinates, device/preferences and an ingress-owned
    /// credential partition. Pending startup selections retain their full evidence.
    async fn resolve_stream(&self, lookup: &Lookup) -> Result<Selection, DomainError>;
}
