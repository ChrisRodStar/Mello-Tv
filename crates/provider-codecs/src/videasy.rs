//! Videasy's recognized 61-element keystream. Explicit wrapping arithmetic
//! preserves the existing Go uint32 behavior in debug and release builds.

use base64::{Engine, engine::general_purpose::STANDARD};
use stream_core::providers::TmdbId;
use zeroize::{Zeroize, Zeroizing};

use super::{CryptoError, MAX_PROTOCOL_BYTES};

const GOLDEN: u32 = 2_654_435_769;
const HASH_TABLE: [u32; 16] = [
    1_116_352_408,
    1_899_447_441,
    3_049_323_471,
    3_921_009_573,
    961_987_163,
    1_508_970_993,
    2_453_635_748,
    2_870_763_221,
    3_624_381_080,
    310_598_401,
    607_225_278,
    1_426_881_987,
    1_925_078_388,
    2_162_078_206,
    2_614_888_103,
    3_248_222_580,
];

fn hash32(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(2_246_822_507);
    value ^= value >> 13;
    value = value.wrapping_mul(3_266_489_909);
    value ^ (value >> 16)
}

fn seed_hash(seed: &str) -> u32 {
    let mut value = 2_166_136_261u32;
    for byte in seed.bytes() {
        value = (value ^ u32::from(byte)).wrapping_mul(16_777_619);
    }
    hash32(value)
}

struct State {
    words: [u32; 61],
    assigned: [bool; 61],
    accumulator: u32,
}

impl State {
    fn new(seed: &str, key: u32) -> Self {
        let mut state = Self {
            words: [0; 61],
            assigned: [false; 61],
            accumulator: 0,
        };
        let mut value = hash32(seed_hash(seed) ^ hash32(key ^ GOLDEN));
        for index in 0u32..8 {
            if index.wrapping_mul(index + 1) & 1 == 0 {
                let slot = value % 61;
                value = value.wrapping_add(GOLDEN).rotate_left(7 + (7 & index));
                // The modulo bounds each index to the fixed 61-word state.
                state.words[slot as usize] = value ^ hash32(value);
                state.assigned[slot as usize] = true;
                value = hash32(value.wrapping_add(slot));
            } else {
                state.words[index as usize] = HASH_TABLE[(15 & index) as usize];
                state.assigned[index as usize] = true;
            }
        }
        state.accumulator = hash32(2_779_096_485 ^ value);
        state
    }

    fn next_word(&mut self, counter: u32) -> u32 {
        let slot = self.accumulator % 61;
        let mask = if self.assigned[slot as usize] {
            u32::MAX
        } else {
            0
        };
        let value = self.words[slot as usize] ^ GOLDEN.wrapping_mul(counter.wrapping_add(1));
        let mixed = (self.accumulator ^ value) | (self.accumulator & value & mask);
        let left = mixed.wrapping_add(self.accumulator).rotate_left(31 & slot);
        let right = self.accumulator.rotate_left(31 & slot.wrapping_mul(7));
        let next = hash32((left ^ right).wrapping_add(GOLDEN));
        self.words[slot as usize] = next;
        self.assigned[slot as usize] = true;
        self.accumulator = next;
        next
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.words.zeroize();
        self.assigned.zeroize();
        self.accumulator.zeroize();
    }
}

/// Applies the observed stream codec without allocating a second keystream.
pub fn apply_keystream(seed: &str, key: u32, data: &mut [u8]) -> Result<(), CryptoError> {
    if seed.len() > 4096 || data.len() > MAX_PROTOCOL_BYTES {
        return Err(CryptoError::InputLimit);
    }
    let mut state = State::new(seed, key);
    for (counter, chunk) in data.chunks_mut(4).enumerate() {
        let counter = u32::try_from(counter).map_err(|_| CryptoError::InputLimit)?;
        for (value, mask) in chunk.iter_mut().zip(state.next_word(counter).to_le_bytes()) {
            *value ^= mask;
        }
    }
    Ok(())
}

pub fn decrypt_payload(
    payload: &str,
    seed: &str,
    tmdb_id: TmdbId,
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if payload.len() < 20 {
        return Err(CryptoError::InvalidEnvelope);
    }
    if payload.len() > MAX_PROTOCOL_BYTES || seed.len() > 4096 {
        return Err(CryptoError::InputLimit);
    }
    // Go's standard decoder ignores CR/LF. Its caller normalizes both alphabets
    // and restores missing padding, including partially supplied padding.
    let mut normalized: Vec<u8> = payload
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n'))
        .map(|byte| match byte {
            b'-' => b'+',
            b'_' => b'/',
            other => other,
        })
        .collect();
    while !normalized.len().is_multiple_of(4) {
        normalized.push(b'=');
    }
    let decoded = STANDARD
        .decode(&normalized)
        .map_err(|_| CryptoError::Encoding)?;
    let mut decoded = Zeroizing::new(decoded);
    // Go ParseUint(...,32) returns the maximum value on overflow; its existing
    // caller discards the error. Preserve that saturation rather than truncation.
    let key = u32::try_from(tmdb_id.get()).unwrap_or(u32::MAX);
    apply_keystream(seed, key, &mut decoded)?;
    if !decoded.starts_with(b"mvm1") {
        return Err(CryptoError::InvalidEnvelope);
    }
    decoded.drain(..4);
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_go_word_vector() {
        let mut state = State::new("test-seed", 1622);
        let expected = [
            1_565_862_539,
            4_109_863_583,
            230_583_110,
            1_106_669_467,
            4_046_983_225,
            3_597_725_526,
            55_526_469,
            3_807_938_347,
        ];
        for (counter, expected) in (0u32..).zip(expected) {
            assert_eq!(state.next_word(counter), expected);
        }
    }
}
