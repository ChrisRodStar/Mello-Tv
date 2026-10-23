#![allow(clippy::unwrap_used)]
use super::*;
fn parent() -> Registration {
    Registration {
        uri: "https://media.example.com/path/master.m3u8".into(),
        kind: ResourceKind::Manifest,
        context: serde_json::from_str("{}").unwrap(),
        range: None,
        expires_at: None,
        encrypted: false,
        manifest: None,
    }
}
fn parse_text(text: &str) -> Result<Plan, Error> {
    let r = parent();
    parse(
        text.as_bytes(),
        &Url::parse(&r.uri).unwrap(),
        &r,
        6,
        128,
        2_097_152,
    )
}
fn media(tags: &str) -> String {
    format!("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n{tags}\n#EXT-X-ENDLIST\n")
}
const MASTER: &str = concat!(
    "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n",
    "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English, Stereo\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"2/JOC\",URI=\"audio/en.m3u8\"\n",
    "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"French\",LANGUAGE=\"fr\",DEFAULT=NO,AUTOSELECT=YES,URI=\"audio/fr.m3u8\"\n",
    "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=NO,AUTOSELECT=YES,FORCED=YES,CHARACTERISTICS=\"public.accessibility.transcribes-spoken-dialog\",URI=\"sub/en.m3u8\"\n",
    "#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID=\"v\",NAME=\"Main\",URI=\"video/alt.m3u8\"\n",
    "#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID=\"cc\",NAME=\"CC\",INSTREAM-ID=\"CC1\"\n",
    "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"other\",NAME=\"Other\",URI=\"unused.m3u8\"\n",
    "#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=100,URI=\"iframe.m3u8\"\n",
    "#EXT-X-STREAM-INF:BANDWIDTH=1000000,AUDIO=\"other\"\n720.m3u8\n",
    "#EXT-X-STREAM-INF:BANDWIDTH=3000000,CODECS=\"avc1.640028,mp4a.40.2\",AUDIO=\"a\",SUBTITLES=\"s\",VIDEO=\"v\",CLOSED-CAPTIONS=\"cc\"\n1080.m3u8\n"
);
#[test]
fn selected_child_keeps_entire_groups_attributes_and_master() {
    let mut r = parent();
    r.manifest = Some(ManifestContext {
        ancestors: vec![],
        selected_variant: Some("https://media.example.com/path/1080.m3u8".into()),
        adaptive_variants: vec![],
        media_only: false,
        subtitles: false,
    });
    let p = parse(
        MASTER.as_bytes(),
        &Url::parse(&r.uri).unwrap(),
        &r,
        6,
        128,
        2_097_152,
    )
    .unwrap();
    assert_eq!(p.registrations.len(), 5);
    assert_eq!(
        p.registrations
            .iter()
            .filter(|r| r.manifest.as_ref().unwrap().subtitles)
            .count(),
        1
    );
    let ids = vec!["opaque-resource-001".into(); 5];
    let out = p
        .publish(&ids, "opaque-session-001", "signed-grant", 65536)
        .unwrap();
    for needle in [
        "French",
        "English, Stereo",
        "CHANNELS=\"2/JOC\"",
        "FORCED=YES",
        "INSTREAM-ID=\"CC1\"",
        "#EXT-X-INDEPENDENT-SEGMENTS",
        "CODECS=\"avc1.640028,mp4a.40.2\"",
    ] {
        assert!(out.contains(needle), "{needle}");
    }
    assert_eq!(out.matches("#EXT-X-STREAM-INF:").count(), 1);
    assert_eq!(out.matches("?st=signed-grant").count(), 5);
    for forbidden in [
        "https:",
        "1080.m3u8",
        "720.m3u8",
        "unused",
        "I-FRAME-STREAM-INF",
    ] {
        assert!(!out.contains(forbidden), "{forbidden}");
    }
}
#[test]
fn missing_ambiguous_and_changed_go_handoff_cases_fail() {
    for text in [
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nother.m3u8\n",
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n1080.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2\n1080.m3u8\n",
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"missing\"\n1080.m3u8\n",
        &media("#EXTINF:6,\na.ts"),
    ] {
        let mut r = parent();
        r.manifest = Some(ManifestContext {
            ancestors: vec![],
            selected_variant: Some("https://media.example.com/path/1080.m3u8".into()),
            adaptive_variants: vec![],
            media_only: false,
            subtitles: false,
        });
        assert!(
            parse(
                text.as_bytes(),
                &Url::parse(&r.uri).unwrap(),
                &r,
                6,
                128,
                2_097_152
            )
            .is_err()
        );
    }
}
#[test]
fn unfiltered_variants_keep_order_and_cc_none_is_valid() {
    let p=parse_text("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,CLOSED-CAPTIONS=NONE\none.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2\ntwo.m3u8\n").unwrap();
    assert!(p.registrations[0].uri.ends_with("one.m3u8"));
    assert!(p.registrations[1].uri.ends_with("two.m3u8"));
}
#[test]
fn ranges_keys_maps_timeline_and_query_values_are_preserved() {
    let text = media(
        "#EXT-X-MEDIA-SEQUENCE:42\n#EXT-X-DISCONTINUITY-SEQUENCE:3\n#EXT-X-PROGRAM-DATE-TIME:2026-01-01T00:00:00Z\n#EXT-X-KEY:METHOD=AES-128,URI=\"//key.example.com/k?signature=a%2Fb\",IV=0x0000000000000000000000000000002a\n#EXT-X-MAP:URI=\"movie.mp4\",BYTERANGE=\"100@500\"\n#EXTINF:6,remove https://private.example.com\n#EXT-X-BYTERANGE:1000@600\nmovie.mp4\n#EXT-X-DISCONTINUITY\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:6,\n#EXT-X-BYTERANGE:2000\nmovie.mp4",
    );
    let p = parse_text(&text).unwrap();
    assert_eq!(p.registrations.len(), 4);
    assert!(p.registrations[0].uri.ends_with("signature=a%2Fb"));
    assert_eq!(p.registrations[1].kind, ResourceKind::Init);
    assert_eq!(p.registrations[2].kind, ResourceKind::Fragment);
    assert!(
        p.registrations[1].encrypted
            && p.registrations[2].encrypted
            && !p.registrations[3].encrypted
    );
    assert_eq!(p.registrations[3].range.as_ref().unwrap().offset, 1600);
    let out = p
        .publish(
            &vec!["opaque-resource-001".into(); 4],
            "opaque-session-001",
            "token",
            65536,
        )
        .unwrap();
    for needle in [
        "#EXT-X-MEDIA-SEQUENCE:42",
        "#EXT-X-DISCONTINUITY-SEQUENCE:3",
        "#EXT-X-PROGRAM-DATE-TIME:",
        "#EXT-X-DISCONTINUITY\n",
        "#EXT-X-BYTERANGE:2000@1600",
        "BYTERANGE=\"100@500\"",
        "IV=0x0000000000000000000000000000002a",
        "#EXT-X-KEY:METHOD=NONE",
    ] {
        assert!(out.contains(needle));
    }
    assert!(!out.contains("private.example"));
}
#[test]
fn malformed_ranges_never_infer_from_different_or_unranged_resources() {
    for tags in [
        "#EXTINF:6,\n#EXT-X-BYTERANGE:10\na.ts",
        "#EXTINF:6,\n#EXT-X-BYTERANGE:10@0\na.ts\n#EXTINF:6,\n#EXT-X-BYTERANGE:10\nb.ts",
        "#EXTINF:6,\n#EXT-X-BYTERANGE:10@0\na.ts\n#EXTINF:6,\na.ts\n#EXTINF:6,\n#EXT-X-BYTERANGE:10\na.ts",
        "#EXTINF:6,\n#EXT-X-BYTERANGE:2@18446744073709551615\na.ts",
        "#EXTINF:6,\n#EXT-X-BYTERANGE:0@0\na.ts",
    ] {
        assert!(parse_text(&media(tags)).is_err(), "{tags}");
    }
}
#[test]
fn unsupported_fetch_extensions_fail_closed() {
    for tag in [
        "#EXT-X-PART:DURATION=1,URI=\"part\"",
        "#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"next\"",
        "#EXT-X-RENDITION-REPORT:URI=\"other\"",
        "#EXT-X-SKIP:SKIPPED-SEGMENTS=1",
        "#EXT-X-DEFINE:NAME=\"x\",VALUE=\"s\"",
        "#EXT-X-CONTENT-STEERING:SERVER-URI=\"steer\"",
        "#EXT-X-NEW:URL=\"https://secret.example.com\"",
        "#EXT-X-DATERANGE:ID=\"ad\",X-URI=\"https://ad.example.com\"",
    ] {
        assert!(
            parse_text(&media(&format!("{tag}\n#EXTINF:6,\na.ts"))).is_err(),
            "{tag}"
        );
    }
}
#[test]
fn malformed_attributes_versions_types_and_linkage_fail() {
    for text in [
        MASTER.replace("NAME=\"French\"", "NAME=\"English, Stereo\""),
        MASTER.replace(
            "DEFAULT=NO,AUTOSELECT=YES,URI=\"audio/fr",
            "DEFAULT=YES,AUTOSELECT=YES,URI=\"audio/fr",
        ),
        MASTER.replace("INSTREAM-ID=\"CC1\"", "INSTREAM-ID=\"CC99\""),
        MASTER.replace("VERSION:7", "VERSION:8"),
        MASTER.replace("BANDWIDTH=1000000", "BANDWIDTH=1,BANDWIDTH=2"),
        MASTER.replace("NAME=\"Main\"", "NAME=\"https://private.example.com\""),
        MASTER.replace("URI=\"video/alt.m3u8\"", "URI=video/alt.m3u8"),
        MASTER.replace("NAME=\"Main\"", "NAME=\"Main"),
    ] {
        assert!(parse_text(&text).is_err());
    }
}
#[test]
fn unsupported_encryption_and_unsafe_uris_fail() {
    for tag in [
        "METHOD=SAMPLE-AES,URI=\"key\"",
        "METHOD=AES-128,URI=\"key\",KEYFORMAT=\"com.apple.streamingkeydelivery\"",
        "METHOD=NONE,URI=\"key\"",
        "METHOD=AES-128,URI=\"key\",IV=0xGG",
        "METHOD=AES-128,URI=\"http://127.0.0.1/key\"",
        "METHOD=AES-128,URI=\"data:abcd\"",
    ] {
        assert!(parse_text(&media(&format!("#EXT-X-KEY:{tag}\n#EXTINF:6,\na.ts"))).is_err());
    }
    for uri in [
        "http://127.0.0.1/a",
        "file:///a",
        "https://user:pass@media.example.com/a",
        "a#fragment",
        "{$x}.ts",
    ] {
        assert!(parse_text(&media(&format!("#EXTINF:6,\n{uri}"))).is_err());
    }
}
#[test]
fn version_minima_and_vod_structure_enforced() {
    for text in [
        media("#EXTINF:6,\na.ts").replace("#EXT-X-ENDLIST\n", ""),
        media("#EXTINF:6,\na.ts").replace("#EXT-X-TARGETDURATION:6\n", ""),
        media("#EXTINF:7,\na.ts"),
        media("#EXT-X-MEDIA-SEQUENCE:18446744073709551615\n#EXTINF:6,\na.ts"),
        media("#EXT-X-MAP:URI=\"init\"\n#EXTINF:6,\na").replace("VERSION:7", "VERSION:5"),
        media("#EXTINF:6,\n#EXT-X-BYTERANGE:5@0\na.ts").replace("VERSION:7", "VERSION:3"),
        media("#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXT-X-MAP:URI=\"init\"\n#EXTINF:6,\na"),
    ] {
        assert!(parse_text(&text).is_err(), "{text}");
    }
}
#[test]
fn ancestry_depth_and_registration_and_output_bounds() {
    let r = parent();
    let base = Url::parse(&r.uri).unwrap();
    let child = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nchild.m3u8\n";
    let p = parse(child.as_bytes(), &base, &r, 2, 1, 2_097_152).unwrap();
    let c = &p.registrations[0];
    let nested = Url::parse(&c.uri).unwrap();
    assert!(parse(child.as_bytes(), &nested, c, 2, 1, 2_097_152).is_err());
    let other = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nnext.m3u8\n";
    let p2 = parse(other.as_bytes(), &nested, c, 3, 1, 2_097_152).unwrap();
    let end = &p2.registrations[0];
    assert!(
        parse(
            media("#EXTINF:6,\na.ts").as_bytes(),
            &Url::parse(&end.uri).unwrap(),
            end,
            2,
            1,
            2_097_152,
        )
        .is_err()
    );
    assert!(parse(MASTER.as_bytes(), &base, &r, 6, 1, 2_097_152).is_err());
    assert!(
        p.publish(
            &["opaque-resource-001".into()],
            "opaque-session-001",
            "grant",
            16
        )
        .is_err()
    );
}
#[test]
fn quoted_commas_escapes_and_duplicate_attributes() {
    let a = attributes("NAME=\"English, \\\"Stereo\\\"\",LANGUAGE=\"en\"").unwrap();
    assert_eq!(get(&a, "NAME"), Some("English, \"Stereo\""));
    assert_eq!(
        render(&a),
        "NAME=\"English, \\\"Stereo\\\"\",LANGUAGE=\"en\""
    );
    for s in [
        "URI=\"x\",URI=\"y\"",
        "URI=\"x\",",
        "URI=\"x\\q\"",
        "URI=\"unterminated",
    ] {
        assert!(attributes(s).is_err());
    }
}

#[test]
fn aggregate_registration_metadata_budget_is_enforced_while_constructing() {
    let r = parent();
    let base = Url::parse(&r.uri).unwrap();
    assert!(parse(MASTER.as_bytes(), &base, &r, 6, 4096, 256).is_err());
    let mut r = parent();
    r.context=serde_json::from_value(serde_json::json!({"scopes":[{"origin":"https://media.example.com","headers":{"Authorization":"x".repeat(4096)}}]})).unwrap();
    assert!(parse(MASTER.as_bytes(), &base, &r, 6, 4096, 8192).is_err());
}

#[test]
fn session_key_and_inline_session_metadata_have_explicit_closure_policy() {
    let text = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-SESSION-DATA:DATA-ID=\"com.example.title\",VALUE=\"Synthetic Movie\",LANGUAGE=\"en\"\n#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"key\",KEYFORMAT=\"identity\",KEYFORMATVERSIONS=\"1\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n";
    let p = parse_text(text).unwrap();
    assert_eq!(p.registrations[0].kind, ResourceKind::Key);
    let out = p
        .publish(
            &[
                "synthetic-resource-001".into(),
                "synthetic-resource-002".into(),
            ],
            "synthetic-session-001",
            "grant",
            65536,
        )
        .unwrap();
    assert!(out.contains("VALUE=\"Synthetic Movie\""));
    assert_eq!(out.matches("?st=grant").count(), 2);
    assert!(
        parse_text(&text.replace("VALUE=\"Synthetic Movie\"", "URI=\"external.json\"")).is_err()
    );
    assert!(parse_text(&text.replace("VERSION:7", "VERSION:6")).is_err());
}

#[test]
fn metadata_cannot_collide_with_structural_uri_rewrite_positions() {
    for attrs in [
        "TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"relay-reference-0\",URI=\"audio.m3u8\"",
        "TYPE=AUDIO,GROUP-ID=\"a\",URI=\"audio.m3u8\",NAME=\"relay-reference-0\"",
    ] {
        let text = format!(
            "#EXTM3U\n#EXT-X-MEDIA:{attrs}\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\nv.m3u8\n"
        );
        let p = parse_text(&text).unwrap();
        let out = p
            .publish(
                &[
                    "synthetic-resource-001".into(),
                    "synthetic-resource-002".into(),
                ],
                "synthetic-session-001",
                "grant",
                65536,
            )
            .unwrap();
        assert!(out.contains("NAME=\"relay-reference-0\""));
        assert!(
            out.contains("URI=\"/stream/synthetic-session-001/r/synthetic-resource-001?st=grant\"")
        );
        assert_eq!(out.matches("?st=grant").count(), 2);
    }
}

#[test]
fn key_url_query_text_does_not_change_feature_version_requirements() {
    for (version, uri) in [(1, "key?IV=1"), (4, "key?param=KEYFORMAT")] {
        let text = format!(
            "#EXTM3U\n#EXT-X-VERSION:{version}\n#EXT-X-TARGETDURATION:6\n#EXT-X-KEY:METHOD=AES-128,URI=\"{uri}\"\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n"
        );
        let p = parse_text(&text).unwrap();
        assert!(p.registrations[0].uri.ends_with(uri));
    }
}

#[test]
fn quoted_none_is_a_real_caption_group_and_attribute_types_are_checked() {
    let base = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,CLOSED-CAPTIONS=\"NONE\"\nv.m3u8\n";
    assert!(parse_text(base).is_err());
    let linked=base.replace("#EXT-X-STREAM-INF", "#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID=\"NONE\",NAME=\"CC\",INSTREAM-ID=\"CC1\"\n#EXT-X-STREAM-INF");
    let p = parse_text(&linked).unwrap();
    let out = p
        .publish(
            &["synthetic-resource-001".into()],
            "synthetic-session-001",
            "grant",
            65536,
        )
        .unwrap();
    assert!(out.contains("GROUP-ID=\"NONE\""));
    for text in [
        linked.replace("GROUP-ID=\"NONE\"", "GROUP-ID=NONE"),
        linked.replace("BANDWIDTH=1", "BANDWIDTH=\"1\""),
        linked.replace("TYPE=CLOSED-CAPTIONS", "TYPE=\"CLOSED-CAPTIONS\""),
    ] {
        assert!(parse_text(&text).is_err());
    }
}

#[test]
fn default_rendition_preserves_optional_autoselect_without_inventing_flags() {
    let absent = MASTER.replace(
        "DEFAULT=YES,AUTOSELECT=YES,CHANNELS",
        "DEFAULT=YES,CHANNELS",
    );
    let p = parse_text(&absent).unwrap();
    let ids = vec!["synthetic-resource-001".into(); p.registrations.len()];
    let out = p
        .publish(&ids, "synthetic-session-001", "grant", 65536)
        .unwrap();
    assert!(out.contains("DEFAULT=YES,CHANNELS"));
    assert!(parse_text(MASTER).is_ok());
    assert!(
        parse_text(&MASTER.replace("DEFAULT=YES,AUTOSELECT=YES", "DEFAULT=YES,AUTOSELECT=NO"))
            .is_err()
    );
}

#[test]
fn legacy_program_id_and_video_range_preserve_selected_master_without_uri_escape() {
    for range in ["SDR", "HLG", "PQ"] {
        let text = format!(
            "#EXTM3U\n#EXT-X-STREAM-INF:PROGRAM-ID=1,BANDWIDTH=1000000,VIDEO-RANGE={range},CODECS=\"avc1.640028,mp4a.40.2\"\nvideo.m3u8?private=synthetic\n"
        );
        let plan = parse_text(&text).unwrap();
        let out = plan
            .publish(&["opaque-resource-001".into()], "session", "grant", 65536)
            .unwrap();
        assert!(out.contains("PROGRAM-ID=1"));
        assert!(out.contains(&format!("VIDEO-RANGE={range}")));
        assert!(out.contains("CODECS=\"avc1.640028,mp4a.40.2\""));
        assert!(!out.contains("private=synthetic"));
        assert!(!out.contains("video.m3u8"));
        assert!(out.contains("/stream/session/r/opaque-resource-001?st=grant"));
    }
    // VERSION may follow the attribute: validate against the final declared value.
    for version in [6, 7] {
        let text = format!(
            "#EXTM3U\n#EXT-X-STREAM-INF:PROGRAM-ID=1,BANDWIDTH=1\nv.m3u8\n#EXT-X-VERSION:{version}\n"
        );
        assert!(matches!(parse_text(&text), Err(error) if error.1 == "unsupported_manifest"));
    }
    for attributes in [
        "PROGRAM-ID=-1",
        "PROGRAM-ID=18446744073709551616",
        "PROGRAM-ID=\"1\"",
        "VIDEO-RANGE=UNKNOWN",
        "VIDEO-RANGE=\"SDR\"",
        "VIDEO-RANGE=SDR,UNSUPPORTED-URI=\"https://private.invalid\"",
    ] {
        let text = format!("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,{attributes}\nv.m3u8\n");
        assert!(parse_text(&text).is_err());
    }
}

#[test]
fn numeric_loudness_metadata_preserves_verified_master_and_uri_closure() {
    let master = "#EXTM3U\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-STREAM-INF:BANDWIDTH=800000,CODECS=\"mp4a.40.2,avc1.42c01e\",RESOLUTION=640x360,FRAME-RATE=24,VIDEO-RANGE=SDR,YT-EXT-ABSOLUTE-LOUDNESS=-27.630,CLOSED-CAPTIONS=NONE\n360.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2700000,CODECS=\"mp4a.40.2,avc1.64001f\",RESOLUTION=1280x720,FRAME-RATE=24,VIDEO-RANGE=SDR,YT-EXT-ABSOLUTE-LOUDNESS=-27.630,CLOSED-CAPTIONS=NONE\n720.m3u8\n";
    let mut root = parent();
    root.manifest = Some(ManifestContext {
        ancestors: vec![],
        selected_variant: Some("https://media.example.com/path/720.m3u8".into()),
        adaptive_variants: serde_json::from_value(serde_json::json!([
            {"url":"https://media.example.com/path/360.m3u8","width":640,"height":360,"bitrate":800000,"codecs":["mp4a.40.2","avc1.42c01e"]},
            {"url":"https://media.example.com/path/720.m3u8","width":1280,"height":720,"bitrate":2700000,"codecs":["mp4a.40.2","avc1.64001f"]}
        ])).unwrap(),
        media_only: false,
        subtitles: false,
    });
    for value in ["-27.630", "0", "12.5"] {
        let text = master.replace("-27.630", value);
        let plan = parse(
            text.as_bytes(),
            &Url::parse(&root.uri).unwrap(),
            &root,
            6,
            128,
            2_097_152,
        )
        .unwrap();
        assert_eq!(plan.registrations.len(), 2);
        let output = plan
            .publish(
                &["opaque-low".into(), "opaque-high".into()],
                "session",
                "grant",
                65536,
            )
            .unwrap();
        assert_eq!(
            output
                .matches(&format!("YT-EXT-ABSOLUTE-LOUDNESS={value}"))
                .count(),
            2
        );
        assert_eq!(output.matches("/stream/session/r/").count(), 2);
        assert!(!output.contains("m3u8") && !output.contains("media.example.com"));
    }
    for value in [
        "\"-27.630\"",
        "NaN",
        "inf",
        "1e3",
        "--27.630",
        "27.6.30",
        "https://private.invalid",
    ] {
        assert!(parse_text(&master.replace("-27.630", value)).is_err());
    }
    for extra in [
        ",YT-EXT-ABSOLUTE-LOUDNESS=-20",
        ",YT-EXT-URI=\"https://private.invalid\"",
        ",UNKNOWN-LOUDNESS=-20",
    ] {
        assert!(
            parse_text(&master.replace(
                "CLOSED-CAPTIONS=NONE",
                &format!("CLOSED-CAPTIONS=NONE{extra}")
            ))
            .is_err()
        );
    }
}

#[test]
fn legacy_allow_cache_is_bounded_metadata_and_never_relaxes_uri_policy() {
    for value in ["YES", "NO"] {
        let text = format!(
            "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-ALLOW-CACHE:{value}\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\nvideo.ts\n#EXT-X-ENDLIST\n"
        );
        let plan = parse_text(&text).unwrap();
        let out = plan
            .publish(&["opaque-resource-001".into()], "session", "grant", 65536)
            .unwrap();
        assert!(out.contains(&format!("#EXT-X-ALLOW-CACHE:{value}")));
        assert!(!out.contains("video.ts"));
        for bad in [
            text.replace(value, "MAYBE"),
            text.replace(value, "https://private.invalid"),
            text.replace("VERSION:6", "VERSION:7"),
            text.replace(
                "#EXT-X-TARGETDURATION",
                &format!("#EXT-X-ALLOW-CACHE:{value}\n#EXT-X-TARGETDURATION"),
            ),
        ] {
            assert!(parse_text(&bad).is_err());
        }
    }
}

#[test]
fn verified_adaptive_ladder_keeps_groups_and_rejects_changed_or_missing_rungs() {
    let master = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080,CODECS=\"avc1.640028,mp4a.40.2\",AUDIO=\"a\"\n1080.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=1500000,RESOLUTION=1280x720,CODECS=\"avc1.64001f,mp4a.40.2\",AUDIO=\"a\"\n720.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=9000000,RESOLUTION=3840x2160,CODECS=\"hev1\"\n4k.m3u8\n";
    let variants = serde_json::from_value(serde_json::json!([
        {"url":"https://media.example.com/path/1080.m3u8","width":1920,"height":1080,"bitrate":3000000,"codecs":["avc1.640028","mp4a.40.2"],"audio_group":"a"},
        {"url":"https://media.example.com/path/720.m3u8","width":1280,"height":720,"bitrate":1500000,"codecs":["avc1.64001f","mp4a.40.2"],"audio_group":"a"}
    ])).unwrap();
    let mut root = parent();
    root.manifest = Some(ManifestContext {
        ancestors: vec![],
        selected_variant: Some("https://media.example.com/path/1080.m3u8".into()),
        adaptive_variants: variants,
        media_only: false,
        subtitles: false,
    });
    let parsed = parse(
        master.as_bytes(),
        &Url::parse(&root.uri).unwrap(),
        &root,
        6,
        128,
        2_097_152,
    )
    .unwrap();
    assert_eq!(parsed.registrations.len(), 3);
    for child in &parsed.registrations {
        let child_url = Url::parse(&child.uri).unwrap();
        assert!(child.manifest.as_ref().unwrap().media_only);
        assert!(parse(master.as_bytes(), &child_url, child, 6, 128, 2_097_152).is_err());
        assert!(
            parse(
                media("#EXTINF:6,\nsegment.ts").as_bytes(),
                &child_url,
                child,
                6,
                128,
                2_097_152
            )
            .is_ok()
        );
    }
    assert!(
        parsed
            .registrations
            .iter()
            .all(|resource| !resource.uri.contains("4k"))
    );
    let output = parsed
        .publish(&vec!["opaque".into(); 3], "session", "grant", 65536)
        .unwrap();
    assert_eq!(output.matches("#EXT-X-STREAM-INF").count(), 2);
    assert!(output.contains("GROUP-ID=\"a\"") && !output.contains("media.example.com"));
    for changed in [
        master.replace("1500000", "2500000"),
        master.replace("1280x720", "1920x1080"),
        master.replace("720.m3u8", "missing.m3u8"),
    ] {
        assert!(
            parse(
                changed.as_bytes(),
                &Url::parse(&root.uri).unwrap(),
                &root,
                6,
                128,
                2_097_152
            )
            .is_err()
        );
    }
}
