//! Bounded, fail-closed RFC 8216 VOD subset. No I/O and no partial publication.
use crate::{
    error::Error,
    session::{ByteRange, ManifestContext, Registration, ResourceKind},
};
use axum::http::StatusCode;
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

pub const INVALID: Error = Error(StatusCode::BAD_GATEWAY, "invalid_manifest");
pub const UNSUPPORTED: Error = Error(StatusCode::BAD_GATEWAY, "unsupported_manifest");
#[derive(Clone)]
struct Attribute {
    name: String,
    value: String,
    quoted: bool,
}
type Attributes = Vec<Attribute>;
fn get<'a>(a: &'a Attributes, name: &str) -> Option<&'a str> {
    a.iter().find(|a| a.name == name).map(|a| a.value.as_str())
}
fn required<'a>(a: &'a Attributes, name: &str) -> Result<&'a str, Error> {
    get(a, name).filter(|s| !s.is_empty()).ok_or(INVALID)
}
fn attributes(mut input: &str) -> Result<Attributes, Error> {
    let mut result = vec![];
    while !input.is_empty() {
        let (name, rest) = input.split_once('=').ok_or(INVALID)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
            || result.iter().any(|a: &Attribute| a.name == name)
        {
            return Err(INVALID);
        }
        let quoted = rest.starts_with('"');
        let (value, tail) = if quoted {
            let mut value = String::new();
            let mut escaped = false;
            let mut end = None;
            for (i, c) in rest[1..].char_indices() {
                if escaped {
                    if c != '"' && c != '\\' {
                        return Err(INVALID);
                    }
                    value.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    end = Some(i + 2);
                    break;
                } else {
                    value.push(c);
                }
            }
            let end = end.ok_or(INVALID)?;
            (value, &rest[end..])
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            (rest[..end].to_owned(), &rest[end..])
        };
        if value.is_empty()
            || value.chars().any(char::is_control)
            || (!quoted && value.contains(['"', '\\']))
        {
            return Err(INVALID);
        }
        result.push(Attribute {
            name: name.into(),
            value,
            quoted,
        });
        if result.len() > 32 {
            return Err(INVALID);
        }
        if tail.is_empty() {
            break;
        }
        input = tail
            .strip_prefix(',')
            .filter(|s| !s.is_empty())
            .ok_or(INVALID)?;
        if result.len() > 32 {
            return Err(INVALID);
        }
    }
    if result.is_empty() {
        return Err(INVALID);
    }
    Ok(result)
}
fn check_attrs(a: &Attributes, allowed: &[&str]) -> Result<(), Error> {
    for item in a {
        if !allowed.contains(&item.name.as_str()) {
            return Err(UNSUPPORTED);
        }
        let string = matches!(
            item.name.as_str(),
            "URI"
                | "GROUP-ID"
                | "NAME"
                | "LANGUAGE"
                | "ASSOC-LANGUAGE"
                | "INSTREAM-ID"
                | "CHARACTERISTICS"
                | "CHANNELS"
                | "CODECS"
                | "AUDIO"
                | "VIDEO"
                | "SUBTITLES"
                | "DATA-ID"
                | "VALUE"
                | "KEYFORMAT"
                | "KEYFORMATVERSIONS"
                | "BYTERANGE"
        );
        if (string && !item.quoted) || (!string && item.name != "CLOSED-CAPTIONS" && item.quoted) {
            return Err(INVALID);
        }
        if item.name == "CLOSED-CAPTIONS" && !item.quoted && item.value != "NONE" {
            return Err(INVALID);
        }
        if item.name == "PROGRAM-ID" {
            number(&item.value)?;
        }
        if item.name == "YT-EXT-ABSOLUTE-LOUDNESS" {
            // Ancillary loudness metadata is a finite number, never a fetch reference.
            decimal(&item.value)?;
        }
        if item.name == "VIDEO-RANGE" && !["SDR", "HLG", "PQ"].contains(&item.value.as_str()) {
            return Err(INVALID);
        }
        if item.name == "BYTERANGE" {
            if !item.value.bytes().all(|b| b.is_ascii_digit() || b == b'@') {
                return Err(INVALID);
            }
            continue;
        }
        if item.name == "CHANNELS" {
            let parts: Vec<_> = item.value.split('/').collect();
            if parts.len() > 2
                || number(parts[0])? == 0
                || parts.get(1).is_some_and(|s| {
                    !s.bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                })
            {
                return Err(INVALID);
            }
            continue;
        }
        if item.name != "URI" && !safe_text(&item.value) {
            return Err(INVALID);
        }
    }
    Ok(())
}
fn safe_text(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || " -_.,+\"\\".contains(c))
}
fn render(a: &[Attribute]) -> String {
    a.iter()
        .map(|a| {
            if a.quoted {
                format!(
                    "{}=\"{}\"",
                    a.name,
                    a.value.replace('\\', "\\\\").replace('"', "\\\"")
                )
            } else {
                format!("{}={}", a.name, a.value)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
fn number(s: &str) -> Result<u64, Error> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(INVALID);
    }
    s.parse().map_err(|_| INVALID)
}
fn decimal(s: &str) -> Result<f64, Error> {
    let n: f64 = s.parse().map_err(|_| INVALID)?;
    if !n.is_finite()
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err(INVALID);
    }
    Ok(n)
}
fn resolved(base: &Url, uri: &str) -> Result<String, Error> {
    if uri.contains('\\')
        || uri.is_empty()
        || uri.contains("{$")
        || uri.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(INVALID);
    }
    let url = base.join(uri).map_err(|_| INVALID)?;
    crate::network::destination::destination(url.as_str()).map_err(|_| INVALID)?;
    Ok(url.into())
}
fn extent(
    text: &str,
    uri: &str,
    previous: &Option<(String, ByteRange)>,
) -> Result<ByteRange, Error> {
    let (length, offset) = match text.split_once('@') {
        Some((n, o)) => (number(n)?, number(o)?),
        None => {
            let (old, r) = previous
                .as_ref()
                .filter(|(old, _)| old == uri)
                .ok_or(INVALID)?;
            let _ = old;
            (
                number(text)?,
                r.offset.checked_add(r.length).ok_or(INVALID)?,
            )
        }
    };
    if length == 0 || offset.checked_add(length).is_none() {
        return Err(INVALID);
    }
    Ok(ByteRange { offset, length })
}
#[derive(Clone)]
enum Line {
    Text(String),
    Variant(Attributes, String),
    Rendition(Attributes),
}
pub struct Plan {
    lines: Vec<String>,
    pub registrations: Vec<Registration>,
    replacements: Vec<(usize, std::ops::Range<usize>)>,
}
impl Plan {
    /// IDs must be the result of a successful durable batch commit.
    pub fn publish(
        self,
        ids: &[String],
        sid: &str,
        grant: &str,
        max: usize,
    ) -> Result<String, Error> {
        if ids.len() != self.registrations.len() {
            return Err(INVALID);
        }
        let mut lines = self.lines;
        let mut size = lines
            .iter()
            .try_fold(0usize, |n, l| n.checked_add(l.len() + 1))
            .ok_or(INVALID)?;
        if size > max {
            return Err(Error(StatusCode::BAD_GATEWAY, "manifest_limit"));
        }
        for ((line, span), rid) in self.replacements.into_iter().zip(ids) {
            let uri = format!("/stream/{sid}/r/{rid}?st={grant}");
            size = size
                .checked_sub(span.len())
                .and_then(|n| n.checked_add(uri.len()))
                .ok_or(INVALID)?;
            if size > max {
                return Err(Error(StatusCode::BAD_GATEWAY, "manifest_limit"));
            }
            lines[line].replace_range(span, &uri);
        }
        Ok(lines.join("\n") + "\n")
    }
}
struct Builder<'a> {
    plan: Plan,
    parent: &'a Registration,
    base: &'a Url,
    ancestry: Vec<String>,
    media_only_children: bool,
    max: usize,
    max_depth: usize,
    remaining_state_bytes: usize,
}
impl Builder<'_> {
    fn text(&mut self, value: String) {
        self.plan.lines.push(value);
    }
    #[allow(clippy::too_many_arguments)]
    fn reference(
        &mut self,
        mut a: Option<Attributes>,
        tag: &str,
        uri: &str,
        kind: ResourceKind,
        range: Option<ByteRange>,
        encrypted: bool,
        subtitles: bool,
    ) -> Result<(), Error> {
        if self.plan.registrations.len() >= self.max {
            return Err(Error(StatusCode::BAD_GATEWAY, "manifest_limit"));
        }
        let uri = resolved(self.base, uri)?;
        let manifest = if kind == ResourceKind::Manifest {
            if self.ancestry.contains(&uri) || self.ancestry.len() >= self.max_depth {
                return Err(INVALID);
            }
            Some(ManifestContext {
                ancestors: self.ancestry.clone(),
                selected_variant: None,
                adaptive_variants: vec![],
                media_only: self.media_only_children,
                subtitles,
            })
        } else {
            None
        };
        let (line, span) = if let Some(ref mut attrs) = a {
            let index = attrs.iter().position(|a| a.name == "URI").ok_or(INVALID)?;
            if !attrs[index].quoted {
                return Err(INVALID);
            }
            // Track the exact URI field structurally. No sentinel string can
            // collide with an adversarial NAME or other retained attribute.
            let before = render(&attrs[..index]);
            let start = tag.len() + 1 + before.len() + usize::from(index > 0) + "URI=\"".len();
            attrs[index].value.clear();
            (format!("{tag}:{}", render(attrs)), start..start)
        } else {
            (String::new(), 0..0)
        };
        self.plan.replacements.push((self.plan.lines.len(), span));
        self.text(line);
        let registration = Registration {
            uri,
            kind,
            context: self.parent.context.clone(),
            range,
            expires_at: self.parent.expires_at,
            manifest,
            encrypted,
        };
        let size = serde_json::to_vec(&registration)
            .map_err(|_| INVALID)?
            .len()
            .checked_add(1)
            .ok_or(INVALID)?;
        self.remaining_state_bytes = self
            .remaining_state_bytes
            .checked_sub(size)
            .ok_or(Error(StatusCode::BAD_GATEWAY, "manifest_limit"))?;
        self.plan.registrations.push(registration);
        Ok(())
    }
}
// The caller bounds input before parsing; counts also bound per-manifest allocation.
pub fn parse(
    bytes: &[u8],
    base: &Url,
    parent: &Registration,
    depth: usize,
    max_refs: usize,
    max_state_bytes: usize,
) -> Result<Plan, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| INVALID)?;
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r')
        || text.starts_with('\u{feff}')
    {
        return Err(INVALID);
    }
    let mut raw = text.lines();
    if raw.next() != Some("#EXTM3U") {
        return Err(INVALID);
    }
    let ctx = parent.manifest.clone().unwrap_or(ManifestContext {
        ancestors: vec![],
        selected_variant: None,
        adaptive_variants: vec![],
        media_only: false,
        subtitles: false,
    });
    let mut ancestry = ctx.ancestors;
    if ancestry.len() >= depth
        || ancestry
            .iter()
            .any(|u| u == base.as_str() || u == &parent.uri)
    {
        return Err(INVALID);
    }
    ancestry.push(parent.uri.clone());
    if base.as_str() != parent.uri {
        ancestry.push(base.to_string());
    }
    if ancestry.len() > depth {
        return Err(INVALID);
    }
    let mut lines = vec![];
    let mut pending = None;
    let mut version = 1;
    let mut seen_version = false;
    let mut legacy_program_id = false;
    let mut master = false;
    let mut media = false;
    for (index, raw) in raw.enumerate() {
        if index >= max_refs.saturating_mul(16).saturating_add(63) {
            return Err(INVALID);
        }
        if raw.is_empty() {
            continue;
        }
        if raw.trim() != raw {
            return Err(INVALID);
        }
        if !raw.starts_with('#') {
            if let Some(attrs) = pending.take() {
                lines.push(Line::Variant(attrs, resolved(base, raw)?));
            } else {
                media = true;
                lines.push(Line::Text(raw.into()));
            }
            continue;
        }
        let (tag, value) = raw.split_once(':').unwrap_or((raw, ""));
        if pending.is_some() && tag.starts_with("#EXT") {
            return Err(INVALID);
        }
        match tag {
            "#EXT-X-VERSION" => {
                if seen_version {
                    return Err(INVALID);
                }
                version = number(value)?;
                seen_version = true;
                if !(1..=7).contains(&version) {
                    return Err(UNSUPPORTED);
                }
                lines.push(Line::Text(raw.into()));
            }
            "#EXT-X-STREAM-INF" => {
                master = true;
                let a = attributes(value)?;
                check_attrs(
                    &a,
                    &[
                        "BANDWIDTH",
                        "AVERAGE-BANDWIDTH",
                        "CODECS",
                        "RESOLUTION",
                        "FRAME-RATE",
                        "HDCP-LEVEL",
                        "PROGRAM-ID",
                        "VIDEO-RANGE",
                        "YT-EXT-ABSOLUTE-LOUDNESS",
                        "AUDIO",
                        "VIDEO",
                        "SUBTITLES",
                        "CLOSED-CAPTIONS",
                    ],
                )?;
                if number(required(&a, "BANDWIDTH")?)? == 0 {
                    return Err(INVALID);
                }
                legacy_program_id |= get(&a, "PROGRAM-ID").is_some();
                pending = Some(a);
            }
            "#EXT-X-MEDIA" => {
                master = true;
                let a = attributes(value)?;
                check_attrs(
                    &a,
                    &[
                        "TYPE",
                        "URI",
                        "GROUP-ID",
                        "LANGUAGE",
                        "ASSOC-LANGUAGE",
                        "NAME",
                        "DEFAULT",
                        "AUTOSELECT",
                        "FORCED",
                        "INSTREAM-ID",
                        "CHARACTERISTICS",
                        "CHANNELS",
                    ],
                )?;
                lines.push(Line::Rendition(a));
            }
            "#EXT-X-I-FRAME-STREAM-INF" => {
                master = true;
                let a = attributes(value)?;
                check_attrs(
                    &a,
                    &[
                        "BANDWIDTH",
                        "AVERAGE-BANDWIDTH",
                        "CODECS",
                        "RESOLUTION",
                        "VIDEO",
                        "URI",
                        "HDCP-LEVEL",
                        "PROGRAM-ID",
                        "VIDEO-RANGE",
                    ],
                )?;
                legacy_program_id |= get(&a, "PROGRAM-ID").is_some();
                resolved(base, required(&a, "URI")?)?; /* explicitly excluded under all filter modes */
            }
            "#EXTINF"
            | "#EXT-X-TARGETDURATION"
            | "#EXT-X-MEDIA-SEQUENCE"
            | "#EXT-X-DISCONTINUITY-SEQUENCE"
            | "#EXT-X-ENDLIST"
            | "#EXT-X-PLAYLIST-TYPE"
            | "#EXT-X-ALLOW-CACHE"
            | "#EXT-X-BYTERANGE"
            | "#EXT-X-MAP"
            | "#EXT-X-KEY"
            | "#EXT-X-PROGRAM-DATE-TIME"
            | "#EXT-X-DISCONTINUITY" => {
                media = true;
                lines.push(Line::Text(raw.into()));
            }
            "#EXT-X-SESSION-KEY" | "#EXT-X-SESSION-DATA" => {
                master = true;
                lines.push(Line::Text(raw.into()));
            }
            "#EXT-X-INDEPENDENT-SEGMENTS" | "#EXT-X-START" => lines.push(Line::Text(raw.into())),
            _ if tag.starts_with("#EXT") => return Err(UNSUPPORTED),
            _ => {} // comments are not a metadata exfiltration path
        }
    }
    if legacy_program_id && version >= 6 {
        return Err(UNSUPPORTED);
    }
    if pending.is_some() || (master && media) || (!master && !media) {
        return Err(INVALID);
    }
    for line in &lines {
        match line {
            Line::Text(s) if s.starts_with("#EXT-X-SESSION-KEY:") && version < 7 => {
                return Err(INVALID);
            }
            Line::Text(s) if s.starts_with("#EXT-X-KEY:") => {
                let a = attributes(s.strip_prefix("#EXT-X-KEY:").ok_or(INVALID)?)?;
                if (get(&a, "IV").is_some() && version < 2)
                    || ((get(&a, "KEYFORMAT").is_some() || get(&a, "KEYFORMATVERSIONS").is_some())
                        && version < 5)
                {
                    return Err(INVALID);
                }
            }
            Line::Rendition(a) if get(a, "TYPE") == Some("SUBTITLES") && version < 4 => {
                return Err(INVALID);
            }
            _ => {}
        }
    }
    let mut b = Builder {
        plan: Plan {
            lines: vec!["#EXTM3U".into()],
            registrations: vec![],
            replacements: vec![],
        },
        parent,
        base,
        ancestry,
        media_only_children: ctx.selected_variant.is_some()
            || !ctx.adaptive_variants.is_empty()
            || ctx.media_only,
        max: max_refs,
        max_depth: depth,
        remaining_state_bytes: max_state_bytes.saturating_sub(2),
    };
    if master {
        if ctx.media_only {
            return Err(UNSUPPORTED);
        }
        master_plan(
            lines,
            ctx.selected_variant.as_deref(),
            &ctx.adaptive_variants,
            &mut b,
        )?;
    } else {
        if ctx.selected_variant.is_some() {
            return Err(INVALID);
        }
        media_plan(lines, version, ctx.subtitles, &mut b)?;
    }
    Ok(b.plan)
}
fn master_plan(
    lines: Vec<Line>,
    selected: Option<&str>,
    adaptive: &[crate::contracts::providers::Variant],
    b: &mut Builder<'_>,
) -> Result<(), Error> {
    let retained = |attributes: &Attributes, uri: &str| -> bool {
        if adaptive.is_empty() {
            return selected.is_none_or(|selected| selected == uri);
        }
        adaptive.iter().any(|variant| {
            variant.url == uri
                && get(attributes, "RESOLUTION")
                    == variant
                        .width
                        .zip(variant.height)
                        .map(|(width, height)| format!("{width}x{height}"))
                        .as_deref()
                && get(attributes, "BANDWIDTH").and_then(|value| value.parse::<i64>().ok())
                    == variant.bitrate
                && get(attributes, "AUDIO") == variant.audio_group.as_deref()
                && get(attributes, "SUBTITLES") == variant.subtitle_group.as_deref()
                && get(attributes, "CODECS")
                    .map(|value| value.split(',').map(str::trim).collect::<Vec<_>>())
                    == variant
                        .codecs
                        .as_ref()
                        .map(|codecs| codecs.iter().map(String::as_str).collect::<Vec<_>>())
        })
    };
    let mut groups: BTreeMap<(String, String), Vec<&Attributes>> = BTreeMap::new();
    let mut variants = vec![];
    let mut matched_uris = BTreeSet::new();
    let mut selected_groups = None;
    for line in &lines {
        match line {
            Line::Variant(a, u) => {
                if retained(a, u) {
                    if !matched_uris.insert(u.as_str()) {
                        return Err(INVALID);
                    }
                    if selected == Some(u.as_str()) {
                        selected_groups = Some((get(a, "VIDEO"), get(a, "CLOSED-CAPTIONS")));
                    }
                    variants.push(a);
                }
            }
            Line::Rendition(a) => {
                let t = required(a, "TYPE")?;
                if !["AUDIO", "VIDEO", "SUBTITLES", "CLOSED-CAPTIONS"].contains(&t) {
                    return Err(UNSUPPORTED);
                }
                let id = required(a, "GROUP-ID")?;
                required(a, "NAME")?;
                if t == "CLOSED-CAPTIONS" {
                    if get(a, "URI").is_some() || !valid_instream(required(a, "INSTREAM-ID")?) {
                        return Err(INVALID);
                    }
                } else if t == "SUBTITLES" {
                    required(a, "URI")?;
                }
                for name in ["DEFAULT", "AUTOSELECT", "FORCED"] {
                    if get(a, name).is_some_and(|v| v != "YES" && v != "NO") {
                        return Err(INVALID);
                    }
                }
                if get(a, "DEFAULT") == Some("YES")
                    && get(a, "AUTOSELECT").is_some_and(|value| value != "YES")
                {
                    return Err(INVALID);
                }
                let group = groups.entry((t.into(), id.into())).or_default();
                if group.iter().any(|old| {
                    get(old, "NAME") == get(a, "NAME")
                        || (get(old, "DEFAULT") == Some("YES") && get(a, "DEFAULT") == Some("YES"))
                }) {
                    return Err(INVALID);
                }
                group.push(a);
            }
            _ => {}
        }
    }
    if variants.is_empty()
        || (adaptive.is_empty() && selected.is_some() && variants.len() != 1)
        || (!adaptive.is_empty()
            && (adaptive.len() > 32
                || variants.len() != adaptive.len()
                || selected.is_none_or(|selected| {
                    !adaptive.iter().any(|variant| variant.url == selected)
                })))
    {
        return Err(INVALID);
    }
    if !adaptive.is_empty()
        && variants.iter().any(|attributes| {
            Some((get(attributes, "VIDEO"), get(attributes, "CLOSED-CAPTIONS"))) != selected_groups
        })
    {
        return Err(INVALID);
    }
    let mut retain = BTreeSet::new();
    for a in variants {
        for t in ["AUDIO", "VIDEO", "SUBTITLES", "CLOSED-CAPTIONS"] {
            if let Some(id) = get(a, t) {
                if t == "CLOSED-CAPTIONS"
                    && id == "NONE"
                    && a.iter().any(|attr| attr.name == t && !attr.quoted)
                {
                    continue;
                }
                let key = (t.into(), id.into());
                if !groups.contains_key(&key) {
                    return Err(INVALID);
                }
                retain.insert(key);
            }
        }
    }
    for line in lines {
        match line {
            Line::Variant(a, u) if retained(&a, &u) => {
                b.text(format!("#EXT-X-STREAM-INF:{}", render(&a)));
                b.reference(None, "", &u, ResourceKind::Manifest, None, false, false)?;
            }
            Line::Rendition(a)
                if retain.contains(&(
                    required(&a, "TYPE")?.into(),
                    required(&a, "GROUP-ID")?.into(),
                )) =>
            {
                if let Some(uri) = get(&a, "URI") {
                    let uri = uri.to_owned();
                    let subtitles = get(&a, "TYPE") == Some("SUBTITLES");
                    b.reference(
                        Some(a),
                        "#EXT-X-MEDIA",
                        &uri,
                        ResourceKind::Manifest,
                        None,
                        false,
                        subtitles,
                    )?;
                } else {
                    b.text(format!("#EXT-X-MEDIA:{}", render(&a)));
                }
            }
            Line::Text(s) => {
                if let Some(value) = s.strip_prefix("#EXT-X-SESSION-KEY:") {
                    key(value, true, b)?;
                } else if let Some(value) = s.strip_prefix("#EXT-X-SESSION-DATA:") {
                    let a = attributes(value)?;
                    check_attrs(&a, &["DATA-ID", "VALUE", "LANGUAGE"])?;
                    required(&a, "DATA-ID")?;
                    required(&a, "VALUE")?;
                    b.text(format!("#EXT-X-SESSION-DATA:{}", render(&a)));
                } else {
                    global(&s, b)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn global(s: &str, b: &mut Builder<'_>) -> Result<(), Error> {
    if let Some(value) = s.strip_prefix("#EXT-X-START:") {
        let a = attributes(value)?;
        check_attrs(&a, &["TIME-OFFSET", "PRECISE"])?;
        decimal(required(&a, "TIME-OFFSET")?)?;
        if get(&a, "PRECISE").is_some_and(|v| v != "YES" && v != "NO") {
            return Err(INVALID);
        }
    } else if s != "#EXT-X-INDEPENDENT-SEGMENTS" && !s.starts_with("#EXT-X-VERSION:") {
        return Err(INVALID);
    }
    b.text(s.into());
    Ok(())
}
fn key(value: &str, session: bool, b: &mut Builder<'_>) -> Result<(bool, bool), Error> {
    let a = attributes(value)?;
    check_attrs(
        &a,
        &["METHOD", "URI", "IV", "KEYFORMAT", "KEYFORMATVERSIONS"],
    )?;
    match required(&a, "METHOD")? {
        "NONE" if !session => {
            if a.len() != 1 {
                return Err(INVALID);
            }
            b.text("#EXT-X-KEY:METHOD=NONE".into());
            Ok((false, false))
        }
        "AES-128" => {
            if get(&a, "KEYFORMAT").is_some_and(|v| v != "identity")
                || get(&a, "KEYFORMATVERSIONS").is_some_and(|v| v != "1")
            {
                return Err(UNSUPPORTED);
            }
            let iv = get(&a, "IV");
            if iv.is_some_and(|s| {
                s.strip_prefix("0x")
                    .or_else(|| s.strip_prefix("0X"))
                    .is_none_or(|v| {
                        v.is_empty() || v.len() > 32 || !v.bytes().all(|b| b.is_ascii_hexdigit())
                    })
            }) {
                return Err(INVALID);
            }
            let uri = required(&a, "URI")?.to_owned();
            let has_iv = iv.is_some();
            b.reference(
                Some(a),
                if session {
                    "#EXT-X-SESSION-KEY"
                } else {
                    "#EXT-X-KEY"
                },
                &uri,
                ResourceKind::Key,
                None,
                false,
                false,
            )?;
            Ok((true, has_iv))
        }
        _ => Err(UNSUPPORTED),
    }
}
fn media_plan(
    lines: Vec<Line>,
    version: u64,
    subtitles: bool,
    b: &mut Builder<'_>,
) -> Result<(), Error> {
    let mut seen = BTreeSet::new();
    let mut duration: Option<f64> = None;
    let mut target = None;
    let mut segments = 0u64;
    let mut sequence = 0u64;
    let mut discontinuities = 0u64;
    let mut pending_range: Option<(String, usize)> = None;
    let mut previous = None;
    let mut previous_map = None;
    let mut mapped = false;
    let mut encrypted = false;
    let mut explicit_iv = false;
    let mut ended = false;
    for line in lines {
        let Line::Text(s) = line else {
            return Err(INVALID);
        };
        if !s.starts_with('#') {
            if ended {
                return Err(INVALID);
            }
            let d = duration.take().ok_or(INVALID)?;
            if target.is_none_or(|t: f64| d.round() > t) {
                return Err(INVALID);
            }
            let uri = resolved(b.base, &s)?;
            let range = if let Some((text, index)) = pending_range.take() {
                let r = extent(&text, &uri, &previous)?;
                b.plan.lines[index] = format!("#EXT-X-BYTERANGE:{}@{}", r.length, r.offset);
                Some(r)
            } else {
                None
            };
            previous = range.clone().map(|r| (uri.clone(), r));
            let kind = if subtitles {
                if encrypted || mapped {
                    return Err(UNSUPPORTED);
                }
                ResourceKind::Subtitle
            } else if mapped {
                ResourceKind::Fragment
            } else {
                ResourceKind::Segment
            };
            b.reference(None, "", &uri, kind, range, encrypted, false)?;
            segments = segments.checked_add(1).ok_or(INVALID)?;
            continue;
        }
        let (tag, value) = s.split_once(':').unwrap_or((&s, ""));
        match tag {
            "#EXTINF" => {
                if duration.is_some() || ended {
                    return Err(INVALID);
                }
                let (d, _) = value.split_once(',').ok_or(INVALID)?;
                let d = decimal(d)?;
                if d <= 0.0 || (version < 3 && d.fract() != 0.0) {
                    return Err(INVALID);
                }
                duration = Some(d);
                b.text(format!(
                    "#EXTINF:{},",
                    value.split_once(',').ok_or(INVALID)?.0
                ));
            }
            "#EXT-X-TARGETDURATION"
            | "#EXT-X-MEDIA-SEQUENCE"
            | "#EXT-X-DISCONTINUITY-SEQUENCE"
            | "#EXT-X-PLAYLIST-TYPE"
            | "#EXT-X-ALLOW-CACHE"
            | "#EXT-X-ENDLIST" => {
                if !seen.insert(tag.to_owned()) {
                    return Err(INVALID);
                }
                match tag {
                    "#EXT-X-TARGETDURATION" => {
                        let n = number(value)?;
                        if n == 0 {
                            return Err(INVALID);
                        }
                        target = Some(n as f64);
                    }
                    "#EXT-X-MEDIA-SEQUENCE" => {
                        if segments > 0 {
                            return Err(INVALID);
                        }
                        sequence = number(value)?;
                    }
                    "#EXT-X-DISCONTINUITY-SEQUENCE" => {
                        if segments > 0 || discontinuities > 0 {
                            return Err(INVALID);
                        }
                        discontinuities = number(value)?;
                    }
                    "#EXT-X-ALLOW-CACHE" => {
                        if version >= 7 {
                            return Err(UNSUPPORTED);
                        }
                        if !["YES", "NO"].contains(&value) {
                            return Err(INVALID);
                        }
                    }
                    "#EXT-X-PLAYLIST-TYPE" => {
                        if value != "VOD" {
                            return Err(UNSUPPORTED);
                        }
                    }
                    _ => {
                        if !value.is_empty() || duration.is_some() || pending_range.is_some() {
                            return Err(INVALID);
                        }
                        ended = true;
                    }
                }
                b.text(s);
            }
            "#EXT-X-BYTERANGE" => {
                if version < 4 || pending_range.is_some() {
                    return Err(INVALID);
                }
                pending_range = Some((value.into(), b.plan.lines.len()));
                b.text(s);
            }
            "#EXT-X-MAP" => {
                if version < 6 || subtitles || (encrypted && !explicit_iv) {
                    return Err(UNSUPPORTED);
                }
                let mut a = attributes(value)?;
                check_attrs(&a, &["URI", "BYTERANGE"])?;
                let uri = resolved(b.base, required(&a, "URI")?)?;
                let range = get(&a, "BYTERANGE")
                    .map(|s| extent(s, &uri, &previous_map))
                    .transpose()?;
                if let Some(r) = &range {
                    let attr = a
                        .iter_mut()
                        .find(|a| a.name == "BYTERANGE")
                        .ok_or(INVALID)?;
                    attr.value = format!("{}@{}", r.length, r.offset);
                }
                previous_map = range.clone().map(|r| (uri.clone(), r));
                b.reference(
                    Some(a),
                    tag,
                    &uri,
                    ResourceKind::Init,
                    range,
                    encrypted,
                    false,
                )?;
                mapped = true;
            }
            "#EXT-X-KEY" => {
                (encrypted, explicit_iv) = key(value, false, b)?;
            }
            "#EXT-X-DISCONTINUITY" => {
                if !value.is_empty() {
                    return Err(INVALID);
                }
                discontinuities = discontinuities.checked_add(1).ok_or(INVALID)?;
                b.text(s);
            }
            "#EXT-X-PROGRAM-DATE-TIME" => {
                chrono::DateTime::parse_from_rfc3339(value).map_err(|_| INVALID)?;
                b.text(s);
            }
            _ => global(&s, b)?,
        }
    }
    sequence.checked_add(segments).ok_or(INVALID)?;
    if segments == 0 || !ended || duration.is_some() || pending_range.is_some() || target.is_none()
    {
        return Err(INVALID);
    }
    Ok(())
}

fn valid_instream(s: &str) -> bool {
    s.strip_prefix("CC")
        .is_some_and(|s| matches!(s, "1" | "2" | "3" | "4"))
        || s.strip_prefix("SERVICE")
            .and_then(|s| number(s).ok())
            .is_some_and(|n| (1..=63).contains(&n))
}

#[cfg(test)]
mod tests;

/// Retained descriptive attributes only. Protocol numbers/flags and authorized
/// URI fields cannot reflect credentials and must not cause incidental matches.
pub(crate) fn metadata_values(output: &str) -> Result<Vec<String>, Error> {
    let mut values = vec![];
    for line in output.lines() {
        let Some((tag, text)) = line.split_once(':') else {
            continue;
        };
        if !matches!(
            tag,
            "#EXT-X-MEDIA" | "#EXT-X-STREAM-INF" | "#EXT-X-SESSION-DATA"
        ) {
            continue;
        }
        for attr in attributes(text)? {
            if matches!(
                attr.name.as_str(),
                "NAME"
                    | "LANGUAGE"
                    | "ASSOC-LANGUAGE"
                    | "GROUP-ID"
                    | "AUDIO"
                    | "VIDEO"
                    | "SUBTITLES"
                    | "CLOSED-CAPTIONS"
                    | "CODECS"
                    | "CHARACTERISTICS"
                    | "DATA-ID"
                    | "VALUE"
            ) {
                values.push(attr.value);
            }
        }
    }
    Ok(values)
}
