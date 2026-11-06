//! Bounded AES-CBC envelope used by 02MovieDownloader.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

use crate::{CryptoError, aes_cbc_decrypt, sha256};

/// Decrypts the provider's `base64(iv):base64(ciphertext)` payload.
pub fn decrypt_payload(envelope: &str, token: &str) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if envelope.len() > 512 * 1024 || token.is_empty() || token.len() > 4096 {
        return Err(CryptoError::InputLimit);
    }
    let (iv, ciphertext) = envelope
        .split_once(':')
        .ok_or(CryptoError::InvalidEnvelope)?;
    if ciphertext.contains(':') {
        return Err(CryptoError::InvalidEnvelope);
    }
    let iv = STANDARD.decode(iv).map_err(|_| CryptoError::Encoding)?;
    let ciphertext = STANDARD
        .decode(ciphertext)
        .map_err(|_| CryptoError::Encoding)?;
    if iv.len() != 16 || ciphertext.len() > 512 * 1024 {
        return Err(CryptoError::InvalidEnvelope);
    }
    let key = Zeroizing::new(sha256(&[token.as_bytes()]));
    aes_cbc_decrypt(key.as_ref(), &iv, &ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aes_cbc_encrypt;
    use base64::engine::general_purpose::STANDARD;

    #[test]
    fn round_trips_provider_envelope_and_rejects_malformed_material()
    -> Result<(), Box<dyn std::error::Error>> {
        let token = "fixture-token";
        let iv = [7u8; 16];
        let key = sha256(&[token.as_bytes()]);
        let ciphertext = aes_cbc_encrypt(&key, &iv, br#"{"success":true}"#)?;
        let envelope = format!("{}:{}", STANDARD.encode(iv), STANDARD.encode(ciphertext));
        assert_eq!(
            decrypt_payload(&envelope, token)?.as_slice(),
            br#"{"success":true}"#
        );
        assert!(decrypt_payload("bad", token).is_err());
        Ok(())
    }

    #[test]
    fn decrypts_independent_openssl_vector() -> Result<(), Box<dyn std::error::Error>> {
        // OpenSSL AES-256-CBC vector with SHA-256(token) and a repeated 0x07 IV.
        let envelope = "BwcHBwcHBwcHBwcHBwcHBw==:DDFZ+L01hXL+lGa5irero60ttrWfToHmqqHrDfgq/ry2WFT2foCYDUkURkYTkQDi5vwJVFpdAcQwYC30ibNO/bTZIBqqHlUZttIB0pBbv1wktgS8TR/T3G7zTn73S9SQ4VZECjbkgvA5vc9sNX7GGQ==";
        assert_eq!(
            decrypt_payload(envelope, "fixture-token")?.as_slice(),
            br#"{"success":true,"externalStreams":[{"url":"https://cdn.example.org/video.mp4","quality":"1080p","type":"mp4"}]}"#
        );
        Ok(())
    }
}
