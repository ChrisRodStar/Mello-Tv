use std::collections::BTreeMap;

use super::{HlsError, MAX_LINE_BYTES};

pub(super) type Attributes<'a> = BTreeMap<&'a str, &'a str>;

/// Attribute values borrow the bounded manifest. Commas inside a quoted value
/// do not split fields; HLS quoted strings have no backslash escape syntax.
pub(super) fn parse(input: &str) -> Result<Attributes<'_>, HlsError> {
    if input.len() > MAX_LINE_BYTES {
        return Err(HlsError::LimitExceeded("attributes"));
    }
    let mut result = BTreeMap::new();
    let mut quoted = false;
    let mut start = 0;
    for (index, byte) in input.bytes().enumerate() {
        if byte == b'"' {
            quoted = !quoted;
        }
        if byte == b',' && !quoted {
            insert(&input[start..index], &mut result)?;
            start = index + 1;
        }
    }
    if quoted {
        return Err(HlsError::Malformed("attribute quote"));
    }
    insert(&input[start..], &mut result)?;
    Ok(result)
}

fn insert<'a>(field: &'a str, attributes: &mut Attributes<'a>) -> Result<(), HlsError> {
    if attributes.len() >= 64 {
        return Err(HlsError::LimitExceeded("attribute count"));
    }
    let (key, raw) = field
        .trim()
        .split_once('=')
        .ok_or(HlsError::Malformed("attribute pair"))?;
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(HlsError::Malformed("attribute name"));
    }
    let raw = raw.trim();
    let value = if let Some(value) = raw.strip_prefix('"') {
        let value = value
            .strip_suffix('"')
            .ok_or(HlsError::Malformed("attribute quote"))?;
        if value.contains('"') {
            return Err(HlsError::Malformed("attribute quote"));
        }
        value
    } else {
        if raw.is_empty() || raw.contains('"') || raw.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            return Err(HlsError::Malformed("attribute value"));
        }
        raw
    };
    if attributes.insert(key, value).is_some() {
        return Err(HlsError::Duplicate("attribute"));
    }
    Ok(())
}

pub(super) fn get<'a>(attributes: &Attributes<'a>, key: &str) -> Option<&'a str> {
    attributes.get(key).copied()
}

pub(super) fn required<'a>(attributes: &Attributes<'a>, key: &str) -> Result<&'a str, HlsError> {
    get(attributes, key)
        .filter(|value| !value.is_empty())
        .ok_or(HlsError::Malformed("required attribute"))
}
