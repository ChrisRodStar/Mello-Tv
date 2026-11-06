//! StreamMafia AES-256-GCM page payload decoder.
use crate::{CryptoError, aes_gcm_decrypt};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

const SECRET: &[u8] = b"Z9#rL!v2K*5qP&7mXw";
/// Decrypt the JSON `{iv,tag,data}` envelope, returning UTF-8 JSON bytes.
pub fn decrypt(iv: &str, tag: &str, data: &str) -> Result<Vec<u8>, CryptoError> {
    if iv.len() + tag.len() + data.len() > crate::MAX_PROTOCOL_BYTES * 2 {
        return Err(CryptoError::InputLimit);
    }
    let iv = STANDARD.decode(iv).map_err(|_| CryptoError::Encoding)?;
    let tag = STANDARD.decode(tag).map_err(|_| CryptoError::Encoding)?;
    let data = STANDARD.decode(data).map_err(|_| CryptoError::Encoding)?;
    if iv.len() != 12 || tag.len() != 16 {
        return Err(CryptoError::InvalidEnvelope);
    }
    let mut envelope = data;
    envelope.extend_from_slice(&tag);
    let key = Sha256::digest(SECRET);
    aes_gcm_decrypt(&key, &iv, &[], &envelope).map(|v| v.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};

    #[test]
    fn decrypts_authenticated_wire_fields_and_rejects_tampering()
    -> Result<(), Box<dyn std::error::Error>> {
        let key = Sha256::digest(SECRET);
        let nonce = [9u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&key)?;
        let nonce = nonce.into();
        let ciphertext = cipher.encrypt(&nonce, br#"{"status":"ok"}"#.as_slice())?;
        let split = ciphertext.len() - 16;
        let iv = STANDARD.encode(nonce);
        let tag = STANDARD.encode(&ciphertext[split..]);
        let data = STANDARD.encode(&ciphertext[..split]);
        assert_eq!(decrypt(&iv, &tag, &data)?, br#"{"status":"ok"}"#);
        assert!(decrypt(&iv, &STANDARD.encode([0u8; 16]), &data).is_err());
        Ok(())
    }
}
