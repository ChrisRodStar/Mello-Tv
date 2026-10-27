//! Fingerprinted VidFast static material and its native zero-environment-mask
//! protocol. No player JavaScript, browser environment or bytecode is executed.
//! Provider discovery/network orchestration belongs to the host adapter.

mod codec;

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CryptoError, sha256, vidcore::ProtocolError};

pub use codec::{REQUEST_ALPHABET, challenge_token, decrypt_envelope, pack_request, response_key};

pub const PLAYER_SHA256: &str = "97f52b92a9f7a1c6066f8b6ff3e2ad79f283f2e44f603de379912963f10809d0";
pub const MAX_PLAYER_BYTES: usize = 3 * 1024 * 1024;
pub const MAX_ENVELOPE_BYTES: usize = 512 * 1024;
pub const CHALLENGE_ACTION: &str = "JgQXcWPNH7s";
pub const LIST_ACTION: &str = "dBWKuO8aW6U";
pub const UNLOCK_ACTION: &str = "M6SyMXJBpeg";

#[derive(Debug, Error)]
pub enum VidFastError {
    #[error("VidFast input exceeds its byte limit")]
    InputLimit,
    #[error("unrecognized VidFast static player version")]
    UnknownPlayer,
    #[error("invalid VidFast protocol field: {0}")]
    Invalid(&'static str),
    #[error("VidFast codec material is invalid")]
    StaticMaterial,
    #[error("VidFast protocol codec rejected input")]
    Protocol(#[from] ProtocolError),
    #[error("VidFast protocol cryptography failed")]
    Crypto(#[from] CryptoError),
}

#[derive(Clone, Eq, PartialEq)]
pub struct ProtocolMaterial {
    root: String,
    csrf: String,
}

impl ProtocolMaterial {
    pub fn root(&self) -> &str {
        &self.root
    }
    pub fn csrf_token(&self) -> &str {
        &self.csrf
    }
}

impl std::fmt::Debug for ProtocolMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProtocolMaterial { [REDACTED] }")
    }
}

pub fn recognize_player(body: &[u8]) -> Result<ProtocolMaterial, VidFastError> {
    recognize_player_version(body, PLAYER_SHA256)
}

fn recognize_player_version(
    body: &[u8],
    expected_digest: &str,
) -> Result<ProtocolMaterial, VidFastError> {
    if body.len() > MAX_PLAYER_BYTES {
        return Err(VidFastError::InputLimit);
    }
    let digest: String = sha256(&[body])
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if !crate::ct_eq(digest.as_bytes(), expected_digest.as_bytes()) {
        return Err(VidFastError::UnknownPlayer);
    }
    let source = std::str::from_utf8(body).map_err(|_| VidFastError::Invalid("player encoding"))?;
    static PATTERNS: OnceLock<Result<(Regex, Regex), regex::Error>> = OnceLock::new();
    let (roots, headers) = PATTERNS
        .get_or_init(|| {
            Ok((
                Regex::new(r#""(/zirlaku/u/[A-Za-z0-9_-]{32,512})""#)?,
                Regex::new(r#""X-Csrf-Token":"([A-Za-z0-9_-]{16,128})""#)?,
            ))
        })
        .as_ref()
        .map_err(|_| VidFastError::StaticMaterial)?;
    let mut root = None;
    for captures in roots.captures_iter(source) {
        let value = captures
            .get(1)
            .ok_or(VidFastError::Invalid("player root"))?
            .as_str();
        if root.is_some_and(|root| root != value) {
            return Err(VidFastError::Invalid("ambiguous player root"));
        }
        root = Some(value);
    }
    let mut csrf = None;
    for captures in headers.captures_iter(source) {
        let value = captures
            .get(1)
            .ok_or(VidFastError::Invalid("CSRF header"))?
            .as_str();
        if csrf.is_some_and(|csrf| csrf != value) {
            return Err(VidFastError::Invalid("ambiguous CSRF header"));
        }
        csrf = Some(value);
    }
    Ok(ProtocolMaterial {
        root: root
            .ok_or(VidFastError::Invalid("missing player root"))?
            .to_owned(),
        csrf: csrf
            .ok_or(VidFastError::Invalid("missing CSRF header"))?
            .to_owned(),
    })
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    pub data: String,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Server")
            .field("data", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

pub fn decode_servers(encoded: &str) -> Result<Vec<Server>, VidFastError> {
    let plaintext = decrypt_envelope(encoded)?;
    let servers: Vec<Server> = serde_json::from_slice::<Option<Vec<Server>>>(&plaintext)
        .map_err(|_| VidFastError::Invalid("server list"))?
        .unwrap_or_default();
    if servers.len() > 16 {
        return Err(VidFastError::Invalid("server count"));
    }
    for server in &servers {
        if server.name.is_empty()
            || server.name.len() > 128
            || server.name.contains(['\r', '\n', '\0'])
            || !opaque_token(&server.data, 8192)
        {
            return Err(VidFastError::Invalid("server identity"));
        }
    }
    Ok(servers)
}

pub fn opaque_token(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_DIGEST: &str = "ec8e48186134e2d088456999c8931601825a1d9e05f3913c3e7b76e94553fad1";
    const FIXTURE: &[u8] = include_bytes!("../../testdata/providers/vidfast/player.txt");

    #[test]
    fn original_go_static_player_fixture_is_verified_before_extracting_material()
    -> Result<(), VidFastError> {
        let material = recognize_player_version(FIXTURE, FIXTURE_DIGEST)?;
        assert_eq!(
            material.root(),
            "/zirlaku/u/synthetic_protocol_root_0123456789abcdef"
        );
        assert_eq!(material.csrf_token(), "synthetic_csrf_token_0123456789");
        assert!(!format!("{material:?}").contains("synthetic_csrf_token"));
        assert!(matches!(
            recognize_player(FIXTURE),
            Err(VidFastError::UnknownPlayer)
        ));
        let mut changed = FIXTURE.to_vec();
        changed.push(b' ');
        assert!(matches!(
            recognize_player_version(&changed, FIXTURE_DIGEST),
            Err(VidFastError::UnknownPlayer)
        ));
        Ok(())
    }

    #[test]
    fn independently_fingerprinted_ambiguous_material_is_still_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = std::str::from_utf8(FIXTURE)?;
        for changed in [
            source.replace("/zirlaku/u/", "/unmapped/"),
            source.replace("X-Csrf-Token", "X-Unmapped-Token"),
            source.replacen(
                "synthetic_protocol_root_0123456789abcdef",
                "different_protocol_root_0123456789abcdef",
                1,
            ),
            source.replacen(
                "synthetic_csrf_token_0123456789",
                "different_csrf_token_0123456789",
                1,
            ),
        ] {
            let digest: String = sha256(&[changed.as_bytes()])
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert!(matches!(
                recognize_player_version(changed.as_bytes(), &digest),
                Err(VidFastError::Invalid(_))
            ));
        }
        Ok(())
    }
}
