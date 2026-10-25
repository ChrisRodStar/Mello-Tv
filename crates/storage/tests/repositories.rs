use std::error::Error;

use chrono::{DateTime, Duration};
use stream_core::{
    accounts::{Preferences, Profile},
    catalog::TitleIdentity,
    domain::{MediaType, ProfileId},
    ports::{CatalogRepository, ProfileRepository},
};
use stream_storage::{AccountStore, CatalogStore, catalog::CatalogWrite, db::Database};

#[tokio::test]
async fn catalog_port_retains_freshness_and_checks_stored_identity() -> Result<(), Box<dyn Error>> {
    let store = CatalogStore::from_database(Database::in_memory(2).await?).await?;
    let now = DateTime::parse_from_rfc3339("2026-10-25T12:00:00Z")?.to_utc();
    let identity = TitleIdentity::new(MediaType::Movie, 603)?;
    assert!(
        CatalogRepository::get_title(&store, &identity, now)
            .await?
            .is_none()
    );
    store.put(CatalogWrite {
        kind: "title".into(), key: identity.key(),
        data: serde_json::json!({"type":"movie", "id":603, "title":"The Matrix", "overview":"", "rating":8.2, "details":false}),
        ttl: Duration::seconds(-1),
    }, now).await?;
    let record = CatalogRepository::get_title(&store, &identity, now)
        .await?
        .ok_or("missing title")?;
    assert_eq!(record.state, "stale");
    assert_eq!(record.data.identity, identity);
    assert_eq!(
        CatalogRepository::search_titles(&store, "matrix", 0, now)
            .await?
            .len(),
        1
    );
    store.database().call(|connection| {
        connection.execute("UPDATE records SET data=? WHERE kind='title'", [serde_json::to_vec(&serde_json::json!({"type":"movie", "id":604, "title":"Changed", "overview":"", "rating":8.0, "details":false}))?])?;
        Ok(())
    }).await?;
    assert!(
        CatalogRepository::get_title(&store, &identity, now)
            .await
            .is_err()
    );
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn profile_port_requires_storage_owner() -> Result<(), Box<dyn Error>> {
    let store = AccountStore::from_database(Database::in_memory(2).await?, [7; 32]).await?;
    let owner = store.select_household(None).await?;
    let profile = Profile {
        id: "viewer".into(),
        name: "Viewer".into(),
        color: "red".into(),
        preferences: Preferences::default(),
    };
    store.create_profile(&owner, profile.clone()).await?;
    let profile_id = ProfileId::new(profile.id.clone())?;
    assert!(
        ProfileRepository::get_profile(&store, &owner, &profile_id)
            .await?
            .is_some()
    );
    assert!(
        ProfileRepository::get_profile(&store, "different-owner", &profile_id)
            .await?
            .is_none()
    );
    store.shutdown().await?;
    Ok(())
}
