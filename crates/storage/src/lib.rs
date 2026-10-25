//! Persistent repositories and bounded caches. All SQLite I/O is off-runtime.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod cache;
pub mod catalog;
pub mod db;
mod error;
mod repository;

pub use accounts::AccountStore;
pub use catalog::CatalogStore;
pub use error::StorageError;
