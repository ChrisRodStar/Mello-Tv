//! Tulnex layered response codec.
use crate::{CryptoError, aes_cbc_decrypt};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256, Sha512};

const L1_KEY: &str = "Sn00pD0g#L1_X0R_M4st3rK3y!2026sex";
const L1_SALT: &str = "xK9!mR2@pL5#nQ8sex";
const L3_KEY: &str = "Sn00pD0g#L3_AES_S3cur3K3y@2026$sex";
const L4_KEY: &str = "Sn00pD0g#L4_HMAC_F1n4lW4ll#2026!sex";

fn hmac256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0u8; 64];
    let mut outer_pad = [0u8; 64];
    for (index, byte) in k.into_iter().enumerate() {
        inner_pad[index] = byte ^ 0x36;
        outer_pad[index] = byte ^ 0x5c;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(digest);
    outer.finalize().into()
}
fn hmac512(key: &[u8], message: &[u8]) -> [u8; 64] {
    let mut k = [0u8; 128];
    if key.len() > 128 {
        k[..64].copy_from_slice(&Sha512::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0u8; 128];
    let mut outer_pad = [0u8; 128];
    for (index, byte) in k.into_iter().enumerate() {
        inner_pad[index] = byte ^ 0x36;
        outer_pad[index] = byte ^ 0x5c;
    }
    let mut inner = Sha512::new();
    inner.update(inner_pad);
    inner.update(message);
    let digest = inner.finalize();
    let mut outer = Sha512::new();
    outer.update(outer_pad);
    outer.update(digest);
    outer.finalize().into()
}
fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32) -> Vec<u8> {
    let mut input = salt.to_vec();
    input.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac256(password, &input);
    let mut out = u;
    for _ in 1..rounds {
        u = hmac256(password, &u);
        for (dst, src) in out.iter_mut().zip(u) {
            *dst ^= src;
        }
    }
    out.to_vec()
}
fn pbkdf2_sha512(password: &[u8], salt: &[u8], rounds: u32) -> Vec<u8> {
    let mut input = salt.to_vec();
    input.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac512(password, &input);
    let mut out = u;
    for _ in 1..rounds {
        u = hmac512(password, &u);
        for (dst, src) in out.iter_mut().zip(u) {
            *dst ^= src;
        }
    }
    out[..32].to_vec()
}
fn decoded_secret(encoded: &str) -> Result<Vec<u8>, CryptoError> {
    Ok(encoded.as_bytes().to_vec())
}
fn hex(value: &str) -> Result<Vec<u8>, CryptoError> {
    if value.is_empty()
        || !value.len().is_multiple_of(2)
        || value.len() > crate::MAX_PROTOCOL_BYTES * 2
    {
        return Err(CryptoError::InvalidEnvelope);
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            u8::from_str_radix(
                std::str::from_utf8(p).map_err(|_| CryptoError::Encoding)?,
                16,
            )
            .map_err(|_| CryptoError::Encoding)
        })
        .collect()
}
fn binary_decode(value: &str) -> Result<Vec<u8>, CryptoError> {
    let raw = STANDARD.decode(value).map_err(|_| CryptoError::Encoding)?;
    let raw = std::str::from_utf8(&raw).map_err(|_| CryptoError::Encoding)?;
    let mut out = Vec::with_capacity(raw.len() / 8);
    for item in raw.split(' ').filter(|x| !x.is_empty()) {
        if item.len() != 8 || !item.bytes().all(|b| b == b'0' || b == b'1') {
            return Err(CryptoError::Encoding);
        }
        out.push(u8::from_str_radix(item, 2).map_err(|_| CryptoError::Encoding)?);
    }
    Ok(out)
}
/// Verify and decode the L4|L3|L2|L1 payload into JSON bytes.
pub fn decrypt(payload: &str) -> Result<Vec<u8>, CryptoError> {
    if payload.len() > crate::MAX_PROTOCOL_BYTES * 2 {
        return Err(CryptoError::InputLimit);
    }
    let (mac, encoded) = payload
        .split_once('|')
        .ok_or(CryptoError::InvalidEnvelope)?;
    let l4 = STANDARD
        .decode(encoded)
        .map_err(|_| CryptoError::Encoding)?;
    let key4 = decoded_secret(L4_KEY)?;
    let actual = hmac512(&key4, &l4);
    let expected = hex(mac)?;
    if !crate::ct_eq(&actual, &expected) {
        return Err(CryptoError::Authentication);
    }
    let l4 = std::str::from_utf8(&l4).map_err(|_| CryptoError::Encoding)?;
    let parts: Vec<_> = l4.split('.').collect();
    if parts.len() != 3 {
        return Err(CryptoError::InvalidEnvelope);
    }
    let iv = STANDARD
        .decode(parts[0])
        .map_err(|_| CryptoError::Encoding)?;
    let salt = STANDARD
        .decode(parts[1])
        .map_err(|_| CryptoError::Encoding)?;
    let ciphertext = STANDARD
        .decode(parts[2])
        .map_err(|_| CryptoError::Encoding)?;
    let key3 = decoded_secret(L3_KEY)?;
    let aes_key = pbkdf2_sha512(&key3, &salt, 100_000);
    let l3 = aes_cbc_decrypt(&aes_key, &iv, &ciphertext)?;
    let l2 = binary_decode(std::str::from_utf8(&l3).map_err(|_| CryptoError::Encoding)?)?;
    let xor_key = pbkdf2_sha256(&decoded_secret(L1_KEY)?, &decoded_secret(L1_SALT)?, 50_000);
    let l1_hex = std::str::from_utf8(&l2).map_err(|_| CryptoError::Encoding)?;
    let mut json = hex(l1_hex)?;
    for (i, byte) in json.iter_mut().enumerate() {
        *byte ^= xor_key[i % xor_key.len()];
    }
    std::str::from_utf8(&json).map_err(|_| CryptoError::Encoding)?;
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha256_matches_rfc4231_case_one() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            hmac256(&[0x0b; 20], b"Hi There").as_slice(),
            hex("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")?
        );
        Ok(())
    }

    #[test]
    fn hmac_sha512_matches_rfc4231_case_one() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(hmac512(&[0x0b; 20], b"Hi There").as_slice(), hex("87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cde daA833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854".replace(' ', "").as_str())?);
        assert!(decrypt("payload without authenticator").is_err());
        Ok(())
    }

    #[test]
    fn pbkdf2_matches_python_hashlib_vectors() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            pbkdf2_sha256(b"password", b"salt", 1000),
            hex("632c2812e46d4604102ba7618e9d6d7d2f8128f6266b4a03264d2a0460b7dcb3")?
        );
        assert_eq!(
            pbkdf2_sha512(b"password", b"salt", 1000),
            hex("afe6c5530785b6cc6b1c6453384731bd5ee432ee549fd42fb6695779ad8a1c5b")?
        );
        Ok(())
    }

    #[test]
    fn decrypts_layered_envelope_payload() -> Result<(), Box<dyn std::error::Error>> {
        let payload = "022ba24ce54806fc82061bed5d07a7826a895cd0c16199b9910275720861ef7f14f792960c3c86c0135c346f1385c969b9457d34ac1e3ac4e867fa3c973f6cc2|UVdBVFJMK3lTKzRkWjU2OVVMUXdzUT09LmNtRnVaRzl0YzJGc2RERXlNdz09LldNRjVDMmVFVHkybzFNeFV4eXV1K2I1Nm9qT04vWHBrUHBQbUlqVjk5ZlE4VGtRUUdENUlPOG9yckhsRC9obFBrWUY4cTJRT1RjR3N6NVJCNzlzZEZXUkIwWHdFV0xEWHFVSXV6eGh3UUNjNzlDdnJPWVJ6Qk42ME5YcjZaWDg1S0ZlOWZ4QmN1Y2RHR0NMalV4TzA5aU1vczRMc2dkc2V6WTFNMUtJM3lCbFVVWWpYZnJHWXNmK05BcE5DNEF6VTRLMXI1a3BqL3hiNXgzd0ZkYlhHL3A4U1BtSGRUVXpiMHFIWmdwR08zZWhqbXJXRUhQRkVScnk1RDk5TmN4SDJqYnpQOVlOUnhCNXhBc2RyY0w2ai9ub2ZDSzU5T3BDRnc1QWhXdGJ6S0N5WndSc1pickhsaDJ0dUNwUFEwdU1VN213V1AzWEQ1d2Z5cnMxa000ajEwMUdKS3orNEN0b1FwUzdQZE9RTzdqbmN1QkxKaWhjL0ZFOUx2VEkzd1E0MlpldktSaWY2ZVdiT0RKQktDd3FZU2xqUnpkMHlxQVdzYy9VTkJCWldxWktDVVQ1cGYvMDVxWFdEbVc2QlczekZMSlU3NHdCOXRKMnF1TkEzQXI1WjBxbllUYnAvL3UvVm9GaHYzTERRK2xJeWJBNXh2bXVXUWVBS3RBS2NnQys3NUgvRmpjNzNLTFhSMmdSSjBjendEczFoajZOZW9SOE93SXZEcFpJa0pqY3BYdWIwUzFTQ2pIaXBaaE41YTNZeXdJYjZkaGhkalNZdlJ5VnJYZEIxM0JQQnJTQkhobGtnZi9vWGs4djBad2pjeXhwMmRkUjh2and3Z05yVmR0TzZFS3RiMWNPclkwOUVIejZybG5tZUtWUjJ6SHgyOGZVT01qbGZDTzlMRzB3emtBKzdjQjQ1ME5UWThBRisxR3V0M3h5ZWg1VTRGTU9mN2xVS0lWT2VSNlpHRElVcmFEb2dTdklFU3FBby85OVR4cFQ2dC90QkVFQzE1c2REMmV0VDNtZFc1R0JDUzNyUjZYeTlQeHNJMmZicjI1K0FPc1VlcFB2UFZucHlLeFZSWTdhdisrVTA0K2RaYWpjWSsxdTc0bm9rdVdFM1Q3M0x0SWZjS0Fyai9YdzA2MFBZNll4NGJIUVNaNjZISmpiTUozdmxrenRWWVVSTnJjNGV1OW5UVGdyUkpXOExRclRtUmNEL2NibXcybzA1NFVnc0h3OFdoRUpKU050eFZZRmxmd2dQcU5QN21WMmR1bXBPc08wUXVXeHlKT0RGbHFsei9ZTWhrSlVJdUV5NURBVDB4OU5icHdxWXBqbzE3aW9zSW1vbVlRMk80UTNUd0Z3SU9vbHlxcmJOc2xLYW5lZW9RYTZOTUdvNVhobVJRS0w3RE9zdjVGVy9ITG1Qdm1GWnNINFJNWG4rTEdoSzJLMW5oZllsSStkcXEyVU55TXdEWDNEN1pQZ0ZyV2VvRWs2N25HWGtLWnRPOTd1NGRzbHFnL3l2dmRVSHFGS0RZRWxkLzFDU2RJVU4ySHV3VytuWE5lamdabGx0S3VTT2c3bFVCY0c5cXBvNVpuWWNoV0RySDkvZGIyNWFKQ2krdnl2akJNWHVlUGJGeG54eHBZajRJdUxZOUlDRGc1a2dSbFpCWXNxUjlKQlJ0K09YSXN0aTZvaDB3NHNrQmdUZEhnMnd1bXBweURaRnZNRlJsOGc3KzRhVm1EM09ORVJ4Ti9XbUZZUEpuVG9BL1c2d3A0NFlEYmZMU2hPZUdYb3VjUEZIWnJxbkVWeUhuSkFyeUJkK1d1M1lsdWgwUVBuTlgyMWRKOHBpdlpuOXgra09NYU5vWmFlcmd5RGtkWmJQZVhvb2JvSnJPYjc4MnZnc1YybnQ1ZG8vVHFJdGtJcndXcVV4c0FvK0VGZENTT2N5WlhQUkI0SmRqZGZwWENYV0ZaTzd2U3JnTFRyOERRMnBoYlNnbmZuUHdONXErM1NEcGxrMVNjSm9FQnNIOUVBaGFtOVJQZ1lqcktCdTJDYjhFRWpwZFNMSndmZDFCSmdhaGlRcnlUTWpuK2RSWGEwQU00WWREU3I4WENjQ0tjdzlRN0ZVWkE0bmtxeW05ZnZnUW1HMytaTXUydzltUkdFUjNQZUdaYjUwNzlodjlZM3ZnSUlBbXNGQ3dyNjZLNk54c1JpVHZuMEJUeURsWW5kRnM4M3lLWWZYWUR6cjVLNkVoY3dEcER2aWdzMnBVbDU3dnlBdkdqVGVPYkRWeVhwekxSKzNOZk5GNXZQTXgrL01BMDQvejFlbjE2K3pEZDlpRUpESkNMVVRjQkFNbnc9PQ==";
        let decrypted = decrypt(payload)?;
        let json_str = std::str::from_utf8(&decrypted)?;
        assert_eq!(
            json_str,
            "{\"url\":\"https://example.com/stream.m3u8\",\"type\":\"hls\"}"
        );
        Ok(())
    }
}
