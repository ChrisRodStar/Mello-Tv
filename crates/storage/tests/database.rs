use std::{error::Error, thread};

use stream_storage::{StorageError, db::Database};

#[tokio::test]
async fn bounds_queue_configuration_and_memory_path() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        Database::in_memory(0).await,
        Err(StorageError::InvalidInput(_))
    ));
    assert!(matches!(
        Database::in_memory(1025).await,
        Err(StorageError::InvalidInput(_))
    ));
    assert!(matches!(
        Database::open(":memory:", 1).await,
        Err(StorageError::InvalidInput(_))
    ));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn runs_sqlite_on_dedicated_thread_and_enforces_foreign_keys() -> Result<(), Box<dyn Error>> {
    let db = Database::in_memory(1).await?;
    let runtime_thread = thread::current().id();
    let (worker_thread, foreign_keys) = db
        .call(|connection| {
            let enabled: i64 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
            Ok((thread::current().id(), enabled))
        })
        .await?;
    assert_ne!(runtime_thread, worker_thread);
    assert_eq!(foreign_keys, 1);
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_caller_does_not_interrupt_an_accepted_mutation() -> Result<(), Box<dyn Error>> {
    let db = Database::in_memory(1).await?;
    db.call(|connection| {
        connection.execute_batch("CREATE TABLE counter(value INTEGER)")?;
        Ok(())
    })
    .await?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let owner = db.clone();
    let pending = tokio::spawn(async move {
        owner
            .call(move |connection| {
                let _ = started_tx.send(());
                release_rx.recv().map_err(|_| StorageError::Closed)?;
                connection.execute("INSERT INTO counter VALUES(1)", [])?;
                Ok(())
            })
            .await
    });
    started_rx.await?;
    pending.abort();
    release_tx.send(())?;
    let count: i64 = db
        .call(|connection| {
            Ok(connection.query_row("SELECT count(*) FROM counter", [], |row| row.get(0))?)
        })
        .await?;
    assert_eq!(count, 1);
    db.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn shutdown_is_global_idempotent_and_releases_files() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory
        .path()
        .join("nested folder")
        .join("catalog café.sqlite");
    let db = Database::open(&path, 2).await?;
    db.call(|connection| {
        connection.execute_batch(
            "CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('kept')",
        )?;
        Ok(())
    })
    .await?;
    let clone = db.clone();
    let (left, right) = tokio::join!(db.shutdown(), clone.shutdown());
    left?;
    right?;
    assert!(matches!(
        clone.call(|_| Ok(())).await,
        Err(StorageError::Closed)
    ));
    let reopened = Database::open(&path, 1).await?;
    let retained: String = reopened
        .call(|connection| {
            Ok(connection.query_row("SELECT value FROM retained", [], |row| row.get(0))?)
        })
        .await?;
    assert_eq!(retained, "kept");
    reopened.shutdown().await?;
    std::fs::remove_file(&path)?;
    Ok(())
}

#[tokio::test]
async fn repository_errors_roll_back_transactions_and_worker_stays_usable()
-> Result<(), Box<dyn Error>> {
    let db = Database::in_memory(1).await?;
    db.call(|connection| {
        connection.execute_batch("CREATE TABLE entries(value INTEGER)")?;
        Ok(())
    })
    .await?;
    let failed: Result<(), StorageError> = db
        .call(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute("INSERT INTO entries VALUES(1)", [])?;
            Err(StorageError::Capacity)
        })
        .await;
    assert!(matches!(failed, Err(StorageError::Capacity)));
    let count: i64 = db
        .call(|connection| {
            Ok(connection.query_row("SELECT count(*) FROM entries", [], |row| row.get(0))?)
        })
        .await?;
    assert_eq!(count, 0);
    db.shutdown().await?;
    Ok(())
}
