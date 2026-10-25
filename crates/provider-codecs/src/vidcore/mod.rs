//! Native translations of the three fingerprinted VidCore player protocols.
//!
//! No JavaScript or bytecode is executed. [`Protocol::from_source`] verifies
//! material and decoded instruction fingerprints before reading known constants.
//! Low-level [`ProtocolParameters`] are useful for interoperability fixtures, but
//! do not establish that an upstream player version has been audited.

mod crypto;
mod program;

use thiserror::Error;
use zeroize::Zeroizing;

use crate::CryptoError;

pub use crypto::permutation;
pub use program::{Material, decode_program, extract_csrf_token, parse_material, string_constant};

pub const MAX_PLAYER_BYTES: usize = 2_500_000;
pub const MAX_SESSION_BYTES: usize = 64 * 1024;
pub const MAX_CHALLENGE_RESPONSE_BYTES: usize = 64 * 1024;
pub const URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("VidCore input exceeds its byte limit")]
    InputLimit,
    #[error("unsupported VidCore player structure")]
    UnsupportedStructure,
    #[error("unmapped VidCore player material version")]
    UnknownMaterial,
    #[error("unmapped VidCore player instruction version")]
    UnknownInstructions,
    #[error("invalid VidCore protocol field: {0}")]
    Invalid(&'static str),
    #[error("VidCore protocol codec failed")]
    Crypto(#[from] CryptoError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolVersion {
    Legacy,
    Current,
    V3,
}

/// Dimensions are fixed by the known translated protocol. Prefixes may differ
/// between versions; transformation operations are fixed rather than supplied
/// by downloaded programs.
#[derive(Clone)]
pub struct ProtocolParameters {
    pub base: String,
    pub challenge_action: String,
    pub list_action: String,
    pub unlock_action: String,
    pub csrf_token: String,
    pub key: [u8; 32],
    pub iv: [u8; 16],
    pub seed: Vec<u8>,
    pub response_seed: [u8; 32],
    pub response_numbers: Vec<u64>,
    /// Targets in standard URL_ALPHABET source order.
    pub alphabet: [u8; 64],
    pub stage_keys: [[u8; 32]; 5],
    pub stage_prefixes: [Vec<u8>; 5],
    pub rc4_keys: [[u8; 32]; 5],
}

impl std::fmt::Debug for ProtocolParameters {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProtocolParameters { [REDACTED] }")
    }
}

#[derive(Clone)]
pub struct Protocol {
    parameters: ProtocolParameters,
    version: Option<ProtocolVersion>,
}

impl std::fmt::Debug for Protocol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Protocol")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl Protocol {
    /// Compile only a recognized player version; unknown programs fail closed.
    pub fn from_source(source: &str) -> Result<Self, ProtocolError> {
        program::compile(source)
    }

    /// Construct a low-level codec from explicitly supplied constants. This is
    /// not proof of an audited upstream version; network adapters must use
    /// `from_source` for player discovery.
    pub fn from_parameters(parameters: ProtocolParameters) -> Result<Self, ProtocolError> {
        if parameters.base.is_empty()
            || parameters.base.len() > 2048
            || !parameters.base.starts_with('/')
            || parameters.base.starts_with("//")
            || parameters.base.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(ProtocolError::Invalid("base"));
        }
        for action in [
            &parameters.challenge_action,
            &parameters.list_action,
            &parameters.unlock_action,
        ] {
            if action.is_empty()
                || action.len() > 256
                || action.bytes().any(|byte| byte.is_ascii_control())
            {
                return Err(ProtocolError::Invalid("action"));
            }
        }
        if parameters.csrf_token.len() > 4096
            || parameters
                .csrf_token
                .bytes()
                .any(|byte| byte.is_ascii_control())
        {
            return Err(ProtocolError::Invalid("CSRF token"));
        }
        if parameters.seed.is_empty()
            || parameters.seed.len() > 65536
            || parameters.response_numbers.len() > 16
            || parameters
                .stage_prefixes
                .iter()
                .any(|prefix| prefix.len() > 65536)
        {
            return Err(ProtocolError::Invalid("key dimensions"));
        }
        let mut seen = [false; 128];
        for value in parameters.alphabet {
            if !value.is_ascii_graphic() {
                return Err(ProtocolError::Invalid("alphabet"));
            }
            let entry = seen
                .get_mut(usize::from(value))
                .ok_or(ProtocolError::Invalid("alphabet"))?;
            if *entry {
                return Err(ProtocolError::Invalid("alphabet"));
            }
            *entry = true;
        }
        Ok(Self {
            parameters,
            version: None,
        })
    }

    pub fn version(&self) -> Option<ProtocolVersion> {
        self.version
    }
    pub fn base(&self) -> &str {
        &self.parameters.base
    }
    pub fn challenge_action(&self) -> &str {
        &self.parameters.challenge_action
    }
    pub fn list_action(&self) -> &str {
        &self.parameters.list_action
    }
    pub fn unlock_action(&self) -> &str {
        &self.parameters.unlock_action
    }
    pub fn csrf_token(&self) -> &str {
        &self.parameters.csrf_token
    }

    /// The caller supplies a fresh cryptographically random nonce and trusted
    /// Unix milliseconds. No hidden clock or RNG makes this codec nondeterministic.
    pub fn challenge(
        &self,
        session: &str,
        unix_milliseconds: i64,
        nonce: &[u8],
    ) -> Result<String, ProtocolError> {
        crypto::challenge(&self.parameters, session, unix_milliseconds, nonce)
    }

    pub fn ticket(&self, challenge_response: &[u8]) -> Result<String, ProtocolError> {
        crypto::ticket(&self.parameters, challenge_response)
    }

    pub fn decode_response(&self, body: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtocolError> {
        crypto::decode_response(&self.parameters, body)
    }
}
