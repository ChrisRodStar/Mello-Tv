//! Bounded ISO BMFF metadata parsing. It establishes container/sample-entry
//! facts only; no sample decoding or provider-label inference occurs here.
use super::{ProbeFailure, ProbeResult};
use stream_core::providers::Variant;

pub(super) struct BoxView<'a> {
    pub kind: &'a [u8],
    pub body: &'a [u8],
}
pub(super) fn boxes(mut bytes: &[u8]) -> ProbeResult<Vec<BoxView<'_>>> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        if result.len() >= 4096 {
            return Err(ProbeFailure::Unsupported);
        }
        let (size, header) = box_header(bytes, bytes.len() as u64)?;
        let size = usize::try_from(size).map_err(|_| ProbeFailure::Protocol)?;
        let contents = bytes.get(header..size).ok_or(ProbeFailure::Protocol)?;
        let kind = bytes.get(4..8).ok_or(ProbeFailure::Protocol)?;
        result.push(BoxView {
            kind,
            body: contents,
        });
        bytes = bytes.get(size..).ok_or(ProbeFailure::Protocol)?;
    }
    Ok(result)
}

pub(super) fn box_header(bytes: &[u8], available: u64) -> ProbeResult<(u64, usize)> {
    let size = u32_at(bytes, 0)?;
    let (size, header) = match size {
        0 => (available, 8),
        1 => (
            u64::from_be_bytes(
                bytes
                    .get(8..16)
                    .ok_or(ProbeFailure::Protocol)?
                    .try_into()
                    .map_err(|_| ProbeFailure::Protocol)?,
            ),
            16,
        ),
        value => (u64::from(value), 8),
    };
    if size < header as u64 || size > available {
        return Err(ProbeFailure::Protocol);
    }
    Ok((size, header))
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> ProbeResult<u32> {
    let end = offset.checked_add(4).ok_or(ProbeFailure::Protocol)?;
    Ok(u32::from_be_bytes(
        bytes
            .get(offset..end)
            .ok_or(ProbeFailure::Protocol)?
            .try_into()
            .map_err(|_| ProbeFailure::Protocol)?,
    ))
}
fn u16_at(bytes: &[u8], offset: usize) -> ProbeResult<u16> {
    let end = offset.checked_add(2).ok_or(ProbeFailure::Protocol)?;
    Ok(u16::from_be_bytes(
        bytes
            .get(offset..end)
            .ok_or(ProbeFailure::Protocol)?
            .try_into()
            .map_err(|_| ProbeFailure::Protocol)?,
    ))
}

pub(super) fn inspect(bytes: &[u8], variant: &mut Variant) -> ProbeResult<()> {
    let mut remaining = 4096;
    inspect_bounded(bytes, variant, 0, &mut remaining)
}

fn inspect_bounded(
    bytes: &[u8],
    variant: &mut Variant,
    depth: u8,
    remaining: &mut usize,
) -> ProbeResult<()> {
    let mut stack = vec![(bytes, depth)];
    while let Some((data, depth)) = stack.pop() {
        if depth > 8 {
            return Err(ProbeFailure::Unsupported);
        }
        let items = boxes(data)?;
        *remaining = remaining
            .checked_sub(items.len())
            .ok_or(ProbeFailure::Unsupported)?;
        for item in items.into_iter().rev() {
            match item.kind {
                b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" => {
                    stack.push((item.body, depth + 1))
                }
                b"pssh" | b"sinf" | b"senc" | b"saiz" | b"saio" => {
                    return Err(ProbeFailure::Unsupported);
                }
                b"stsd" => sample_description(item.body, variant, depth, remaining)?,
                _ => {}
            }
        }
    }
    Ok(())
}

fn sample_description(
    body: &[u8],
    variant: &mut Variant,
    depth: u8,
    remaining: &mut usize,
) -> ProbeResult<()> {
    if depth > 8 || u32_at(body, 0)? != 0 || u32_at(body, 4)? != 1 {
        return Err(ProbeFailure::Unsupported);
    }
    let entries = boxes(body.get(8..).ok_or(ProbeFailure::Protocol)?)?;
    *remaining = remaining
        .checked_sub(entries.len())
        .ok_or(ProbeFailure::Unsupported)?;
    if entries.len() != 1 {
        return Err(ProbeFailure::Unsupported);
    }
    let entry = entries.first().ok_or(ProbeFailure::Protocol)?;
    let kind = std::str::from_utf8(entry.kind).map_err(|_| ProbeFailure::Protocol)?;
    let config = match entry.kind {
        b"avc1" | b"avc3" | b"hvc1" | b"hev1" => {
            if variant.height.is_some() || entry.body.len() < 78 {
                return Err(ProbeFailure::Unsupported);
            }
            let width = i64::from(u16_at(entry.body, 24)?);
            let height = i64::from(u16_at(entry.body, 26)?);
            if width == 0 || height == 0 {
                return Err(ProbeFailure::Protocol);
            }
            variant.width = Some(width);
            variant.height = Some(height);
            entry.body.get(78..).ok_or(ProbeFailure::Protocol)?
        }
        b"mp4a" | b"ac-3" | b"ec-3" => {
            if entry.body.len() < 28 || u16_at(entry.body, 8)? != 0 {
                return Err(ProbeFailure::Unsupported);
            }
            entry.body.get(28..).ok_or(ProbeFailure::Protocol)?
        }
        b"text" if entry.body.len() >= 8 => return Ok(()),
        _ => return Err(ProbeFailure::Unsupported),
    };
    if depth >= 8 {
        return Err(ProbeFailure::Unsupported);
    }
    inspect_bounded(config, variant, depth + 1, remaining)?;
    if variant.codecs.len() >= 32 {
        return Err(ProbeFailure::Unsupported);
    }
    variant.codecs.push(sample_codec(kind, config)?);
    Ok(())
}

fn sample_codec(kind: &str, config: &[u8]) -> ProbeResult<String> {
    let mut codec = kind.to_owned();
    let mut found = false;
    for item in boxes(config)? {
        if matches!(kind, "avc1" | "avc3") && item.kind == b"avcC" {
            if found || item.body.len() < 7 || item.body.first() != Some(&1) {
                return Err(ProbeFailure::Protocol);
            }
            let profile = item.body.get(1..4).ok_or(ProbeFailure::Protocol)?;
            codec = format!(
                "{kind}.{:02x}{:02x}{:02x}",
                profile[0], profile[1], profile[2]
            );
            found = true;
        }
        if kind == "mp4a" && item.kind == b"esds" {
            if found {
                return Err(ProbeFailure::Protocol);
            }
            codec = audio_codec(item.body)?;
            found = true;
        }
    }
    Ok(codec)
}

fn descriptor(body: &[u8]) -> ProbeResult<(u8, &[u8], &[u8])> {
    let tag = *body.first().ok_or(ProbeFailure::Protocol)?;
    let mut size = 0_usize;
    for index in 1..=4 {
        let byte = *body.get(index).ok_or(ProbeFailure::Protocol)?;
        size = (size << 7) | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            let start = index + 1;
            let end = start.checked_add(size).ok_or(ProbeFailure::Protocol)?;
            return Ok((
                tag,
                body.get(start..end).ok_or(ProbeFailure::Protocol)?,
                body.get(end..).ok_or(ProbeFailure::Protocol)?,
            ));
        }
    }
    Err(ProbeFailure::Protocol)
}

fn audio_codec(body: &[u8]) -> ProbeResult<String> {
    if u32_at(body, 0)? != 0 {
        return Err(ProbeFailure::Unsupported);
    }
    let (tag, elementary, remaining) = descriptor(body.get(4..).ok_or(ProbeFailure::Protocol)?)?;
    if tag != 3 || !remaining.is_empty() || elementary.len() < 3 {
        return Err(ProbeFailure::Protocol);
    }
    let flags = *elementary.get(2).ok_or(ProbeFailure::Protocol)?;
    let mut offset = 3_usize;
    if flags & 0x80 != 0 {
        offset += 2;
    }
    if flags & 0x40 != 0 {
        offset += 1 + usize::from(*elementary.get(offset).ok_or(ProbeFailure::Protocol)?);
    }
    if flags & 0x20 != 0 {
        offset += 2;
    }
    let (tag, decoder, _) = descriptor(elementary.get(offset..).ok_or(ProbeFailure::Protocol)?)?;
    if tag != 4 || decoder.len() < 13 || (decoder[1] >> 2) & 0x3f != 5 {
        return Err(ProbeFailure::Protocol);
    }
    let object = *decoder.first().ok_or(ProbeFailure::Protocol)?;
    if object != 0x40 {
        return Ok(format!("mp4a.{object:02x}"));
    }
    let mut rest = decoder.get(13..).ok_or(ProbeFailure::Protocol)?;
    while !rest.is_empty() {
        let (tag, specific, next) = descriptor(rest)?;
        rest = next;
        if tag == 5 {
            if specific.len() < 2 {
                return Err(ProbeFailure::Protocol);
            }
            let mut kind = u16::from(specific[0] >> 3);
            if kind == 31 {
                kind = 32 + (u16_at(specific, 0)? >> 5 & 0x3f);
            }
            if kind == 0 {
                return Err(ProbeFailure::Protocol);
            }
            return Ok(format!("mp4a.40.{kind}"));
        }
    }
    Ok("mp4a.40".into())
}

pub(super) fn transport_stream(body: &[u8]) -> bool {
    if body.len() < 5 * 188 {
        return false;
    }
    for packet in body.as_chunks::<188>().0.iter().take(5) {
        if packet[0] != 0x47
            || packet[1] & 0x80 != 0
            || packet[3] & 0x30 == 0
            || (packet[3] & 0x20 != 0 && usize::from(packet[4]) + 5 > 188)
        {
            return false;
        }
    }
    true
}

pub(super) fn fragment(body: &[u8]) -> bool {
    let mut cursor = body;
    for _ in 0..16 {
        if cursor.len() < 8 {
            return false;
        }
        let Some(kind) = cursor.get(4..8) else {
            return false;
        };
        let Ok(size) = u32_at(cursor, 0) else {
            return false;
        };
        if size < 8 {
            return false;
        }
        if matches!(kind, b"moof" | b"mdat") {
            return true;
        }
        if !matches!(kind, b"styp" | b"sidx" | b"free" | b"emsg") {
            return false;
        }
        let Ok(size) = usize::try_from(size) else {
            return false;
        };
        let Some(next) = cursor.get(size..) else {
            return false;
        };
        cursor = next;
    }
    false
}
