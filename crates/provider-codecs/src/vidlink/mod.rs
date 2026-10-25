//! VidLink's recognized public-player token codec. The player digest is checked
//! before the host uses this native translation; no downloaded Wasm is executed.

use crate::sha256;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use crypto_secretbox::{
    Nonce, XSalsa20Poly1305,
    aead::{Aead, KeyInit},
};
use stream_core::{
    domain::MediaType,
    providers::{ProviderTitle, TmdbId},
};
use thiserror::Error;
use zeroize::Zeroizing;

pub const PLAYER_SHA256: &str = "27b65f7f7df985942df6197868896adbbdaf230fac7778089a8a0fa259a38f56";
pub const MAX_PLAYER_BYTES: usize = 3 * 1024 * 1024;
pub const TOKEN_LIFETIME_SECONDS: i64 = 120;

// Public key material embedded in the fingerprinted fu.wasm, not a user key.
const PUBLIC_KEY: [u8; 32] = [
    0xc7, 0x51, 0x36, 0xc5, 0x66, 0x8b, 0xbf, 0xe6, 0x5a, 0x7e, 0xca, 0xd4, 0x31, 0xa7, 0x45, 0xdb,
    0x68, 0xb5, 0xf3, 0x81, 0x55, 0x5b, 0x38, 0xd8, 0xf6, 0xc6, 0x99, 0x44, 0x9c, 0xf1, 0x1f, 0xcd,
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VidLinkError {
    #[error("VidLink player exceeds its byte limit")]
    InputLimit,
    #[error("unrecognized VidLink public player version")]
    UnknownPlayer,
    #[error("invalid VidLink title coordinates")]
    InvalidTitle,
    #[error("invalid VidLink token expiry")]
    InvalidExpiry,
    #[error("VidLink public token codec failed")]
    Cipher,
}

/// Evidence of successful static fingerprint recognition; not constructible
/// from an unchecked module or a caller-supplied alternate digest.
#[derive(Clone, Copy, Debug)]
pub struct RecognizedPlayer {
    _verified: (),
}
impl RecognizedPlayer {
    pub fn generate_token(
        self,
        tmdb_id: TmdbId,
        unix_seconds: i64,
    ) -> Result<String, VidLinkError> {
        generate_token(tmdb_id, unix_seconds)
    }
}

pub fn recognize_player(module: &[u8]) -> Result<RecognizedPlayer, VidLinkError> {
    if module.len() > MAX_PLAYER_BYTES {
        return Err(VidLinkError::InputLimit);
    }
    let digest: String = sha256(&[module])
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != PLAYER_SHA256 {
        return Err(VidLinkError::UnknownPlayer);
    }
    Ok(RecognizedPlayer { _verified: () })
}

/// Low-level public protocol codec. Production adapters must first obtain a
/// [`RecognizedPlayer`] from the fetched player and call its token method.
/// The protocol mandates a zero nonce with public material; it must never be
/// reused as application encryption for private data.
pub fn generate_token(tmdb_id: TmdbId, unix_seconds: i64) -> Result<String, VidLinkError> {
    let expiry = unix_seconds
        .checked_add(TOKEN_LIFETIME_SECONDS)
        .filter(|expiry| *expiry >= 0)
        .ok_or(VidLinkError::InvalidExpiry)?;
    let mut message = Zeroizing::new(tmdb_id.get().to_string().into_bytes());
    message.extend_from_slice(&(expiry as u64).to_be_bytes());
    let cipher = XSalsa20Poly1305::new_from_slice(&PUBLIC_KEY).map_err(|_| VidLinkError::Cipher)?;
    let nonce: Nonce = [0u8; 24].into();
    // NaCl secretbox prepends its 16-byte MAC. RustCrypto's compatibility AEAD
    // preserves that layout, unlike AEADs whose tags follow the ciphertext.
    let sealed = cipher
        .encrypt(&nonce, message.as_slice())
        .map_err(|_| VidLinkError::Cipher)?;
    let mut envelope = Vec::with_capacity(24 + sealed.len());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&sealed);
    Ok(URL_SAFE_NO_PAD.encode(envelope))
}

pub fn embed_path(title: &ProviderTitle) -> Result<String, VidLinkError> {
    title.validate().map_err(|_| VidLinkError::InvalidTitle)?;
    match title.media_type {
        MediaType::Movie => Ok(format!("/movie/{}", title.tmdb_id.get())),
        MediaType::Tv => Ok(format!(
            "/tv/{}/{}/{}",
            title.tmdb_id.get(),
            title.season.ok_or(VidLinkError::InvalidTitle)?,
            title.episode.ok_or(VidLinkError::InvalidTitle)?
        )),
    }
}

pub fn api_path(
    player: RecognizedPlayer,
    title: &ProviderTitle,
    unix_seconds: i64,
) -> Result<String, VidLinkError> {
    title.validate().map_err(|_| VidLinkError::InvalidTitle)?;
    let token = player.generate_token(title.tmdb_id, unix_seconds)?;
    match title.media_type {
        MediaType::Movie => Ok(format!("/api/b/movie/{token}?multiLang=1")),
        MediaType::Tv => Ok(format!(
            "/api/b/tv/{token}/{}/{}?multiLang=1",
            title.season.ok_or(VidLinkError::InvalidTitle)?,
            title.episode.ok_or(VidLinkError::InvalidTitle)?
        )),
    }
}
