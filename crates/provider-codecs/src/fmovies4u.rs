//! CryptoJS passphrase envelope used by Fmovies4U (OpenSSL salted format).
use base64::{Engine as _, engine::general_purpose::STANDARD};
use md5::{Digest, Md5};
use zeroize::{Zeroize, Zeroizing};

use crate::{CryptoError, aes_cbc_decrypt};

const MAX_ENVELOPE_BYTES: usize = 512 * 1024;

/// Decrypts CryptoJS's OpenSSL-compatible `Salted__` AES-256-CBC string.
pub fn decrypt_payload(envelope: &str) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if envelope.is_empty() || envelope.len() > MAX_ENVELOPE_BYTES {
        return Err(CryptoError::InputLimit);
    }
    let binary = STANDARD
        .decode(envelope)
        .map_err(|_| CryptoError::Encoding)?;
    if binary.len() < 32 || &binary[..8] != b"Salted__" {
        return Err(CryptoError::InvalidEnvelope);
    }
    let salt = &binary[8..16];
    let ciphertext = &binary[16..];
    let mut material = Zeroizing::new(Vec::with_capacity(48));
    let mut previous = Zeroizing::new(Vec::new());
    while material.len() < 48 {
        let mut digest = Md5::new();
        digest.update(&previous);
        digest.update(PASSPHRASE_B64_DECODED);
        digest.update(salt);
        let block = digest.finalize();
        previous.zeroize();
        previous.extend_from_slice(&block);
        material.extend_from_slice(&previous);
    }
    aes_cbc_decrypt(&material[..32], &material[32..48], ciphertext)
}

const PASSPHRASE_B64_DECODED: &[u8] = b"fM0v1es4U_2026_SecureKey_DoNotShare_v2_Protected";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aes_cbc_encrypt;

    // Independently generated OpenSSL-compatible fixture: EVP_BytesToKey MD5.
    #[test]
    fn decrypts_salted_aes256_cbc_payload() -> Result<(), Box<dyn std::error::Error>> {
        let salt = *b"salt1234";
        let mut material = Vec::new();
        let mut previous = Vec::new();
        while material.len() < 48 {
            let mut digest = Md5::new();
            digest.update(&previous);
            digest.update(PASSPHRASE_B64_DECODED);
            digest.update(salt);
            previous = digest.finalize().to_vec();
            material.extend_from_slice(&previous);
        }
        let plaintext = br#"{"success":true,"sources":[]}"#;
        let ciphertext = aes_cbc_encrypt(&material[..32], &material[32..48], plaintext)?;
        let mut envelope = b"Salted__".to_vec();
        envelope.extend_from_slice(&salt);
        envelope.extend_from_slice(&ciphertext);
        assert_eq!(
            decrypt_payload(&STANDARD.encode(envelope))?.as_slice(),
            plaintext
        );
        assert!(decrypt_payload("bad").is_err());
        Ok(())
    }

    #[test]
    fn decrypts_independent_openssl_evp_bytes_to_key_vector()
    -> Result<(), Box<dyn std::error::Error>> {
        // Produced by `openssl enc -aes-256-cbc -salt -md md5 -pass pass:<provider passphrase> -a`.
        let fixture = "U2FsdGVkX1/8PPSrrYbAWr+UDKgbuD35f5fyNpe2a2iSL4sV38qy32I6OFsW7L09lF79f0KVWFS7016P+C41NXiFPlM3HLET8ANn8/Ca6Rq9TSnypTXzV+Fpye9rMgZMvvqBTXINdLhf4zJ2qsPLtbhdQKMazDzErnk5auFroCLG0893CNq8Uk3SHEXYt8c7E8Lg+cp9aC0iUIPWqA6nvJtDC9bU7A/7rmQV1iUlD1nhPgv2bcdsvIrb+IS+yNfJRbqQfJIaATJrg0YVh2QlwZhJiSc+4HR1T0PJev9VeSc=";
        assert_eq!(
            decrypt_payload(fixture)?.as_slice(),
            br#"{"success":true,"sources":[{"url":[{"link":"https://cdn.example.org/master.m3u8","type":"hls","quality":"1080p"}],"tracks":[{"kind":"captions","file":"https://cdn.example.org/en.vtt","label":"English"}]}]}"#
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_envelope_conditions() {
        assert!(matches!(decrypt_payload(""), Err(CryptoError::InputLimit)));
        assert!(matches!(
            decrypt_payload(&"a".repeat(MAX_ENVELOPE_BYTES + 1)),
            Err(CryptoError::InputLimit)
        ));
        assert!(matches!(
            decrypt_payload("not-base64!"),
            Err(CryptoError::Encoding)
        ));
        assert!(matches!(
            decrypt_payload(&STANDARD.encode(b"too_short")),
            Err(CryptoError::InvalidEnvelope)
        ));
        assert!(matches!(
            decrypt_payload(&STANDARD.encode(b"NotSalted__123456789012345678901234")),
            Err(CryptoError::InvalidEnvelope)
        ));
    }
}
