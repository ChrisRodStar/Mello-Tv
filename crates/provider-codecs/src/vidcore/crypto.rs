use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use zeroize::Zeroizing;

use crate::{MAX_PROTOCOL_BYTES, aes_cbc_encrypt, aes_gcm_decrypt, rc4_xor, sha256};

use super::{
    MAX_CHALLENGE_RESPONSE_BYTES, MAX_SESSION_BYTES, ProtocolError, ProtocolParameters,
    URL_ALPHABET,
};

const STAGE_OPERATIONS: [[u8; 10]; 5] = [
    [0, 1, 2, 3, 4, 1, 4, 3, 4, 0],
    [5, 2, 4, 6, 2, 7, 4, 5, 6, 8],
    [2, 3, 7, 3, 6, 6, 0, 7, 6, 9],
    [3, 2, 0, 8, 1, 8, 3, 5, 2, 6],
    [10, 6, 9, 4, 10, 4, 0, 9, 6, 2],
];

fn concatenate(parts: &[&[u8]]) -> Result<Vec<u8>, ProtocolError> {
    let length = parts
        .iter()
        .try_fold(0usize, |total, part| total.checked_add(part.len()))
        .ok_or(ProtocolError::InputLimit)?;
    if length > MAX_PROTOCOL_BYTES {
        return Err(ProtocolError::InputLimit);
    }
    let mut result = Vec::with_capacity(length);
    for part in parts {
        result.extend_from_slice(part);
    }
    Ok(result)
}

struct Random(u32);

impl Random {
    fn new(seed: &[u8]) -> Self {
        let digest = sha256(&[seed]);
        let mut state = 0;
        for word in digest.as_chunks::<4>().0.iter().take(4) {
            state ^= u32::from_le_bytes(*word);
        }
        Self(if state == 0 { 0x9e3779b9 } else { state })
    }

    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

/// Bounded Fisher-Yates order using the exact SHA/xorshift Go seed behavior.
pub fn permutation(length: usize, seed: &[u8]) -> Result<Vec<usize>, ProtocolError> {
    if length > MAX_PROTOCOL_BYTES || seed.len() > MAX_PROTOCOL_BYTES {
        return Err(ProtocolError::InputLimit);
    }
    let mut result: Vec<_> = (0..length).collect();
    let mut random = Random::new(seed);
    for index in (1..length).rev() {
        let bound = u64::try_from(index + 1).map_err(|_| ProtocolError::InputLimit)?;
        let other = usize::try_from(u64::from(random.next()) % bound)
            .map_err(|_| ProtocolError::InputLimit)?;
        result.swap(index, other);
    }
    Ok(result)
}

pub(super) fn challenge(
    parameters: &ProtocolParameters,
    session: &str,
    unix_milliseconds: i64,
    nonce: &[u8],
) -> Result<String, ProtocolError> {
    if nonce.len() != 16 || session.is_empty() || session.len() > MAX_SESSION_BYTES {
        return Err(ProtocolError::Invalid("challenge input"));
    }
    let stamp = unix_milliseconds.to_le_bytes();
    let plaintext = Zeroizing::new(concatenate(&[nonce, &stamp, session.as_bytes()])?);
    // The shared primitive performs the single required PKCS#7 padding pass.
    let mut encrypted = aes_cbc_encrypt(&parameters.key, &parameters.iv, &plaintext)?;
    let mut mask = sha256(&[&parameters.seed, nonce]);
    for (index, value) in encrypted.iter_mut().enumerate() {
        if index > 0 && index % 32 == 0 {
            mask = sha256(&[&mask]);
        }
        *value ^= mask[index % 32];
    }
    let rotations = sha256(&[&parameters.key, nonce]);
    for (index, value) in encrypted.iter_mut().enumerate() {
        let rotation = rotations[index % 32];
        *value = value
            .rotate_left(u32::from(rotation & 7))
            .wrapping_add(rotation ^ 165);
    }
    let sbox = permutation(
        256,
        &concatenate(&[nonce, &parameters.seed, &parameters.iv])?,
    )?;
    for value in &mut encrypted {
        *value = u8::try_from(
            *sbox
                .get(usize::from(*value))
                .ok_or(ProtocolError::Invalid("S-box"))?,
        )
        .map_err(|_| ProtocolError::Invalid("S-box"))?;
    }
    let block_order = permutation(
        encrypted.len() / 16,
        &concatenate(&[&parameters.seed, nonce])?,
    )?;
    let mut blocks = Vec::with_capacity(encrypted.len());
    for index in &block_order {
        let start = index.checked_mul(16).ok_or(ProtocolError::InputLimit)?;
        blocks.extend_from_slice(
            encrypted
                .get(start..start + 16)
                .ok_or(ProtocolError::Invalid("block permutation"))?,
        );
    }
    // Go explicitly truncates the buffer length to one byte in this seed.
    let length_byte = [u8::try_from(blocks.len() % 256).map_err(|_| ProtocolError::InputLimit)?];
    let positions = permutation(
        blocks.len(),
        &concatenate(&[&parameters.key, nonce, &length_byte])?,
    )?;
    for (destination, source) in encrypted.iter_mut().zip(positions) {
        *destination = *blocks
            .get(source)
            .ok_or(ProtocolError::Invalid("byte permutation"))?;
    }
    let count = u16::try_from(block_order.len()).map_err(|_| ProtocolError::InputLimit)?;
    let mut serialized = Vec::with_capacity(block_order.len() * 4);
    for index in block_order {
        serialized.extend_from_slice(
            &u32::try_from(index)
                .map_err(|_| ProtocolError::InputLimit)?
                .to_le_bytes(),
        );
    }
    let checksum = sha256(&[&serialized, &encrypted]);
    let payload = concatenate(&[
        &[1],
        nonce,
        &count.to_le_bytes(),
        &serialized,
        &encrypted,
        &checksum[..8],
    ])?;
    let mut encoded = URL_SAFE_NO_PAD.encode(payload).into_bytes();
    for value in &mut encoded {
        let index = URL_ALPHABET
            .iter()
            .position(|source| source == value)
            .ok_or(ProtocolError::Invalid("challenge alphabet"))?;
        *value = *parameters
            .alphabet
            .get(index)
            .ok_or(ProtocolError::Invalid("challenge alphabet"))?;
    }
    String::from_utf8(encoded).map_err(|_| ProtocolError::Invalid("challenge encoding"))
}

fn transform(value: u8, operation: u8) -> u8 {
    match operation {
        0 => value.wrapping_sub(226),
        1 => value.wrapping_sub(76),
        2 => value.rotate_left(2),
        3 => value.wrapping_sub(200),
        4 => value ^ 59,
        5 => value.wrapping_sub(218),
        6 => value.rotate_left(1),
        7 => value.rotate_right(2),
        8 => value.wrapping_add(230),
        9 => value.rotate_left(7),
        10 => value.wrapping_sub(216),
        _ => value,
    }
}

pub(super) fn ticket(
    parameters: &ProtocolParameters,
    response: &[u8],
) -> Result<String, ProtocolError> {
    if response.is_empty() || response.len() > MAX_CHALLENGE_RESPONSE_BYTES || !response.is_ascii()
    {
        return Err(ProtocolError::Invalid("challenge response"));
    }
    let mut current = Vec::with_capacity(response.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in response.iter().rev() {
        current.push(HEX[usize::from(byte >> 4)]);
        current.push(HEX[usize::from(byte & 15)]);
    }
    for stage in [0usize, 4, 1, 2, 3] {
        let prefix = parameters
            .stage_prefixes
            .get(stage)
            .ok_or(ProtocolError::Invalid("stage"))?;
        let key = parameters
            .stage_keys
            .get(stage)
            .ok_or(ProtocolError::Invalid("stage"))?;
        let operations = STAGE_OPERATIONS
            .get(stage)
            .ok_or(ProtocolError::Invalid("stage"))?;
        let length = current
            .len()
            .checked_add(current.len().min(prefix.len()))
            .ok_or(ProtocolError::InputLimit)?;
        if length > MAX_PROTOCOL_BYTES {
            return Err(ProtocolError::InputLimit);
        }
        let mut output = Vec::with_capacity(length);
        for (index, byte) in current.iter().enumerate() {
            if let Some(prefix) = prefix.get(index) {
                output.push(*prefix);
            }
            output.push(transform(byte ^ key[index % 32], operations[index % 10]));
        }
        rc4_xor(
            parameters
                .rc4_keys
                .get(stage)
                .ok_or(ProtocolError::Invalid("stage"))?,
            &mut output,
        )?;
        current = output;
    }
    Ok(URL_SAFE_NO_PAD.encode(current))
}

pub(super) fn decode_response(
    parameters: &ProtocolParameters,
    body: &[u8],
) -> Result<Zeroizing<Vec<u8>>, ProtocolError> {
    if body.len() > MAX_PROTOCOL_BYTES {
        return Err(ProtocolError::InputLimit);
    }
    let raw = std::str::from_utf8(body)
        .map_err(|_| ProtocolError::Invalid("response encoding"))?
        .trim();
    let envelope = STANDARD
        .decode(raw)
        .or_else(|_| STANDARD_NO_PAD.decode(raw))
        .map_err(|_| ProtocolError::Invalid("response envelope"))?;
    if envelope.len() < 44 {
        return Err(ProtocolError::Invalid("response envelope"));
    }
    let salt = envelope
        .get(..16)
        .ok_or(ProtocolError::Invalid("response salt"))?;
    let nonce = envelope
        .get(16..28)
        .ok_or(ProtocolError::Invalid("response nonce"))?;
    let ciphertext = envelope
        .get(28..)
        .ok_or(ProtocolError::Invalid("response ciphertext"))?;
    let mut seed = Zeroizing::new(parameters.response_seed.to_vec());
    for number in &parameters.response_numbers {
        seed.extend_from_slice(&number.to_le_bytes());
    }
    let master = sha256(&[&seed]);
    let derived = Zeroizing::new(sha256(&[&master, salt]));
    let plain = aes_gcm_decrypt(derived.as_ref(), nonce, &[], ciphertext)?;
    Ok(Zeroizing::new(
        plain
            .get(8..)
            .ok_or(ProtocolError::Invalid("response plaintext prefix"))?
            .to_vec(),
    ))
}
