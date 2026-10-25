use crate::accounts::Profile;
use crate::catalog::{CatalogRecord, Title, TitleIdentity};
use crate::domain::ProfileId;
use crate::error::DomainError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait CatalogRepository: Send + Sync {
    async fn get_title(
        &self,
        id: &TitleIdentity,
        now: DateTime<Utc>,
    ) -> Result<Option<CatalogRecord<Title>>, DomainError>;
    async fn search_titles(
        &self,
        query: &str,
        offset: u32,
        now: DateTime<Utc>,
    ) -> Result<Vec<CatalogRecord<Title>>, DomainError>;
}

/// Profiles are account-owned state, independent of catalog persistence.
#[async_trait]
pub trait ProfileRepository: Send + Sync {
    async fn list_profiles(&self, owner: &str) -> Result<Vec<Profile>, DomainError>;
    async fn get_profile(
        &self,
        owner: &str,
        id: &ProfileId,
    ) -> Result<Option<Profile>, DomainError>;
}
