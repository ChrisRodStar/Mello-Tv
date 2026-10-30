//! Shared provider contracts and pure validation. No runtime or I/O ownership.

#![forbid(unsafe_code)]

pub mod domain;
pub mod error;
pub mod packages;
pub mod ports;
pub mod providers;

pub use domain::MediaType;
pub use error::DomainError;
