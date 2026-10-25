//! Offline Go fixture vectors and bounded HLS edge cases. No DNS or HTTP calls.

use stream_media::hls::{
    ByteRange, HlsError, MAX_LINE_BYTES, MAX_PLAYLIST_BYTES, MAX_RESOLVED_URI_BYTES, MAX_SEGMENTS,
    MAX_TRACKS, MAX_VARIANTS, PlaylistKind, parse_playlist,
};
use url::Url;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn base() -> Result<Url, url::ParseError> {
    Url::parse("https://example.com/media.m3u8")
}

const GO_VARIANTS_AND_TRACKS: &str = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English, Stereo\",LANGUAGE=\"en\",URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720,CODECS=\"avc1.4d401f,mp4a.40.2\",AUDIO=\"a\"\n720.m3u8\n";

const GO_BYTE_RANGE: &str = "#EXTM3U\n#EXT-X-BYTERANGE:1000@500\n#EXTINF:4,\nmedia.ts\n";

const GO_IMPLICIT_RANGES: &str = "#EXTM3U\n#EXT-X-BYTERANGE:1000@500\n#EXTINF:4,\nmedia.ts\n#EXT-X-BYTERANGE:2000\n#EXTINF:4,\nmedia.ts\n#EXT-X-BYTERANGE:3000\n#EXTINF:4,\nmedia.ts\n";

#[test]
fn go_variant_and_quoted_track_vectors_keep_all_context() -> TestResult {
    let playlist = parse_playlist(GO_VARIANTS_AND_TRACKS.as_bytes(), &base()?)?;
    assert_eq!(playlist.kind, PlaylistKind::Master);
    assert_eq!(playlist.metadata.variants.len(), 1);
    assert_eq!(playlist.metadata.tracks.len(), 1);
    let variant = playlist
        .metadata
        .variants
        .first()
        .ok_or("missing variant")?;
    assert_eq!(variant.height, Some(720));
    assert_eq!(variant.width, Some(1280));
    assert_eq!(variant.bitrate, Some(3_000_000));
    assert_eq!(variant.codecs, ["avc1.4d401f", "mp4a.40.2"]);
    assert_eq!(variant.url, "https://example.com/720.m3u8");
    assert_eq!(variant.audio_group, "a");
    let track = playlist.metadata.tracks.first().ok_or("missing audio")?;
    assert_eq!(track.name, "English, Stereo");
    assert_eq!(track.kind, "audio");
    assert_eq!(track.group, "a");
    assert_eq!(track.url, "https://example.com/audio.m3u8");
    assert!(playlist.metadata.tracks_known);
    assert!(playlist.samples.is_empty());
    Ok(())
}

#[test]
fn go_explicit_and_implicit_byte_range_vectors_match_exactly() -> TestResult {
    let playlist = parse_playlist(GO_BYTE_RANGE.as_bytes(), &base()?)?;
    let first = playlist.samples.first().ok_or("missing sample")?;
    assert_eq!(first.byte_range, Some(ByteRange::new(500, 1000)?));
    assert_eq!(first.url, "https://example.com/media.ts");
    let playlist = parse_playlist(GO_IMPLICIT_RANGES.as_bytes(), &base()?)?;
    let offsets: Vec<_> = playlist
        .samples
        .iter()
        .map(|sample| sample.byte_range.map(|range| range.start))
        .collect();
    assert_eq!(offsets, [Some(500), Some(1500), Some(3500)]);
    let lengths: Vec<_> = playlist
        .samples
        .iter()
        .map(|sample| sample.byte_range.map(|range| range.length))
        .collect();
    assert_eq!(lengths, [Some(1000), Some(2000), Some(3000)]);
    Ok(())
}

#[test]
fn go_invalid_range_and_private_resource_vectors_are_rejected() -> TestResult {
    for manifest in [
        "#EXTM3U\n#EXTINF:4,\nhttp://127.0.0.1/media\n",
        "#EXTM3U\n#EXT-X-BYTERANGE:1000\n#EXTINF:4,\nmedia.ts\n",
        "not a playlist",
        "#EXTM3U\n#EXT-X-BYTERANGE:1000@500\n#EXTINF:4,\nfirst.ts\n#EXT-X-BYTERANGE:1000\n#EXTINF:4,\nother.ts\n",
        "#EXTM3U\n#EXT-X-BYTERANGE:1000@9223372036854775800\n#EXTINF:4,\nmedia.ts\n",
    ] {
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    Ok(())
}

#[test]
fn implicit_ranges_require_the_immediately_preceding_ranged_resource() -> TestResult {
    let manifest = "#EXTM3U\n#EXTINF:4,\nmedia.ts\n#EXT-X-BYTERANGE:1000\n#EXTINF:4,\nmedia.ts\n";
    assert!(matches!(
        parse_playlist(manifest.as_bytes(), &base()?),
        Err(HlsError::MissingRangePredecessor)
    ));
    let manifest = "#EXTM3U\n#EXT-X-BYTERANGE:1000@500\n#EXTINF:4,\nmedia.ts\n#EXTINF:4,\nmedia.ts\n#EXT-X-BYTERANGE:1000\n#EXTINF:4,\nmedia.ts\n";
    assert!(matches!(
        parse_playlist(manifest.as_bytes(), &base()?),
        Err(HlsError::MissingRangePredecessor)
    ));
    Ok(())
}

#[test]
fn range_arithmetic_and_http_headers_are_checked_at_the_signed_boundary() -> TestResult {
    assert_eq!(ByteRange::new(500, 1000)?.header_value()?, "bytes=500-1499");
    assert!(ByteRange::new(-1, 1).is_err());
    assert!(ByteRange::new(0, 0).is_err());
    assert!(matches!(
        ByteRange::new(i64::MAX, 1),
        Err(HlsError::RangeOverflow)
    ));
    assert_eq!(ByteRange::new(i64::MAX - 1, 1)?.end_exclusive()?, i64::MAX);
    for range in [
        "0@0",
        "-1@0",
        "1@-1",
        "1@2@3",
        "@0",
        "1@",
        "+1@0",
        "1 @0",
        "9223372036854775808@0",
    ] {
        let manifest = format!("#EXTM3U\n#EXT-X-BYTERANGE:{range}\n#EXTINF:4,\nmedia.ts\n");
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    Ok(())
}

#[test]
fn probe_sample_positions_are_first_floor_midpoint_last_without_duplicates() -> TestResult {
    for (count, expected) in [
        (1, vec![0]),
        (2, vec![0, 1]),
        (3, vec![0, 1, 2]),
        (5, vec![0, 2, 4]),
        (6, vec![0, 2, 5]),
    ] {
        let mut manifest = "#EXTM3U\n".to_owned();
        for index in 0..count {
            manifest.push_str(&format!("#EXTINF:4,\n{index}.ts\n"));
        }
        let playlist = parse_playlist(manifest.as_bytes(), &base()?)?;
        let actual: Vec<_> = playlist
            .probe_samples()
            .map(|sample| sample.url.clone())
            .collect();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|index| format!("https://example.com/{index}.ts"))
            .collect();
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[test]
fn relative_uris_follow_the_actual_manifest_directory_without_parent_query_leakage() -> TestResult {
    let base = Url::parse("https://example.com/show/season/media.m3u8?parent=secret")?;
    let playlist = parse_playlist(b"#EXTM3U\r\n#EXTINF:4,\r\n../segment.ts?token=child\r\n#EXTINF:4,\r\n//cdn.example.com/segment.ts\r\n", &base)?;
    assert_eq!(
        playlist.samples.first().ok_or("missing first sample")?.url,
        "https://example.com/show/segment.ts?token=child"
    );
    assert_eq!(
        playlist.samples.get(1).ok_or("missing second sample")?.url,
        "https://cdn.example.com/segment.ts"
    );
    Ok(())
}

#[test]
fn malformed_or_incomplete_attributes_never_hide_duplicate_or_quoted_values() -> TestResult {
    for attributes in [
        "BANDWIDTH=1000,BANDWIDTH=2000",
        "BANDWIDTH=1000,",
        "BANDWIDTH=1000,CODECS=\"avc1,mp4a",
        "BANDWIDTH=1000,CODECS=\"avc1\"junk",
        "BANDWIDTH=1000,CODECS=avc\"1\"",
        "BANDWIDTH=1000,=empty",
        "bandwidth=1000",
        "BANDWIDTH=1000,VALUE=",
        "BANDWIDTH=1000,VALUE=\"\"\"\"",
    ] {
        let manifest = format!("#EXTM3U\n#EXT-X-STREAM-INF:{attributes}\nvariant.m3u8\n");
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    Ok(())
}

#[test]
fn average_bandwidth_overrides_peak_while_dimensions_and_codecs_remain_optional() -> TestResult {
    let playlist = parse_playlist(
        b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=6000000,AVERAGE-BANDWIDTH=3000000\nvariant.m3u8\n",
        &base()?,
    )?;
    let variant = playlist
        .metadata
        .variants
        .first()
        .ok_or("missing variant")?;
    assert_eq!(variant.bitrate, Some(3_000_000));
    assert!(variant.width.is_none());
    assert!(variant.height.is_none());
    assert!(variant.codecs.is_empty());
    for attributes in [
        "BANDWIDTH=0",
        "BANDWIDTH=-1",
        "BANDWIDTH=9223372036854775808",
        "BANDWIDTH=1,RESOLUTION=1920x0",
        "BANDWIDTH=1,RESOLUTION=1920X1080",
        "BANDWIDTH=1,RESOLUTION=1x2x3",
        "BANDWIDTH=1,CODECS=\"avc1,,mp4a\"",
    ] {
        let manifest = format!("#EXTM3U\n#EXT-X-STREAM-INF:{attributes}\nvariant.m3u8\n");
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    Ok(())
}

#[test]
fn audio_and_subtitle_groups_and_defaults_remain_distinct() -> TestResult {
    let playlist = parse_playlist(concat!("#EXTM3U\n",
        "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio.m3u8\"\n",
        "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=NO,URI=\"subs.m3u8\"\n",
        "#EXT-X-STREAM-INF:BANDWIDTH=1000000,AUDIO=\"audio\",SUBTITLES=\"subs\"\nvideo.m3u8\n").as_bytes(), &base()?)?;
    let audio = playlist.metadata.tracks.first().ok_or("missing audio")?;
    let subtitle = playlist.metadata.tracks.get(1).ok_or("missing subtitle")?;
    assert!(audio.default);
    assert!(!subtitle.default);
    assert_eq!(subtitle.kind, "subtitles");
    assert_eq!(subtitle.group, "subs");
    let variant = playlist
        .metadata
        .variants
        .first()
        .ok_or("missing variant")?;
    assert_eq!(variant.audio_group, "audio");
    assert_eq!(variant.subtitle_group, "subs");
    Ok(())
}

#[test]
fn encryption_and_drm_flags_accumulate_even_if_a_later_segment_is_clear() -> TestResult {
    let supported = parse_playlist(b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4,\nfirst.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:4,\nsecond.ts\n", &base()?)?;
    assert!(supported.encrypted);
    assert!(!supported.unsupported_protection);
    let unsupported = parse_playlist(b"#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://opaque-key\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:4,\nfirst.ts\n", &base()?)?;
    assert!(unsupported.encrypted);
    assert!(unsupported.unsupported_protection);
    let unsupported_format = parse_playlist(b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"opaque\",KEYFORMAT=\"other-format\"\n#EXTINF:4,\nfirst.ts\n", &base()?)?;
    assert!(unsupported_format.unsupported_protection);
    let session_key = parse_playlist(b"#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nvariant.m3u8\n", &base()?)?;
    assert!(session_key.encrypted);
    assert_eq!(session_key.kind, PlaylistKind::Master);
    Ok(())
}

#[test]
fn malformed_keys_and_private_identity_keys_are_rejected() -> TestResult {
    for key in [
        "URI=\"key.bin\"",
        "METHOD=AES-128",
        "METHOD=NONE,URI=\"key.bin\"",
        "METHOD=AES-128,URI=\"key.bin\",IV=0x",
        "METHOD=AES-128,URI=\"key.bin\",IV=0xxyz",
        "METHOD=AES-128,URI=\"http://127.0.0.1/key\"",
    ] {
        let manifest = format!("#EXTM3U\n#EXT-X-KEY:{key}\n#EXTINF:4,\nsegment.ts\n");
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    let invalid_session =
        b"#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=NONE\n#EXT-X-STREAM-INF:BANDWIDTH=1\nvariant.m3u8\n";
    assert!(parse_playlist(invalid_session, &base()?).is_err());
    Ok(())
}

#[test]
fn single_initialization_map_preserves_its_range_and_rejects_later_changes() -> TestResult {
    let playlist = parse_playlist(b"#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"400@0\"\n#EXTINF:4,\nfirst.m4s\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"400@0\"\n#EXTINF:4,\nlast.m4s\n", &base()?)?;
    let initialization = playlist.initialization.ok_or("missing initialization")?;
    assert_eq!(initialization.url, "https://example.com/init.mp4");
    assert_eq!(initialization.byte_range, Some(ByteRange::new(0, 400)?));
    for manifest in [
        "#EXTM3U\n#EXT-X-MAP:URI=\"first-init.mp4\"\n#EXTINF:4,\nfirst.m4s\n#EXT-X-MAP:URI=\"other-init.mp4\"\n#EXTINF:4,\nlast.m4s\n",
        "#EXTM3U\n#EXTINF:4,\nfirst.ts\n#EXT-X-MAP:URI=\"new-init.mp4\"\n#EXTINF:4,\nlast.m4s\n",
    ] {
        assert!(matches!(
            parse_playlist(manifest.as_bytes(), &base()?),
            Err(HlsError::ChangingInitialization)
        ));
    }
    assert!(
        parse_playlist(
            b"#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"100\"\n#EXTINF:4,\nfirst.m4s\n",
            &base()?
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn encrypted_initialization_requires_an_explicit_iv() -> TestResult {
    let no_iv = b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\nfirst.m4s\n";
    assert!(parse_playlist(no_iv, &base()?).is_err());
    let with_iv = b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x1\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\nfirst.m4s\n";
    assert!(parse_playlist(with_iv, &base()?)?.encrypted);
    Ok(())
}

#[test]
fn master_and_media_tags_cannot_be_mixed() -> TestResult {
    for manifest in [
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nvariant.m3u8\n#EXTINF:4,\nsegment.ts\n",
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nvariant.m3u8\n",
        "#EXTM3U\n#EXTINF:4,\nsegment.ts\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\"\n",
    ] {
        assert!(matches!(
            parse_playlist(manifest.as_bytes(), &base()?),
            Err(HlsError::MixedPlaylist)
        ));
    }
    Ok(())
}

#[test]
fn pending_tags_and_unassociated_uris_are_not_silently_discarded() -> TestResult {
    for manifest in [
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\n",
        "#EXTM3U\n#EXTINF:4,\n",
        "#EXTM3U\n#EXT-X-BYTERANGE:1000@0\n",
        "#EXTM3U\n#EXTINF:4,\n#EXTINF:4,\nsegment.ts\n",
        "#EXTM3U\n#EXT-X-BYTERANGE:1@0\n#EXT-X-BYTERANGE:2@0\n#EXTINF:4,\nsegment.ts\n",
        "#EXTM3U\nsegment.ts\n",
        "#EXTM3U\n#EXT-X-STREAM-INF\nvariant.m3u8\n",
        "#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:4,\nsegment.ts\n",
    ] {
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    Ok(())
}

#[test]
fn duplicate_singleton_tags_and_invalid_segment_durations_are_rejected() -> TestResult {
    for tag in [
        "#EXT-X-VERSION:3",
        "#EXT-X-TARGETDURATION:4",
        "#EXT-X-MEDIA-SEQUENCE:0",
        "#EXT-X-ENDLIST",
    ] {
        let manifest = format!("#EXTM3U\n{tag}\n{tag}\n#EXTINF:4,\nsegment.ts\n");
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    for duration in [
        "NaN,", "inf,", "1e3,", "-4,", "0,", "1.2.3,", "4", ",", "+4,",
    ] {
        let manifest = format!("#EXTM3U\n#EXTINF:{duration}\nsegment.ts\n");
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    let complete_vod =
        b"#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:0.004,\nsegment.ts\n#EXT-X-ENDLIST\n";
    assert!(parse_playlist(complete_vod, &base()?)?.end_list);
    Ok(())
}

#[test]
fn input_body_physical_lines_and_utf8_are_bounded() -> TestResult {
    assert!(matches!(
        parse_playlist(&vec![b'a'; MAX_PLAYLIST_BYTES + 1], &base()?),
        Err(HlsError::LimitExceeded("body"))
    ));
    let long_comment = format!(
        "#EXTM3U\n#{}\n#EXTINF:4,\nsegment.ts\n",
        "a".repeat(MAX_LINE_BYTES)
    );
    assert!(matches!(
        parse_playlist(long_comment.as_bytes(), &base()?),
        Err(HlsError::LimitExceeded("line"))
    ));
    let long_prefix = format!(
        "{}#EXTM3U\n#EXTINF:4,\nsegment.ts\n",
        " ".repeat(MAX_LINE_BYTES)
    );
    assert!(matches!(
        parse_playlist(long_prefix.as_bytes(), &base()?),
        Err(HlsError::LimitExceeded("line"))
    ));
    assert!(matches!(
        parse_playlist(b"#EXTM3U\n#EXTINF:4,\ninvalid\xff.ts\n", &base()?),
        Err(HlsError::InvalidUtf8)
    ));
    assert!(parse_playlist(b"\xef\xbb\xbf#EXTM3U\n#EXTINF:4,\nsegment.ts\n", &base()?).is_err());
    for header in [
        "#EXTM3U_BAD\n#EXTINF:4,\nsegment.ts\n",
        "<html>Not a playlist</html>",
        "#EXTM3U\n",
        "#EXTM3U\n#EXTM3U\n#EXTINF:4,\nsegment.ts\n",
    ] {
        assert!(parse_playlist(header.as_bytes(), &base()?).is_err());
    }
    Ok(())
}

#[test]
fn metadata_and_segment_count_limits_are_enforced_before_growing_unbounded() -> TestResult {
    let mut variants = "#EXTM3U\n".to_owned();
    for index in 0..=MAX_VARIANTS {
        variants.push_str(&format!("#EXT-X-STREAM-INF:BANDWIDTH=1000\n{index}.m3u8\n"));
    }
    assert!(matches!(
        parse_playlist(variants.as_bytes(), &base()?),
        Err(HlsError::LimitExceeded("variants"))
    ));
    let mut tracks = "#EXTM3U\n".to_owned();
    for index in 0..=MAX_TRACKS {
        tracks.push_str(&format!(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"Track {index}\"\n"
        ));
    }
    tracks.push_str("#EXT-X-STREAM-INF:BANDWIDTH=1000\nvariant.m3u8\n");
    assert!(matches!(
        parse_playlist(tracks.as_bytes(), &base()?),
        Err(HlsError::LimitExceeded("tracks"))
    ));
    let segments = format!(
        "#EXTM3U\n{}",
        "#EXTINF:4,\nsegment.ts\n".repeat(MAX_SEGMENTS + 1)
    );
    assert!(matches!(
        parse_playlist(segments.as_bytes(), &base()?),
        Err(HlsError::LimitExceeded("segments"))
    ));
    Ok(())
}

#[test]
fn long_but_bounded_movie_playlist_keeps_all_1800_go_fixture_segments() -> TestResult {
    let manifest = format!(
        "#EXTM3U\n{}",
        format!("#EXTINF:4,\n{}.ts\n", "a".repeat(300)).repeat(1800)
    );
    let playlist = parse_playlist(manifest.as_bytes(), &base()?)?;
    assert_eq!(playlist.samples.len(), 1800);
    assert_eq!(playlist.probe_samples().count(), 3);
    Ok(())
}

#[test]
fn repeated_long_base_urls_have_an_aggregate_resolved_uri_budget() -> TestResult {
    let base = Url::parse(&format!(
        "https://example.com/{}/media.m3u8",
        "a".repeat(7000)
    ))?;
    let segment_count = MAX_RESOLVED_URI_BYTES / 7000 + 2;
    let manifest = format!(
        "#EXTM3U\n{}",
        "#EXTINF:4,\nsegment.ts\n".repeat(segment_count)
    );
    assert!(matches!(
        parse_playlist(manifest.as_bytes(), &base),
        Err(HlsError::LimitExceeded("resolved URI bytes"))
    ));
    Ok(())
}

#[test]
fn resource_validation_rejects_local_credentials_schemes_fragments_and_ambiguous_authorities()
-> TestResult {
    for resource in [
        "http://localhost/a.ts",
        "http://private.local/a.ts",
        "http://[::1]/a.ts",
        "http://127.0.0.1/a.ts",
        "http://192.168.1.1/a.ts",
        "http://2130706433/a.ts",
        "http://0177.0.0.1/a.ts",
        "http://0x7f000001/a.ts",
        "https://user:secret@example.com/a.ts",
        "https://example.com:8080/a.ts",
        "file:///etc/passwd",
        "https://example.com/a.ts#token",
        "https://example.com\\a.ts",
        "{$variable}/a.ts",
    ] {
        let manifest = format!("#EXTM3U\n#EXTINF:4,\n{resource}\n");
        assert!(parse_playlist(manifest.as_bytes(), &base()?).is_err());
    }
    Ok(())
}

#[test]
fn errors_and_sample_debug_do_not_reveal_signed_urls() -> TestResult {
    let playlist = parse_playlist(
        b"#EXTM3U\n#EXTINF:4,\nsegment.ts?token=do-not-log\n",
        &base()?,
    )?;
    let diagnostic = format!("{:?}", playlist.samples.first().ok_or("missing sample")?);
    assert!(!diagnostic.contains("do-not-log"));
    assert!(!diagnostic.contains("example.com"));
    let error = parse_playlist(
        b"#EXTM3U\n#EXTINF:4,\nhttps://secret:do-not-log@example.com/a.ts\n",
        &base()?,
    )
    .err()
    .ok_or("expected invalid resource")?;
    assert!(!format!("{error:?} {error}").contains("do-not-log"));
    Ok(())
}

#[test]
fn hls_parser_edge_cases_empty_and_unclosed_quotes() -> TestResult {
    // Empty body
    assert!(matches!(parse_playlist(b"", &base()?), Err(HlsError::InvalidHeader)));
    // Header only (no media or master variants)
    assert!(matches!(parse_playlist(b"#EXTM3U\n", &base()?), Err(HlsError::Empty)));
    // Target duration but 0 segments
    assert!(matches!(
        parse_playlist(b"#EXTM3U\n#EXT-X-TARGETDURATION:10\n", &base()?),
        Err(HlsError::Empty)
    ));
    // Unclosed quote in attribute
    assert!(matches!(
        parse_playlist(
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000,CODECS=\"avc1.4d401f\nvariant.m3u8\n",
            &base()?
        ),
        Err(HlsError::Malformed("attribute quote"))
    ));
    // Empty URI in attribute
    assert!(matches!(
        parse_playlist(
            b"#EXTM3U\n#EXT-X-MAP:URI=\"\"\n#EXTINF:4,\nseg.ts\n",
            &base()?
        ),
        Err(HlsError::Malformed("required attribute"))
    ));
    // Trailing comma in attributes
    assert!(matches!(
        parse_playlist(
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000,\nvariant.m3u8\n",
            &base()?
        ),
        Err(HlsError::Malformed("attribute pair"))
    ));
    Ok(())
}
