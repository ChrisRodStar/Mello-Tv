use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use serde_json::json;
use stream_core::accounts::{
    PlaybackRequest, Preferences, Profile, ProfileInput, ProgressUpdate, RelayReady, Target,
};
use stream_storage::{
    AccountStore, StorageError,
    accounts::{ListKind, NewPlayback},
    db::Database,
};
use zeroize::Zeroizing;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const KEY: [u8; 32] = [7; 32];

fn now() -> Result<DateTime<Utc>, chrono::ParseError> {
    "2026-10-25T12:00:00Z".parse()
}

fn profile(id: &str) -> Profile {
    Profile {
        id: id.into(),
        name: "Viewer".into(),
        color: "#446e88".into(),
        preferences: Preferences::default(),
    }
}

async fn store_with_profiles() -> Result<(AccountStore, String), Box<dyn std::error::Error>> {
    let database = Database::in_memory(32).await?;
    let store = AccountStore::from_database(database, KEY).await?;
    let owner = store.select_household(None).await?;
    store.create_profile(&owner, profile("first")).await?;
    store.create_profile(&owner, profile("second")).await?;
    Ok((store, owner))
}

fn request() -> Result<PlaybackRequest, serde_json::Error> {
    serde_json::from_value(json!({"type":"tv","id":1622,"season":0,"episode":1}))
}

fn ready() -> Result<RelayReady, serde_json::Error> {
    serde_json::from_value(json!({
        "contract_version":"relay.v1","session_id":"relay_session","entry_path":"/stream/relay_session/entry",
        "format":"hls","expires_at":"2026-10-25T18:00:00Z","revision":2,"auth_revision":1,
        "source_generation":1,"state":"active","resume_required":false,"available_heights":[1080,720]
    }))
}

fn allocation(id: &str) -> Result<NewPlayback, Box<dyn std::error::Error>> {
    Ok(NewPlayback {
        id: id.into(),
        request: request()?,
        ready: Some(ready()?),
        created_at: now()?,
        expires_at: now()? + Duration::hours(6),
        idempotency_key: format!("idem_{id}"),
        fingerprint: "a".repeat(64),
    })
}

fn update(playback: &str, sequence: i64, position: f64, watched: f64) -> ProgressUpdate {
    ProgressUpdate {
        playback_id: playback.into(),
        sequence,
        position_seconds: position,
        duration_seconds: 100.0,
        watched_seconds: watched,
    }
}

fn legacy_schema() -> Result<&'static str, Box<dyn std::error::Error>> {
    Ok(r#"
CREATE TABLE settings(key TEXT PRIMARY KEY,value BLOB NOT NULL);
CREATE TABLE accounts(id TEXT PRIMARY KEY,username TEXT UNIQUE NOT NULL,password BLOB NOT NULL,salt BLOB NOT NULL,admin INTEGER NOT NULL);
CREATE TABLE sessions(hash TEXT PRIMARY KEY,account TEXT NOT NULL REFERENCES accounts(id),expires INTEGER NOT NULL);
CREATE INDEX session_expiry ON sessions(expires);
CREATE TABLE profiles(id TEXT PRIMARY KEY,account TEXT NOT NULL REFERENCES accounts(id),data BLOB NOT NULL,version INTEGER NOT NULL DEFAULT 1,synced INTEGER NOT NULL DEFAULT 0,last_sync INTEGER NOT NULL DEFAULT 0);
CREATE TABLE lists(profile TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,kind TEXT NOT NULL,target TEXT NOT NULL,data BLOB NOT NULL,PRIMARY KEY(profile,kind,target));
CREATE TABLE progress(profile TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,target TEXT NOT NULL,type TEXT NOT NULL,title_id INTEGER NOT NULL,data BLOB NOT NULL,PRIMARY KEY(profile,target));
CREATE INDEX progress_title ON progress(profile,type,title_id);
CREATE TABLE activity(profile TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,day TEXT NOT NULL,PRIMARY KEY(profile,day));
CREATE TABLE playbacks(id TEXT PRIMARY KEY,profile TEXT NOT NULL,request BLOB NOT NULL,ready BLOB,revoked INTEGER NOT NULL DEFAULT 0,revoked_done INTEGER NOT NULL DEFAULT 0,expires INTEGER NOT NULL,total REAL NOT NULL DEFAULT 0,sequence INTEGER NOT NULL DEFAULT 0,created INTEGER NOT NULL,idem TEXT NOT NULL,fingerprint TEXT NOT NULL,stage BLOB,UNIQUE(profile,idem));
CREATE INDEX playback_expiry ON playbacks(expires);
CREATE TABLE mutations(playback TEXT PRIMARY KEY REFERENCES playbacks(id) ON DELETE CASCADE,idem TEXT NOT NULL,kind TEXT NOT NULL,revision INTEGER NOT NULL,payload BLOB,base BLOB NOT NULL,done INTEGER NOT NULL DEFAULT 0,result BLOB);
CREATE TABLE mutation_history(playback TEXT NOT NULL REFERENCES playbacks(id) ON DELETE CASCADE,idem TEXT NOT NULL,kind TEXT NOT NULL,revision INTEGER NOT NULL,result BLOB NOT NULL,PRIMARY KEY(playback,idem));
CREATE TABLE deletions(handle TEXT PRIMARY KEY);
CREATE TABLE refresh_jobs(key TEXT PRIMARY KEY,data BLOB NOT NULL);
CREATE TABLE snapshot(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL,data BLOB NOT NULL);
PRAGMA user_version=1;"#)
}

fn envelope_fixture(hex: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !hex.len().is_multiple_of(2) {
        return Err("invalid hex fixture".into());
    }
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

#[tokio::test]
async fn profiles_and_independent_edits_survive_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("accounts.sqlite");
    let store = AccountStore::open(&path, KEY).await?;
    let owner = store.select_household(None).await?;
    store.create_profile(&owner, profile("viewer")).await?;
    let preferences: Preferences =
        serde_json::from_value(json!({"completed":true,"format":"tv","genres":["drama"]}))?;
    let (renamed, preferred) = tokio::join!(
        store.update_profile(
            &owner,
            "viewer",
            ProfileInput {
                name: "New name".into(),
                color: "blue".into()
            }
        ),
        store.update_preferences(&owner, "viewer", preferences.clone())
    );
    renamed?;
    preferred?;
    store.shutdown().await?;
    let reopened = AccountStore::open(&path, KEY).await?;
    assert_eq!(reopened.select_household(None).await?, owner);
    let profile = reopened.get_profile(&owner, "viewer").await?;
    assert_eq!(profile.name, "New name");
    assert_eq!(profile.preferences, preferences);
    let version: i64 = reopened
        .database()
        .call(|connection| {
            Ok(connection.query_row(
                "SELECT version FROM profiles WHERE id='viewer'",
                [],
                |row| row.get(0),
            )?)
        })
        .await?;
    assert_eq!(version, 3);
    assert!(!format!("{reopened:?}").contains("[7, 7"));
    reopened.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn exact_go_schema_and_nonce_prefix_envelopes_are_readable() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("legacy.sqlite");
    // Independent Python cryptography AESGCM fixtures, key=bytes([7])*32,
    // nonce=bytes([3])*12; nonce + AESGCM(key).encrypt(nonce, plain, aad).
    // This is Go's GCM.Seal(nonce, nonce, plaintext, aad) envelope layout.
    let sentinel = envelope_fixture(
        "030303030303030303030303439fce6a365173310e32263d867a9e7c841296de7c0751f0aef000e1347ccb7099c09275",
    )?;
    let stage = envelope_fixture(
        "0303030303030303030303035edcd066364d3d36132f2d7ed1758c698e4e859b96e858d47727909cb47775e7eac69c83b040",
    )?;
    {
        let connection = Connection::open(&path)?;
        connection.execute_batch(legacy_schema()?)?;
        connection.execute(
            "INSERT INTO settings VALUES('encryption-check',?1)",
            [&sentinel],
        )?;
        connection.execute(
            "INSERT INTO accounts VALUES('owner','household',X'',X'',1)",
            [],
        )?;
        connection.execute(
            "INSERT INTO profiles(id,account,data) VALUES('viewer','owner',?1)",
            [serde_json::to_vec(&profile("viewer"))?],
        )?;
        connection.execute("INSERT INTO playbacks(id,profile,request,expires,created,idem,fingerprint,stage) VALUES('legacy_playback','viewer',?1,?2,?3,'legacy_idem',?4,?5)",params![serde_json::to_vec(&request()?)?,now()?.timestamp()+21_600,now()?.timestamp(),"a".repeat(64),stage])?;
    }
    let store = AccountStore::open(&path, KEY).await?;
    assert_eq!(store.select_household(None).await?, "owner");
    assert_eq!(store.get_profile("owner", "viewer").await?.name, "Viewer");
    let stage = store
        .load_stage("owner", "viewer", "legacy_playback")
        .await?
        .ok_or("missing stage")?;
    assert_eq!(stage.as_slice(), br#"{"selection":"secret"}"#);
    let retained: Vec<u8> = store
        .database()
        .call(|connection| {
            Ok(connection.query_row(
                "SELECT value FROM settings WHERE key='encryption-check'",
                [],
                |row| row.get(0),
            )?)
        })
        .await?;
    assert_eq!(retained, sentinel);
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn wrong_key_and_corrupt_sentinel_are_not_replaced() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("accounts.sqlite");
    let store = AccountStore::open(&path, KEY).await?;
    let sentinel: Vec<u8> = store
        .database()
        .call(|connection| {
            Ok(connection.query_row(
                "SELECT value FROM settings WHERE key='encryption-check'",
                [],
                |row| row.get(0),
            )?)
        })
        .await?;
    store.shutdown().await?;
    assert!(matches!(
        AccountStore::open(&path, [8; 32]).await,
        Err(StorageError::Crypto)
    ));
    let connection = Connection::open(&path)?;
    assert_eq!(
        connection.query_row(
            "SELECT value FROM settings WHERE key='encryption-check'",
            [],
            |row| row.get::<_, Vec<u8>>(0)
        )?,
        sentinel
    );
    connection.execute(
        "UPDATE settings SET value=X'010203' WHERE key='encryption-check'",
        [],
    )?;
    drop(connection);
    assert!(matches!(
        AccountStore::open(&path, KEY).await,
        Err(StorageError::Crypto)
    ));
    let connection = Connection::open(&path)?;
    assert_eq!(
        connection.query_row(
            "SELECT value FROM settings WHERE key='encryption-check'",
            [],
            |row| row.get::<_, Vec<u8>>(0)
        )?,
        vec![1, 2, 3]
    );
    Ok(())
}

#[tokio::test]
async fn unsupported_and_unrelated_databases_are_rejected_without_mutation() -> TestResult {
    let directory = tempfile::tempdir()?;
    for (name, schema) in [
        (
            "future",
            "CREATE TABLE important(value TEXT); INSERT INTO important VALUES('retain'); PRAGMA user_version=2;",
        ),
        (
            "unrelated",
            "CREATE TABLE important(value TEXT); INSERT INTO important VALUES('retain');",
        ),
        (
            "impostor",
            "CREATE TABLE important(value TEXT); INSERT INTO important VALUES('retain'); PRAGMA user_version=1;",
        ),
    ] {
        let path = directory.path().join(name);
        let connection = Connection::open(&path)?;
        connection.execute_batch(schema)?;
        drop(connection);
        let before = std::fs::read(&path)?;
        assert!(AccountStore::open(&path, KEY).await.is_err());
        assert_eq!(std::fs::read(&path)?, before);
    }
    Ok(())
}

#[tokio::test]
async fn owners_and_profiles_are_isolated_and_deletion_cascades() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    store
        .database()
        .call(|connection| {
            connection.execute(
                "INSERT INTO accounts VALUES('other_owner','other',X'',X'',1)",
                [],
            )?;
            Ok(())
        })
        .await?;
    assert!(matches!(
        store.get_profile("other_owner", "first").await,
        Err(StorageError::NotFound)
    ));
    let title = serde_json::from_value(
        json!({"type":"movie","id":603,"title":"The Matrix","overview":"","rating":8.7,"details":false}),
    )?;
    store
        .put_title(&owner, "first", ListKind::Library, title)
        .await?;
    assert!(
        store
            .list_titles(&owner, "second", ListKind::Library)
            .await?
            .is_empty()
    );
    store
        .create_playback(&owner, "first", allocation("playback")?)
        .await?;
    store
        .update_progress_at(
            &owner,
            "first",
            update("playback", 1, 20.0, 20.0),
            now()? + Duration::seconds(30),
        )
        .await?;
    assert!(matches!(
        store.get_playback(&owner, "second", "playback").await,
        Err(StorageError::NotFound)
    ));
    store.delete_profile(&owner, "first").await?;
    let state: (i64, i64, i64, i64, bool) = store
        .database()
        .call(|connection| {
            Ok((
                connection.query_row(
                    "SELECT count(*) FROM lists WHERE profile='first'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT count(*) FROM progress WHERE profile='first'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT count(*) FROM activity WHERE profile='first'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT count(*) FROM deletions WHERE handle='first'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT revoked FROM playbacks WHERE id='playback'",
                    [],
                    |row| row.get(0),
                )?,
            ))
        })
        .await?;
    assert_eq!(state, (0, 0, 0, 1, true));
    assert_eq!(store.list_profiles(&owner).await?.len(), 1);
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn progress_sequences_counters_seeks_and_completion_match_go() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    store
        .create_playback(&owner, "first", allocation("playback")?)
        .await?;
    let first = store
        .update_progress_at(
            &owner,
            "first",
            update("playback", 1, 80.0, 30.0),
            now()? + Duration::seconds(30),
        )
        .await?;
    assert_eq!(first.position_seconds, 80.0);
    assert!(!first.completed);
    let stale = store
        .update_progress_at(
            &owner,
            "first",
            update("playback", 1, 1.0, 1.0),
            now()? + Duration::seconds(40),
        )
        .await?;
    assert_eq!(stale, first);
    assert!(matches!(
        store
            .update_progress_at(
                &owner,
                "first",
                update("playback", 2, 1.0, 29.0),
                now()? + Duration::seconds(40)
            )
            .await,
        Err(StorageError::Conflict("counter_regression"))
    ));
    let seek = store
        .update_progress_at(
            &owner,
            "first",
            update("playback", 2, 10.0, 40.0),
            now()? + Duration::seconds(40),
        )
        .await?;
    assert_eq!(seek.position_seconds, 10.0);
    assert_eq!(seek.watched_seconds, 40.0);
    let finished = store
        .update_progress_at(
            &owner,
            "first",
            update("playback", 3, 150.0, 60.0),
            now()? + Duration::seconds(60),
        )
        .await?;
    assert_eq!(finished.position_seconds, 100.0);
    assert!(finished.completed);
    let rewound = store
        .update_progress_at(
            &owner,
            "first",
            update("playback", 4, 0.0, 60.0),
            now()? + Duration::seconds(70),
        )
        .await?;
    assert!(rewound.completed);
    let stored = store
        .get_progress(&owner, "first", request()?.target)
        .await?
        .ok_or("missing progress")?;
    assert_eq!(stored, rewound);
    let version: i64 = store
        .database()
        .call(|connection| {
            Ok(connection.query_row(
                "SELECT version FROM profiles WHERE id='first'",
                [],
                |row| row.get(0),
            )?)
        })
        .await?;
    assert_eq!(version, 5); // four applied writes; stale/conflicting writes add none.
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn separate_playbacks_add_only_each_sessions_monotonic_delta() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    store
        .create_playback(&owner, "first", allocation("one")?)
        .await?;
    store
        .create_playback(&owner, "first", allocation("two")?)
        .await?;
    let (one, two) = tokio::join!(
        store.update_progress_at(
            &owner,
            "first",
            update("one", 1, 10.0, 10.0),
            now()? + Duration::seconds(10)
        ),
        store.update_progress_at(
            &owner,
            "first",
            update("two", 1, 20.0, 20.0),
            now()? + Duration::seconds(20)
        ),
    );
    one?;
    two?;
    let result = store
        .update_progress_at(
            &owner,
            "first",
            update("one", 2, 15.0, 15.0),
            now()? + Duration::seconds(30),
        )
        .await?;
    assert_eq!(result.watched_seconds, 35.0);
    assert!(
        store
            .get_progress(&owner, "second", request()?.target)
            .await?
            .is_none()
    );
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn progress_rejects_pending_revoked_expired_and_implausible_updates() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    let mut pending = allocation("pending")?;
    pending.ready = None;
    store.create_playback(&owner, "first", pending).await?;
    assert!(matches!(
        store
            .update_progress_at(&owner, "first", update("pending", 1, 1.0, 1.0), now()?)
            .await,
        Err(StorageError::Conflict("setup_pending"))
    ));
    store
        .create_playback(&owner, "first", allocation("playback")?)
        .await?;
    assert!(matches!(
        store
            .update_progress_at(&owner, "first", update("playback", 1, 1.0, 31.0), now()?)
            .await,
        Err(StorageError::InvalidInput(_))
    ));
    assert!(matches!(
        store
            .update_progress_at(
                &owner,
                "first",
                update("playback", 1, 1.0, 1.0),
                now()? + Duration::hours(6) + Duration::seconds(301)
            )
            .await,
        Err(StorageError::Conflict("session_expired"))
    ));
    store
        .update_progress_at(
            &owner,
            "first",
            update("playback", 1, 1.0, 1.0),
            now()? + Duration::hours(6) + Duration::seconds(300),
        )
        .await?;
    store.revoke_playback(&owner, "first", "playback").await?;
    assert!(matches!(
        store
            .update_progress_at(
                &owner,
                "first",
                update("playback", 2, 2.0, 2.0),
                now()? + Duration::seconds(2)
            )
            .await,
        Err(StorageError::Conflict("session_expired"))
    ));
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn idempotency_and_stage_encryption_are_profile_scoped() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    let mut pending = allocation("one")?;
    pending.ready = None;
    let original = store.create_playback(&owner, "first", pending).await?;
    let mut repeat = allocation("ignored_new_id")?;
    repeat.idempotency_key = "idem_one".into();
    let recovered = store
        .create_playback(&owner, "first", repeat.clone())
        .await?;
    assert_eq!(recovered.id, original.id);
    repeat.fingerprint = "b".repeat(64);
    assert!(matches!(
        store.create_playback(&owner, "first", repeat).await,
        Err(StorageError::Conflict("idempotency_conflict"))
    ));
    store
        .save_stage(
            &owner,
            "first",
            "one",
            Zeroizing::new(b"private-provider-credentials".to_vec()),
        )
        .await?;
    let stage = store
        .load_stage(&owner, "first", "one")
        .await?
        .ok_or("missing stage")?;
    assert_eq!(stage.as_slice(), b"private-provider-credentials");
    let encrypted: Vec<u8> = store
        .database()
        .call(|connection| {
            Ok(
                connection.query_row("SELECT stage FROM playbacks WHERE id='one'", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .await?;
    assert!(
        !encrypted
            .windows(b"private-provider-credentials".len())
            .any(|value| value == b"private-provider-credentials")
    );
    assert!(matches!(
        store.load_stage(&owner, "second", "one").await,
        Err(StorageError::NotFound)
    ));
    store
        .create_playback(&owner, "first", allocation("two")?)
        .await?;
    store
        .database()
        .call(move |connection| {
            connection.execute("UPDATE playbacks SET stage=?1 WHERE id='two'", [encrypted])?;
            Ok(())
        })
        .await?;
    assert!(matches!(
        store.load_stage(&owner, "first", "two").await,
        Err(StorageError::Crypto)
    ));
    store.save_ready(&owner, "first", "one", ready()?).await?;
    assert!(store.load_stage(&owner, "first", "one").await?.is_none());
    store.save_ready(&owner, "first", "one", ready()?).await?;
    let mut late = ready()?;
    late.revision += 1;
    assert!(matches!(
        store.save_ready(&owner, "first", "one", late).await,
        Err(StorageError::Conflict("ready_already_registered"))
    ));
    assert!(matches!(
        store
            .save_stage(&owner, "first", "one", Zeroizing::new(vec![1]))
            .await,
        Err(StorageError::Conflict("ready_already_registered"))
    ));
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn lists_enforce_limits_allow_replacement_and_increment_versions() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    for id in 1..=50 {
        let title = serde_json::from_value(
            json!({"type":"movie","id":id,"title":"Title","overview":"","rating":0.0,"details":false}),
        )?;
        store
            .put_title(&owner, "first", ListKind::Favorites, title)
            .await?;
    }
    let replacement = serde_json::from_value(
        json!({"type":"movie","id":1,"title":"Updated","overview":"","rating":0.0,"details":false}),
    )?;
    store
        .put_title(&owner, "first", ListKind::Favorites, replacement)
        .await?;
    let overflow = serde_json::from_value(
        json!({"type":"movie","id":51,"title":"Title","overview":"","rating":0.0,"details":false}),
    )?;
    assert!(matches!(
        store
            .put_title(&owner, "first", ListKind::Favorites, overflow)
            .await,
        Err(StorageError::Conflict("list_limit"))
    ));
    let titles = store
        .list_titles(&owner, "first", ListKind::Favorites)
        .await?;
    assert_eq!(titles.len(), 50);
    assert!(titles.iter().any(|title| title.title == "Updated"));
    store
        .remove_title(
            &owner,
            "first",
            ListKind::Favorites,
            titles.first().ok_or("missing title")?.identity,
        )
        .await?;
    assert_eq!(
        store
            .list_titles(&owner, "first", ListKind::Favorites)
            .await?
            .len(),
        49
    );
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn household_ambiguity_requires_explicit_selection() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    store
        .database()
        .call(|connection| {
            connection.execute("DELETE FROM settings WHERE key='household-account-id'", [])?;
            connection.execute(
                "INSERT INTO accounts VALUES('other_owner','other',X'',X'',1)",
                [],
            )?;
            Ok(())
        })
        .await?;
    assert!(matches!(
        store.select_household(None).await,
        Err(StorageError::Conflict("multiple_household_owners"))
    ));
    assert_eq!(store.select_household(Some(&owner)).await?, owner);
    assert!(matches!(
        store.select_household(Some("missing_owner")).await,
        Err(StorageError::NotFound)
    ));
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn empty_go_stage_values_and_unready_sessions_are_not_invented() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    let mut playback = allocation("pending")?;
    playback.ready = None;
    store.create_playback(&owner, "first", playback).await?;
    let result = store.get_playback(&owner, "first", "pending").await?;
    assert!(result.ready.is_none());
    assert!(
        store
            .load_stage(&owner, "first", "pending")
            .await?
            .is_none()
    );
    store
        .database()
        .call(|connection| {
            connection.execute("UPDATE playbacks SET stage=X'' WHERE id='pending'", [])?;
            Ok(())
        })
        .await?;
    assert!(
        store
            .load_stage(&owner, "first", "pending")
            .await?
            .is_none()
    );
    assert!(
        store
            .get_progress(
                &owner,
                "first",
                serde_json::from_value::<Target>(json!({"type":"movie","id":603}))?
            )
            .await?
            .is_none()
    );
    store.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn matching_column_names_do_not_bypass_constraint_ownership_checks() -> TestResult {
    let directory = tempfile::tempdir()?;
    let original = legacy_schema()?;
    for (name, altered) in [
        (
            "foreign_keys",
            original.replace(" REFERENCES profiles(id) ON DELETE CASCADE", ""),
        ),
        (
            "primary_key",
            original.replace("handle TEXT PRIMARY KEY", "handle TEXT"),
        ),
        (
            "affinity",
            original.replace("data BLOB NOT NULL", "data TEXT NOT NULL"),
        ),
        (
            "default",
            original.replace(
                "version INTEGER NOT NULL DEFAULT 1",
                "version INTEGER NOT NULL DEFAULT 2",
            ),
        ),
        (
            "index",
            original.replace(
                "CREATE INDEX progress_title ON progress(profile,type,title_id);",
                "",
            ),
        ),
    ] {
        let path = directory.path().join(name);
        let connection = Connection::open(&path)?;
        connection.execute_batch(&altered)?;
        drop(connection);
        let before = std::fs::read(&path)?;
        assert!(matches!(
            AccountStore::open(&path, KEY).await,
            Err(StorageError::Corrupt(_))
        ));
        assert_eq!(std::fs::read(&path)?, before);
    }
    Ok(())
}

#[tokio::test]
async fn rejecting_legacy_key_does_not_switch_journal_or_rewrite_sentinel() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("legacy.sqlite");
    let sentinel = envelope_fixture(
        "030303030303030303030303439fce6a365173310e32263d867a9e7c841296de7c0751f0aef000e1347ccb7099c09275",
    )?;
    let connection = Connection::open(&path)?;
    connection.execute_batch(legacy_schema()?)?;
    connection.execute(
        "INSERT INTO settings VALUES('encryption-check',?1)",
        [sentinel],
    )?;
    drop(connection);
    let before = std::fs::read(&path)?;
    assert!(matches!(
        AccountStore::open(&path, [8; 32]).await,
        Err(StorageError::Crypto)
    ));
    assert_eq!(std::fs::read(&path)?, before);
    assert!(!directory.path().join("legacy.sqlite-wal").exists());
    let connection = Connection::open(&path)?;
    assert_eq!(
        connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?,
        "delete"
    );
    Ok(())
}

#[tokio::test]
async fn playback_allocation_rejects_nonpositive_or_inverted_timestamps() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    let mut bad_created = allocation("bad_created")?;
    bad_created.created_at = DateTime::from_timestamp(0, 0).ok_or("valid zero timestamp")?;
    assert!(matches!(
        store.create_playback(&owner, "first", bad_created).await,
        Err(StorageError::InvalidInput(_))
    ));

    let mut go_zero = allocation("go_zero")?;
    go_zero.created_at =
        DateTime::from_timestamp(-62_135_596_800, 0).ok_or("valid go-zero timestamp")?;
    assert!(matches!(
        store.create_playback(&owner, "first", go_zero).await,
        Err(StorageError::InvalidInput(_))
    ));
    Ok(())
}

#[tokio::test]
async fn mutate_playback_rejects_overflowing_revision() -> TestResult {
    let (store, owner) = store_with_profiles().await?;
    store
        .create_playback(&owner, "first", allocation("mutate_overflow")?)
        .await?;
    let mut new_ready = ready()?;
    new_ready.revision = 2;
    assert!(matches!(
        store
            .mutate_playback(
                &owner,
                "first",
                "mutate_overflow",
                "idem_mutate_overflow",
                "refresh",
                u64::MAX,
                new_ready
            )
            .await,
        Err(StorageError::InvalidInput(_))
    ));
    Ok(())
}
