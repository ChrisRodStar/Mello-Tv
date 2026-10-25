use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use stream_storage::{
    StorageError,
    catalog::{CatalogLimits, CatalogStore, CatalogWrite},
    db::Database,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// Literal schema from the Go v1 migration, before the search-index v2 upgrade.
const GO_V1_SCHEMA: &str = "
CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);
INSERT INTO schema_migrations VALUES(1);
CREATE TABLE records(kind TEXT NOT NULL,key TEXT NOT NULL,data BLOB NOT NULL,updated INTEGER NOT NULL,expires INTEGER NOT NULL,revision INTEGER NOT NULL,PRIMARY KEY(kind,key));
CREATE INDEX records_expiry ON records(expires,kind);
CREATE TABLE revision(value INTEGER NOT NULL);
INSERT INTO revision VALUES(7);
CREATE TABLE jobs(key TEXT PRIMARY KEY,spec BLOB NOT NULL,due INTEGER NOT NULL DEFAULT 0,last_success INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0);
CREATE INDEX jobs_due ON jobs(due);
";

fn now() -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339("2026-10-25T20:00:00.123Z").map(|time| time.with_timezone(&Utc))
}

fn title(id: i64, name: &str, details: bool) -> Value {
    json!({"type":"movie","id":id,"title":name,"overview":"Overview","rating":8.4,"details":details})
}

fn write(kind: &str, key: &str, data: Value) -> CatalogWrite {
    CatalogWrite {
        kind: kind.into(),
        key: key.into(),
        data,
        ttl: Duration::hours(1),
    }
}

async fn memory_store() -> Result<CatalogStore, StorageError> {
    CatalogStore::from_database(Database::in_memory(64).await?).await
}

#[tokio::test]
async fn new_store_creates_go_compatible_tables_indexes_and_revision() -> TestResult {
    let store = memory_store().await?;
    store.health().await?;
    let (version, tables, indexes, revision) = store.database().call(|connection| {
        let version: i64 = connection.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get(0))?;
        let tables: i64 = connection.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('schema_migrations','records','revision','jobs')", [], |row| row.get(0))?;
        let indexes: i64 = connection.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='index' AND name IN ('records_expiry','records_title_search','jobs_due')", [], |row| row.get(0))?;
        let revision: i64 = connection.query_row("SELECT value FROM revision", [], |row| row.get(0))?;
        Ok((version, tables, indexes, revision))
    }).await?;
    assert_eq!((version, tables, indexes, revision), (2, 4, 3, 0));
    let empty = store.export_raw(now()?).await?;
    assert_eq!(empty.version, "1");
    assert!(empty.records.is_empty());
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn go_v1_upgrade_preserves_existing_records_jobs_and_artwork() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("catalog.sqlite");
    let instant = now()?;
    let artwork = json!({"legacy":"retained"});
    {
        let connection = Connection::open(&path)?;
        connection.execute_batch(GO_V1_SCHEMA)?;
        connection.execute(
            "INSERT INTO records VALUES('title','movie:550',?,?,?,7)",
            params![
                serde_json::to_vec(&title(550, "Fight Club", true))?,
                instant.timestamp_millis(),
                (instant + Duration::hours(1)).timestamp_millis()
            ],
        )?;
        connection.execute(
            "INSERT INTO records VALUES('artwork','legacy',?,?,?,7)",
            params![
                serde_json::to_vec(&artwork)?,
                instant.timestamp_millis(),
                instant.timestamp_millis()
            ],
        )?;
        connection.execute(
            "INSERT INTO jobs VALUES('title/movie:550',?,123,122,1)",
            [br#"{"kind":"title","type":"movie","id":550}"#.as_slice()],
        )?;
    }
    let store = CatalogStore::open(&path).await?;
    assert_eq!(
        store.get("title", "movie:550", instant).await?.data["title"],
        "Fight Club"
    );
    assert_eq!(store.get("artwork", "legacy", instant).await?.data, artwork);
    assert_eq!(store.search("fight", 0, instant).await?.len(), 1);
    let version_and_job = store
        .database()
        .call(|connection| {
            let version: i64 =
                connection.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get(0)
                })?;
            let job: (i64, i64, i64) = connection.query_row(
                "SELECT due,last_success,failed FROM jobs WHERE key='title/movie:550'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            Ok((version, job))
        })
        .await?;
    assert_eq!(version_and_job, (2, (123, 122, 1)));
    store.close().await?;
    let reopened = CatalogStore::open(&path).await?;
    assert_eq!(reopened.export_raw(instant).await?.revision, 7);
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn future_schema_is_rejected_without_adding_or_removing_tables() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("newer.sqlite");
    {
        let connection = Connection::open(&path)?;
        connection.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); INSERT INTO schema_migrations VALUES(99); CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('keep');")?;
    }
    assert!(matches!(
        CatalogStore::open(&path).await,
        Err(StorageError::UnsupportedSchema {
            found: 99,
            supported: 2
        })
    ));
    let connection = Connection::open(&path)?;
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table'",
        [],
        |row| row.get(0),
    )?;
    let sentinel: String =
        connection.query_row("SELECT value FROM sentinel", [], |row| row.get(0))?;
    assert_eq!(count, 2);
    assert_eq!(sentinel, "keep");
    Ok(())
}

#[tokio::test]
async fn api_database_is_rejected_without_creating_catalog_tables_or_changing_journal() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("api.sqlite");
    {
        let connection = Connection::open(&path)?;
        connection.execute_batch("PRAGMA user_version=1; CREATE TABLE accounts(id TEXT PRIMARY KEY); INSERT INTO accounts VALUES('household');")?;
    }
    assert!(matches!(
        CatalogStore::open(&path).await,
        Err(StorageError::InvalidInput(_))
    ));
    let connection = Connection::open(&path)?;
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table'",
        [],
        |row| row.get(0),
    )?;
    let journal: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    let account: String = connection.query_row("SELECT id FROM accounts", [], |row| row.get(0))?;
    assert_eq!(count, 1);
    assert_eq!(journal, "delete");
    assert_eq!(account, "household");
    Ok(())
}

#[tokio::test]
async fn unversioned_unowned_database_is_rejected_unchanged() -> TestResult {
    let database = Database::in_memory(64).await?;
    database
        .call(|connection| {
            connection.execute_batch(
                "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('keep');",
            )?;
            Ok(())
        })
        .await?;
    assert!(matches!(
        CatalogStore::from_database(database.clone()).await,
        Err(StorageError::InvalidInput(_))
    ));
    let count: i64 = database
        .call(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table'",
                [],
                |row| row.get(0),
            )?)
        })
        .await?;
    assert_eq!(count, 1);
    database.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn malformed_existing_catalog_index_is_rejected_without_version_bump() -> TestResult {
    let database = Database::in_memory(64).await?;
    database
        .call(|connection| {
            connection.execute_batch(GO_V1_SCHEMA)?;
            connection.execute_batch("CREATE INDEX records_title_search ON records(key);")?;
            Ok(())
        })
        .await?;
    assert!(matches!(
        CatalogStore::from_database(database.clone()).await,
        Err(StorageError::Corrupt(_))
    ));
    let version: i64 = database
        .call(|connection| {
            Ok(
                connection.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .await?;
    assert_eq!(version, 1);
    database.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn record_expiry_boundary_and_negative_ttl_preserve_stale_reads() -> TestResult {
    let store = memory_store().await?;
    let instant = now()?;
    store
        .put(write("shelf", "home", json!({"titles":[]})), instant)
        .await?;
    assert_eq!(store.get("shelf", "home", instant).await?.state, "fresh");
    assert_eq!(
        store
            .get("shelf", "home", instant + Duration::hours(1))
            .await?
            .state,
        "stale"
    );
    let mut expired = write("shelf", "expired", json!({"titles":[]}));
    expired.ttl = Duration::seconds(-1);
    store.put(expired, instant).await?;
    let record = store.get("shelf", "expired", instant).await?;
    assert_eq!(record.state, "stale");
    assert!(record.expires_at < record.updated_at);
    assert!(matches!(
        store.get("shelf", "missing", instant).await,
        Err(StorageError::NotFound)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn quota_failure_rolls_back_all_records_and_revision() -> TestResult {
    let store = memory_store().await?.with_limits(CatalogLimits {
        max_bytes: 1024,
        max_records: 1,
    })?;
    let instant = now()?;
    assert_eq!(
        store
            .put(write("shelf", "a", json!({"generation":1})), instant)
            .await?,
        1
    );
    let failure = store
        .put_batch(
            vec![
                write("shelf", "a", json!({"generation":2})),
                write("shelf", "b", json!({"generation":2})),
            ],
            instant,
        )
        .await;
    assert!(matches!(failure, Err(StorageError::Capacity)));
    assert_eq!(
        store.get("shelf", "a", instant).await?.data["generation"],
        1
    );
    assert!(matches!(
        store.get("shelf", "b", instant).await,
        Err(StorageError::NotFound)
    ));
    assert_eq!(store.export_raw(instant).await?.revision, 1);
    let too_large = write("shelf", "a", json!({"payload":"x".repeat(2048)}));
    assert!(matches!(
        store.put(too_large, instant).await,
        Err(StorageError::Capacity)
    ));
    assert_eq!(store.export_raw(instant).await?.revision, 1);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn detailed_titles_and_cross_ids_survive_brief_and_full_refreshes() -> TestResult {
    let store = memory_store().await?;
    let instant = now()?;
    let mut original = title(550, "Detailed", true);
    original["imdb_id"] = json!("tt0137523");
    original["mal_id"] = json!(123);
    store
        .put(write("title", "movie:550", original.clone()), instant)
        .await?;
    store
        .put(
            write("title", "movie:550", title(550, "Brief", false)),
            instant + Duration::minutes(1),
        )
        .await?;
    let retained = store.get("title", "movie:550", instant).await?;
    assert_eq!(retained.data, original);
    assert_eq!(retained.revision, 1);
    assert_eq!(store.export_raw(instant).await?.revision, 2);
    store
        .put(
            write("title", "movie:550", title(550, "New detailed", true)),
            instant,
        )
        .await?;
    let next = store.get("title", "movie:550", instant).await?;
    assert_eq!(next.data["title"], "New detailed");
    assert_eq!(next.data["imdb_id"], "tt0137523");
    assert_eq!(next.data["mal_id"], 123);
    assert_eq!(next.revision, 3);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn title_reads_merge_cross_ids_without_rewriting_raw_record() -> TestResult {
    let store = memory_store().await?;
    let instant = now()?;
    store
        .put(
            write("title", "movie:550", title(550, "Fight Club", true)),
            instant,
        )
        .await?;
    store
        .put(
            write(
                "cross_ids",
                "movie:550",
                json!({"imdb_id":"tt0137523","mal_id":123}),
            ),
            instant,
        )
        .await?;
    let enriched = store.title("movie:550", instant).await?;
    assert_eq!(enriched.data["imdb_id"], "tt0137523");
    assert_eq!(enriched.data["mal_id"], 123);
    assert_eq!(enriched.revision, 2);
    assert!(
        store
            .get("title", "movie:550", instant)
            .await?
            .data
            .get("imdb_id")
            .is_none()
    );
    assert_eq!(store.get("title", "movie:550", instant).await?.revision, 1);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn substring_search_is_literal_case_insensitive_and_paginates_by_key() -> TestResult {
    let store = memory_store().await?;
    let instant = now()?;
    store
        .put_batch(
            vec![
                write("title", "movie:1", title(1, "100% FIGHT_club", true)),
                write("title", "movie:2", title(2, "Fight club", true)),
                write("title", "movie:3", title(3, "Other", true)),
            ],
            instant,
        )
        .await?;
    assert_eq!(store.search("FIGHT", 0, instant).await?.len(), 2);
    assert_eq!(store.search("%", 0, instant).await?.len(), 1);
    assert_eq!(store.search("_", 0, instant).await?.len(), 1);
    assert_eq!(store.search("fight", 1, instant).await?[0].key, "movie:2");
    assert!(store.search("' OR 1=1 --", 0, instant).await?.is_empty());
    assert_eq!(store.list("title", 2, instant).await?.len(), 2);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_raw_exports_never_observe_torn_batch_revisions() -> TestResult {
    let store = memory_store().await?;
    let instant = now()?;
    let writer = async {
        for generation in 0..20 {
            store
                .put_batch(
                    vec![
                        write("shelf", "a", json!({"generation":generation})),
                        write("shelf", "b", json!({"generation":generation})),
                    ],
                    instant,
                )
                .await?;
        }
        Ok::<_, StorageError>(())
    };
    let reader = async {
        for _ in 0..20 {
            let snapshot = store.export_raw(instant).await?;
            if snapshot.records.len() == 2 {
                assert_eq!(snapshot.records[0].revision, snapshot.records[1].revision);
                assert_eq!(snapshot.records[0].data, snapshot.records[1].data);
                assert_eq!(snapshot.records[0].revision, snapshot.revision);
            }
        }
        Ok::<_, StorageError>(())
    };
    let (written, read) = tokio::join!(writer, reader);
    written?;
    read?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn raw_export_preserves_recommendation_inputs_instead_of_claiming_enrichment() -> TestResult {
    let store = memory_store().await?;
    let instant = now()?;
    let inputs = json!({"completed":true,"format":"both","genres":[],"favorites":[],"signals":[],"activity_days":[]});
    store
        .put(write("inputs", "profile-one", inputs.clone()), instant)
        .await?;
    let snapshot = store.export_raw(instant).await?;
    assert_eq!(snapshot.records[0].kind, "inputs");
    assert_eq!(snapshot.records[0].data, inputs);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_coordinates_and_artwork_writes_cannot_mutate_revision() -> TestResult {
    let store = memory_store().await?;
    let instant = now()?;
    assert!(matches!(
        store
            .put(
                write("title", "movie:551", title(550, "Wrong key", true)),
                instant
            )
            .await,
        Err(StorageError::InvalidInput(_))
    ));
    assert!(matches!(
        store
            .put(write("artwork", "image", json!({})), instant)
            .await,
        Err(StorageError::InvalidInput(_))
    ));
    assert_eq!(store.export_raw(instant).await?.revision, 0);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn revision_overflow_does_not_change_schema_or_write_records() -> TestResult {
    let store = memory_store().await?;
    store
        .database()
        .call(|connection| {
            connection.execute("UPDATE revision SET value=?", [i64::MAX])?;
            Ok(())
        })
        .await?;
    assert!(matches!(
        store.put(write("shelf", "a", json!({})), now()?).await,
        Err(StorageError::Capacity)
    ));
    let snapshot = store.export_raw(now()?).await?;
    assert_eq!(snapshot.revision, i64::MAX);
    assert!(snapshot.records.is_empty());
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn backup_creates_consistent_sqlite_copy_and_rejects_existing() -> TestResult {
    let directory = tempfile::tempdir()?;
    let db_path = directory.path().join("source.sqlite");
    let backup_path = directory.path().join("backup.sqlite");
    let instant = now()?;

    let store = CatalogStore::open(&db_path).await?;
    store
        .put(
            write("title", "movie:550", title(550, "Fight Club", true)),
            instant,
        )
        .await?;

    // Successful backup to non-existing destination
    store.backup(&backup_path).await?;
    assert!(backup_path.exists());

    // Fails if destination already exists
    assert!(matches!(
        store.backup(&backup_path).await,
        Err(StorageError::InvalidInput(_))
    ));

    store.close().await?;

    // Verify backed up database can be opened and has the data
    let backed_up = CatalogStore::open(&backup_path).await?;
    let record = backed_up.get("title", "movie:550", instant).await?;
    assert_eq!(record.data["title"], "Fight Club");
    backed_up.close().await?;

    Ok(())
}
