//! VidNest's custom-base64 permutation.
use crate::{CryptoError, aes_gcm_decrypt, sha256};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
const ALPHABET: &[u8; 65] = b"RB0fpH8ZEyVLkv7c2i6MAJ5u3IKFDxlS1NTsnGaqmXYdUrtzjwObCgQP94hoeW+/=";
const V2_KEY_SALT: &[u8] = b"K3p8vQ2mX9rL5wZ0fN7bG1sH4tJ6dC9_y8S3wM1nQ5pL2";

/// Decrypt the current VidNest v2 `{v:2,d,n}` AES-GCM payload.
pub fn decrypt_v2(data: &str, nonce_label: &str) -> Result<Vec<u8>, CryptoError> {
    if data.len() > crate::MAX_PROTOCOL_BYTES * 2
        || nonce_label.is_empty()
        || nonce_label.len() > 256
    {
        return Err(CryptoError::InputLimit);
    }
    let envelope = URL_SAFE_NO_PAD
        .decode(data)
        .map_err(|_| CryptoError::Encoding)?;
    if envelope.len() < 12 + 16 {
        return Err(CryptoError::InvalidEnvelope);
    }
    let key = sha256(&[nonce_label.as_bytes(), b"|", V2_KEY_SALT]);
    let plaintext = aes_gcm_decrypt(&key, &envelope[..12], &[], &envelope[12..])?;
    Ok(plaintext.to_vec())
}
/// Decode the provider's custom alphabet as bounded UTF-8 JSON bytes.
pub fn decrypt(input: &str) -> Result<Vec<u8>, CryptoError> {
    if input.is_empty() || input.len() > crate::MAX_PROTOCOL_BYTES * 2 {
        return Err(CryptoError::InputLimit);
    }
    if !input.len().is_multiple_of(4) {
        return Err(CryptoError::Encoding);
    }
    let mut output = Vec::with_capacity(input.len() * 3 / 4 + 3);
    let mut values = [64u8; 4];
    let mut chunk_len = 0usize;
    let chunks = input.len() / 4;
    for (chunk_index, byte) in input.bytes().enumerate() {
        let value = ALPHABET
            .iter()
            .position(|c| *c == byte)
            .ok_or(CryptoError::Encoding)? as u8;
        values[chunk_len] = value;
        chunk_len += 1;
        if chunk_len == 4 {
            let is_last_chunk = chunk_index / 4 + 1 == chunks;
            if values[0] == 64 || values[1] == 64 {
                return Err(CryptoError::Encoding);
            }
            if (values[2] == 64 && (!is_last_chunk || values[3] != 64))
                || (values[3] == 64 && !is_last_chunk)
            {
                return Err(CryptoError::Encoding);
            }
            output.push((values[0] << 2) | (values[1] >> 4));
            if values[2] != 64 {
                output.push((values[1] << 4) | (values[2] >> 2));
            }
            if values[3] != 64 {
                output.push((values[2] << 6) | values[3]);
            }
            values = [64; 4];
            chunk_len = 0;
        }
    }
    if output.len() > crate::MAX_PROTOCOL_BYTES {
        return Err(CryptoError::InputLimit);
    }
    std::str::from_utf8(&output).map_err(|_| CryptoError::Encoding)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    const STANDARD: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";

    #[test]
    fn decodes_custom_alphabet_json_and_rejects_unknown_characters()
    -> Result<(), Box<dyn std::error::Error>> {
        let plain = br#"{"streams":[]}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(plain);
        let custom: String = encoded
            .bytes()
            .map(|byte| {
                let index = STANDARD
                    .iter()
                    .position(|c| *c == byte)
                    .ok_or(CryptoError::Encoding)?;
                Ok(ALPHABET[index] as char)
            })
            .collect::<Result<_, CryptoError>>()?;
        assert_eq!(decrypt(&custom)?, plain);
        assert!(decrypt("!!").is_err());
        assert!(decrypt("A").is_err());
        assert!(decrypt("AAA").is_err());
        assert!(decrypt("AAAA=AAA").is_err());
        Ok(())
    }

    #[test]
    fn decrypts_current_v2_authenticated_envelope() -> Result<(), Box<dyn std::error::Error>> {
        let label = "fixture nonce label";
        // Fixed vector generated independently with Node.js crypto's AES-256-GCM.
        let independent = "CwsLCwsLCwsLCwsLitj3a0SzZvvTOD1Q8SFsMxZi3CFc19Z_yPaMgC2l";
        assert_eq!(decrypt_v2(independent, label)?, br#"{"streams":[]}"#);
        let nonce = [11u8; 12];
        let key = sha256(&[label.as_bytes(), b"|", V2_KEY_SALT]);
        let cipher = Aes256Gcm::new_from_slice(&key)?;
        let gcm_nonce = nonce.into();
        let encrypted = cipher.encrypt(&gcm_nonce, br#"{"streams":[]}"#.as_slice())?;
        let mut envelope = nonce.to_vec();
        envelope.extend_from_slice(&encrypted);
        let encoded = URL_SAFE_NO_PAD.encode(envelope);
        assert_eq!(decrypt_v2(&encoded, label)?, br#"{"streams":[]}"#);
        assert!(decrypt_v2(&encoded, "wrong label").is_err());
        Ok(())
    }
}
