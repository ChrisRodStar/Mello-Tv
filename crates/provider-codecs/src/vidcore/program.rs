use std::{collections::BTreeMap, sync::OnceLock};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use regex::{Captures, Regex};

use crate::{MAX_PROTOCOL_BYTES, sha256};

use super::{
    MAX_PLAYER_BYTES, Protocol, ProtocolError, ProtocolParameters, ProtocolVersion, URL_ALPHABET,
};

struct Patterns {
    module: Regex,
    boundary: Regex,
    list: Regex,
    route: Regex,
    unlock: Regex,
    unlock_end: Regex,
    codec: Regex,
    shift_codec: Regex,
    bytes: Regex,
    blob: Regex,
    xor_blob: Regex,
    order: Regex,
    mask: Regex,
    alphabet: Regex,
    csrf: Regex,
}

fn patterns() -> Result<&'static Patterns, ProtocolError> {
    static PATTERNS: OnceLock<Result<Patterns, regex::Error>> = OnceLock::new();
    PATTERNS.get_or_init(|| Ok(Patterns {
        module: Regex::new(r"var ([\w$]+)=([\w$]+)\((\d+)\),[\w$]+=([\w$]+)\((\d+)\)\.Buffer;")?,
        boundary: Regex::new(r"let [\w$]+=[\w$]+=>\{let [\w$]+=Math\.floor\(")?,
        list: Regex::new(r"var [\w$]+=0[;,](?:[\w$]+=function\(\)\{for\(var e=(?:Array\(\d+\)|\[,+\])|[\w$]+\[\[[\d,]+\]\.map\(function\([\w$]+\)|[\w$]+=function\([^)]*\)\{for\(var [\w$]+=new Uint8Array)")?,
        route: Regex::new(r#"fetch\(""\.concat\("([^"]+)","/"\)\.concat\("([^"]+)","/"\)"#)?,
        unlock: Regex::new(r"let ([\w$]+)=\[\];var [\w$,]+=0[;,]")?,
        unlock_end: Regex::new(r";await ([\w$]+),[\w$]+\.current=")?,
        codec: Regex::new(r#"([\w$]+)=\[([\d,]+)\]\.map\(function\([\w$]+\)\{return String\.fromCharCode\((\d+)\^[\w$]+\)\}\)\.join\(""\)"#)?,
        shift_codec: Regex::new(r#"([\w$]+)="([^"]+)"\.split\(""\)\.map\(function\([\w$]+\)\{return String\.fromCharCode\([\w$]+\.charCodeAt\(0\)-(\d+)\)\}\)\.join\(""\)"#)?,
        bytes: Regex::new(r"([\w$]+)=new Uint8Array\(\[([\d,]+)\]\)")?,
        blob: Regex::new(r#"([\w$]+)=[\w$]+\.from\("([^"]+)",([\w$]+)\)"#)?,
        xor_blob: Regex::new(r#"([\w$]+)=function\([^)]*\)\{[^}]*\.from\(e,([\w$]+)\)[^}]*\^=(\d+);return [\w$]+\}\("([^"]+)",0\)"#)?,
        order: Regex::new(r"case \d+:[\w$]+\[(\d+)\]=([\w$]+)")?,
        mask: Regex::new(r"a=(0x[\da-f]+),r=0;r<256;r\+\+\)\{var [\w$]+=a=a\+(0x[\da-f]+)>>>0;")?,
        alphabet: Regex::new(r#"let [\w$]+=(\[[^;]+?\]),[\w$]+=JSON.parse\('(\[[^']+\])'\);"#)?,
        csrf: Regex::new(r#"X-Csrf-Token["']\s*:\s*["']([^"']+)["']"#)?,
    })).as_ref().map_err(|_| ProtocolError::UnsupportedStructure)
}

fn capture<'a>(captures: &Captures<'a>, index: usize) -> Result<&'a str, ProtocolError> {
    captures
        .get(index)
        .map(|value| value.as_str())
        .ok_or(ProtocolError::UnsupportedStructure)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Material {
    pub base: String,
    pub action: String,
    pub require_name: String,
    pub crypto_module: String,
    pub buffer_module: String,
}

struct Fragments<'a> {
    material: Material,
    prefix: &'a str,
    list: &'a str,
    unlock: &'a str,
}

fn fragments(source: &str) -> Result<Fragments<'_>, ProtocolError> {
    if source.len() > MAX_PLAYER_BYTES {
        return Err(ProtocolError::InputLimit);
    }
    let patterns = patterns()?;
    let modules = patterns
        .module
        .captures(source)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let module = modules.get(0).ok_or(ProtocolError::UnsupportedStructure)?;
    let boundary = patterns
        .boundary
        .find(source)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    if boundary.start() < module.start() || capture(&modules, 2)? != capture(&modules, 4)? {
        return Err(ProtocolError::UnsupportedStructure);
    }
    let prefix = source
        .get(module.start()..boundary.start())
        .ok_or(ProtocolError::UnsupportedStructure)?;
    if !prefix.contains("JSON.parse('[") {
        return Err(ProtocolError::UnsupportedStructure);
    }
    let remainder = source
        .get(boundary.start()..)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let list = patterns
        .list
        .find(remainder)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let list_start = boundary
        .start()
        .checked_add(list.start())
        .ok_or(ProtocolError::InputLimit)?;
    let remainder = source
        .get(list_start..)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let list_length = remainder
        .find(",()=>{clearInterval(")
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let list_end = list_start
        .checked_add(list_length)
        .ok_or(ProtocolError::InputLimit)?;
    let list = source
        .get(list_start..list_end)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let tail = source
        .get(list_end..)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let route = patterns
        .route
        .captures(tail)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let base = capture(&route, 1)?;
    if !base.starts_with('/') || base.starts_with("//") {
        return Err(ProtocolError::UnsupportedStructure);
    }
    let unlock_start = patterns
        .unlock
        .find(tail)
        .ok_or(ProtocolError::UnsupportedStructure)?
        .start();
    let unlock = tail
        .get(unlock_start..)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    let unlock_end = patterns
        .unlock_end
        .find(unlock)
        .ok_or(ProtocolError::UnsupportedStructure)?
        .start();
    let unlock = unlock
        .get(..unlock_end)
        .ok_or(ProtocolError::UnsupportedStructure)?;
    Ok(Fragments {
        material: Material {
            base: base.to_owned(),
            action: capture(&route, 2)?.to_owned(),
            require_name: capture(&modules, 2)?.to_owned(),
            crypto_module: capture(&modules, 3)?.to_owned(),
            buffer_module: capture(&modules, 5)?.to_owned(),
        },
        prefix,
        list,
        unlock,
    })
}

pub fn parse_material(source: &str) -> Result<Material, ProtocolError> {
    Ok(fragments(source)?.material)
}

pub fn extract_csrf_token(source: &str) -> Result<Option<String>, ProtocolError> {
    if source.len() > MAX_PLAYER_BYTES {
        return Err(ProtocolError::InputLimit);
    }
    let mut token: Option<&str> = None;
    for captures in patterns()?.csrf.captures_iter(source) {
        let value = capture(&captures, 1)?;
        if value.len() > 4096
            || value.bytes().any(|byte| byte.is_ascii_control())
            || token.is_some_and(|prior| prior != value)
        {
            return Err(ProtocolError::Invalid("CSRF token"));
        }
        token = Some(value);
    }
    Ok(token.map(str::to_owned))
}

fn literal(raw: &str, codec: &str) -> Result<Vec<u8>, ProtocolError> {
    if raw.len() > MAX_PROTOCOL_BYTES {
        return Err(ProtocolError::InputLimit);
    }
    match codec {
        "base64" => STANDARD
            .decode(raw)
            .map_err(|_| ProtocolError::Invalid("base64 literal")),
        "hex" => {
            // Preserve Buffer/Go's deliberate odd-nibble truncation.
            let raw = raw
                .get(..raw.len() / 2 * 2)
                .ok_or(ProtocolError::Invalid("hex literal"))?;
            raw.as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let high = char::from(pair[0])
                        .to_digit(16)
                        .ok_or(ProtocolError::Invalid("hex literal"))?;
                    let low = char::from(pair[1])
                        .to_digit(16)
                        .ok_or(ProtocolError::Invalid("hex literal"))?;
                    u8::try_from(high * 16 + low).map_err(|_| ProtocolError::Invalid("hex literal"))
                })
                .collect()
        }
        _ => Err(ProtocolError::Invalid("literal encoding")),
    }
}

fn byte_mask(mut seed: u32, step: u32) -> [u8; 256] {
    let mut output = [0; 256];
    for value in &mut output {
        seed = seed.wrapping_add(step);
        let mut mixed = seed;
        mixed = (mixed ^ (mixed >> 16)).wrapping_mul(0x85ebca6b);
        mixed = (mixed ^ (mixed >> 13)).wrapping_mul(0xc2b2ae35);
        mixed ^= mixed >> 16;
        *value = mixed.to_le_bytes()[0];
    }
    output
}

/// Decode bounded static program fragments, without executing instructions.
/// Only `Protocol::from_source` establishes recognized instruction fingerprints.
pub fn decode_program(
    prefix: &str,
    stage: &str,
    expected_parts: usize,
) -> Result<Vec<u8>, ProtocolError> {
    if prefix.len() > MAX_PLAYER_BYTES
        || stage.len() > MAX_PLAYER_BYTES
        || !(1..=64).contains(&expected_parts)
    {
        return Err(ProtocolError::InputLimit);
    }
    let patterns = patterns()?;
    let mut codecs = BTreeMap::new();
    let mut blobs = BTreeMap::new();
    for (index, captures) in patterns.codec.captures_iter(prefix).enumerate() {
        if index >= 4096 {
            return Err(ProtocolError::InputLimit);
        }
        let key = capture(&captures, 3)?
            .parse::<u32>()
            .map_err(|_| ProtocolError::Invalid("codec key"))?;
        let mut codec = String::new();
        for raw in capture(&captures, 2)?.split(',') {
            let value = raw
                .parse::<u32>()
                .map_err(|_| ProtocolError::Invalid("codec codepoint"))?
                ^ key;
            codec.push(char::from_u32(value).ok_or(ProtocolError::Invalid("codec codepoint"))?);
        }
        codecs.insert(capture(&captures, 1)?, codec);
    }
    for (index, captures) in patterns.shift_codec.captures_iter(prefix).enumerate() {
        if index >= 4096 {
            return Err(ProtocolError::InputLimit);
        }
        let shift = capture(&captures, 3)?
            .parse::<u32>()
            .map_err(|_| ProtocolError::Invalid("codec shift"))?;
        let mut codec = String::new();
        for character in capture(&captures, 2)?.chars() {
            let value = u32::from(character)
                .checked_sub(shift)
                .and_then(char::from_u32)
                .ok_or(ProtocolError::Invalid("codec shift"))?;
            codec.push(value);
        }
        codecs.insert(capture(&captures, 1)?, codec);
    }
    for (index, captures) in patterns.bytes.captures_iter(prefix).enumerate() {
        if index >= 4096 {
            return Err(ProtocolError::InputLimit);
        }
        let values: Vec<u8> = capture(&captures, 2)?
            .split(',')
            .map(|raw| {
                raw.parse::<u8>()
                    .map_err(|_| ProtocolError::Invalid("byte literal"))
            })
            .collect::<Result<_, _>>()?;
        blobs.insert(capture(&captures, 1)?, values);
    }
    for (index, captures) in patterns.blob.captures_iter(prefix).enumerate() {
        if index >= 4096 {
            return Err(ProtocolError::InputLimit);
        }
        if let Some(codec) = codecs.get(capture(&captures, 3)?) {
            blobs.insert(
                capture(&captures, 1)?,
                literal(capture(&captures, 2)?, codec)?,
            );
        }
    }
    for (index, captures) in patterns.xor_blob.captures_iter(prefix).enumerate() {
        if index >= 4096 {
            return Err(ProtocolError::InputLimit);
        }
        let codec = codecs
            .get(capture(&captures, 2)?)
            .ok_or(ProtocolError::Invalid("blob codec"))?;
        let mut data = literal(capture(&captures, 4)?, codec)?;
        let key = capture(&captures, 3)?
            .parse::<u8>()
            .map_err(|_| ProtocolError::Invalid("blob XOR key"))?;
        for value in &mut data {
            *value ^= key;
        }
        blobs.insert(capture(&captures, 1)?, data);
    }
    let mut ordered = vec![None; expected_parts];
    for (match_index, captures) in patterns.order.captures_iter(stage).enumerate() {
        if match_index >= expected_parts {
            return Err(ProtocolError::Invalid("part ordering"));
        }
        let index = capture(&captures, 1)?
            .parse::<usize>()
            .map_err(|_| ProtocolError::Invalid("part index"))?;
        let slot = ordered
            .get_mut(index)
            .ok_or(ProtocolError::Invalid("part index"))?;
        if slot.is_some() {
            return Err(ProtocolError::Invalid("duplicate part index"));
        }
        *slot = Some(
            blobs
                .get(capture(&captures, 2)?)
                .ok_or(ProtocolError::Invalid("missing part"))?
                .as_slice(),
        );
    }
    let mut combined = Vec::new();
    for part in ordered {
        let part = part
            .filter(|part| !part.is_empty())
            .ok_or(ProtocolError::Invalid("missing part"))?;
        if combined
            .len()
            .checked_add(part.len())
            .is_none_or(|length| length > MAX_PROTOCOL_BYTES)
        {
            return Err(ProtocolError::InputLimit);
        }
        combined.extend_from_slice(part);
    }
    let mut masks = [[0; 256]; 2];
    let mut count = 0usize;
    for captures in patterns.mask.captures_iter(stage) {
        let slot = masks
            .get_mut(count)
            .ok_or(ProtocolError::Invalid("mask structure"))?;
        let seed = u32::from_str_radix(
            capture(&captures, 1)?
                .get(2..)
                .ok_or(ProtocolError::Invalid("mask seed"))?,
            16,
        )
        .map_err(|_| ProtocolError::Invalid("mask seed"))?;
        let step = u32::from_str_radix(
            capture(&captures, 2)?
                .get(2..)
                .ok_or(ProtocolError::Invalid("mask step"))?,
            16,
        )
        .map_err(|_| ProtocolError::Invalid("mask step"))?;
        *slot = byte_mask(seed, step);
        count += 1;
    }
    if count != 2 {
        return Err(ProtocolError::Invalid("mask structure"));
    }
    for (index, value) in combined.iter_mut().enumerate() {
        *value ^= masks[0][index % 256] ^ masks[1][index % 256];
    }
    Ok(combined)
}

/// Read a mapped string only after instruction fingerprint verification.
pub fn string_constant(code: &[u8], offset: usize) -> Result<&str, ProtocolError> {
    let end = offset
        .checked_add(4)
        .ok_or(ProtocolError::Invalid("constant offset"))?;
    let header = code
        .get(offset..end)
        .ok_or(ProtocolError::Invalid("constant header"))?;
    if !matches!((header[0], header[1]), (103, 188) | (89, 4) | (72, 105)) {
        return Err(ProtocolError::Invalid("constant opcode"));
    }
    let length = usize::from(u16::from_be_bytes([header[2], header[3]]));
    let bytes = code
        .get(
            end..end
                .checked_add(length)
                .ok_or(ProtocolError::Invalid("constant length"))?,
        )
        .ok_or(ProtocolError::Invalid("constant length"))?;
    std::str::from_utf8(bytes).map_err(|_| ProtocolError::Invalid("constant text"))
}

struct Layout {
    version: ProtocolVersion,
    material: &'static str,
    list: &'static str,
    unlock: &'static str,
    list_parts: usize,
    bases: [usize; 2],
    actions: [usize; 2],
    key: usize,
    iv: usize,
    seed: usize,
    response_seed: usize,
    response_numbers: [usize; 2],
    stage_keys: [usize; 5],
    prefixes: [usize; 5],
    rc4_keys: [usize; 5],
}

const LAYOUTS: [Layout; 3] = [
    Layout {
        version: ProtocolVersion::V3,
        material: "503e3df6cc9ffc6d239ce5dc92d99bc7720b85d8dfb83658733a74e98fbd6def",
        list: "178831c68e6a878d155f092ed2eab62d1309291be032e6f046196bf96857f801",
        unlock: "8f85b5136be7712309d6fb8dd8e157066c82a8bea454c224d9b07f297478b419",
        list_parts: 5,
        bases: [45603, 51421],
        actions: [45862, 51680],
        key: 36901,
        iv: 37027,
        seed: 37120,
        response_seed: 48097,
        response_numbers: [48506, 48608],
        stage_keys: [29951, 31250, 32615, 34051, 35487],
        prefixes: [30034, 31333, 32698, 34134, 35570],
        rc4_keys: [46801, 47316, 47574, 47832, 47058],
    },
    Layout {
        version: ProtocolVersion::Current,
        material: "af13d5c75ee77178a31554392c906bd94ee3e979d62691690108f3ded337a885",
        list: "a39fcc5958024e588c963ebd34ece56b49f59e37539edab7df082288b28a23e3",
        unlock: "ac9a1f7e7b5b4d2b28a32c2e73f6dc494f2596ed8dfedac3e6ed86b5f6545086",
        list_parts: 6,
        bases: [45623, 51230],
        actions: [45706, 51313],
        key: 36912,
        iv: 37038,
        seed: 37132,
        response_seed: 47894,
        response_numbers: [48303, 48405],
        stage_keys: [29963, 31265, 32632, 34070, 35495],
        prefixes: [30046, 31348, 32715, 34153, 35578],
        rc4_keys: [46598, 47114, 47372, 47629, 46856],
    },
    Layout {
        version: ProtocolVersion::Legacy,
        material: "52dbf762faf5d6f5fee0aa71e8cffda15567a7640fd217610eb8a13fda8ff8d5",
        list: "e5872b95b115bc1ab52e148454837d0354cbd96c73c8fbf28fa75c6808c749d1",
        unlock: "18948e99225f9d2fd88a05e0615a64b5d34e80ed3a9597a75da6beb560e560e4",
        list_parts: 6,
        bases: [45636, 51272],
        actions: [45754, 51390],
        key: 36941,
        iv: 37067,
        seed: 37161,
        response_seed: 47943,
        response_numbers: [48351, 48453],
        stage_keys: [29980, 31282, 32650, 34087, 35525],
        prefixes: [30063, 31365, 32733, 34170, 35608],
        rc4_keys: [46646, 47162, 47420, 47678, 46904],
    },
];

fn fingerprint(parts: &[&[u8]]) -> String {
    sha256(parts)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixed<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], ProtocolError> {
    bytes
        .try_into()
        .map_err(|_| ProtocolError::Invalid("key dimensions"))
}

pub(super) fn compile(source: &str) -> Result<Protocol, ProtocolError> {
    let fragments = fragments(source)?;
    let material_hash = fingerprint(&[
        fragments.prefix.as_bytes(),
        b"\n",
        fragments.list.as_bytes(),
        b"\n",
        fragments.unlock.as_bytes(),
    ]);
    let layout = LAYOUTS
        .iter()
        .find(|layout| layout.material == material_hash)
        .ok_or(ProtocolError::UnknownMaterial)?;
    let list = decode_program(fragments.prefix, fragments.list, layout.list_parts)?;
    let unlock = decode_program(fragments.prefix, fragments.unlock, 5)?;
    if fingerprint(&[&list]) != layout.list || fingerprint(&[&unlock]) != layout.unlock {
        return Err(ProtocolError::UnknownInstructions);
    }
    let read = |offset| string_constant(&list, offset);
    if layout.bases.into_iter().any(|offset| match read(offset) {
        Ok(value) => value != fragments.material.base,
        Err(_) => true,
    }) {
        return Err(ProtocolError::Invalid("base mismatch"));
    }
    let mut stage_keys = [[0; 32]; 5];
    let mut prefixes = std::array::from_fn(|_| Vec::new());
    let mut rc4_keys = [[0; 32]; 5];
    for index in 0..5 {
        stage_keys[index] = fixed(literal(read(layout.stage_keys[index])?, "base64")?)?;
        prefixes[index] = literal(read(layout.prefixes[index])?, "base64")?;
        rc4_keys[index] = fixed(literal(read(layout.rc4_keys[index])?, "base64")?)?;
    }
    let alphabet_capture = patterns()?
        .alphabet
        .captures(fragments.prefix)
        .ok_or(ProtocolError::Invalid("alphabet"))?;
    let sources: Vec<String> = serde_json::from_str(capture(&alphabet_capture, 1)?)
        .map_err(|_| ProtocolError::Invalid("alphabet"))?;
    let targets: Vec<String> = serde_json::from_str(capture(&alphabet_capture, 2)?)
        .map_err(|_| ProtocolError::Invalid("alphabet"))?;
    if sources.len() != 64 || targets.len() != 64 {
        return Err(ProtocolError::Invalid("alphabet"));
    }
    let mut alphabet = [0u8; 64];
    let mut assigned = [false; 64];
    for (source, target) in sources.iter().zip(targets) {
        if source.len() != 1 || target.len() != 1 {
            return Err(ProtocolError::Invalid("alphabet"));
        }
        let index = URL_ALPHABET
            .iter()
            .position(|symbol| Some(symbol) == source.as_bytes().first())
            .ok_or(ProtocolError::Invalid("alphabet"))?;
        if assigned[index] {
            return Err(ProtocolError::Invalid("alphabet"));
        }
        assigned[index] = true;
        alphabet[index] = *target
            .as_bytes()
            .first()
            .ok_or(ProtocolError::Invalid("alphabet"))?;
    }
    let response_numbers = layout
        .response_numbers
        .into_iter()
        .map(|offset| {
            read(offset)?
                .parse::<u64>()
                .map_err(|_| ProtocolError::Invalid("response-key component"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut protocol = Protocol::from_parameters(ProtocolParameters {
        base: fragments.material.base,
        challenge_action: read(layout.actions[0])?.to_owned(),
        list_action: read(layout.actions[1])?.to_owned(),
        unlock_action: fragments.material.action,
        csrf_token: extract_csrf_token(source)?.unwrap_or_default(),
        key: fixed(literal(read(layout.key)?, "hex")?)?,
        iv: fixed(literal(read(layout.iv)?, "hex")?)?,
        seed: literal(read(layout.seed)?, "hex")?,
        response_seed: fixed(literal(read(layout.response_seed)?, "hex")?)?,
        response_numbers,
        alphabet,
        stage_keys,
        stage_prefixes: prefixes,
        rc4_keys,
    })?;
    protocol.version = Some(layout.version);
    Ok(protocol)
}
