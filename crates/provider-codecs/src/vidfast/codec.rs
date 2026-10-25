use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use zeroize::Zeroizing;

use crate::{
    aes_gcm_decrypt, sha256,
    vidcore::{Protocol, ProtocolParameters},
};

use super::{CHALLENGE_ACTION, LIST_ACTION, MAX_ENVELOPE_BYTES, UNLOCK_ACTION, VidFastError};

pub const REQUEST_ALPHABET: &[u8; 64] =
    b"W_Anx1lGLyJEfTOU3Xeqdh58ki9SHjPm4-Yts0rvCbBg6cDRVFzQNoZI2wMapKu7";

const KEY: [u8; 32] = [
    0xd9, 0xe8, 0x46, 0x3c, 0x18, 0x96, 0x17, 0x6b, 0x48, 0xc1, 0x2f, 0xb2, 0xa4, 0x52, 0x16, 0x01,
    0x30, 0x74, 0xcb, 0x60, 0x03, 0x89, 0x4c, 0x2a, 0xac, 0xf3, 0x20, 0xf6, 0x33, 0xcd, 0xfe, 0xa8,
];
const IV: [u8; 16] = [
    0x48, 0x09, 0x6b, 0x61, 0x50, 0x43, 0x15, 0x11, 0x5f, 0xc8, 0xff, 0x0e, 0xdb, 0xe4, 0x7a, 0x92,
];
// The observed odd trailing hex nibble is discarded: this seed is seven bytes.
const SEED: [u8; 7] = [0xff, 0xb6, 0xb2, 0x23, 0x47, 0x0f, 0x60];
const RESPONSE_SEED: [u8; 32] = [
    0x38, 0xf6, 0x91, 0x3e, 0x38, 0xcf, 0x38, 0xe7, 0xc9, 0x0e, 0xa6, 0xf1, 0xfa, 0x31, 0xbf, 0x75,
    0x01, 0x10, 0x8e, 0xff, 0xc5, 0x7f, 0x6c, 0xd7, 0x78, 0xa3, 0xb9, 0xdd, 0x68, 0xe3, 0xf5, 0x1c,
];

fn fixed_key(encoded: &str) -> Result<[u8; 32], VidFastError> {
    STANDARD
        .decode(encoded)
        .map_err(|_| VidFastError::StaticMaterial)?
        .try_into()
        .map_err(|_| VidFastError::StaticMaterial)
}

fn known_codec() -> Result<&'static Protocol, VidFastError> {
    static CODEC: OnceLock<Result<Protocol, VidFastError>> = OnceLock::new();
    CODEC
        .get_or_init(|| {
            // Constants in observed execution order. Map into the shared primitive's
            // fixed stage identities; this does not evaluate downloaded instructions.
            let key_strings = [
                "JZltXDoh7Eags8L7e0hyBaX6HTZ4ErE1rydu4MTAhx0=",
                "T1CiTh7ajlEieB/+USBMBpwFLQqBy7r+u0v+LEDGDrQ=",
                "oO0GK4QVPEEc7k2uGOPwQ9+uO+g0cWi2tupQfOZH5m4=",
                "oa2189LasXo4boT0vDgZDE02/YMqD1qtaXZQzM7MStw=",
                "svlpt4PNgksvqHbiResvgC9SO7RZzgmm+TK1uXVNkAw=",
            ];
            let prefix_strings = [
                "2PcZ/vEJN24=",
                "iXAPz1fd9GQ=",
                "f7prYx4j",
                "xDGTyVE=",
                "p3uks0I=",
            ];
            let stream_strings = [
                "wCrbGEDVWBGRVggKP8pODbU55I28p/AnwOqtkKsAt8Q=",
                "4yIb/l2zh9Qz62DrakKcfjlcgmldjWIIfBadCsL/blo=",
                "xj1hV8IXKy38jEOtXojtsDXi3+Sr2D2ve2GhbcHKvLI=",
                "1OqtIu7/lgRxDn1zaGDc8PtVF0cIJZNOsiNse4qxKqQ=",
                "yOwHRwrYyRB1fwQFiSRjzVFZpWatYUr8xBzZP0zdpO4=",
            ];
            let mut stage_keys = [[0; 32]; 5];
            let mut prefixes = std::array::from_fn(|_| Vec::new());
            let mut stream_keys = [[0; 32]; 5];
            for (source, target) in [0usize, 4, 1, 2, 3].into_iter().enumerate() {
                stage_keys[target] = fixed_key(key_strings[source])?;
                prefixes[target] = STANDARD
                    .decode(prefix_strings[source])
                    .map_err(|_| VidFastError::StaticMaterial)?;
                stream_keys[target] = fixed_key(stream_strings[source])?;
            }
            Ok(Protocol::from_parameters(ProtocolParameters {
                base: "/zirlaku/u".into(),
                challenge_action: CHALLENGE_ACTION.into(),
                list_action: LIST_ACTION.into(),
                unlock_action: UNLOCK_ACTION.into(),
                csrf_token: String::new(),
                key: KEY,
                iv: IV,
                seed: SEED.to_vec(),
                response_seed: RESPONSE_SEED,
                response_numbers: vec![9_633_601_958, 3_000_935_290],
                alphabet: *REQUEST_ALPHABET,
                stage_keys,
                stage_prefixes: prefixes,
                rc4_keys: stream_keys,
            })?)
        })
        .as_ref()
        .map_err(|_| VidFastError::StaticMaterial)
}

/// Deterministic request packing. The host supplies a fresh random 16-byte salt
/// and trusted nonnegative Unix milliseconds; all clocks and RNG stay outside.
pub fn pack_request(token: &str, salt: &[u8], timestamp: i64) -> Result<String, VidFastError> {
    if token.is_empty() || token.len() > 4096 || salt.len() != 16 || timestamp < 0 {
        return Err(VidFastError::Invalid("request input"));
    }
    Ok(known_codec()?.challenge(token, timestamp, salt)?)
}

pub fn challenge_token(response: &[u8]) -> Result<String, VidFastError> {
    if response.is_empty()
        || response.len() > 4096
        || response.iter().any(|byte| !(32..=127).contains(byte))
    {
        return Err(VidFastError::Invalid("challenge text"));
    }
    Ok(known_codec()?.ticket(response)?)
}

pub fn response_key() -> [u8; 32] {
    sha256(&[
        &RESPONSE_SEED,
        &9_633_601_958u64.to_le_bytes(),
        &3_000_935_290u64.to_le_bytes(),
    ])
}

pub fn decrypt_envelope(encoded: &str) -> Result<Zeroizing<Vec<u8>>, VidFastError> {
    if encoded.len() > MAX_ENVELOPE_BYTES {
        return Err(VidFastError::InputLimit);
    }
    // Go's strict base64 decoder still ignores CR/LF, but rejects other
    // whitespace, missing padding and noncanonical trailing pad bits.
    let filtered;
    let encoded = if encoded.contains(['\r', '\n']) {
        filtered = encoded
            .bytes()
            .filter(|byte| !matches!(byte, b'\r' | b'\n'))
            .collect::<Vec<_>>();
        filtered.as_slice()
    } else {
        encoded.as_bytes()
    };
    let packet = STANDARD
        .decode(encoded)
        .map_err(|_| VidFastError::Invalid("response envelope"))?;
    if packet.len() < 52 {
        return Err(VidFastError::Invalid("response framing"));
    }
    let salt = packet
        .get(..16)
        .ok_or(VidFastError::Invalid("response salt"))?;
    let nonce = packet
        .get(16..28)
        .ok_or(VidFastError::Invalid("response nonce"))?;
    let ciphertext = packet
        .get(28..)
        .ok_or(VidFastError::Invalid("response ciphertext"))?;
    let key = Zeroizing::new(sha256(&[&response_key(), salt]));
    let plaintext = aes_gcm_decrypt(key.as_ref(), nonce, &[], ciphertext)?;
    let body = plaintext
        .get(8..)
        .ok_or(VidFastError::Invalid("response plaintext prefix"))?;
    std::str::from_utf8(body).map_err(|_| VidFastError::Invalid("response text"))?;
    Ok(Zeroizing::new(body.to_vec()))
}
