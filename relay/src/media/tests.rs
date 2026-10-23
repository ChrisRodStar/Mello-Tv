#![allow(clippy::unwrap_used)]
use super::*;
use crate::session::{ByteRange, Registration};
fn headers(value: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("range", value.parse().unwrap());
    h
}
#[test]
fn range_syntax_overflow_and_single_header_policy() {
    for bad in [
        "items=0-1",
        "bytes=",
        "bytes=1-0",
        "bytes=1-2,4-5",
        "bytes=1-2-3",
        "bytes= 1-2",
        "bytes=+1-2",
        "bytes=18446744073709551616-",
    ] {
        assert!(Range::parse(&headers(bad)).is_err(), "{bad}");
    }
    let mut duplicate = headers("bytes=1-2");
    duplicate.append("range", "bytes=3-4".parse().unwrap());
    assert!(Range::parse(&duplicate).is_err());
    assert_eq!(
        Range::parse(&headers("bytes=0-18446744073709551615"))
            .unwrap()
            .unwrap()
            .resolve(100)
            .unwrap(),
        (0, 99)
    );
}
#[test]
fn range_resolution_preserves_absolute_offsets_and_unsatisfiable_distinction() {
    for (wire, expected) in [
        ("bytes=500-999", (500, 999)),
        ("bytes=500-", (500, 999)),
        ("bytes=-100", (900, 999)),
        ("bytes=-2000", (0, 999)),
    ] {
        assert_eq!(
            Range::parse(&headers(wire))
                .unwrap()
                .unwrap()
                .resolve(1000)
                .unwrap(),
            expected
        );
    }
    for wire in ["bytes=1000-", "bytes=-0"] {
        assert_eq!(
            Range::parse(&headers(wire))
                .unwrap()
                .unwrap()
                .resolve(1000)
                .unwrap_err()
                .0,
            416
        );
    }
    let limit = ByteRange {
        offset: 500,
        length: 100,
    };
    assert!(Range::Closed(500, 599).constrain(&limit).is_ok());
    for r in [
        Range::Closed(499, 599),
        Range::Closed(500, 600),
        Range::Open(500),
        Range::Suffix(100),
    ] {
        assert!(r.constrain(&limit).is_err());
    }
    assert!(Range::Closed(0, 0).resolve(0).is_err());
}
#[test]
fn upstream_range_must_have_known_total_valid_bounds_and_no_duplicates() {
    for value in [
        "bytes 5-4/10",
        "bytes 0-10/10",
        "bytes 0-1/*",
        "bytes 0-1/0",
        "bytes */10",
        "bytes 0-18446744073709551615/18446744073709551615",
    ] {
        let mut h = HeaderMap::new();
        h.insert("content-range", value.parse().unwrap());
        assert!(range::content_range(&h).is_err());
    }
    let mut h = HeaderMap::new();
    h.insert("content-range", "bytes 500-599/1000".parse().unwrap());
    assert_eq!(range::content_range(&h).unwrap(), (500, 599, 1000));
    h.append("content-range", "bytes 500-599/1000".parse().unwrap());
    assert!(range::content_range(&h).is_err());
}
#[test]
fn prefix_checks_containers_errors_ciphertext_and_interior_ranges() {
    let mut ts = vec![0; 564];
    for i in [0, 188, 376] {
        ts[i] = 0x47;
        ts[i + 3] = 0x10;
    }
    assert!(validate_prefix(&ts, &ResourceKind::Segment, false, 0).is_ok());
    assert!(validate_prefix(&ts[..187], &ResourceKind::Segment, false, 0).is_err());
    ts[188] = 0;
    assert!(validate_prefix(&ts, &ResourceKind::Segment, false, 0).is_err());
    for bytes in [
        b"<html>upstream error".as_slice(),
        b"{\"error\":true}",
        b"#EXTM3U\n",
        b"arbitrary",
    ] {
        assert!(validate_prefix(bytes, &ResourceKind::Segment, false, 0).is_err());
    }
    for (kind, prefix) in [
        (ResourceKind::Init, b"\0\0\0\x18ftypisom".as_slice()),
        (ResourceKind::Fragment, b"\0\0\0\x10moof0000"),
        (ResourceKind::Subtitle, b"WEBVTT\n\n"),
    ] {
        assert!(validate_prefix(prefix, &kind, false, 0).is_ok());
    }
    assert!(validate_prefix(b"1\n00:00:00,000", &ResourceKind::Subtitle, false, 0).is_err());
    assert!(validate_prefix(b"<arbitrary cipher", &ResourceKind::Fragment, true, 0).is_ok());
    assert!(validate_prefix(b"arbitrary payload", &ResourceKind::Fragment, false, 132).is_ok());
}
#[test]
fn subtitle_utf8_validation_spans_frames_without_collecting_body() {
    let mut v = Utf8::default();
    for b in "WEBVTT\nCafé 🎬\n".as_bytes() {
        v.push(&[*b]).unwrap();
        assert!(v.tail.len() <= 3);
    }
    assert!(v.tail.is_empty());
    assert!(v.push(&[0xff]).is_err());
    let mut v = Utf8::default();
    v.push(&[0xc3]).unwrap();
    assert!(!v.tail.is_empty());
    assert!(v.push(b"x").is_err());
}
#[test]
fn metadata_redaction_covers_query_and_context_values() {
    let r=Registration{uri:"https://media.example.com/master?sig=synthetic-secret".into(),kind:ResourceKind::Manifest,context:serde_json::from_str(r#"{"scopes":[{"origin":"https://media.example.com","headers":{"Authorization":"synthetic-bearer"}}]}"#).unwrap(),range:None,expires_at:None,manifest:None,encrypted:false};
    let base = url::Url::parse(&r.uri).unwrap();
    assert!(redact("#EXT-X-MEDIA:NAME=\"synthetic-secret\"", &r, &base, &[]).is_err());
    assert!(redact("#EXT-X-MEDIA:NAME=\"synthetic-bearer\"", &r, &base, &[]).is_err());
    assert!(redact("#EXT-X-MEDIA:NAME=\"English\"", &r, &base, &[]).is_ok());
}
#[tokio::test]
async fn connection_lease_closes_without_body_polling() {
    let lease = connection::ConnectionLease::default();
    let stop = tokio_util::sync::CancellationToken::new();
    let ended = lease.ended();
    tokio::pin!(ended);
    lease.set(
        stop.clone(),
        tokio_util::sync::CancellationToken::new(),
        Instant::now() + Duration::from_secs(30),
    );
    stop.cancel();
    tokio::time::timeout(Duration::from_millis(50), ended)
        .await
        .unwrap();
    let lease = connection::ConnectionLease::default();
    lease.set(
        tokio_util::sync::CancellationToken::new(),
        tokio_util::sync::CancellationToken::new(),
        Instant::now() + Duration::from_millis(10),
    );
    tokio::time::timeout(Duration::from_millis(100), lease.ended())
        .await
        .unwrap();
}

#[test]
fn short_tokens_cookie_values_and_child_query_values_cannot_hide_in_metadata() {
    let r=Registration{uri:"https://media.example.com/master?sig=abc".into(),kind:ResourceKind::Manifest,context:serde_json::from_str(r#"{"scopes":[{"origin":"https://media.example.com","headers":{"Authorization":"Bearer short-token"},"cookies":"session=cookie-secret; other=def","cookie_path":"/"}]}"#).unwrap(),range:None,expires_at:None,manifest:None,encrypted:false};
    let base = url::Url::parse(&r.uri).unwrap();
    let mut child = r.clone();
    child.uri = "https://media.example.com/child?sig=child-secret".into();
    for value in ["abc", "def", "short-token", "cookie-secret", "child-secret"] {
        assert!(
            redact(
                &format!("#EXT-X-MEDIA:NAME=\"{value}\""),
                &r,
                &base,
                &[child.clone()]
            )
            .is_err()
        );
    }
    assert!(redact("#EXT-X-MEDIA:NAME=\"English\",URI=\"/stream/abc/r/def?st=short-token\"\n/stream/abc/r/def?st=cookie-secret",&r,&base,&[child]).is_ok());
}

#[test]
fn ordinary_query_versions_and_languages_do_not_collide_with_codec_metadata() {
    let mut r=Registration{uri:"https://media.example.com/master?v=1&lang=en&locale=en-US&counter=1".into(),kind:ResourceKind::Manifest,context:serde_json::from_str(r#"{"scopes":[{"origin":"https://media.example.com","cookies":"flag=1","cookie_path":"/"}]}"#).unwrap(),range:None,expires_at:None,manifest:None,encrypted:false};
    let metadata = "#EXT-X-MEDIA:NAME=\"English\",LANGUAGE=\"en\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"avc1.640028,mp4a.40.2\"";
    let base = url::Url::parse(&r.uri).unwrap();
    assert!(redact(metadata, &r, &base, &[]).is_ok());
    r.uri = "https://media.example.com/master?v=synthetic-secret&token=abc".into();
    let base = url::Url::parse(&r.uri).unwrap();
    for name in ["synthetic-secret", "abc", "private abc"] {
        assert!(redact(&format!("#EXT-X-MEDIA:NAME=\"{name}\""), &r, &base, &[]).is_err());
    }
}
