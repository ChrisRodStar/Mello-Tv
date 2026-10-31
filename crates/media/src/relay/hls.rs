use thiserror::Error;
use url::Url;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HlsRewriteError {
    #[error("manifest is not valid UTF-8")]
    InvalidUtf8,
    #[error("manifest is missing #EXTM3U header")]
    MissingHeader,
    #[error("manifest contains invalid URI reference")]
    InvalidUri,
}

pub struct HlsManifestRewriter;

impl HlsManifestRewriter {
    pub fn rewrite<F>(
        manifest_bytes: &[u8],
        base_url: &Url,
        session_id: &str,
        token: &str,
        register_uri: F,
    ) -> Result<String, HlsRewriteError>
    where
        F: FnMut(String) -> String,
    {
        Self::rewrite_with_public_url(
            manifest_bytes,
            base_url,
            session_id,
            token,
            None,
            register_uri,
        )
    }

    /// An explicit public base is used by remote library consumers whose own
    /// HTTP proxy cannot resolve Mello's root-relative resource URLs.
    pub fn rewrite_with_public_url<F>(
        manifest_bytes: &[u8],
        base_url: &Url,
        session_id: &str,
        token: &str,
        public_url: Option<&str>,
        mut register_uri: F,
    ) -> Result<String, HlsRewriteError>
    where
        F: FnMut(String) -> String,
    {
        let public_url = public_url.unwrap_or("").trim_end_matches('/');
        let raw_text =
            std::str::from_utf8(manifest_bytes).map_err(|_| HlsRewriteError::InvalidUtf8)?;
        let text = raw_text.strip_prefix('\u{feff}').unwrap_or(raw_text).trim();

        let mut lines = text.lines();
        let Some(first) = lines.next().map(str::trim) else {
            return Err(HlsRewriteError::MissingHeader);
        };
        if !first.starts_with("#EXTM3U") {
            return Err(HlsRewriteError::MissingHeader);
        }

        let mut out = String::with_capacity(text.len() + 1024);
        out.push_str("#EXTM3U\n");

        struct VariantEntry {
            header: String,
            uri_line: String,
            priority: (u64, u64),
            index: usize,
        }
        let mut variants = Vec::new();

        while let Some(raw_line) = lines.next() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with("#EXT-X-STREAM-INF:") {
                let priority = variant_priority(line);
                let header = line.to_string();
                let mut uri_line = None;
                while let Some(next_raw) = lines.next() {
                    let next_line = next_raw.trim();
                    if next_line.is_empty() {
                        continue;
                    }
                    let resolved = resolve_uri(base_url, next_line)?;
                    let rid = register_uri(resolved);
                    let mut rewritten_uri = String::new();
                    rewritten_uri.push_str(public_url);
                    rewritten_uri.push_str("/stream/");
                    rewritten_uri.push_str(session_id);
                    rewritten_uri.push_str("/r/");
                    rewritten_uri.push_str(&rid);
                    rewritten_uri.push_str("?st=");
                    rewritten_uri.push_str(token);
                    uri_line = Some(rewritten_uri);
                    break;
                }
                let Some(uri_line) = uri_line else {
                    return Err(HlsRewriteError::InvalidUri);
                };
                let index = variants.len();
                variants.push(VariantEntry {
                    header,
                    uri_line,
                    priority,
                    index,
                });
            } else if !line.starts_with('#') {
                let resolved = resolve_uri(base_url, line)?;
                let rid = register_uri(resolved);
                out.push_str(public_url);
                out.push_str("/stream/");
                out.push_str(session_id);
                out.push_str("/r/");
                out.push_str(&rid);
                out.push_str("?st=");
                out.push_str(token);
                out.push('\n');
            } else if line.contains("URI=\"") {
                let rewritten = rewrite_tag_uri(
                    line,
                    base_url,
                    session_id,
                    token,
                    public_url,
                    &mut register_uri,
                )?;
                out.push_str(&rewritten);
                out.push('\n');
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }

        if !variants.is_empty() {
            variants.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then_with(|| a.index.cmp(&b.index))
            });
            for v in variants {
                out.push_str(&v.header);
                out.push('\n');
                out.push_str(&v.uri_line);
                out.push('\n');
            }
        }

        Ok(out)
    }
}

fn variant_priority(tag: &str) -> (u64, u64) {
    let Some(attrs_str) = tag.strip_prefix("#EXT-X-STREAM-INF:") else {
        return (0, 0);
    };
    let Ok(attrs) = crate::hls::attributes::parse(attrs_str) else {
        return (0, 0);
    };
    let pixels = crate::hls::attributes::get(&attrs, "RESOLUTION")
        .and_then(|res| res.split_once(['x', 'X']))
        .and_then(|(w, h)| {
            let w = w.trim().parse::<u64>().ok()?;
            let h = h.trim().parse::<u64>().ok()?;
            Some(w.saturating_mul(h))
        })
        .unwrap_or(0);
    let bandwidth = crate::hls::attributes::get(&attrs, "BANDWIDTH")
        .and_then(|b| b.trim().parse::<u64>().ok())
        .unwrap_or(0);
    (pixels, bandwidth)
}

fn resolve_uri(base_url: &Url, raw: &str) -> Result<String, HlsRewriteError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(HlsRewriteError::InvalidUri);
    }
    let resolved = if trimmed.starts_with("//") {
        Url::parse(&format!("{}:{trimmed}", base_url.scheme()))
            .map_err(|_| HlsRewriteError::InvalidUri)?
    } else if let Ok(abs) = Url::parse(trimmed) {
        abs
    } else {
        base_url
            .join(trimmed)
            .map_err(|_| HlsRewriteError::InvalidUri)?
    };
    if !matches!(resolved.scheme(), "http" | "https") {
        return Err(HlsRewriteError::InvalidUri);
    }
    Ok(resolved.into())
}

fn rewrite_tag_uri<F>(
    line: &str,
    base_url: &Url,
    session_id: &str,
    token: &str,
    public_url: &str,
    register_uri: &mut F,
) -> Result<String, HlsRewriteError>
where
    F: FnMut(String) -> String,
{
    let mut result = String::with_capacity(line.len() + 64);
    let mut rest = line;

    while let Some(pos) = rest.find("URI=\"") {
        let valid_boundary = pos == 0 || matches!(rest.as_bytes()[pos - 1], b':' | b',');
        let prefix_end = pos + "URI=\"".len();
        result.push_str(&rest[..prefix_end]);
        rest = &rest[prefix_end..];

        if !valid_boundary {
            continue;
        }

        let Some(end_quote) = rest.find('"') else {
            return Err(HlsRewriteError::InvalidUri);
        };
        let raw_uri = &rest[..end_quote];
        if !raw_uri.is_empty() {
            let resolved = resolve_uri(base_url, raw_uri)?;
            let rid = register_uri(resolved);
            result.push_str(public_url);
            result.push_str("/stream/");
            result.push_str(session_id);
            result.push_str("/r/");
            result.push_str(&rid);
            result.push_str("?st=");
            result.push_str(token);
        }
        result.push('"');
        rest = &rest[end_quote + 1..];
    }
    result.push_str(rest);
    Ok(result)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_master_and_media_playlists() {
        let base = Url::parse("https://cdn.example.com/hls/master.m3u8").unwrap();
        let master = b"#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"Eng\",URI=\"audio/en.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1920x1080\n1080p/index.m3u8\n";
        let mut registered = Vec::new();
        let out = HlsManifestRewriter::rewrite(master, &base, "sess123", "tok456", |u| {
            registered.push(u);
            format!("r{}", registered.len())
        })
        .unwrap();

        assert_eq!(
            registered,
            vec![
                "https://cdn.example.com/hls/audio/en.m3u8",
                "https://cdn.example.com/hls/1080p/index.m3u8"
            ]
        );
        assert!(out.contains("URI=\"/stream/sess123/r/r1?st=tok456\""));
        assert!(out.contains("\n/stream/sess123/r/r2?st=tok456\n"));
    }

    #[test]
    fn sorts_variants_highest_quality_first() {
        let base = Url::parse("https://cdn.example.com/hls/master.m3u8").unwrap();
        let master = b"#EXTM3U\n\
            #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n\
            360p.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n\
            720p.m3u8\n";
        let out = HlsManifestRewriter::rewrite(master, &base, "sess", "tok", |u| u).unwrap();
        let pos_720 = out.find("RESOLUTION=1280x720").unwrap();
        let pos_360 = out.find("RESOLUTION=640x360").unwrap();
        assert!(pos_720 < pos_360);
    }
}
