//! Adapters from concrete repositories to the application-facing core ports.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use stream_core::{
    accounts::Profile,
    catalog::{CatalogRecord, Title, TitleIdentity},
    domain::ProfileId,
    error::DomainError,
    ports::{CatalogRepository, ProfileRepository},
};

use crate::{AccountStore, CatalogStore, StorageError};

impl From<StorageError> for DomainError {
    fn from(error: StorageError) -> Self {
        Self::Infrastructure(Box::new(error))
    }
}

fn typed_title(record: CatalogRecord) -> Result<CatalogRecord<Title>, StorageError> {
    let data: Title = serde_json::from_value(record.data)?;
    data.validate()
        .map_err(|_| StorageError::Corrupt("catalog title"))?;
    if record.kind != "title" || record.key != data.identity.key() {
        return Err(StorageError::Corrupt("catalog title identity"));
    }
    Ok(CatalogRecord {
        kind: record.kind,
        key: record.key,
        data,
        updated_at: record.updated_at,
        expires_at: record.expires_at,
        revision: record.revision,
        state: record.state,
    })
}

#[async_trait]
impl CatalogRepository for CatalogStore {
    async fn get_title(
        &self,
        id: &TitleIdentity,
        now: DateTime<Utc>,
    ) -> Result<Option<CatalogRecord<Title>>, DomainError> {
        match self.title(&id.key(), now).await {
            Ok(record) => Ok(Some(typed_title(record)?)),
            Err(StorageError::NotFound) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    async fn search_titles(
        &self,
        query: &str,
        offset: u32,
        now: DateTime<Utc>,
    ) -> Result<Vec<CatalogRecord<Title>>, DomainError> {
        self.search(query, offset, now)
            .await?
            .into_iter()
            .map(typed_title)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

#[async_trait]
impl ProfileRepository for AccountStore {
    async fn list_profiles(&self, owner: &str) -> Result<Vec<Profile>, DomainError> {
        Ok(AccountStore::list_profiles(self, owner).await?)
    }

    async fn get_profile(
        &self,
        owner: &str,
        id: &ProfileId,
    ) -> Result<Option<Profile>, DomainError> {
        match AccountStore::get_profile(self, owner, id.as_str()).await {
            Ok(profile) => Ok(Some(profile)),
            Err(StorageError::NotFound) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}
