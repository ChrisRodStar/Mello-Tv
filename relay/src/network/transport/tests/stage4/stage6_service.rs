//! Explicit opt-in Go HTTP -> production relay control/stream integration.
use super::*;

#[tokio::test]
#[ignore = "opt-in cross-language check requires sibling providers checkout, Go, Python and OpenSSL 3"]
async fn provider_http_uses_production_control_and_stream_listeners() {
    let temporary = tempfile::tempdir().unwrap();
    let fixture = temporary.path().join("provider-fixture");
    let providers = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("family-stream-providers");
    let build = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new("go")
            .kill_on_drop(true)
            .current_dir(providers)
            .env("GOPROXY", "off")
            .env("GOSUMDB", "off")
            .args(["test", "-c", "-o"])
            .arg(&fixture)
            .arg("./internal/transport/http")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(build.status.success(), "Go fixture build rejected");
    for mp4 in [false, true] {
        let h = Harness::start_format_gate(|_| {}, mp4, Some(4 * 1024 * 1024)).await;
        if mp4 {
            h.put("/movie.mp4", Wire::ranged(super::super::stage5::movie()));
        } else {
            h.put("/master.m3u8", Wire::body(MASTER));
            for path in ["/v.m3u8", "/audio.m3u8"] {
                h.put(path, Wire::body(VOD));
            }
            h.put(
                "/subs.m3u8",
                Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.vtt\n#EXT-X-ENDLIST\n"),
            );
            let mut ts = vec![0; 188 * 3];
            for offset in [0, 188, 376] {
                ts[offset] = 0x47;
                ts[offset + 3] = 0x10;
            }
            h.put("/a.ts", Wire::body(ts));
            h.put(
                "/a.vtt",
                Wire::body("WEBVTT\n\n00:00.000 --> 00:01.000\nSynthetic\n"),
            );
        }
        let ready = h._dir.path().join("provider-ready.json");
        let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control_port = control.local_addr().unwrap().port();
        let stream_port = stream.local_addr().unwrap().port();
        let stop = CancellationToken::new();
        let relay = tokio::spawn(crate::runtime::run(control, stream, app(&h), stop.clone()));
        let outcome = {
            let write = |name: &str, data: &[u8]| {
                let path = h._dir.path().join(name);
                std::fs::write(&path, data).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
                path
            };
            let provider_bearer =
                write("provider-bearer", b"synthetic-service-01234567890123456789");
            let control_bearer = write("control-bearer", BEARER.as_bytes());
            // PKCS#8 Ed25519 seed matching app()'s synthetic verification key.
            let mut der = vec![
                0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
                0x04, 0x20,
            ];
            der.extend_from_slice(&[7; 32]);
            let signer = write("signer.der", &der);
            let lookup = write("lookup.json", &serde_json::to_vec(&json!({
                "version":"providers.v1", "context_key":"synthetic-household",
                "lookup":{"title":{"type":"tv","tmdb_id":1429,"imdb_id":"tt2560140","my_anime_list_id":16498,"season":1,"episode":1},
                "preferences":{},"device":{"formats":[if mp4 {"mp4"} else {"hls"}],"allow_unknown_codecs":true,"allow_unknown_resolution":true}}
            })).unwrap());
            let openssl = std::env::var("FAMILY_STREAM_OPENSSL")
                .unwrap_or_else(|_| "/opt/homebrew/opt/openssl@3/bin/openssl".into());
            let config = write("client.json", &serde_json::to_vec(&json!({
                "provider_port":1, "provider_command":[fixture,"-test.run=^TestAcceptanceHTTPService$"], "provider_ready_file":ready,"control_port":control_port,"stream_port":stream_port,
                "provider_bearer_file":provider_bearer,"control_bearer_file":control_bearer,
                "signing_key_file":signer,"openssl":openssl,"kid":"test-key","owner_id":OWNER,
                "lookup_file":lookup,"failover":true,"refresh":true
            })).unwrap());
            run_owned_client(config).await
        };
        stop.cancel();
        relay.await.unwrap().unwrap();
        let requests = h.requests.lock().unwrap().clone();
        let exchanges: i64 = rusqlite::Connection::open(h._dir.path().join("acceptance.sqlite"))
            .unwrap()
            .query_row(
                "SELECT remaining FROM acceptance_requests WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exchanges > 0 && exchanges < 10000);
        let received = h.transport.acceptance.as_ref().unwrap().stop.is_cancelled();
        h.close().await;
        let output = outcome.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["evidence"], "provider_http_to_production_relay");
        assert_eq!(report["actions"].as_array().unwrap().len(), 4);
        assert!(!received, "small synthetic run exhausted its allowance");
        assert!(
            requests
                .iter()
                .any(|request| request.contains("authorization: synthetic-upstream"))
        );
        assert!(
            requests
                .iter()
                .all(|request| !request.contains(BEARER) && !request.contains("st="))
        );
    }
}

async fn run_owned_client(config: std::path::PathBuf) -> Option<std::process::Output> {
    let mut child = tokio::process::Command::new("python3")
        .kill_on_drop(true)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tools/service_acceptance.py"
        ))
        .arg("--opt-in")
        .arg("--owned-test-client")
        .arg("--config")
        .arg(config)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // The inner owner has a ten-second deadline and retains each client leader
    // unreaped until its complete group is stopped. The Rust timeout signals
    // that owner first, allowing its finally path to stop/join the Go descendant.
    let completed = tokio::time::timeout(Duration::from_secs(20), child.wait()).await;
    let status = match completed {
        Ok(status) => status.ok()?,
        Err(_) => {
            if let Some(pid) = child.id() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(2),
                    tokio::process::Command::new("/bin/kill")
                        .args(["-TERM", &pid.to_string()])
                        .status(),
                )
                .await;
            }
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(status) => status.ok()?,
                Err(_) => {
                    child.kill().await.ok()?;
                    return None;
                }
            }
        }
    };
    let mut stdout = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(1),
        child.stdout.take()?.take(8193).read_to_end(&mut stdout),
    )
    .await
    .ok()?
    .ok()?;
    Some(std::process::Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}
