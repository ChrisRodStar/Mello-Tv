use std::path::PathBuf;
use stream_storage::CatalogStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs_or_home()
        });

    println!("Seeding catalog database at: {}", db_path.display());
    let catalog = CatalogStore::open(&db_path).await?;
    stream_server::seed::seed_if_empty(&catalog).await?;
    println!("Catalog database successfully seeded!");
    Ok(())
}

fn dirs_or_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/dev".to_string());
    PathBuf::from(home).join("Library/Application Support/Stream/data/catalog.db")
}
