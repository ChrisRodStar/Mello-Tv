//! VidSrc's fingerprinted static ChaCha20 layout. Downloaded instructions are
//! never compiled or executed; only recognized layouts yield a usable key.

use crate::{CryptoError, chacha20_xor, sha256};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{collections::BTreeSet, fmt};
use thiserror::Error;
use zeroize::Zeroizing;

const MAX_MODULE_BYTES: usize = 128 * 1024;
const MAX_STREAM_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum VidSrcError {
    #[error("VidSrc input exceeds its byte limit")]
    InputLimit,
    #[error("unrecognized VidSrc static module: {0}")]
    Module(&'static str),
    #[error("invalid VidSrc encrypted stream list")]
    Envelope,
    #[error("invalid VidSrc plaintext encoding")]
    Plaintext,
    #[error("VidSrc native cipher rejected material")]
    Crypto(#[from] CryptoError),
}

/// A key can only be constructed by recognizing the pinned native algorithm.
#[derive(Clone)]
pub struct RecognizedKey(Zeroizing<[u8; 32]>);
impl fmt::Debug for RecognizedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecognizedKey { [REDACTED] }")
    }
}

struct Reader<'a> {
    remaining: &'a [u8],
}
impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], VidSrcError> {
        let (value, rest) = self
            .remaining
            .split_at_checked(size)
            .ok_or(VidSrcError::Module("truncated binary"))?;
        self.remaining = rest;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, VidSrcError> {
        Ok(*self
            .take(1)?
            .first()
            .ok_or(VidSrcError::Module("truncated byte"))?)
    }
    fn unsigned(&mut self) -> Result<u32, VidSrcError> {
        let mut value = 0u32;
        for index in 0..5 {
            let part = self.byte()?;
            if index == 4 && part > 15 {
                return Err(VidSrcError::Module("integer overflow"));
            }
            value |= u32::from(part & 127) << (7 * index);
            if part & 128 == 0 {
                return Ok(value);
            }
        }
        Err(VidSrcError::Module("unterminated integer"))
    }
    fn data_offset(&mut self) -> Result<u32, VidSrcError> {
        let mut value = 0u32;
        for index in 0..5 {
            let part = self.byte()?;
            if index == 4 && part > 7 {
                return Err(VidSrcError::Module("offset overflow"));
            }
            value |= u32::from(part & 127) << (7 * index);
            if part & 128 == 0 {
                if part & 64 != 0 {
                    return Err(VidSrcError::Module("negative offset"));
                }
                return Ok(value);
            }
        }
        Err(VidSrcError::Module("unterminated offset"))
    }
    fn finish(self) -> Result<(), VidSrcError> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(VidSrcError::Module("trailing binary"))
        }
    }
}

fn digest_matches(value: &[u8], expected: &str) -> bool {
    let actual: String = sha256(&[value])
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    actual == expected
}

/// Recognize only the traced type/memory/function/export/data arrangement.
pub fn module_key(module: &[u8]) -> Result<RecognizedKey, VidSrcError> {
    if module.len() > MAX_MODULE_BYTES {
        return Err(VidSrcError::InputLimit);
    }
    if module.get(..8) != Some(b"\0asm\x01\0\0\0") {
        return Err(VidSrcError::Module("header"));
    }
    let mut reader = Reader {
        remaining: module.get(8..).ok_or(VidSrcError::Module("header"))?,
    };
    let mut sections: [Option<&[u8]>; 12] = [None; 12];
    let mut last = 0;
    while !reader.remaining.is_empty() {
        let identifier = reader.byte()?;
        let size = reader.unsigned()? as usize;
        let payload = reader.take(size)?;
        if identifier <= last || !matches!(identifier, 1 | 3 | 5 | 6 | 7 | 10 | 11) {
            return Err(VidSrcError::Module("section layout"));
        }
        last = identifier;
        *sections
            .get_mut(usize::from(identifier))
            .ok_or(VidSrcError::Module("section index"))? = Some(payload);
    }
    let section = |index: usize| {
        sections
            .get(index)
            .copied()
            .flatten()
            .ok_or(VidSrcError::Module("required section"))
    };
    for (index, digest) in [
        (
            1,
            "4ea697dca6034d419831565f2ead73494f4c732e97c7632d5df08e146879076f",
        ),
        (
            5,
            "dbecab43cb8facaa2f8537a707d982cbc168a7767f3b0d3b1f3cd4aa5ef272bf",
        ),
    ] {
        if !digest_matches(section(index)?, digest) {
            return Err(VidSrcError::Module("type or memory fingerprint"));
        }
    }
    let decoys = module_functions(section(3)?)?;
    check_exports(section(7)?, decoys)?;
    let mut code = Reader {
        remaining: section(10)?,
    };
    let count = code.unsigned()?;
    if count != decoys + 3 {
        return Err(VidSrcError::Module("function count"));
    }
    let digests = [
        "a6058c9901cb96927c0da12657be846dff74134fb4e198154f66fbbeb5ede21a",
        "4c292b28a90f8d74b9967e9ab678f12308c6627c3a2b859fbf1bc43208e246ff",
        "3a5e8ab52d551a477c02c6bb906c7b8a109fab114feb0019317671da55718dbc",
    ];
    let mut mask_offset = 0;
    for index in 0..count {
        let size = code.unsigned()? as usize;
        let body = code.take(size)?;
        if index < decoys {
            continue;
        }
        let normalized;
        let checked = if index == decoys {
            let (body, offset) = normalize_block(body)?;
            normalized = body;
            mask_offset = offset;
            normalized.as_slice()
        } else if index == decoys + 2 {
            normalized = normalize_decrypt(body, decoys)?;
            normalized.as_slice()
        } else {
            body
        };
        let expected = digests
            .get((index - decoys) as usize)
            .ok_or(VidSrcError::Module("function index"))?;
        if !digest_matches(checked, expected) {
            return Err(VidSrcError::Module("cipher fingerprint"));
        }
    }
    code.finish()?;
    let mut data = Reader {
        remaining: section(11)?,
    };
    let count = data.unsigned()?;
    if count > 32 {
        return Err(VidSrcError::Module("data segment count"));
    }
    let mut memory = Zeroizing::new([0u8; 4096]);
    let mut occupied = [false; 4096];
    for _ in 0..count {
        if data.unsigned()? != 0 || data.byte()? != 0x41 {
            return Err(VidSrcError::Module("data initializer"));
        }
        let offset = data.data_offset()? as usize;
        if data.byte()? != 0x0b {
            return Err(VidSrcError::Module("data expression"));
        }
        let size = data.unsigned()? as usize;
        let end = offset
            .checked_add(size)
            .ok_or(VidSrcError::Module("data overflow"))?;
        let slots = occupied
            .get_mut(offset..end)
            .ok_or(VidSrcError::Module("data memory bounds"))?;
        if slots.iter().any(|value| *value) {
            return Err(VidSrcError::Module("overlapping data"));
        }
        slots.fill(true);
        memory
            .get_mut(offset..end)
            .ok_or(VidSrcError::Module("data memory bounds"))?
            .copy_from_slice(data.take(size)?);
    }
    data.finish()?;
    let mask = usize::try_from(mask_offset).map_err(|_| VidSrcError::Module("mask offset"))?;
    let mut key = Zeroizing::new([0; 32]);
    for (index, byte) in key.iter_mut().enumerate() {
        let position = mask
            .checked_add(index)
            .ok_or(VidSrcError::Module("mask offset"))?;
        if !occupied.get(index).copied().unwrap_or(false)
            || !occupied.get(position).copied().unwrap_or(false)
        {
            return Err(VidSrcError::Module("missing key material"));
        }
        *byte = *memory.get(index).ok_or(VidSrcError::Module("key bounds"))?
            ^ *memory
                .get(position)
                .ok_or(VidSrcError::Module("mask bounds"))?;
    }
    Ok(RecognizedKey(key))
}

pub fn decrypt_streams(encoded: &str, module: &[u8]) -> Result<Vec<String>, VidSrcError> {
    decrypt_streams_with_key(encoded, &module_key(module)?)
}

/// This upstream envelope has no MAC. The host must validate every returned
/// destination and never infer content identity from successful decryption.
pub fn decrypt_streams_with_key(
    encoded: &str,
    key: &RecognizedKey,
) -> Result<Vec<String>, VidSrcError> {
    if encoded.len() > 128 * 1024 {
        return Err(VidSrcError::InputLimit);
    }
    let filtered: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n'))
        .collect();
    let ciphertext = STANDARD
        .decode(filtered)
        .map_err(|_| VidSrcError::Envelope)?;
    if ciphertext.len() <= 12 || ciphertext.len() > MAX_STREAM_BYTES {
        return Err(VidSrcError::Envelope);
    }
    let (nonce, body) = ciphertext
        .split_at_checked(12)
        .ok_or(VidSrcError::Envelope)?;
    let mut plaintext = Zeroizing::new(body.to_vec());
    chacha20_xor(key.0.as_ref(), nonce, 0, &mut plaintext)?;
    if plaintext.contains(&0) || plaintext.contains(&b'\r') {
        return Err(VidSrcError::Plaintext);
    }
    let text = std::str::from_utf8(&plaintext).map_err(|_| VidSrcError::Plaintext)?;
    Ok(text
        .split('\n')
        .filter(|stream| !stream.is_empty())
        .map(str::to_owned)
        .collect())
}

fn module_functions(section: &[u8]) -> Result<u32, VidSrcError> {
    let mut reader = Reader { remaining: section };
    let count = reader.unsigned()?;
    if !(3..=19).contains(&count) {
        return Err(VidSrcError::Module("function layout"));
    }
    let decoys = count - 3;
    for index in 0..count {
        let expected = if index == decoys {
            1
        } else if index == decoys + 2 {
            2
        } else {
            0
        };
        if reader.unsigned()? != expected {
            return Err(VidSrcError::Module("function signature"));
        }
    }
    reader.finish()?;
    Ok(decoys)
}

fn check_exports(section: &[u8], decoys: u32) -> Result<(), VidSrcError> {
    let mut reader = Reader { remaining: section };
    let count = reader.unsigned()?;
    if count != decoys + 3 {
        return Err(VidSrcError::Module("export count"));
    }
    let mut names = BTreeSet::new();
    let mut indices = BTreeSet::new();
    for _ in 0..count {
        let size = reader.unsigned()? as usize;
        if size > 64 {
            return Err(VidSrcError::Module("export name"));
        }
        let name = reader.take(size)?;
        let kind = reader.byte()?;
        let target = reader.unsigned()?;
        if !names.insert(name) {
            return Err(VidSrcError::Module("duplicate export"));
        }
        if name == b"memory" && kind == 2 && target == 0 {
            continue;
        }
        if kind != 0
            || indices.contains(&target)
            || target == decoys
            || target > decoys + 2
            || (target == decoys + 1 && name != b"alloc")
            || (target == decoys + 2 && name != b"decrypt")
            || (target < decoys && matches!(name, b"alloc" | b"decrypt" | b"memory"))
        {
            return Err(VidSrcError::Module("export mapping"));
        }
        indices.insert(target);
    }
    reader.finish()?;
    if !names.contains(b"memory".as_slice())
        || !names.contains(b"alloc".as_slice())
        || !names.contains(b"decrypt".as_slice())
        || indices.len() != decoys as usize + 2
    {
        return Err(VidSrcError::Module("required exports"));
    }
    Ok(())
}

fn normalize_block(body: &[u8]) -> Result<(Vec<u8>, u32), VidSrcError> {
    let mut normalized = body.to_vec();
    let mut mask = 0;
    for word in 0u32..8 {
        let prefix = [0x41, (word * 4) as u8, 0x28, 2, 0, 0x41];
        let mut count = 0;
        let mut cursor = 0;
        while cursor < body.len() {
            let Some(relative) = body.get(cursor..).and_then(|rest| {
                rest.windows(prefix.len())
                    .position(|candidate| candidate == prefix)
            }) else {
                break;
            };
            let position = cursor + relative + prefix.len();
            let remaining = body
                .get(position..)
                .ok_or(VidSrcError::Module("key load bounds"))?;
            let mut reader = Reader { remaining };
            let offset = reader.data_offset()?;
            let consumed = remaining.len() - reader.remaining.len();
            if consumed != 2
                || reader.remaining.get(..4) != Some(&[0x28, 2, 0, 0x73])
                || offset < word * 4
            {
                return Err(VidSrcError::Module("key load"));
            }
            let base = offset - word * 4;
            if !(512..=3968).contains(&base)
                || !base.is_multiple_of(128)
                || (mask != 0 && mask != base)
            {
                return Err(VidSrcError::Module("inconsistent key load"));
            }
            mask = base;
            let canonical = 3584 + word * 4;
            let output = normalized
                .get_mut(position..position + 2)
                .ok_or(VidSrcError::Module("key load bounds"))?;
            output.copy_from_slice(&[(canonical & 127) as u8 | 128, (canonical >> 7) as u8]);
            count += 1;
            cursor = position + consumed;
        }
        if count != 2 {
            return Err(VidSrcError::Module("key load count"));
        }
    }
    Ok((normalized, mask))
}

fn normalize_decrypt(body: &[u8], block_index: u32) -> Result<Vec<u8>, VidSrcError> {
    let prefix = [0x20, 4, 0x41, 0xc0, 0, 0x10];
    let mut positions = body
        .windows(prefix.len())
        .enumerate()
        .filter(|(_, candidate)| *candidate == prefix)
        .map(|(position, _)| position + prefix.len());
    let position = positions.next().ok_or(VidSrcError::Module("block call"))?;
    if positions.next().is_some() || body.get(position).copied() != u8::try_from(block_index).ok() {
        return Err(VidSrcError::Module("block call index"));
    }
    let mut normalized = body.to_vec();
    *normalized
        .get_mut(position)
        .ok_or(VidSrcError::Module("block call bounds"))? = 3;
    Ok(normalized)
}
