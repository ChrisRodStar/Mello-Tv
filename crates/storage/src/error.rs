use thiserror::Error;

/// Internal causes are retained; HTTP handlers must map to sanitized errors.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite operation failed")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored JSON could not be encoded or decoded")]
    Json(#[from] serde_json::Error),
    #[error("database filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("database worker is closed")]
    Closed,
    #[error("invalid storage input: {0}")]
    InvalidInput(&'static str),
    #[error("database schema {found} is newer than supported schema {supported}")]
    UnsupportedSchema { found: i64, supported: i64 },
    #[error("storage quota exceeded")]
    Capacity,
    #[error("stored data is inconsistent: {0}")]
    Corrupt(&'static str),
    #[error("record not found")]
    NotFound,
    #[error("storage conflict: {0}")]
    Conflict(&'static str),
    #[error("stored ciphertext or encryption key is invalid")]
    Crypto,
}
