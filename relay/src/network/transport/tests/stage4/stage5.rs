use super::*;
pub(super) fn movie() -> Vec<u8> {
    let mut bytes = b"\0\0\0\x18ftypisom0000000000000000".to_vec();
    bytes.extend((0..4096).map(|i| (i % 251) as u8));
    bytes
}
fn tagged(mut wire: Wire) -> Wire {
    wire.headers = "Content-Type: text/html\r\nETag: \"v1\"\r\nLast-Modified: Sun, 06 Nov 1994 08:49:37 GMT\r\nDate: Sun, 06 Nov 1994 08:49:39 GMT\r\nSet-Cookie: secret\r\nLocation: https://private.example/\r\n".into();
    wire
}
async fn conditional(
    h: &Harness,
    range: Option<&str>,
    condition: &str,
) -> axum::response::Response {
    let mut req = Request::builder()
        .uri(h.entry())
        .header("if-range", condition);
    if let Some(range) = range {
        req = req.header("range", range);
    }
    h.router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}
#[tokio::test]
async fn mp4_entry_and_durable_resource_full_head_and_exact_absolute_ranges() {
    let h = Harness::start_format(|_| {}, true).await;
    let movie = movie();
    h.put("/movie.mp4", tagged(Wire::ranged(&movie)));
    let root = h.store.lookup(&h.sid, &Resource::Entry).await.unwrap();
    let ids = h
        .store
        .register(h.sid.clone(), 1, vec![root.registration.unwrap()])
        .await
        .unwrap();
    let resource = format!("/stream/{}/r/{}?st={}", h.sid, ids[0], h.grant);
    for path in [h.entry(), resource] {
        for (range, a, b) in [
            (None, 0, movie.len() - 1),
            (Some("bytes=0-1"), 0, 1),
            (Some("bytes=10-19"), 10, 19),
            (Some("bytes=100-"), 100, movie.len() - 1),
            (Some("bytes=-7"), movie.len() - 7, movie.len() - 1),
        ] {
            for method in ["GET", "HEAD"] {
                let response = h.request(&path, method, range).await;
                assert_eq!(response.status(), if range.is_some() { 206 } else { 200 });
                assert_eq!(response.headers()["content-type"], "video/mp4");
                assert_eq!(
                    response.headers()["content-length"],
                    (b - a + 1).to_string()
                );
                assert_eq!(response.headers()["etag"], "\"v1\"");
                for unsafe_header in ["set-cookie", "location", "digest"] {
                    assert!(!response.headers().contains_key(unsafe_header));
                }
                if range.is_some() {
                    assert_eq!(
                        response.headers()["content-range"],
                        format!("bytes {a}-{b}/{}", movie.len())
                    );
                }
                let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
                if method == "HEAD" {
                    assert!(bytes.is_empty());
                } else {
                    assert_eq!(bytes, &movie[a..=b]);
                }
            }
        }
    }
    // Every discovery is GET, so servers that reject HEAD remain usable.
    assert!(
        h.requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.starts_with("GET "))
    );
    h.close().await;
}
#[tokio::test]
async fn mp4_if_range_strong_weak_date_and_full_fallback() {
    let h = Harness::start_format(|_| {}, true).await;
    let expected = movie();
    h.put("/movie.mp4", tagged(Wire::ranged(&expected)));
    for (condition, status) in [
        ("\"v1\"", 206),
        ("\"v0\"", 200),
        ("W/\"v1\"", 200),
        ("Sun, 06 Nov 1994 08:49:37 GMT", 206),
        ("Sun, 06 Nov 1994 08:49:36 GMT", 200),
        ("Sun, 06 Nov 1994 08:49:38 GMT", 200),
    ] {
        let response = conditional(&h, Some("bytes=0-1"), condition).await;
        assert_eq!(response.status(), status, "{condition}");
        let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
        assert_eq!(
            bytes,
            if status == 206 {
                &expected[..2]
            } else {
                &expected[..]
            }
        );
    }
    let response = conditional(&h, None, "invalid ignored without Range").await;
    assert_eq!(response.status(), 200);
    drop(response);
    let count = h.requests.lock().unwrap().len();
    for bad in ["unquoted", "\"one\", \"two\"", "yesterday"] {
        assert_eq!(conditional(&h, Some("bytes=0-1"), bad).await.status(), 400);
    }
    assert_eq!(h.requests.lock().unwrap().len(), count);
    h.close().await;
}
#[tokio::test]
async fn mp4_malformed_unsatisfiable_and_ignored_ranges() {
    let h = Harness::start_format(|_| {}, true).await;
    h.put("/movie.mp4", tagged(Wire::ranged(movie())));
    for bad in ["bytes=4-1", "bytes=0-1,4-5", "bytes=18446744073709551616-"] {
        assert_eq!(h.request(&h.entry(), "GET", Some(bad)).await.status(), 400);
    }
    let req = Request::builder()
        .uri(h.entry())
        .header("range", "bytes=0-1")
        .header("range", "bytes=2-3")
        .body(Body::empty())
        .unwrap();
    assert_eq!(h.router.clone().oneshot(req).await.unwrap().status(), 400);
    assert!(h.requests.lock().unwrap().is_empty());
    for range in ["bytes=99999-", "bytes=-0"] {
        let response = h.request(&h.entry(), "GET", Some(range)).await;
        assert_eq!(response.status(), 416);
        assert_eq!(
            response.headers()["content-range"],
            format!("bytes */{}", movie().len())
        );
    }
    h.put("/movie.mp4", tagged(Wire::body(movie())));
    assert_eq!(
        h.request(&h.entry(), "GET", Some("bytes=100-"))
            .await
            .status(),
        502
    );
    // A matching If-Range cannot justify a silently ignored range either.
    assert_eq!(
        conditional(&h, Some("bytes=0-1"), "\"v1\"").await.status(),
        502
    );
    h.close().await;
}
#[tokio::test]
async fn hls_capability_never_allows_if_range_full_fallback() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put("/v.m3u8", Wire::body(VIDEO));
    let master = h.text(&h.entry()).await;
    let video = h.text(&uris(&master)[2]).await;
    let path = &uris(&video)[2];
    h.put("/file.mp4", tagged(Wire::ranged(movie())));
    let req = Request::builder()
        .uri(path)
        .header("range", "bytes=116-131")
        .header("if-range", "\"stale\"")
        .body(Body::empty())
        .unwrap();
    assert_eq!(h.router.clone().oneshot(req).await.unwrap().status(), 502);
    h.close().await;
}

#[tokio::test]
async fn mp4_large_logical_slow_reader_is_bounded_and_admission_recovers_on_drop() {
    let h = Harness::start_format(
        |c| {
            c.network.segment_timeout_s = 1;
        },
        true,
    )
    .await;
    // Three independent full MP4 streams; each advertises 1 TiB but the fixture
    // generates only one reusable 16 KiB chunk as writes are accepted.
    let mut wire = tagged(Wire::body(movie()));
    wire.logical_total = Some(1 << 40);
    let written = wire.written.clone();
    let closed = wire.closed.clone();
    h.put("/movie.mp4", wire);
    let mut responses = Vec::new();
    for _ in 0..3 {
        let response = h.request(&h.entry(), "GET", None).await;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers()["content-length"],
            (1u64 << 40).to_string()
        );
        responses.push(response);
    }
    // Slow consumption, and a pause exceeding the configured HLS segment total.
    let mut consumed = 0;
    for _ in 0..32 {
        for response in &mut responses {
            let bytes = response
                .body_mut()
                .frame()
                .await
                .unwrap()
                .unwrap()
                .into_data()
                .unwrap();
            assert!(bytes.len() <= h.config.network.body_chunk_bytes as usize);
            consumed += bytes.len();
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let before = written.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after = written.load(Ordering::SeqCst);
    assert_eq!(before, after, "unpolled readers must stop upstream writes");
    assert!(
        after < 32 * 1024 * 1024,
        "bounded loopback socket read-ahead"
    );
    for response in &mut responses {
        assert!(
            response.body_mut().frame().await.unwrap().is_ok(),
            "MP4 must outlive segment timeout"
        );
    }
    println!(
        "stage5 measurement: logical_bytes_per_stream={} streams=3 consumed_bytes={consumed} upstream_written_bytes={after} idle_growth_bytes={} frame_cap={} sniff_cap={}",
        1u64 << 40,
        after - before,
        h.config.network.body_chunk_bytes,
        h.config.limits.max_sniff_bytes
    );
    assert_eq!(h.transport.slots.available_permits(), 29);
    // Per-session cap is eight; held bodies exhaust it without extra dials.
    h.transport.guard.synthetic("exit-a");
    for _ in 3..8 {
        let response = h.request(&h.entry(), "GET", None).await;
        assert_eq!(response.status(), 200);
        responses.push(response);
    }
    let dials = h
        .transport
        .mock
        .as_ref()
        .unwrap()
        .dials
        .load(Ordering::SeqCst);
    assert_eq!(h.request(&h.entry(), "GET", None).await.status(), 429);
    assert_eq!(
        h.transport
            .mock
            .as_ref()
            .unwrap()
            .dials
            .load(Ordering::SeqCst),
        dials
    );
    drop(responses);
    tokio::time::timeout(Duration::from_secs(2), async {
        while closed.load(Ordering::SeqCst) != 8 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(h.transport.slots.available_permits(), 32);
    assert!(h.transport.counts.lock().unwrap().is_empty());
    h.close().await;
}

#[tokio::test]
async fn mp4_unpolled_real_connections_release_on_guard_revoke_transfer_shutdown_disconnect_and_write_idle()
 {
    for attack in 0..6 {
        let h = Harness::start_format(
            |c| {
                if attack == 2 {
                    c.network.mp4_max_transfer_s = 1;
                }
            },
            true,
        )
        .await;
        let mut wire = tagged(Wire::body(movie()));
        wire.logical_total = Some(1 << 40);
        let closed = wire.closed.clone();
        h.put("/movie.mp4", wire);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let stop = CancellationToken::new();
        let task = tokio::spawn(crate::runtime::listener_with_idle(
            listener,
            h.router.clone(),
            stop.clone(),
            2,
            Duration::from_millis(100),
            if attack == 5 {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(3)
            },
        ));
        let mut client = TcpStream::connect(address).await.unwrap();
        client
            .write_all(
                format!(
                    "GET {} HTTP/1.1\r\nHost: relay.example.com\r\n\r\n",
                    h.entry()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut data = [0; 64];
        assert!(client.read(&mut data).await.unwrap() > 0);
        assert_eq!(h.transport.slots.available_permits(), 31);
        match attack {
            0 => h.transport.guard.invalidate(),
            1 => h.store.revoke(h.sid.clone()).await.unwrap(),
            3 => stop.cancel(),
            4 => drop(client),
            _ => {}
        }
        // Never poll or drain the downstream body after the initial headers.
        tokio::time::timeout(Duration::from_secs(2), async {
            while closed.load(Ordering::SeqCst) != 1 || h.transport.slots.available_permits() != 32
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(h.transport.counts.lock().unwrap().is_empty());
        stop.cancel();
        task.await.unwrap().unwrap();
        h.close().await;
    }
}

#[tokio::test]
async fn headless_hls_constrained_discovery_stays_inside_capability_and_preserves_total() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put("/v.m3u8", Wire::body(VIDEO));
    let mut wire = tagged(Wire::ranged([9; 148]));
    wire.head_status = Some(405);
    h.put("/file.mp4", wire);
    let root = h.text(&h.entry()).await;
    let video = h.text(&uris(&root)[2]).await;
    let path = &uris(&video)[3];
    for range in ["bytes=132-", "bytes=-16"] {
        let response = h.request(path, "GET", Some(range)).await;
        assert_eq!(response.status(), 206);
        assert_eq!(response.headers()["content-range"], "bytes 132-147/148");
        assert_eq!(
            to_bytes(response.into_body(), 32).await.unwrap(),
            [9; 16].as_slice()
        );
    }
    let response = h.request(path, "GET", Some("bytes=-17")).await;
    assert_eq!(response.status(), 416);
    assert_eq!(response.headers()["content-range"], "bytes */148");
    for request in h
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.starts_with("GET /file.mp4"))
    {
        let request = request.to_ascii_lowercase();
        assert!(
            request.contains("range: bytes=132-132\r\n")
                || request.contains("range: bytes=132-147\r\n")
        );
    }
    h.close().await;
}
#[tokio::test]
async fn mp4_changes_between_evidence_and_seek_and_bad_206_metadata_fail_before_headers() {
    for attack in 0..9 {
        let h = Harness::start_format(|_| {}, true).await;
        let mut wire = tagged(Wire::ranged(movie()));
        match attack {
            0 => wire.evidence_headers = Some(wire.headers.replace("\"v1\"", "\"changed\"")),
            1 => wire.evidence_headers = Some(wire.headers.replace("08:49:37", "08:49:35")),
            2 => wire.headers.push_str("Content-Encoding: gzip\r\n"),
            3 => wire.headers.push_str("Content-Range: bytes 0-15/*\r\n"),
            4 => wire
                .headers
                .push_str("Content-Range: bytes 0-99999/100000\r\n"),
            5 => wire.headers.push_str("ETag: \"duplicate\"\r\n"),
            6 => wire.headers = wire.headers.replace("text/html", "multipart/byteranges"),
            7 => wire.body = b"<html>upstream error page</html>".to_vec(),
            _ => wire.body = b"\0\0\0\x18moofisom0000000000000000".to_vec(),
        }
        h.put("/movie.mp4", wire);
        let response = h.request(&h.entry(), "GET", Some("bytes=100-199")).await;
        assert_eq!(response.status(), 502, "attack {attack}");
        h.close().await;
    }
}
#[tokio::test]
async fn mp4_truncated_and_overlong_chunked_bodies_abort_without_appending_errors() {
    for attack in 0..3 {
        let h = Harness::start_format(|_| {}, true).await;
        let mut wire = tagged(Wire::body(movie()));
        match attack {
            0 => wire.length += 1,
            1 => wire.truncate_to = Some(24),
            _ => {
                wire = tagged(Wire::ranged(movie()));
                wire.chunked = true;
                wire.extra_range_bytes = 1;
            }
        }
        h.put("/movie.mp4", wire);
        let response = if attack == 2 {
            conditional(&h, Some("bytes=100-115"), "\"v1\"").await
        } else {
            h.request(&h.entry(), "GET", None).await
        };
        if response.status() == 200 || response.status() == 206 {
            assert!(to_bytes(response.into_body(), 8192).await.is_err());
        } else {
            assert_eq!(response.status(), 502);
        }
        h.close().await;
    }
}

#[tokio::test]
async fn bounded_global_wait_and_authority_admission_do_not_start_queued_downloads() {
    for global in [true, false] {
        let h = Harness::start_format(
            |c| {
                if global {
                    c.network.max_streams_count = 2;
                    c.network.max_streams_per_session_count = 2;
                    c.network.max_streams_per_authority_count = 2;
                } else {
                    c.network.max_streams_per_authority_count = 2;
                }
                c.network.admission_wait_s = 1;
            },
            true,
        )
        .await;
        let mut wire = tagged(Wire::body(movie()));
        wire.length = 1 << 30;
        wire.stall = true;
        h.put("/movie.mp4", wire);
        let first = h.request(&h.entry(), "GET", None).await;
        let second = h.request(&h.entry(), "GET", None).await;
        let dials = h
            .transport
            .mock
            .as_ref()
            .unwrap()
            .dials
            .load(Ordering::SeqCst);
        let path = h.entry();
        {
            let third = h.request(&path, "GET", None);
            tokio::pin!(third);
            if global {
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), &mut third)
                        .await
                        .is_err()
                );
                assert_eq!(
                    h.transport
                        .mock
                        .as_ref()
                        .unwrap()
                        .dials
                        .load(Ordering::SeqCst),
                    dials
                );
                assert_eq!(third.await.status(), 429);
                drop(first);
                let response = h.request(&h.entry(), "GET", None).await;
                assert_eq!(response.status(), 200);
                drop(response);
            } else {
                assert_eq!(third.await.status(), 429);
                assert_eq!(
                    h.transport
                        .mock
                        .as_ref()
                        .unwrap()
                        .dials
                        .load(Ordering::SeqCst),
                    dials
                );
                drop(first);
            }
        }
        drop(second);
        assert!(h.transport.counts.lock().unwrap().is_empty());
        h.close().await;
    }
}
#[tokio::test]
async fn mp4_read_idle_is_separate_from_active_transfer_deadline() {
    let h = Harness::start_format(
        |c| {
            c.network.read_idle_timeout_s = 1;
        },
        true,
    )
    .await;
    let mut wire = tagged(Wire::body(movie()));
    wire.length = 1 << 30;
    wire.stall = true;
    h.put("/movie.mp4", wire);
    let response = h.request(&h.entry(), "GET", None).await;
    assert_eq!(response.status(), 200);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), to_bytes(response.into_body(), 8192))
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(h.transport.slots.available_permits(), 32);
    h.close().await;
}
#[tokio::test]
async fn changing_trailer_validators_abort_and_private_or_hop_headers_are_not_published() {
    let h = Harness::start_format(|_| {}, true).await;
    let mut wire = tagged(Wire::body(movie()));
    wire.chunked = true;
    wire.trailers = "ETag: \"changed\"\r\n".into();
    h.put("/movie.mp4", wire);
    let response = h.request(&h.entry(), "GET", None).await;
    assert_eq!(response.status(), 200);
    assert!(to_bytes(response.into_body(), 8192).await.is_err());
    for tag in [
        "\"https://source.example.com/private\"",
        "\"source-only\"",
        "\"csrf-only\"",
    ] {
        let mut wire = tagged(Wire::body(movie()));
        wire.headers = wire.headers.replace("\"v1\"", tag);
        h.put("/movie.mp4", wire);
        let response = h.request(&h.entry(), "GET", None).await;
        assert_eq!(response.status(), 200);
        assert!(!response.headers().contains_key("etag"));
        drop(response);
    }
    let mut wire = tagged(Wire::body(movie()));
    wire.headers.push_str("Connection: ETag, Last-Modified\r\n");
    h.put("/movie.mp4", wire);
    let response = h.request(&h.entry(), "GET", None).await;
    assert!(!response.headers().contains_key("etag"));
    assert!(!response.headers().contains_key("last-modified"));
    drop(response);
    h.close().await;
}

#[tokio::test]
async fn extended_size_mp4_boxes_support_full_head_short_and_interior_ranges() {
    let h = Harness::start_format(|_| {}, true).await;
    let mut bytes = b"\0\0\0\x01ftyp".to_vec();
    bytes.extend_from_slice(&32u64.to_be_bytes());
    bytes.extend_from_slice(&movie()[8..]);
    h.put("/movie.mp4", tagged(Wire::ranged(&bytes)));
    for (range, a, b) in [
        (None, 0, bytes.len() - 1),
        (Some("bytes=0-1"), 0, 1),
        (Some("bytes=100-199"), 100, 199),
    ] {
        for method in ["GET", "HEAD"] {
            let response = h.request(&h.entry(), method, range).await;
            assert_eq!(response.status(), if range.is_some() { 206 } else { 200 });
            let body = to_bytes(response.into_body(), 8192).await.unwrap();
            if method == "GET" {
                assert_eq!(body, &bytes[a..=b]);
            } else {
                assert!(body.is_empty());
            }
        }
    }
    for invalid_size in [0u64, 8, 15] {
        let mut invalid = bytes.clone();
        invalid[8..16].copy_from_slice(&invalid_size.to_be_bytes());
        h.put("/movie.mp4", tagged(Wire::ranged(invalid)));
        assert_eq!(
            h.request(&h.entry(), "GET", Some("bytes=0-1"))
                .await
                .status(),
            502
        );
    }
    h.close().await;
}
#[tokio::test]
async fn conditional_eof_416_without_validators_uses_bounded_metadata_and_checks_total() {
    for headless in [false, true] {
        let h = Harness::start_format(|_| {}, true).await;
        let mut wire = tagged(Wire::ranged(movie()));
        wire.bare_unsatisfiable = true;
        if headless {
            wire.head_status = Some(405);
        }
        h.put("/movie.mp4", wire.clone());
        for condition in ["\"v1\"", "Sun, 06 Nov 1994 08:49:37 GMT"] {
            let response = conditional(&h, Some("bytes=99999-"), condition).await;
            assert_eq!(response.status(), 416);
            assert_eq!(
                response.headers()["content-range"],
                format!("bytes */{}", movie().len())
            );
        }
        wire.unsatisfiable_total = Some(movie().len() as u64 + 1);
        h.put("/movie.mp4", wire);
        assert_eq!(
            conditional(&h, Some("bytes=99999-"), "\"v1\"")
                .await
                .status(),
            502
        );
        let requests = h.requests.lock().unwrap().clone();
        assert!(
            requests.iter().filter(|r| r.starts_with("GET ")).all(|r| r
                .contains("range: bytes=99999-\r\n")
                || r.contains("range: bytes=0-0\r\n"))
        );
        h.close().await;
    }
}

#[tokio::test]
async fn conditional_206_can_omit_last_modified_without_inventing_response_headers() {
    let h = Harness::start_format(|_| {}, true).await;
    let mut wire = tagged(Wire::ranged(movie()));
    wire.omit_conditional_modified = true;
    h.put("/movie.mp4", wire);
    for condition in ["\"v1\"", "Sun, 06 Nov 1994 08:49:37 GMT"] {
        let response = conditional(&h, Some("bytes=100-115"), condition).await;
        assert_eq!(response.status(), 206);
        assert!(!response.headers().contains_key("last-modified"));
        assert_eq!(
            to_bytes(response.into_body(), 32).await.unwrap(),
            &movie()[100..=115]
        );
    }
    h.close().await;
}
