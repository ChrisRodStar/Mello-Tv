//! Peachify's versioned AES-GCM response envelope.
use crate::{CryptoError, aes_gcm_decrypt};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

const KEY_HEX: &str = "a8f2a1b5e9c470814f6b2c3a5d8e7f9c1a2b3c4d5e3f7a8b8cad1e2d0a4d5c5b";

fn hex_bytes(value: &str) -> Result<Vec<u8>, CryptoError> {
    if !value.len().is_multiple_of(2) {
        return Err(CryptoError::InvalidMaterial);
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let s = std::str::from_utf8(p).map_err(|_| CryptoError::Encoding)?;
            u8::from_str_radix(s, 16).map_err(|_| CryptoError::Encoding)
        })
        .collect()
}

/// Decrypt `base64url(iv).base64url(ciphertext).base64url(tag)` to bounded UTF-8 JSON.
pub fn decrypt(payload: &str) -> Result<Vec<u8>, CryptoError> {
    if payload.len() > crate::MAX_PROTOCOL_BYTES * 2 {
        return Err(CryptoError::InputLimit);
    }
    let mut parts = payload.split('.');
    let (Some(iv), Some(ciphertext), Some(tag), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(CryptoError::InvalidEnvelope);
    };
    let iv = URL_SAFE_NO_PAD
        .decode(iv)
        .map_err(|_| CryptoError::Encoding)?;
    let ciphertext = URL_SAFE_NO_PAD
        .decode(ciphertext)
        .map_err(|_| CryptoError::Encoding)?;
    let tag = URL_SAFE_NO_PAD
        .decode(tag)
        .map_err(|_| CryptoError::Encoding)?;
    if iv.len() != 12 || tag.len() != 16 || ciphertext.len() > crate::MAX_PROTOCOL_BYTES {
        return Err(CryptoError::InvalidEnvelope);
    }
    let mut envelope = ciphertext;
    envelope.extend_from_slice(&tag);
    aes_gcm_decrypt(&hex_bytes(KEY_HEX)?, &iv, &[], &envelope).map(|v| v.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};

    #[test]
    fn decrypts_provider_envelope_and_rejects_bad_shapes() -> Result<(), Box<dyn std::error::Error>>
    {
        let key = hex_bytes(KEY_HEX)?;
        let nonce = [7u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&key)?;
        let nonce = nonce.into();
        let ciphertext = cipher.encrypt(&nonce, br#"{"sources":[]}"#.as_slice())?;
        let split = ciphertext.len() - 16;
        let payload = format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(&ciphertext[..split]),
            URL_SAFE_NO_PAD.encode(&ciphertext[split..])
        );
        assert_eq!(decrypt(&payload)?, br#"{"sources":[]}"#);
        assert!(decrypt("broken").is_err());
        Ok(())
    }
}
