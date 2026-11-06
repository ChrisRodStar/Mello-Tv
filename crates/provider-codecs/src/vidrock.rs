//! VidRock URL envelope observed in its player on 2026-11-06.
//! AES-256-GCM: 12-byte nonce followed by ciphertext and a 16-byte tag.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use zeroize::Zeroizing;

use crate::{CryptoError, aes_gcm_decrypt};

const KEY: [u8; 32] = [
    0x7f, 0x3e, 0x9c, 0x2a, 0x8b, 0x5d, 0x1f, 0x4e, 0x6a, 0x9c, 0x3b, 0x7d, 0x2e, 0x5f, 0x8a, 0x1c,
    0x4b, 0x6d, 0x9e, 0x2f, 0x5a, 0x8c, 0x1b, 0x4d, 0x7e, 0x9f, 0x2a, 0x5c, 0x8b, 0x1d, 0x4e, 0x7f,
];

pub fn decrypt_url(encoded: &str) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if encoded.len() > 16 * 1024 {
        return Err(CryptoError::InputLimit);
    }
    let envelope = URL_SAFE_NO_PAD
        .decode(encoded.trim_end_matches('='))
        .map_err(|_| CryptoError::Encoding)?;
    if envelope.len() < 28 {
        return Err(CryptoError::InvalidEnvelope);
    }
    aes_gcm_decrypt(&KEY, &envelope[..12], &[], &envelope[12..])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_node_gcm_vector_rejects_tampering_and_bounds() {
        let envelope = "BwcHBwcHBwcHBwcHUIDmI2upLjuC1aGKIukhZeM6LFAjnj7NR4QXN3j7KptnsoFvi_ClGoJ1JLGTJininJ-BBXw";
        assert_eq!(
            decrypt_url(envelope).ok().as_deref().map(|v| v.as_slice()),
            Some(b"https://media.example.com/master.m3u8".as_slice())
        );
        let mut bytes = URL_SAFE_NO_PAD.decode(envelope).unwrap_or_default();
        bytes[20] ^= 1;
        assert_eq!(
            decrypt_url(&URL_SAFE_NO_PAD.encode(bytes)),
            Err(CryptoError::Authentication)
        );
        assert!(decrypt_url("AAAA").is_err());
        assert!(decrypt_url(&"A".repeat(16385)).is_err());
    }
}
