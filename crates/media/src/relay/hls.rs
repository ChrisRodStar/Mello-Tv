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

/// What a rewritten URI points at. The relay uses this to give every proxied
/// URL a conventional file extension: ffmpeg (and therefore Jellyfin) checks
/// HLS segment extensions, and many CDNs disguise segments as images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Playlist,
    SubtitlePlaylist,
    Segment,
    SubtitleSegment,
    InitSegment,
    Key,
}

impl ResourceKind {
    pub fn is_playlist(self) -> bool {
        matches!(self, Self::Playlist | Self::SubtitlePlaylist)
    }

    fn extension(self, upstream: &Url, fragmented: bool) -> &'static str {
        match self {
            Self::Playlist | Self::SubtitlePlaylist => "m3u8",
            Self::SubtitleSegment => "vtt",
            Self::InitSegment => "mp4",
            Self::Key => "key",
            Self::Segment if fragmented => "m4s",
            Self::Segment => {
                let path = upstream.path().to_ascii_lowercase();
                let original = path.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
                match original {
                    "ts" => "ts",
                    "aac" => "aac",
                    "mp3" => "mp3",
                    "ac3" => "ac3",
                    "eac3" => "eac3",
                    "m4s" => "m4s",
                    "mp4" => "mp4",
                    "m4a" => "m4a",
                    "m4v" => "m4v",
                    "vtt" | "webvtt" => "vtt",
                    _ => "ts",
                }
            }
        }
    }
}

pub struct HlsManifestRewriter;

impl HlsManifestRewriter {
    /// Rewrites every URI in a playlist to a relay URL. `url_base` (e.g.
    /// `http://192.168.1.50:8080`, or empty for root-relative paths) makes the
    /// URLs absolute so the playlist still works when another server, such as
    /// Jellyfin, proxies it. `playlist_kind` says whether this playlist was
    /// reached as a subtitle rendition, so its segments are labelled WebVTT.
    #[allow(clippy::too_many_arguments)]
    pub fn rewrite<F>(
        manifest_bytes: &[u8],
        base_url: &Url,
        url_base: &str,
        session_id: &str,
        token: &str,
        playlist_kind: ResourceKind,
        mut register_uri: F,
    ) -> Result<String, HlsRewriteError>
    where
        F: FnMut(String, ResourceKind) -> String,
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

        let segment_kind = if playlist_kind == ResourceKind::SubtitlePlaylist {
            ResourceKind::SubtitleSegment
        } else {
            ResourceKind::Segment
        };
        let fragmented = text.contains("#EXT-X-MAP");
        let mut writer = Writer {
            url_base,
            session_id,
            token,
            fragmented,
        };

        let mut out = String::with_capacity(text.len() + 1024);
        out.push_str("#EXTM3U\n");
        let mut next_is_variant = false;

        for raw_line in lines {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if !line.starts_with('#') {
                let kind = if next_is_variant {
                    ResourceKind::Playlist
                } else {
                    segment_kind
                };
                next_is_variant = false;
                let resolved = resolve_uri(base_url, line)?;
                let rid = register_uri(resolved.to_string(), kind);
                writer.path(&mut out, &rid, kind, &resolved);
                out.push('\n');
                continue;
            }
            if line.starts_with("#EXT-X-STREAM-INF") {
                next_is_variant = true;
            }
            if line.contains("URI=\"") {
                let kind = tag_kind(line, segment_kind);
                let rewritten =
                    rewrite_tag_uri(line, base_url, kind, &mut writer, &mut register_uri)?;
                out.push_str(&rewritten);
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }

        Ok(out)
    }
}

struct Writer<'a> {
    url_base: &'a str,
    session_id: &'a str,
    token: &'a str,
    fragmented: bool,
}

impl Writer<'_> {
    fn path(&mut self, out: &mut String, rid: &str, kind: ResourceKind, upstream: &Url) {
        out.push_str(self.url_base);
        out.push_str("/stream/");
        out.push_str(self.session_id);
        out.push_str("/r/");
        out.push_str(rid);
        out.push('.');
        out.push_str(kind.extension(upstream, self.fragmented));
        out.push_str("?st=");
        out.push_str(self.token);
    }
}

fn tag_kind(line: &str, segment_kind: ResourceKind) -> ResourceKind {
    if line.starts_with("#EXT-X-MEDIA:") {
        if line.contains("TYPE=SUBTITLES") {
            ResourceKind::SubtitlePlaylist
        } else {
            ResourceKind::Playlist
        }
    } else if line.starts_with("#EXT-X-I-FRAME-STREAM-INF")
        || line.starts_with("#EXT-X-RENDITION-REPORT")
    {
        ResourceKind::Playlist
    } else if line.starts_with("#EXT-X-KEY") || line.starts_with("#EXT-X-SESSION-KEY") {
        ResourceKind::Key
    } else if line.starts_with("#EXT-X-MAP") {
        ResourceKind::InitSegment
    } else {
        segment_kind
    }
}

fn resolve_uri(base_url: &Url, raw: &str) -> Result<Url, HlsRewriteError> {
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
    Ok(resolved)
}

fn rewrite_tag_uri<F>(
    line: &str,
    base_url: &Url,
    kind: ResourceKind,
    writer: &mut Writer<'_>,
    register_uri: &mut F,
) -> Result<String, HlsRewriteError>
where
    F: FnMut(String, ResourceKind) -> String,
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
            let rid = register_uri(resolved.to_string(), kind);
            writer.path(&mut result, &rid, kind, &resolved);
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

    fn rewrite(manifest: &str, kind: ResourceKind) -> (String, Vec<(String, ResourceKind)>) {
        let base = Url::parse("https://cdn.example.com/a/master.m3u8").unwrap();
        let mut seen = Vec::new();
        let out = HlsManifestRewriter::rewrite(
            manifest.as_bytes(),
            &base,
            "",
            "sid",
            "tok",
            kind,
            |url, kind| {
                seen.push((url, kind));
                format!("r{}", seen.len())
            },
        )
        .unwrap();
        (out, seen)
    }

    #[test]
    fn master_playlist_variants_and_renditions_get_m3u8() {
        let (out, seen) = rewrite(
            "#EXTM3U\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",URI=\"subs/en.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhigh/index.txt\n",
            ResourceKind::Playlist,
        );
        assert!(out.contains("URI=\"/stream/sid/r/r1.m3u8?st=tok\""));
        assert!(out.contains("\n/stream/sid/r/r2.m3u8?st=tok\n"));
        assert_eq!(seen[0].1, ResourceKind::SubtitlePlaylist);
        assert_eq!(seen[1].0, "https://cdn.example.com/a/high/index.txt");
        assert_eq!(seen[1].1, ResourceKind::Playlist);
    }

    #[test]
    fn disguised_segments_become_ts_and_keys_keep_their_kind() {
        let (out, seen) = rewrite(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://k.example.com/key\"\n#EXTINF:4,\nseg-1.png\n#EXTINF:4,\nseg-2.aac\n",
            ResourceKind::Playlist,
        );
        assert!(out.contains("URI=\"/stream/sid/r/r1.key?st=tok\""));
        assert!(out.contains("/stream/sid/r/r2.ts?st=tok"));
        assert!(out.contains("/stream/sid/r/r3.aac?st=tok"));
        assert_eq!(seen[0].1, ResourceKind::Key);
    }

    #[test]
    fn fragmented_and_subtitle_playlists() {
        let (out, _) = rewrite(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.jpg\"\n#EXTINF:4,\nchunk.jpg\n",
            ResourceKind::Playlist,
        );
        assert!(out.contains("URI=\"/stream/sid/r/r1.mp4?st=tok\""));
        assert!(out.contains("/stream/sid/r/r2.m4s?st=tok"));
        let (out, _) = rewrite(
            "#EXTM3U\n#EXTINF:60,\nsub0.txt\n",
            ResourceKind::SubtitlePlaylist,
        );
        assert!(out.contains("/stream/sid/r/r1.vtt?st=tok"));
    }

    #[test]
    fn absolute_base_is_prefixed() {
        let base = Url::parse("https://cdn.example.com/").unwrap();
        let out = HlsManifestRewriter::rewrite(
            b"#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:4,\na.ts\n",
            &base,
            "http://10.0.0.5:8080",
            "s",
            "t",
            ResourceKind::Playlist,
            |_, _| "r".into(),
        )
        .unwrap();
        assert!(out.contains("URI=\"http://10.0.0.5:8080/stream/s/r/r.mp4?st=t\""));
        assert!(out.contains("\nhttp://10.0.0.5:8080/stream/s/r/r.m4s?st=t\n"));
    }

    #[test]
    fn rejects_non_http_uris() {
        let base = Url::parse("https://cdn.example.com/").unwrap();
        let result = HlsManifestRewriter::rewrite(
            b"#EXTM3U\nfile:///etc/passwd\n",
            &base,
            "",
            "s",
            "t",
            ResourceKind::Playlist,
            |_, _| String::new(),
        );
        assert_eq!(result, Err(HlsRewriteError::InvalidUri));
    }
}
