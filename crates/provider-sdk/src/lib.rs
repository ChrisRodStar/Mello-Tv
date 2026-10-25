//! Versioned provider component interface and safe JSON contract helpers.
//!
//! The SDK intentionally contains no pointer ABI or ambient network/filesystem
//! APIs. Guest generators consume the included WIT using their supported tools.

pub mod http;

use std::{fmt, io::Write};

use stream_core::providers::{AdapterResult, Lookup, ProviderId, ValidationError};
use thiserror::Error;

pub const INTERFACE_WIT: &str = include_str!("../wit/provider.wit");
pub const INTERFACE_PACKAGE: &str = "stream-tv:provider@1.0.0";
pub const MAX_LOOKUP_BYTES: usize = 32 * 1024;
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;

#[derive(Error)]
pub enum ContractError {
    #[error("provider JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("provider contract validation failed")]
    Validation(#[from] ValidationError),
    #[error("provider JSON exceeds its byte limit")]
    InputLimit,
    #[error("provider JSON encoding is invalid")]
    Encoding,
}

impl fmt::Debug for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

/// The trusted host supplies credential partitioning separately; context_key
/// cannot be reconstructed or overridden through the guest's JSON payload.
pub fn decode_lookup(json: &str) -> Result<Lookup, ContractError> {
    if json.len() > MAX_LOOKUP_BYTES {
        return Err(ContractError::InputLimit);
    }
    let lookup: Lookup = serde_json::from_str(json)?;
    lookup.validate()?;
    Ok(lookup)
}

pub fn encode_result(
    result: &AdapterResult,
    provider: &ProviderId,
    lookup: &Lookup,
) -> Result<String, ContractError> {
    result.validate_for(provider, &lookup.title, 32)?;
    // Enforce the wire limit while serializing, before an oversized provider
    // payload can allocate an unrestricted intermediate JSON string.
    let mut writer = BoundedJson {
        bytes: Vec::new(),
        exhausted: false,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, result) {
        return Err(if writer.exhausted {
            ContractError::InputLimit
        } else {
            ContractError::Json(error)
        });
    }
    String::from_utf8(writer.bytes).map_err(|_| ContractError::Encoding)
}

struct BoundedJson {
    bytes: Vec<u8>,
    exhausted: bool,
}

impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|length| length > MAX_RESULT_BYTES)
        {
            self.exhausted = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "provider JSON byte limit exceeded",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
