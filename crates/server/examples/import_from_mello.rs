use std::fs;
use stream_storage::AccountStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key_bytes = fs::read("/Volumes/Pi/stream-tv/private/application/api-encryption")?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&key_bytes);

    let db_path = "/tmp/mello_api.sqlite";
    let _ = fs::remove_file(db_path);
    // Copy locally so we have a clean consistent snapshot
    fs::copy(
        "/Volumes/Pi/stream-tv/state/application/api.sqlite",
        db_path,
    )?;

    println!("Testing AccountStore::open with key on Mello database...");
    let accounts = AccountStore::open(db_path, key).await?;
    println!("AccountStore opened successfully!");

    let household_id = accounts.select_household(None).await?;
    println!("Household ID: {}", household_id);

    let profiles = accounts.list_profiles(&household_id).await?;
    println!("Found {} profiles:", profiles.len());
    for p in &profiles {
        println!(" - Profile ID: {}, Name: {}", p.id, p.name);
        let hist = accounts.history(&household_id, &p.id, 100).await?;
        println!("   * {} history items", hist.len());
        let favs = accounts
            .list_titles(
                &household_id,
                &p.id,
                stream_storage::accounts::ListKind::Favorites,
            )
            .await?;
        println!("   * {} favorites", favs.len());
    }

    let _ = accounts.shutdown().await;
    let _ = fs::remove_file(db_path);
    Ok(())
}
