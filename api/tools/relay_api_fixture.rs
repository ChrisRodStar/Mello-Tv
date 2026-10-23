
// Appended only to a disposable cfg(test) relay source copy by the API tool.
#[tokio::test]
#[ignore = "explicit API-owned offline production-listener integration"]
async fn public_api_production_listeners() {
    for mp4 in [false, true] {
        let mut h = Harness::start_format_gate(|_| {}, mp4, Some(4*1024*1024)).await;
        let guard=h.transport.guard.clone();let guard_stop=h.stop.clone();h.jobs.push(tokio::spawn(async move{loop{tokio::select!{_ = guard_stop.cancelled()=>break,_ = tokio::time::sleep(Duration::from_millis(100))=>guard.api_fixture_tick()}}}));
        if mp4 {
            h.put("/movie.mp4", Wire::ranged(super::super::stage5::movie()));
        } else {
            h.put("/master.m3u8", Wire::body(MASTER));
            for path in ["/v.m3u8", "/audio.m3u8"] { h.put(path, Wire::body(VOD)); }
            h.put("/subs.m3u8", Wire::body("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.vtt\n#EXT-X-ENDLIST\n"));
            let mut ts=vec![0;188*3];for i in [0,188,376]{ts[i]=0x47;ts[i+3]=0x10;}
            h.put("/a.ts",Wire::body(ts));
            h.put("/a.vtt",Wire::body("WEBVTT\n\n00:00.000 --> 00:01.000\nSynthetic\n"));
        }
        let control = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control_address=control.local_addr().unwrap();let stream_address=stream.local_addr().unwrap();
        let mut stop=CancellationToken::new();
        let mut job=Some(tokio::spawn(crate::runtime::run(control,stream,app(&h),stop.clone())));
        let mailbox=h._dir.path().join("restart-mailbox");
        let mut child=tokio::process::Command::new(std::env::var("FAMILY_STREAM_API_TEST_BINARY").unwrap())
            .kill_on_drop(true).arg("-test.run=^TestProductionListeners$").arg("-test.v")
            .env("FAMILY_STREAM_RELAY_CONTROL",format!("http://{control_address}"))
            .env("FAMILY_STREAM_RELAY_STREAM",format!("http://{stream_address}"))
            .env("FAMILY_STREAM_RESTART_MAILBOX",&mailbox)
            .env("FAMILY_STREAM_MEDIA_FORMAT",if mp4{"mp4"}else{"hls"})
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        let result=tokio::time::timeout(Duration::from_secs(40),async {
            loop {
                if mailbox.exists(){
                    stop.cancel();job.take().unwrap().await.unwrap().unwrap();
                    h.api_storage_stop.cancel();h.jobs.remove(0).await.unwrap();
                    let (store,worker)=Store::open(h.config.clone()).unwrap();
                    h.store=store;h.api_storage_stop=CancellationToken::new();
                    let token=h.api_storage_stop.clone();
                    h.jobs.insert(0,tokio::spawn(async move{worker.run(token).await.unwrap();}));
                    let control=TcpListener::bind(control_address).await.unwrap();
                    let stream=TcpListener::bind(stream_address).await.unwrap();
                    stop=CancellationToken::new();job=Some(tokio::spawn(crate::runtime::run(control,stream,app(&h),stop.clone())));
                    std::fs::remove_file(&mailbox).unwrap();
                    break;
                }
                if let Some(status)=child.try_wait().unwrap(){panic!("API fixture exited before restart: {status}");}
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            child.wait().await.unwrap()
        }).await;
        if result.is_err(){let _=child.kill().await;let _=child.wait().await;}
        stop.cancel();job.take().unwrap().await.unwrap().unwrap();
        let requests=h.requests.lock().unwrap().clone();h.close().await;
        let mut output=Vec::new();if let Some(mut stdout)=child.stdout.take(){stdout.read_to_end(&mut output).await.unwrap();}
        let mut stderr=Vec::new();if let Some(mut stream)=child.stderr.take(){stream.read_to_end(&mut stderr).await.unwrap();}
        assert!(result.unwrap().success(),"{} {}",String::from_utf8_lossy(&output),String::from_utf8_lossy(&stderr));
        assert!(requests.iter().any(|r|r.contains("authorization: synthetic-upstream")));
        assert!(requests.iter().all(|r|!r.contains(BEARER)&&!r.contains("st=")));
    }
}
