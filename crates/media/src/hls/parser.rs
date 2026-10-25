use std::collections::BTreeSet;

use stream_core::providers::{Metadata, Track, Variant};
use url::Url;

use crate::network::{parse_destination_url, validate_destination_url};

use super::{
    ByteRange, HlsError, MAX_LINE_BYTES, MAX_PLAYLIST_BYTES, MAX_RESOLVED_URI_BYTES, MAX_SEGMENTS,
    MAX_TRACKS, MAX_VARIANTS, MediaSample, Playlist, PlaylistKind,
    attributes::{self, Attributes},
};

pub fn parse_playlist(body: &[u8], base: &Url) -> Result<Playlist, HlsError> {
    if body.len() > MAX_PLAYLIST_BYTES {
        return Err(HlsError::LimitExceeded("body"));
    }
    if body
        .split(|byte| *byte == b'\n')
        .any(|line| line.len() > MAX_LINE_BYTES)
    {
        return Err(HlsError::LimitExceeded("line"));
    }
    validate_destination_url(base).map_err(|_| HlsError::InvalidResource)?;
    let text = std::str::from_utf8(body)
        .map_err(|_| HlsError::InvalidUtf8)?
        .trim();
    let mut lines = text.split('\n');
    if lines.next().map(str::trim) != Some("#EXTM3U") {
        return Err(HlsError::InvalidHeader);
    }
    let mut state = Parser {
        base,
        kind: None,
        metadata: Metadata {
            tracks_known: true,
            ..Metadata::default()
        },
        samples: Vec::new(),
        initialization: None,
        encrypted: false,
        unsupported_protection: false,
        end_list: false,
        pending_variant: None,
        pending_range: None,
        pending_segment: false,
        uri_bytes: 0,
        track_names: BTreeSet::new(),
        singleton_tags: BTreeSet::new(),
        current_aes_key: false,
        current_key_iv: false,
        vod: false,
    };
    for raw in lines {
        if raw.len() > MAX_LINE_BYTES {
            return Err(HlsError::LimitExceeded("line"));
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line
            .bytes()
            .any(|byte| (byte < 32 && byte != b'\t') || byte == 127)
        {
            return Err(HlsError::Malformed("line controls"));
        }
        if line.is_empty() {
            continue;
        }
        if !line.starts_with('#') {
            state.resource(line)?;
            continue;
        }
        let (tag, value) = line
            .split_once(':')
            .map_or((line, None), |(tag, value)| (tag, Some(value)));
        match tag {
            "#EXTM3U" => return Err(HlsError::Duplicate("header")),
            "#EXT-X-STREAM-INF" => {
                state.variant(value.ok_or(HlsError::Malformed("variant attributes"))?)?
            }
            "#EXT-X-MEDIA" => state.track(value.ok_or(HlsError::Malformed("track attributes"))?)?,
            "#EXT-X-KEY" => {
                state.mark(PlaylistKind::Media)?;
                state.key(value, false)?;
            }
            "#EXT-X-SESSION-KEY" => {
                state.mark(PlaylistKind::Master)?;
                state.key(value, true)?;
            }
            "#EXT-X-MAP" => {
                state.map(value.ok_or(HlsError::Malformed("initialization attributes"))?)?
            }
            "#EXT-X-BYTERANGE" => {
                state.mark(PlaylistKind::Media)?;
                if state.pending_range.is_some() {
                    return Err(HlsError::Duplicate("byte range"));
                }
                let value = value.ok_or(HlsError::Malformed("byte range"))?;
                parse_range(value, None)?;
                state.pending_range = Some(value);
            }
            "#EXTINF" => {
                state.mark(PlaylistKind::Media)?;
                if state.pending_segment {
                    return Err(HlsError::Incomplete);
                }
                segment_duration(value.ok_or(HlsError::Malformed("segment duration"))?)?;
                state.pending_segment = true;
            }
            "#EXT-X-ENDLIST" => {
                state.mark(PlaylistKind::Media)?;
                state.singleton(tag)?;
                if value.is_some() {
                    return Err(HlsError::Malformed("end list"));
                }
                state.end_list = true;
            }
            "#EXT-X-VERSION" => {
                state.singleton(tag)?;
                positive(value.ok_or(HlsError::Malformed("version"))?, "version")?;
            }
            "#EXT-X-TARGETDURATION" => {
                state.mark(PlaylistKind::Media)?;
                state.singleton(tag)?;
                positive(
                    value.ok_or(HlsError::Malformed("target duration"))?,
                    "target duration",
                )?;
            }
            "#EXT-X-MEDIA-SEQUENCE" | "#EXT-X-DISCONTINUITY-SEQUENCE" => {
                state.mark(PlaylistKind::Media)?;
                state.singleton(tag)?;
                unsigned(value.ok_or(HlsError::Malformed("sequence"))?, "sequence")?;
            }
            "#EXT-X-PLAYLIST-TYPE" => {
                state.mark(PlaylistKind::Media)?;
                state.singleton(tag)?;
                state.vod = match value {
                    Some("VOD") => true,
                    Some("EVENT") => false,
                    _ => return Err(HlsError::Malformed("playlist type")),
                };
            }
            "#EXT-X-INDEPENDENT-SEGMENTS" => {
                state.singleton(tag)?;
                if value.is_some() {
                    return Err(HlsError::Malformed("independent segments"));
                }
            }
            "#EXT-X-DISCONTINUITY" => {
                state.mark(PlaylistKind::Media)?;
                if value.is_some() {
                    return Err(HlsError::Malformed("discontinuity"));
                }
            }
            "#EXT-X-I-FRAME-STREAM-INF" => {
                state.mark(PlaylistKind::Master)?;
                let attributes =
                    attributes::parse(value.ok_or(HlsError::Malformed("I-frame attributes"))?)?;
                state.uri(attributes::required(&attributes, "URI")?)?;
            }
            "#EXT-X-GAP" | "#EXT-X-I-FRAMES-ONLY" => {
                return Err(HlsError::UnsupportedFeature("gap or I-frame-only segments"));
            }
            _ => {}
        }
    }
    if state.pending_variant.is_some()
        || state.pending_segment
        || state.pending_range.is_some()
        || (state.vod && !state.end_list)
    {
        return Err(HlsError::Incomplete);
    }
    let kind = state.kind.ok_or(HlsError::Empty)?;
    if (kind == PlaylistKind::Master && state.metadata.variants.is_empty())
        || (kind == PlaylistKind::Media && state.samples.is_empty())
    {
        return Err(HlsError::Empty);
    }
    state
        .metadata
        .validate()
        .map_err(|_| HlsError::Malformed("metadata"))?;
    Ok(Playlist {
        kind,
        metadata: state.metadata,
        samples: state.samples,
        initialization: state.initialization,
        encrypted: state.encrypted,
        unsupported_protection: state.unsupported_protection,
        end_list: state.end_list,
    })
}

struct Parser<'a> {
    base: &'a Url,
    kind: Option<PlaylistKind>,
    metadata: Metadata,
    samples: Vec<MediaSample>,
    initialization: Option<MediaSample>,
    encrypted: bool,
    unsupported_protection: bool,
    end_list: bool,
    pending_variant: Option<Attributes<'a>>,
    pending_range: Option<&'a str>,
    pending_segment: bool,
    uri_bytes: usize,
    track_names: BTreeSet<(String, String, String)>,
    singleton_tags: BTreeSet<&'a str>,
    current_aes_key: bool,
    current_key_iv: bool,
    vod: bool,
}

impl<'a> Parser<'a> {
    fn mark(&mut self, kind: PlaylistKind) -> Result<(), HlsError> {
        if self.kind.is_some_and(|prior| prior != kind) {
            return Err(HlsError::MixedPlaylist);
        }
        self.kind = Some(kind);
        Ok(())
    }

    fn singleton(&mut self, tag: &'a str) -> Result<(), HlsError> {
        if !self.singleton_tags.insert(tag) {
            return Err(HlsError::Duplicate("playlist tag"));
        }
        Ok(())
    }

    fn uri(&mut self, raw: &str) -> Result<String, HlsError> {
        if raw.is_empty()
            || raw.len() > MAX_LINE_BYTES
            || raw.contains(['\\', '{', '}'])
            || raw.chars().any(char::is_whitespace)
            || raw.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(HlsError::InvalidResource);
        }
        // Absolute and network-relative resources must be checked before URL
        // normalization can erase ambiguous octal/hex/numeric authorities.
        let absolute = if raw.starts_with("//") {
            Some(
                parse_destination_url(&format!("{}:{raw}", self.base.scheme()))
                    .map_err(|_| HlsError::InvalidResource)?,
            )
        } else if Url::parse(raw).is_ok() {
            Some(parse_destination_url(raw).map_err(|_| HlsError::InvalidResource)?)
        } else {
            None
        };
        let resolved = match absolute {
            Some(url) => url,
            None => self.base.join(raw).map_err(|_| HlsError::InvalidResource)?,
        };
        validate_destination_url(&resolved).map_err(|_| HlsError::InvalidResource)?;
        if resolved.as_str().len() > 8192 {
            return Err(HlsError::LimitExceeded("resolved URI"));
        }
        self.uri_bytes = self
            .uri_bytes
            .checked_add(resolved.as_str().len())
            .ok_or(HlsError::LimitExceeded("resolved URI bytes"))?;
        if self.uri_bytes > MAX_RESOLVED_URI_BYTES {
            return Err(HlsError::LimitExceeded("resolved URI bytes"));
        }
        Ok(resolved.into())
    }

    fn variant(&mut self, raw: &'a str) -> Result<(), HlsError> {
        self.mark(PlaylistKind::Master)?;
        if self.pending_variant.is_some() {
            return Err(HlsError::Incomplete);
        }
        if self.metadata.variants.len() >= MAX_VARIANTS {
            return Err(HlsError::LimitExceeded("variants"));
        }
        self.pending_variant = Some(attributes::parse(raw)?);
        Ok(())
    }

    fn track(&mut self, raw: &str) -> Result<(), HlsError> {
        self.mark(PlaylistKind::Master)?;
        let attrs = attributes::parse(raw)?;
        let kind = attributes::required(&attrs, "TYPE")?;
        if matches!(kind, "VIDEO" | "CLOSED-CAPTIONS") {
            return Ok(());
        }
        if !matches!(kind, "AUDIO" | "SUBTITLES") {
            return Err(HlsError::Malformed("track type"));
        }
        if self.metadata.tracks.len() >= MAX_TRACKS {
            return Err(HlsError::LimitExceeded("tracks"));
        }
        let group = attributes::required(&attrs, "GROUP-ID")?.to_owned();
        let name = attributes::required(&attrs, "NAME")?.to_owned();
        if !self
            .track_names
            .insert((kind.to_owned(), group.clone(), name.clone()))
        {
            return Err(HlsError::Duplicate("track name"));
        }
        let default = match attributes::get(&attrs, "DEFAULT") {
            None | Some("NO") => false,
            Some("YES") => true,
            _ => return Err(HlsError::Malformed("track default")),
        };
        let url = match attributes::get(&attrs, "URI") {
            Some(raw) => self.uri(raw)?,
            None if kind == "SUBTITLES" => return Err(HlsError::Malformed("subtitle URI")),
            None => String::new(),
        };
        self.metadata.tracks.push(Track {
            kind: kind.to_ascii_lowercase(),
            language: attributes::get(&attrs, "LANGUAGE")
                .unwrap_or_default()
                .to_owned(),
            name,
            group,
            url,
            default,
        });
        Ok(())
    }

    fn key(&mut self, raw: Option<&str>, session: bool) -> Result<(), HlsError> {
        let attrs = attributes::parse(raw.ok_or(HlsError::Malformed("key attributes"))?)?;
        let method = attributes::required(&attrs, "METHOD")?;
        if method == "NONE" {
            if session || attrs.len() != 1 {
                return Err(HlsError::Malformed("unencrypted key"));
            }
            self.current_aes_key = false;
            self.current_key_iv = false;
            return Ok(());
        }
        self.encrypted = true;
        let format = attributes::get(&attrs, "KEYFORMAT").unwrap_or("identity");
        let versions = attributes::get(&attrs, "KEYFORMATVERSIONS").unwrap_or("1");
        let supported = method == "AES-128" && format == "identity" && versions == "1";
        let uri = attributes::required(&attrs, "URI")?;
        if supported {
            self.uri(uri)?;
        } else {
            self.unsupported_protection = true;
        }
        let iv = attributes::get(&attrs, "IV");
        if let Some(iv) = iv {
            let digits = iv.strip_prefix("0x").ok_or(HlsError::Malformed("key IV"))?;
            if digits.is_empty()
                || digits.len() > 32
                || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(HlsError::Malformed("key IV"));
            }
        }
        if !session {
            self.current_aes_key = supported;
            self.current_key_iv = iv.is_some();
        }
        Ok(())
    }

    fn map(&mut self, raw: &str) -> Result<(), HlsError> {
        self.mark(PlaylistKind::Media)?;
        if self.current_aes_key && !self.current_key_iv {
            return Err(HlsError::Malformed("encrypted map requires explicit IV"));
        }
        let attrs = attributes::parse(raw)?;
        let url = self.uri(attributes::required(&attrs, "URI")?)?;
        let byte_range = attributes::get(&attrs, "BYTERANGE")
            .map(|raw| resolve_range(raw, None, &url))
            .transpose()?;
        let map = MediaSample { url, byte_range };
        if self
            .initialization
            .as_ref()
            .is_some_and(|prior| prior != &map)
        {
            return Err(HlsError::ChangingInitialization);
        }
        // A map first appearing after earlier segments also changes association.
        if self.initialization.is_none() && !self.samples.is_empty() {
            return Err(HlsError::ChangingInitialization);
        }
        self.initialization = Some(map);
        Ok(())
    }

    fn resource(&mut self, raw: &str) -> Result<(), HlsError> {
        let url = self.uri(raw)?;
        if let Some(attrs) = self.pending_variant.take() {
            let bandwidth = attributes::get(&attrs, "BANDWIDTH")
                .map(|raw| positive(raw, "bandwidth"))
                .transpose()?;
            let average = attributes::get(&attrs, "AVERAGE-BANDWIDTH")
                .map(|raw| positive(raw, "average bandwidth"))
                .transpose()?;
            let (width, height) = if let Some(raw) = attributes::get(&attrs, "RESOLUTION") {
                let (width, height) = raw
                    .split_once('x')
                    .ok_or(HlsError::Malformed("resolution"))?;
                (
                    Some(positive(width, "width")?),
                    Some(positive(height, "height")?),
                )
            } else {
                (None, None)
            };
            let codecs = if let Some(raw) = attributes::get(&attrs, "CODECS") {
                let mut codecs = Vec::new();
                for codec in raw.split(',').map(str::trim) {
                    if codecs.len() >= 32 || codec.is_empty() || codec.len() > 64 {
                        return Err(HlsError::Malformed("codecs"));
                    }
                    codecs.push(codec.to_owned());
                }
                codecs
            } else {
                Vec::new()
            };
            self.metadata.variants.push(Variant {
                url,
                audio_group: attributes::get(&attrs, "AUDIO")
                    .unwrap_or_default()
                    .to_owned(),
                subtitle_group: attributes::get(&attrs, "SUBTITLES")
                    .unwrap_or_default()
                    .to_owned(),
                codecs,
                bitrate: average.or(bandwidth),
                width,
                height,
            });
        } else if self.pending_segment {
            if self.samples.len() >= MAX_SEGMENTS {
                return Err(HlsError::LimitExceeded("segments"));
            }
            let byte_range = self
                .pending_range
                .take()
                .map(|raw| resolve_range(raw, self.samples.last(), &url))
                .transpose()?;
            self.samples.push(MediaSample { url, byte_range });
            self.pending_segment = false;
        } else {
            return Err(HlsError::Malformed(
                "URI without a preceding variant or segment tag",
            ));
        }
        Ok(())
    }
}

fn unsigned(raw: &str, field: &'static str) -> Result<i64, HlsError> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HlsError::Malformed(field));
    }
    raw.parse::<i64>().map_err(|_| HlsError::Malformed(field))
}

fn positive(raw: &str, field: &'static str) -> Result<i64, HlsError> {
    let value = unsigned(raw, field)?;
    if value == 0 {
        return Err(HlsError::Malformed(field));
    }
    Ok(value)
}

fn segment_duration(raw: &str) -> Result<(), HlsError> {
    let (duration, _) = raw
        .split_once(',')
        .ok_or(HlsError::Malformed("segment duration"))?;
    if duration.is_empty()
        || !duration
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || duration.bytes().filter(|byte| *byte == b'.').count() > 1
    {
        return Err(HlsError::Malformed("segment duration"));
    }
    let value = duration
        .parse::<f64>()
        .map_err(|_| HlsError::Malformed("segment duration"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(HlsError::Malformed("segment duration"));
    }
    Ok(())
}

fn parse_range(raw: &str, implicit_start: Option<i64>) -> Result<(i64, Option<i64>), HlsError> {
    let (length, offset) = match raw.split_once('@') {
        Some((length, offset)) => (length, Some(unsigned(offset, "byte range offset")?)),
        None => (raw, implicit_start),
    };
    let length = positive(length, "byte range length")?;
    if let Some(offset) = offset {
        ByteRange::new(offset, length)?;
    }
    Ok((length, offset))
}

fn resolve_range(
    raw: &str,
    previous: Option<&MediaSample>,
    url: &str,
) -> Result<ByteRange, HlsError> {
    let implicit_start = if !raw.contains('@') {
        let previous = previous
            .filter(|sample| sample.url == url)
            .ok_or(HlsError::MissingRangePredecessor)?;
        Some(
            previous
                .byte_range
                .ok_or(HlsError::MissingRangePredecessor)?
                .end_exclusive()?,
        )
    } else {
        None
    };
    let (length, offset) = parse_range(raw, implicit_start)?;
    ByteRange::new(offset.ok_or(HlsError::MissingRangePredecessor)?, length)
}
