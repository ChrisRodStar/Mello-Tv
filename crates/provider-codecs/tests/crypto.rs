use std::error::Error;

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use stream_core::providers::TmdbId;
use stream_provider_codecs::{self as crypto, CryptoError, videasy};

fn hex(value: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("odd fixture hex length".into());
    }
    pairs
        .iter()
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

#[test]
fn cbc_matches_published_rustcrypto_vector() -> Result<(), Box<dyn Error>> {
    let plaintext = b"hello world! this is my plaintext.";
    let expected = hex(
        "c7fe247ef97b21f07cbdd26cb5d346bfd27867cb00d9486723e159978fb9a5f914cfb228a710de4171e396e7b6cf859e",
    )?;
    let actual = crypto::aes_cbc_encrypt(&[0x42; 16], &[0x24; 16], plaintext)?;
    assert_eq!(actual, expected);
    assert_eq!(
        crypto::aes_cbc_decrypt(&[0x42; 16], &[0x24; 16], &actual)?.as_slice(),
        plaintext
    );
    Ok(())
}

#[test]
fn cbc256_matches_nist_first_blocks_and_pkcs7_boundaries() -> Result<(), Box<dyn Error>> {
    let key = hex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4")?;
    let iv = hex("000102030405060708090a0b0c0d0e0f")?;
    let plaintext = hex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51")?;
    let expected = hex("f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d")?;
    let actual = crypto::aes_cbc_encrypt(&key, &iv, &plaintext)?;
    assert_eq!(actual.get(..32), Some(expected.as_slice()));
    assert_eq!(actual.len(), 48);
    assert_eq!(
        crypto::aes_cbc_decrypt(&key, &iv, &actual)?.as_slice(),
        plaintext
    );
    let empty = crypto::aes_cbc_encrypt(&key, &iv, &[])?;
    assert_eq!(empty.len(), 16);
    assert!(crypto::aes_cbc_decrypt(&key, &iv, &empty)?.is_empty());
    assert!(crypto::aes_cbc_encrypt(&[0; 17], &iv, &[]).is_err());
    assert!(crypto::aes_cbc_decrypt(&key, &iv, &[1; 15]).is_err());
    assert!(crypto::aes_cbc_decrypt(&key, &[0; 15], &actual).is_err());
    Ok(())
}

#[test]
fn hmac_sha256_matches_rfc4231_test_vectors() -> Result<(), Box<dyn Error>> {
    // RFC 4231 Test Case 1
    let key1 = [0x0bu8; 20];
    let data1 = b"Hi There";
    let expected1 = hex("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")?;
    let actual1 = crypto::hmac_sha256(&key1, data1)?;
    assert_eq!(actual1.as_slice(), expected1.as_slice());

    // RFC 4231 Test Case 2
    let key2 = b"Jefe";
    let data2 = b"what do ya want for nothing?";
    let expected2 = hex("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")?;
    let actual2 = crypto::hmac_sha256(key2, data2)?;
    assert_eq!(actual2.as_slice(), expected2.as_slice());

    Ok(())
}

#[test]
fn rc4_known_vector_and_invalid_key_sizes() -> Result<(), Box<dyn Error>> {
    let mut bytes = b"Plaintext".to_vec();
    crypto::rc4_xor(b"Key", &mut bytes)?;
    assert_eq!(bytes, hex("bbf316e8d940af0ad3")?);
    assert!(matches!(
        crypto::rc4_xor(&[], &mut bytes),
        Err(CryptoError::InvalidMaterial)
    ));
    assert!(crypto::rc4_xor(&[0; 257], &mut bytes).is_err());
    Ok(())
}

#[test]
fn chacha_matches_rfc8439_counter_and_stream_limits() -> Result<(), Box<dyn Error>> {
    let key: Vec<u8> = (0..32).collect();
    let nonce = hex("000000000000004a00000000")?;
    let mut plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.".to_vec();
    crypto::chacha20_xor(&key, &nonce, 1, &mut plaintext)?;
    assert_eq!(
        plaintext,
        hex(
            "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0bf91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d807ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab77937365af90bbf74a35be6b40b8eedf2785e42874d"
        )?
    );
    assert!(crypto::chacha20_xor(&key, &nonce, u32::MAX, &mut [0; 65]).is_err());
    assert!(crypto::chacha20_xor(&key, &nonce[..11], 0, &mut [0; 16]).is_err());
    Ok(())
}

#[test]
fn gcm_matches_nist_authenticated_vector_and_rejects_tampering() -> Result<(), Box<dyn Error>> {
    let ciphertext = hex("cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919")?;
    assert_eq!(
        crypto::aes_gcm_decrypt(&[0; 32], &[0; 12], &[], &ciphertext)?.as_slice(),
        &[0; 16]
    );
    let mut changed = ciphertext.clone();
    if let Some(byte) = changed.last_mut() {
        *byte ^= 1;
    }
    assert!(matches!(
        crypto::aes_gcm_decrypt(&[0; 32], &[0; 12], &[], &changed),
        Err(CryptoError::Authentication)
    ));
    assert!(crypto::aes_gcm_decrypt(&[0; 32], &[0; 12], b"wrong-aad", &ciphertext).is_err());
    Ok(())
}

#[test]
fn videasy_accepts_observed_alphabets_and_tmdb_overflow_semantics() -> Result<(), Box<dyn Error>> {
    for id in [1622, i64::MAX] {
        let key = u32::try_from(id).unwrap_or(u32::MAX);
        let expected = b"mvm1{\"sources\":[]}";
        let mut encrypted = expected.to_vec();
        videasy::apply_keystream("test-seed", key, &mut encrypted)?;
        for encoded in [
            STANDARD.encode(&encrypted),
            URL_SAFE_NO_PAD.encode(&encrypted),
        ] {
            let decoded = videasy::decrypt_payload(&encoded, "test-seed", TmdbId::try_from(id)?)?;
            assert_eq!(decoded.as_slice(), &expected[4..]);
            assert!(
                videasy::decrypt_payload(&encoded, "wrong-seed", TmdbId::try_from(id)?).is_err()
            );
        }
    }
    assert!(videasy::decrypt_payload("short", "seed", TmdbId::try_from(1)?).is_err());
    assert!(videasy::decrypt_payload("a", "seed", TmdbId::try_from(1)?).is_err());
    Ok(())
}

#[test]
fn cbc_rejects_deterministically_invalid_pkcs7_padding() -> Result<(), Box<dyn Error>> {
    let key = [0x5au8; 32];
    let iv = [0x2bu8; 16];
    let plaintext = b"exact-16-bytes!!";
    let ciphertext = crypto::aes_cbc_encrypt(&key, &iv, plaintext)?;
    assert_eq!(ciphertext.len(), 32); // 16 bytes plaintext + 16 bytes PKCS#7 padding block

    // CBC XORs the previous ciphertext block into the next plaintext block.
    // Change the final padding byte from 0x10 to 0x11, an invalid padding length.
    let mut corrupted = ciphertext;
    corrupted[15] ^= 0x01;
    assert_eq!(
        crypto::aes_cbc_decrypt(&key, &iv, &corrupted),
        Err(CryptoError::InvalidEnvelope)
    );

    // For a single-block ciphertext the IV controls the same plaintext byte.
    // "sample" has ten padding bytes; change only the last from 0x0a to 0x0b.
    let key128 = [0x7eu8; 16];
    let ct128 = crypto::aes_cbc_encrypt(&key128, &iv, b"sample")?;
    let mut corrupted_iv = iv;
    corrupted_iv[15] ^= 0x01;
    assert_eq!(
        crypto::aes_cbc_decrypt(&key128, &corrupted_iv, &ct128),
        Err(CryptoError::InvalidEnvelope)
    );

    Ok(())
}

#[test]
fn constant_time_equality_checks() {
    assert!(crypto::ct_eq(b"hello", b"hello"));
    assert!(!crypto::ct_eq(b"hello", b"world"));
    assert!(!crypto::ct_eq(b"hello", b"hell"));
    assert!(!crypto::ct_eq(b"", b"a"));
    assert!(crypto::ct_eq(b"", b""));
}

#[test]
fn cbc_media_prefix_matches_nist_without_requiring_final_padding() -> Result<(), Box<dyn Error>> {
    let key = [
        0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f,
        0x3c,
    ];
    let iv = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let ciphertext = [
        0x76, 0x49, 0xab, 0xac, 0x81, 0x19, 0xb2, 0x46, 0xce, 0xe9, 0x8e, 0x9b, 0x12, 0xe9, 0x19,
        0x7d, 0x50, 0x86, 0xcb, 0x9b, 0x50, 0x72, 0x19, 0xee, 0x95, 0xdb, 0x11, 0x3a, 0x91, 0x76,
        0x78, 0xb2,
    ];
    let plaintext = [
        0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17,
        0x2a, 0xae, 0x2d, 0x8a, 0x57, 0x1e, 0x03, 0xac, 0x9c, 0x9e, 0xb7, 0x6f, 0xac, 0x45, 0xaf,
        0x8e, 0x51,
    ];
    assert_eq!(
        crypto::aes_cbc_decrypt_prefix(&key, &iv, &ciphertext)?.as_slice(),
        plaintext
    );
    assert!(crypto::aes_cbc_decrypt_prefix(&key, &iv, &ciphertext[..31]).is_err());
    assert!(crypto::aes_cbc_decrypt_prefix(&key, &iv[..15], &ciphertext).is_err());
    Ok(())
}
