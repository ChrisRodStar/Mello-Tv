//! Real Hyper through the existing cfg(test) injected resolver/dial boundary.
use super::*;
use crate::{
    auth::Verifier,
    config::Config,
    contracts::CreateSession,
    control,
    media::Delivery,
    session::{AuthorizationStore, Resource, Store},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use tower::ServiceExt;
const MASTER: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio.m3u8\"\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"English\",URI=\"subs.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\",SUBTITLES=\"s\"\nv.m3u8\n";
const VIDEO: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:30\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\",IV=0x0000000000000000000000000000001e\n#EXT-X-MAP:URI=\"init\",BYTERANGE=\"16@100\"\n#EXTINF:6,\n#EXT-X-BYTERANGE:16@116\nfile.mp4\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:6,\n#EXT-X-BYTERANGE:16\nfile.mp4\n#EXT-X-ENDLIST\n";
#[derive(Clone)]
struct Wire {
    status: u16,
    headers: String,
    body: Vec<u8>,
    length: usize,
    stall: bool,
    dynamic_range: bool,
    head_status: Option<u16>,
    bare_unsatisfiable: bool,
    omit_conditional_modified: bool,
    unsatisfiable_total: Option<u64>,
    evidence_headers: Option<String>,
    truncate_to: Option<usize>,
    extra_range_bytes: usize,
    trailers: String,
    chunked: bool,
    logical_total: Option<u64>,
    delayed_tail: Option<(Arc<tokio::sync::Notify>, Vec<u8>)>,
    written: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
}
impl Wire {
    fn body(body: impl AsRef<[u8]>) -> Self {
        let b = body.as_ref().to_vec();
        Self {
            status: 200,
            headers: "Content-Type: text/html\r\nSet-Cookie: secret\r\nETag: secret\r\nDigest: secret\r\n".into(),
            length: b.len(), body: b, stall: false, dynamic_range: false,
            head_status: None,
            bare_unsatisfiable: false,
            omit_conditional_modified: false,
            unsatisfiable_total: None, evidence_headers: None, truncate_to: None,
            extra_range_bytes: 0, trailers: String::new(), chunked: false,
            delayed_tail: None, logical_total: None, written: Arc::new(AtomicUsize::new(0)), closed: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn ranged(body: impl AsRef<[u8]>) -> Self {
        Self {
            dynamic_range: true,
            ..Self::body(body)
        }
    }
    fn range(body: impl AsRef<[u8]>, a: u64, b: u64, total: u64) -> Self {
        let mut wire = Self::body(body);
        wire.status = 206;
        wire.headers
            .push_str(&format!("Content-Range: bytes {a}-{b}/{total}\r\n"));
        wire
    }
}
struct Harness {
    _dir: Arc<tempfile::TempDir>,
    config: Arc<Config>,
    store: Store,
    router: Router,
    sid: String,
    grant: String,
    stop: CancellationToken,
    jobs: Vec<tokio::task::JoinHandle<()>>,
    responses: Arc<Mutex<BTreeMap<String, Wire>>>,
    requests: Arc<Mutex<Vec<String>>>,
    transport: Arc<Transport>,
}
impl Harness {
    async fn start(configure: impl FnOnce(&mut Config)) -> Self {
        Self::start_format(configure, false).await
    }
    async fn start_format(configure: impl FnOnce(&mut Config), mp4: bool) -> Self {
        Self::start_format_gate(configure, mp4, None).await
    }
    async fn start_format_gate(
        configure: impl FnOnce(&mut Config),
        mp4: bool,
        gate_bytes: Option<i64>,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut config =
            Config::parse(include_str!("../../../../config/relay.example.toml")).unwrap();
        config.state.database_path = dir
            .path()
            .join("state/relay.sqlite")
            .to_str()
            .unwrap()
            .into();
        config.state.encryption_key_file = dir.path().join("key").to_str().unwrap().into();
        std::fs::write(&config.state.encryption_key_file,json!({"active_key_id":"synthetic-key-001","keys":[{"kid":"synthetic-key-001","key":B64.encode([9;32])}]}).to_string()).unwrap();
        std::fs::set_permissions(
            &config.state.encryption_key_file,
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        configure(&mut config);
        let config = Arc::new(config);
        let (store, worker) = Store::open(config.clone()).unwrap();
        let stop = CancellationToken::new();
        let storage = tokio::spawn({
            let stop = stop.clone();
            async move {
                worker.run(stop).await.unwrap();
            }
        });
        let mut create:CreateSession=serde_json::from_value(json!({"contract_version":"relay.v1","owner_id":"synthetic-owner-001","expires_at":(chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339(),"selection":serde_json::from_str::<serde_json::Value>(include_str!("../../../../tests/fixtures/providers/renditions.json")).unwrap()})).unwrap();
        if mp4 {
            create.selection = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/providers/progressive-mp4.json"
            ))
            .unwrap();
        }
        let source = &mut create.selection.selected.as_mut().unwrap().candidate.source;
        source.url = "http://source.example.com/master.m3u8".into();
        source.selected_variant_url = Some("http://source.example.com/v.m3u8".into());
        if mp4 {
            source.url = "http://source.example.com/movie.mp4".into();
            source.selected_variant_url = None;
        }
        source.fetch_context = context();
        let sid = store
            .create(create, "hls-create".into())
            .await
            .unwrap()
            .session_id;
        let now = chrono::Utc::now().timestamp();
        let key = SigningKey::from_bytes(&[7; 32]);
        let verifier=Verifier::from_json(&config.auth,&serde_json::to_vec(&json!({"keys":[{"kid":"test-key","public_key":B64.encode(key.verifying_key().as_bytes())}]})).unwrap()).unwrap();
        let data=format!("{}.{}",B64.encode(r#"{"alg":"EdDSA","kid":"test-key"}"#),B64.encode(json!({"iss":"family-stream-api","aud":"family-stream-relay","sub":"synthetic-owner-001","sid":sid,"arv":1,"iat":now-1,"nbf":now-1,"exp":now+100,"jti":"synthetic-request-001"}).to_string()));
        let grant = format!(
            "{data}.{}",
            B64.encode(key.sign(data.as_bytes()).to_bytes())
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut transport, mut network, _) = setup_network(
            listener.local_addr().unwrap(),
            &["8.8.8.8"],
            config.network.clone(),
        );
        if let Some(bytes) = gate_bytes {
            let path = dir.path().join("acceptance.sqlite");
            crate::network::acceptance::tests::ledger(&path, bytes, 30_000);
            rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TABLE acceptance_requests (id INTEGER PRIMARY KEY, request_limit INTEGER, remaining INTEGER); INSERT INTO acceptance_requests VALUES (1,10000,10000);").unwrap();
            let gate = crate::network::acceptance::Gate::open(&path).unwrap();
            Arc::get_mut(&mut transport).unwrap().acceptance = Some(gate.clone());
            network.acceptance = Some(gate);
        }
        let delivery = Arc::new(Delivery {
            config: config.clone(),
            store: store.clone(),
            transport: transport.clone(),
        });
        let router = control::streaming_delivery(
            Some(Arc::new(verifier)),
            Arc::new(store.clone()),
            Some(delivery),
        );
        let responses = Arc::new(Mutex::new(BTreeMap::<String, Wire>::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server = tokio::spawn({
            let stop = stop.clone();
            let responses = responses.clone();
            let requests = requests.clone();
            async move {
                let mut connections = JoinSet::new();
                loop {
                    let mut socket = tokio::select! {_=stop.cancelled()=>break, _=connections.join_next(), if !connections.is_empty()=>continue, r=listener.accept(), if connections.len()<64=>r.unwrap().0};
                    let responses = responses.clone();
                    let requests = requests.clone();
                    let stop = stop.clone();
                    connections.spawn(async move {
                    let req = read_request(&mut socket).await;
                    let path = req.split_whitespace().nth(1).unwrap().to_owned();
                    let head = req.starts_with("HEAD ");
                    let conditional = req
                        .lines()
                        .find_map(|line| line.strip_prefix("if-range: "))
                        .map(str::to_owned);
                    let range = req
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("range: ")
                                .or_else(|| line.strip_prefix("Range: "))
                        })
                        .map(str::to_owned);
                    requests.lock().unwrap().push(req);
                    let mut wire = responses
                        .lock()
                        .unwrap()
                        .get(&path)
                        .cloned()
                        .unwrap_or(Wire {
                            status: 404,
                            ..Wire::body([])
                        });
                    if range.as_deref() == Some("bytes=0-15") && let Some(headers) = &wire.evidence_headers { wire.headers = headers.clone(); }
                    let matches = conditional.as_ref().is_none_or(|v| {
                        !v.starts_with("W/")
                            && (wire.headers.contains(&format!("ETag: {v}\r\n"))
                                || wire.headers.contains(&format!("Last-Modified: {v}\r\n")))
                    });
                    if wire.dynamic_range
                        && matches
                        && let Some(range) = range
                    {
                        let mut headers = HeaderMap::new();
                        headers.insert("range", range.parse().unwrap());
                        let total = wire.body.len() as u64;
                        let resolved = crate::media::range::Range::parse(&headers)
                            .unwrap()
                            .unwrap()
                            .resolve(total);
                        if let Ok((a, b)) = resolved {
                            wire.status = 206;
                            wire.body = wire.body[a as usize..=b as usize].to_vec();
                            wire.length = wire.body.len();
                            if a != 0 { wire.body.extend(std::iter::repeat_n(0,wire.extra_range_bytes)); }
                            wire.headers
                                .push_str(&format!("Content-Range: bytes {a}-{b}/{total}\r\n"));
                        } else {
                            wire.status = 416;
                            if wire.bare_unsatisfiable { wire.headers.clear(); }
                            let total = wire.unsatisfiable_total.unwrap_or(total);
                            wire.body.clear();
                            wire.length = 0;
                            wire.headers
                                .push_str(&format!("Content-Range: bytes */{total}\r\n"));
                        }
                    }
                    if wire.status == 206 && conditional.is_some() && wire.omit_conditional_modified {
                        wire.headers = wire.headers.lines().filter(|line| !line.starts_with("Last-Modified:")).map(|line|format!("{line}\r\n")).collect();
                    }
                    if head && wire.status == 206 {
                        wire.length = wire
                            .headers
                            .split("Content-Range: ")
                            .nth(1)
                            .unwrap()
                            .split('/')
                            .nth(1)
                            .unwrap()
                            .split('\r')
                            .next()
                            .unwrap()
                            .parse()
                            .unwrap();
                        wire.headers = wire
                            .headers
                            .lines()
                            .filter(|line| !line.starts_with("Content-Range:"))
                            .map(|line| format!("{line}\r\n"))
                            .collect();
                        wire.status = 200;
                    }
                    if head && let Some(status) = wire.head_status { wire.status = status; }
                    let framing = if wire.chunked { "Transfer-Encoding: chunked\r\n".to_owned() }
                        else { format!("Content-Length: {}\r\n",wire.logical_total.unwrap_or(wire.length as u64)) };
                    let header = format!("HTTP/1.1 {} Synthetic\r\n{framing}{}\r\n",wire.status,wire.headers);
                    let _ = socket.write_all(header.as_bytes()).await;
                    if !head {
                        if let Some(len) = wire.truncate_to { wire.body.truncate(len); }
                        if wire.chunked {
                            let _ = socket.write_all(format!("{:x}\r\n",wire.body.len()).as_bytes()).await;
                            let _ = socket.write_all(&wire.body).await;
                            let _ = socket.write_all(format!("\r\n0\r\n{}\r\n",wire.trailers).as_bytes()).await;
                        } else { let _ = socket.write_all(&wire.body).await; }
                    }
                    if !head && let Some((gate, tail)) = &wire.delayed_tail {
                        tokio::select! {
                            _ = stop.cancelled() => {},
                            _ = socket.read_u8() => {},
                            _ = gate.notified() => { let _ = socket.write_all(tail).await; }
                        }
                        wire.closed.fetch_add(1,Ordering::SeqCst);
                    }
                    if let Some(total) = wire.logical_total {
                        let chunk = [0x5a; 16384];
                        let mut count = wire.body.len() as u64;
                        while count < total {
                            let len = (total-count).min(chunk.len() as u64) as usize;
                            let result = tokio::select! { _=stop.cancelled()=>break, r=socket.write_all(&chunk[..len])=>r };
                            if result.is_err() { break; }
                            count += len as u64;
                            wire.written.fetch_add(len,Ordering::SeqCst);
                        }
                        wire.closed.fetch_add(1,Ordering::SeqCst);
                    }
                    if wire.stall {
                        tokio::select! {_=stop.cancelled()=>{},_=socket.read_u8()=>{}}
                    }
                    });
                }
                connections.abort_all();
                while connections.join_next().await.is_some() {}
            }
        });
        let net = tokio::spawn({
            let stop = stop.clone();
            async move {
                network.run_inner(stop, false).await;
            }
        });
        Self {
            _dir: Arc::new(dir),
            config,
            store,
            router,
            sid,
            grant,
            stop,
            jobs: vec![storage, server, net],
            responses,
            requests,
            transport,
        }
    }
    fn put(&self, path: &str, wire: Wire) {
        self.responses.lock().unwrap().insert(path.into(), wire);
    }
    fn entry(&self) -> String {
        format!("/stream/{}/entry?st={}", self.sid, self.grant)
    }
    async fn request(
        &self,
        path: &str,
        method: &str,
        range: Option<&str>,
    ) -> axum::response::Response {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(range) = range {
            req = req.header("range", range);
        }
        self.router
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }
    async fn text(&self, path: &str) -> String {
        let response = self.request(path, "GET", None).await;
        assert_eq!(response.status(), 200);
        String::from_utf8(
            to_bytes(response.into_body(), 65536)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }
    async fn close(self) {
        self.stop.cancel();
        for job in self.jobs {
            tokio::time::timeout(Duration::from_secs(3), job)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
fn uris(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            if line.starts_with("/stream/") {
                Some(line.into())
            } else {
                line.split_once("URI=\"")
                    .and_then(|(_, tail)| tail.split('"').next())
                    .map(str::to_owned)
            }
        })
        .collect()
}
#[tokio::test]
async fn routes_close_master_audio_subtitles_keys_init_and_ranges_without_leaks() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put("/v.m3u8", Wire::body(VIDEO));
    h.put(
        "/audio.m3u8",
        Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n"),
    );
    h.put(
        "/subs.m3u8",
        Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.vtt\n#EXT-X-ENDLIST\n"),
    );
    h.put("/key", Wire::body([9; 16]));
    h.put("/init", Wire::range([8; 16], 100, 115, 500));
    h.put("/file.mp4", Wire::range([7; 16], 116, 131, 500));
    h.put(
        "/a.vtt",
        Wire::body("WEBVTT\n\n00:00.000 --> 00:01.000\nSynthetic\n"),
    );
    let root = h.text(&h.entry()).await;
    assert!(!root.contains("source.example"));
    assert_eq!(h.text(&h.entry()).await, root);
    let children = uris(&root);
    assert_eq!(children.len(), 3);
    let subtitles = h.text(&children[1]).await;
    let vtt = h.request(&uris(&subtitles)[0], "GET", None).await;
    assert_eq!(vtt.headers()["content-type"], "text/vtt");
    assert!(
        to_bytes(vtt.into_body(), 1024)
            .await
            .unwrap()
            .starts_with(b"WEBVTT")
    );
    let audio = h.text(&children[0]).await;
    assert!(audio.contains("#EXTINF:6,"));
    let video = h.text(&children[2]).await;
    assert!(video.contains("#EXT-X-BYTERANGE:16@132"));
    let resources = uris(&video);
    assert_eq!(resources.len(), 4);
    let key = h.request(&resources[0], "GET", None).await;
    assert_eq!(key.headers()["content-type"], "application/octet-stream");
    assert_eq!(
        to_bytes(key.into_body(), 32).await.unwrap(),
        [9; 16].as_slice()
    );
    for (index, range, mime) in [
        (1, "bytes=100-115", "video/mp4"),
        (2, "bytes=116-131", "video/mp4"),
    ] {
        let response = h.request(&resources[index], "GET", Some(range)).await;
        assert_eq!(response.status(), 206);
        assert_eq!(response.headers()["content-type"], mime);
        assert!(!response.headers().contains_key("etag"));
        assert_eq!(to_bytes(response.into_body(), 32).await.unwrap().len(), 16);
    }
    assert_eq!(h.request(&resources[2], "GET", None).await.status(), 400);
    assert_eq!(
        h.request(&resources[2], "GET", Some("bytes=0-131"))
            .await
            .status(),
        416
    );
    let head = h.request(&h.entry(), "HEAD", None).await;
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], root.len().to_string());
    for header in ["etag", "digest", "set-cookie"] {
        assert!(!head.headers().contains_key(header));
    }
    assert!(to_bytes(head.into_body(), 8).await.unwrap().is_empty());
    for req in h.requests.lock().unwrap().iter() {
        assert!(!req.contains(&h.grant));
        assert!(!req.contains("st="));
        assert!(
            req.to_ascii_lowercase()
                .contains("accept-encoding: identity")
        );
    }
    h.close().await;
}
#[tokio::test]
async fn durable_failure_prevents_manifest_publication_and_restart_reuses_ids() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    let first = h.text(&h.entry()).await;
    let refs = uris(&first);
    let record = h
        .store
        .lookup(
            &h.sid,
            &Resource::Registered(
                refs[0]
                    .split("/r/")
                    .nth(1)
                    .unwrap()
                    .split('?')
                    .next()
                    .unwrap()
                    .into(),
            ),
        )
        .await
        .unwrap();
    assert!(record.registration.unwrap().manifest.unwrap().ancestors[0].ends_with("master.m3u8"));
    let db = rusqlite::Connection::open(&h.config.state.database_path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_hls BEFORE INSERT ON resources BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    h.put(
        "/master.m3u8",
        Wire::body(MASTER.replace("audio.m3u8", "new-audio.m3u8")),
    );
    let response = h.request(&h.entry(), "GET", None).await;
    assert_eq!(response.status(), 503);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("/stream/"));
    db.execute_batch("DROP TRIGGER reject_hls;").unwrap();
    drop(db);
    let config = h.config.clone();
    let sid = h.sid.clone();
    let directory = h._dir.clone();
    let grant = h.grant.clone();
    h.close().await;
    let (store, worker) = Store::open(config).unwrap();
    let stop = CancellationToken::new();
    let job = tokio::spawn(worker.run(stop.clone()));
    let root = store.lookup(&sid, &Resource::Entry).await.unwrap();
    let registration = root.registration.unwrap();
    let plan = crate::hls::parse(
        MASTER.as_bytes(),
        &Url::parse(&registration.uri).unwrap(),
        &registration,
        6,
        4096,
        2_097_152,
    )
    .unwrap();
    let ids = store
        .register(sid.clone(), 1, plan.registrations.clone())
        .await
        .unwrap();
    assert_eq!(plan.publish(&ids, &sid, &grant, 65536).unwrap(), first);
    stop.cancel();
    job.await.unwrap().unwrap();
    drop(directory);
}
#[tokio::test]
async fn malformed_manifests_and_registration_size_limits_return_only_safe_errors() {
    for (limit, manifest) in [
        (128, MASTER.to_owned()),
        (1, MASTER.to_owned()),
        (128, MASTER.replace("v.m3u8", "http://127.0.0.1/private")),
    ] {
        let h = Harness::start(|c| {
            if limit == 128 {
                c.limits.max_playlist_bytes = 64;
            } else {
                c.limits.max_manifest_registrations_count = 1;
            }
        })
        .await;
        h.put("/master.m3u8", Wire::body(manifest));
        let response = h.request(&h.entry(), "GET", None).await;
        assert_eq!(response.status(), 502);
        let text = String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec())
            .unwrap();
        for secret in [&h.grant, "127.0.0.1", "source.example", "URI="] {
            assert!(!text.contains(secret));
        }
        h.close().await;
    }
}
#[tokio::test]
async fn revocation_expiry_guard_loss_and_client_drop_cancel_active_media() {
    for attack in 0..4 {
        let h = Harness::start(|_| {}).await;
        h.put("/master.m3u8", Wire::body(MASTER));
        h.put(
            "/v.m3u8",
            Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n"),
        );
        let mut ts = vec![0u8; 564];
        for i in [0, 188, 376] {
            ts[i] = 0x47;
            ts[i + 3] = 0x10;
        }
        let mut wire = Wire::body(ts);
        wire.length = 1000;
        wire.stall = true;
        h.put("/a.ts", wire);
        let master = h.text(&h.entry()).await;
        let video = h.text(&uris(&master)[2]).await;
        let mut response = h.request(&uris(&video)[0], "GET", None).await;
        assert_eq!(response.status(), 200);
        let first = response
            .body_mut()
            .frame()
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert_eq!(first.len(), 380);
        match attack {
            0 => h.store.revoke(h.sid.clone()).await.unwrap(),
            1 => h.transport.guard.invalidate(),
            2 => {
                drop(response);
                h.close().await;
                continue;
            }
            _ => {
                let row = h.store.lookup(&h.sid, &Resource::Entry).await.unwrap();
                row.cancelled.cancel();
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(1), response.body_mut().frame())
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        drop(response);
        h.close().await;
    }
}

#[tokio::test]
async fn range_and_body_extent_mismatches_fail_without_forged_representations() {
    for attack in 0..6 {
        let h = Harness::start(|_| {}).await;
        h.put("/master.m3u8", Wire::body(MASTER));
        h.put("/v.m3u8", Wire::body(VIDEO));
        let root = h.text(&h.entry()).await;
        let video = h.text(&uris(&root)[2]).await;
        let path = &uris(&video)[2];
        let mut wire = Wire::range([9; 16], 116, 131, 500);
        match attack {
            0 => wire.status = 200,
            1 => wire.headers = wire.headers.replace("116-131/500", "115-130/500"),
            2 => wire.length = 15,
            3 => wire.headers.push_str("Content-Encoding: gzip\r\n"),
            4 => {
                wire.body.truncate(8);
            }
            _ => {
                wire.headers = wire.headers.replace("116-131/500", "116-131/*");
            }
        }
        h.put("/file.mp4", wire);
        let response = h.request(path, "GET", Some("bytes=116-131")).await;
        if attack == 4 {
            assert_eq!(response.status(), 206);
            assert!(to_bytes(response.into_body(), 32).await.is_err());
        } else {
            assert_eq!(response.status(), 502);
        }
        h.close().await;
    }
}
#[tokio::test]
async fn complete_ts_is_replayed_exactly_and_head_uses_bounded_evidence() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put(
        "/v.m3u8",
        Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n"),
    );
    let mut ts = vec![0; 188 * 20];
    for i in (0..ts.len()).step_by(188) {
        ts[i] = 0x47;
        ts[i + 3] = 0x10;
    }
    h.put("/a.ts", Wire::body(&ts));
    let root = h.text(&h.entry()).await;
    let video = h.text(&uris(&root)[2]).await;
    let path = &uris(&video)[0];
    let response = h.request(path, "GET", None).await;
    assert_eq!(response.headers()["content-type"], "video/mp2t");
    assert_eq!(
        to_bytes(response.into_body(), 65536).await.unwrap(),
        ts.as_slice()
    );
    let response = h.request(path, "HEAD", None).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-length"], ts.len().to_string());
    assert!(to_bytes(response.into_body(), 1).await.unwrap().is_empty());
    assert!(
        h.requests
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .starts_with("GET /a.ts ")
    );
    h.close().await;
}
#[tokio::test]
async fn malformed_or_unproven_subtitles_and_keys_are_not_relabelled() {
    for (body, key) in [
        (b"1\n00:00:00,000 --> 00:00:01,000".as_slice(), false),
        (b"WEBVTT\n\nabc\xff".as_slice(), false),
        (b"<html>error key</html>".as_slice(), true),
        (b"too-short".as_slice(), true),
    ] {
        let h = Harness::start(|_| {}).await;
        h.put("/master.m3u8", Wire::body(MASTER));
        if key {
            h.put("/v.m3u8", Wire::body(VIDEO));
            h.put("/key", Wire::body(body));
        } else {
            h.put(
                "/subs.m3u8",
                Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.vtt\n#EXT-X-ENDLIST\n"),
            );
            h.put("/a.vtt", Wire::body(body));
        }
        let master = h.text(&h.entry()).await;
        let playlist = h.text(&uris(&master)[if key { 2 } else { 1 }]).await;
        let response = h.request(&uris(&playlist)[0], "GET", None).await;
        if response.status() == 200 {
            assert!(to_bytes(response.into_body(), 1024).await.is_err());
        } else {
            assert_eq!(response.status(), 502);
        }
        h.close().await;
    }
}
#[tokio::test]
async fn nested_cycle_redirect_alias_and_depth_fail_without_crawling() {
    for attack in 0..3 {
        let h = Harness::start(|c| {
            if attack == 2 {
                c.limits.max_playlist_depth_count = 1;
            }
        })
        .await;
        h.put("/master.m3u8", Wire::body(MASTER));
        if attack == 2 {
            h.put("/master.m3u8", Wire::body(MASTER));
            assert_eq!(h.request(&h.entry(), "GET", None).await.status(), 502);
            h.close().await;
            continue;
        }
        let root = h.text(&h.entry()).await;
        let path = &uris(&root)[2];
        let nested = if attack == 0 {
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nmaster.m3u8\n"
        } else {
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nnext.m3u8\n"
        };
        if attack == 1 {
            let mut wire = Wire::body([]);
            wire.status = 302;
            wire.headers.push_str("Location: /master.m3u8\r\n");
            h.put("/v.m3u8", wire);
        } else {
            h.put("/v.m3u8", Wire::body(nested));
        }
        assert_eq!(h.request(path, "GET", None).await.status(), 502);
        assert!(
            !h.requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.contains(" /next.m3u8 "))
        );
        h.close().await;
    }
}

#[tokio::test]
async fn active_grant_expiry_is_exact_and_does_not_use_clock_leeway() {
    let mut h = Harness::start(|_| {}).await;
    let now = chrono::Utc::now().timestamp();
    let expiry = now + 2;
    let key = SigningKey::from_bytes(&[7; 32]);
    let data=format!("{}.{}",B64.encode(r#"{"alg":"EdDSA","kid":"test-key"}"#),B64.encode(json!({"iss":"family-stream-api","aud":"family-stream-relay","sub":"synthetic-owner-001","sid":h.sid,"arv":1,"iat":now-1,"nbf":now-1,"exp":expiry,"jti":"synthetic-request-001"}).to_string()));
    h.grant = format!(
        "{data}.{}",
        B64.encode(key.sign(data.as_bytes()).to_bytes())
    );
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put(
        "/v.m3u8",
        Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n"),
    );
    let mut ts = vec![0; 564];
    for i in [0, 188, 376] {
        ts[i] = 0x47;
        ts[i + 3] = 0x10;
    }
    let mut wire = Wire::body(ts);
    wire.length = 1000;
    wire.stall = true;
    h.put("/a.ts", wire);
    let root = h.text(&h.entry()).await;
    let video = h.text(&uris(&root)[2]).await;
    let mut response = h.request(&uris(&video)[0], "GET", None).await;
    assert_eq!(response.status(), 200);
    let body = response.body_mut();
    assert!(body.frame().await.unwrap().is_ok());
    assert!(body.frame().await.unwrap().is_ok());
    let error = tokio::time::timeout(Duration::from_secs(3), body.frame())
        .await
        .unwrap()
        .unwrap();
    assert!(error.is_err());
    assert!(chrono::Utc::now().timestamp_millis() <= expiry * 1000 + 300);
    drop(response);
    h.close().await;
}
#[tokio::test]
async fn real_listener_revocation_drops_body_reader_without_downstream_polling() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put(
        "/v.m3u8",
        Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n"),
    );
    let mut ts = vec![0; 564];
    for i in [0, 188, 376] {
        ts[i] = 0x47;
        ts[i + 3] = 0x10;
    }
    let mut wire = Wire::body(ts);
    wire.length = 1000;
    wire.stall = true;
    h.put("/a.ts", wire);
    let root = h.text(&h.entry()).await;
    let video = h.text(&uris(&root)[2]).await;
    let path = &uris(&video)[0];
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let task = tokio::spawn(crate::runtime::listener(
        listener,
        h.router.clone(),
        stop.clone(),
        2,
        Duration::from_secs(1),
    ));
    let mut client = TcpStream::connect(address).await.unwrap();
    client
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: relay.example.com\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut data = [0; 64];
    assert!(client.read(&mut data).await.unwrap() > 0);
    h.store.revoke(h.sid.clone()).await.unwrap();
    let mut tail = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut tail))
        .await
        .unwrap();
    stop.cancel();
    task.await.unwrap().unwrap();
    h.close().await;
}

#[tokio::test]
async fn constrained_open_and_suffix_ranges_use_only_metadata_then_exact_absolute_extent() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put("/v.m3u8", Wire::body(VIDEO));
    h.put("/file.mp4", Wire::range([9; 16], 132, 147, 148));
    let root = h.text(&h.entry()).await;
    let video = h.text(&uris(&root)[2]).await;
    let path = &uris(&video)[3];
    for range in ["bytes=132-", "bytes=-16"] {
        let response = h.request(path, "GET", Some(range)).await;
        assert_eq!(response.status(), 206);
        assert_eq!(response.headers()["content-range"], "bytes 132-147/148");
        assert_eq!(to_bytes(response.into_body(), 32).await.unwrap().len(), 16);
    }
    let count = h.requests.lock().unwrap().len();
    assert_eq!(
        h.request(path, "GET", Some("bytes=-100")).await.status(),
        416
    );
    assert_eq!(h.requests.lock().unwrap().len(), count + 1);
    let head = h.request(path, "HEAD", None).await;
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], "148");
    assert!(to_bytes(head.into_body(), 1).await.unwrap().is_empty());
    let requests = h.requests.lock().unwrap().clone();
    assert!(
        requests
            .iter()
            .filter(|r| r.starts_with("GET /file.mp4 "))
            .all(|r| r.to_ascii_lowercase().contains("range: bytes=132-147\r\n"))
    );
    h.close().await;
}

#[tokio::test]
async fn short_initial_ts_and_init_ranges_have_separate_bounded_evidence() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put("/v.m3u8",Wire::body("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST\n"));
    let mut ts = vec![0; 564];
    for i in [0, 188, 376] {
        ts[i] = 0x47;
        ts[i + 3] = 0x10;
    }
    h.put("/a.ts", Wire::ranged(&ts));
    let root = h.text(&h.entry()).await;
    let video = h.text(&uris(&root)[2]).await;
    let path = &uris(&video)[0];
    for (range, len) in [("bytes=0-187", 188), ("bytes=0-3", 4)] {
        for method in ["GET", "HEAD"] {
            let response = h.request(path, method, Some(range)).await;
            assert_eq!(response.status(), 206);
            assert_eq!(response.headers()["content-length"], len.to_string());
            let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
            if method == "GET" {
                assert_eq!(bytes, &ts[..len]);
            } else {
                assert!(bytes.is_empty());
            }
        }
    }
    assert!(
        h.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.to_ascii_lowercase().contains("range: bytes=0-379\r\n"))
    );
    h.put("/v.m3u8",Wire::body("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:6,\na.m4s\n#EXT-X-ENDLIST\n"));
    let init = b"\0\0\0\x18ftypisom0000000000000000";
    h.put("/init.mp4", Wire::ranged(init));
    let video = h.text(&uris(&root)[2]).await;
    let response = h.request(&uris(&video)[0], "GET", Some("bytes=0-3")).await;
    assert_eq!(response.status(), 206);
    assert_eq!(
        to_bytes(response.into_body(), 16).await.unwrap(),
        &init[..4]
    );
    h.close().await;
}
#[tokio::test]
async fn subtitle_ranges_may_start_or_end_inside_utf8_characters() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put(
        "/subs.m3u8",
        Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.vtt\n#EXT-X-ENDLIST\n"),
    );
    let vtt = "WEBVTT\n\nCafé 🎬\n".as_bytes();
    h.put("/a.vtt", Wire::ranged(vtt));
    let root = h.text(&h.entry()).await;
    let sub = h.text(&uris(&root)[1]).await;
    let path = &uris(&sub)[0];
    let index = vtt.iter().position(|b| *b == 0xc3).unwrap();
    for (a, b) in [
        (0, 3),
        (index, index),
        (index + 1, index + 1),
        (index + 1, vtt.len() - 1),
    ] {
        let response = h
            .request(path, "GET", Some(&format!("bytes={a}-{b}")))
            .await;
        assert_eq!(response.status(), 206);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            &vtt[a..=b]
        );
    }
    h.close().await;
}

#[tokio::test]
async fn subtitle_evidence_stays_inside_original_absolute_capability() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    let vtt = "WEBVTT\n\nCafé\n".as_bytes();
    let mut representation = vec![0; 100];
    representation.extend_from_slice(vtt);
    representation.extend_from_slice(&[0; 100]);
    h.put("/a.vtt", Wire::ranged(representation));
    h.put("/subs.m3u8",Wire::body(format!("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\n#EXT-X-BYTERANGE:{}@100\na.vtt\n#EXT-X-ENDLIST\n",vtt.len())));
    let root = h.text(&h.entry()).await;
    let sub = h.text(&uris(&root)[1]).await;
    let path = &uris(&sub)[0];
    let index = vtt.iter().position(|b| *b == 0xc3).unwrap() + 101;
    let response = h
        .request(path, "GET", Some(&format!("bytes={index}-{index}")))
        .await;
    assert_eq!(response.status(), 206);
    assert_eq!(
        to_bytes(response.into_body(), 2).await.unwrap(),
        [0xa9].as_slice()
    );
    let requests = h.requests.lock().unwrap().clone();
    let fetches = requests
        .iter()
        .filter(|r| r.starts_with("GET /a.vtt "))
        .collect::<Vec<_>>();
    assert_eq!(fetches.len(), 2);
    assert!(
        fetches[0]
            .to_ascii_lowercase()
            .contains(&format!("range: bytes=100-{}\r\n", 100 + vtt.len() - 1))
    );
    h.close().await;
}

#[tokio::test]
async fn master_with_ordinary_query_parameters_preserves_codec_and_language_attributes() {
    let h = Harness::start(|_| {}).await;
    let mut redirect = Wire::body([]);
    redirect.status = 302;
    redirect
        .headers
        .push_str("Location: /master.m3u8?v=1&lang=en\r\n");
    h.put("/master.m3u8", redirect);
    h.put(
        "/master.m3u8?v=1&lang=en",
        Wire::body(MASTER.replace(
            "BANDWIDTH=1,",
            "BANDWIDTH=1,CODECS=\"avc1.640028,mp4a.40.2\",",
        )),
    );
    let out = h.text(&h.entry()).await;
    assert!(out.contains("CODECS=\"avc1.640028,mp4a.40.2\""));
    assert_eq!(uris(&out).len(), 3);
    h.close().await;
}

mod stage5;

mod stage6;

#[tokio::test]
async fn player_open_ranges_on_rewritten_playlists_and_keys_return_complete_representations() {
    let h = Harness::start(|_| {}).await;
    h.put("/master.m3u8", Wire::body(MASTER));
    h.put("/v.m3u8", Wire::body(VIDEO));
    h.put("/key", Wire::body([9; 16]));
    let root = h.text(&h.entry()).await;
    let video = h.text(&uris(&root)[2]).await;
    let key = uris(&video)[0].clone();
    for (path, expected) in [(h.entry(), root.into_bytes()), (key, vec![9; 16])] {
        for method in ["GET", "HEAD"] {
            for range in ["bytes=0-", "bytes=1-2", "bytes=-1", "bytes=99999999-"] {
                let response = h.request(&path, method, Some(range)).await;
                assert_eq!(response.status(), 200);
                assert_eq!(
                    response.headers()["content-length"],
                    expected.len().to_string()
                );
                assert!(!response.headers().contains_key("content-range"));
                assert_eq!(response.headers()["cache-control"], "private, no-store");
                let body = to_bytes(response.into_body(), 65536).await.unwrap();
                assert_eq!(
                    body.as_ref(),
                    if method == "HEAD" {
                        &[]
                    } else {
                        expected.as_slice()
                    }
                );
            }
        }
        assert_eq!(
            h.request(&path, "GET", Some("bytes=invalid"))
                .await
                .status(),
            400
        );
    }
    assert!(
        h.requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| !request.to_ascii_lowercase().contains("\r\nrange:"))
    );
    h.close().await;
}
