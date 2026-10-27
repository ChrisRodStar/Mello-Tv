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
        mut register_uri: F,
    ) -> Result<String, HlsRewriteError>
    where
        F: FnMut(String) -> String,
    {
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

        for raw_line in lines {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if !line.starts_with('#') {
                let resolved = resolve_uri(base_url, line)?;
                let rid = register_uri(resolved);
                out.push_str("/stream/");
                out.push_str(session_id);
                out.push_str("/r/");
                out.push_str(&rid);
                out.push_str("?st=");
                out.push_str(token);
                out.push('\n');
            } else if line.contains("URI=\"") {
                let rewritten =
                    rewrite_tag_uri(line, base_url, session_id, token, &mut register_uri)?;
                out.push_str(&rewritten);
                out.push('\n');
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }

        Ok(out)
    }
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
}
