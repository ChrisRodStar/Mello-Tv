//! Pure, bounded provider codecs shared by native hosts and component guests.
//! Bounded implementations of legacy provider protocol primitives. These are
//! interoperability codecs; application state uses authenticated encryption.

pub mod vidcore;
pub mod videasy;
pub mod vidfast;
pub mod vidsrc;
pub mod vidlink;

use aes::cipher::{
    BlockModeDecrypt, BlockModeEncrypt, KeyIvInit, StreamCipher, StreamCipherSeek,
    block_padding::Pkcs7,
};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroizing;

pub const MAX_PROTOCOL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("protocol input exceeds its byte limit")]
    InputLimit,
    #[error("invalid protocol cipher material")]
    InvalidMaterial,
    #[error("invalid encrypted protocol envelope")]
    InvalidEnvelope,
    #[error("protocol authentication failed")]
    Authentication,
    #[error("invalid protocol encoding")]
    Encoding,
}

fn bounded_input(input: &[u8]) -> Result<(), CryptoError> {
    if input.len() > MAX_PROTOCOL_BYTES {
        return Err(CryptoError::InputLimit);
    }
    Ok(())
}

/// Hash parts without concatenation or an intermediate allocation.
pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    digest.finalize().into()
}

/// Constant-time comparison between byte slices to prevent timing side-channels.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

pub fn aes_cbc_encrypt(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    bounded_input(plaintext)?;
    match key.len() {
        16 => Ok(cbc::Encryptor::<aes::Aes128>::new_from_slices(key, iv)
            .map_err(|_| CryptoError::InvalidMaterial)?
            .encrypt_padded_vec::<Pkcs7>(plaintext)),
        32 => Ok(cbc::Encryptor::<aes::Aes256>::new_from_slices(key, iv)
            .map_err(|_| CryptoError::InvalidMaterial)?
            .encrypt_padded_vec::<Pkcs7>(plaintext)),
        _ => Err(CryptoError::InvalidMaterial),
    }
}

pub fn aes_cbc_decrypt(
    key: &[u8],
    iv: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if ciphertext.is_empty()
        || ciphertext.len() > MAX_PROTOCOL_BYTES + 16
        || !ciphertext.len().is_multiple_of(16)
    {
        return Err(CryptoError::InvalidEnvelope);
    }
    let plaintext = match key.len() {
        16 => cbc::Decryptor::<aes::Aes128>::new_from_slices(key, iv)
            .map_err(|_| CryptoError::InvalidMaterial)?
            .decrypt_padded_vec::<Pkcs7>(ciphertext),
        32 => cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv)
            .map_err(|_| CryptoError::InvalidMaterial)?
            .decrypt_padded_vec::<Pkcs7>(ciphertext),
        _ => return Err(CryptoError::InvalidMaterial),
    }
    .map_err(|_| CryptoError::InvalidEnvelope)?;
    Ok(Zeroizing::new(plaintext))
}

/// RC4 is retained only where the upstream wire protocol requires it.
pub fn rc4_xor(key: &[u8], input: &mut [u8]) -> Result<(), CryptoError> {
    bounded_input(input)?;
    let mut cipher = rc4::Rc4::new_from_slice(key).map_err(|_| CryptoError::InvalidMaterial)?;
    cipher
        .try_apply_keystream(input)
        .map_err(|_| CryptoError::InvalidEnvelope)
}

pub fn chacha20_xor(
    key: &[u8],
    nonce: &[u8],
    counter: u32,
    input: &mut [u8],
) -> Result<(), CryptoError> {
    bounded_input(input)?;
    let mut cipher = chacha20::ChaCha20::new_from_slices(key, nonce)
        .map_err(|_| CryptoError::InvalidMaterial)?;
    cipher
        .try_seek(u64::from(counter) * 64)
        .map_err(|_| CryptoError::InvalidEnvelope)?;
    cipher
        .try_apply_keystream(input)
        .map_err(|_| CryptoError::InvalidEnvelope)
}

pub fn aes_gcm_decrypt(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    bounded_input(ciphertext)?;
    bounded_input(aad)?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| CryptoError::InvalidMaterial)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidMaterial)?;
    cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| CryptoError::Authentication)
}
