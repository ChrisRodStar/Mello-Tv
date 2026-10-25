//! Shared Stream TV contracts and pure validation. No runtime or I/O ownership.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod catalog;
pub mod domain;
pub mod error;
pub mod packages;
pub mod ports;
pub mod providers;

pub use domain::{MediaFormat, MediaType, Profile, Resolution, Title};
pub use error::DomainError;
