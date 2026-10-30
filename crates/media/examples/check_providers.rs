#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use stream_core::{
    domain::MediaType,
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, StreamFormat, TmdbId,
    },
};
use stream_provider_adapters::resolve;
use stream_provider_sdk::http::{Header, HostError, ProviderHost, Request, Response};

struct LiveHost {
    rt: Arc<tokio::runtime::Runtime>,
    client: reqwest::Client,
}

impl LiveHost {
    fn new() -> Self {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .expect("failed to build tokio runtime"),
        );

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Duration::from_secs(90))
            .timeout(Duration::from_secs(5))
            .build()
            .expect("failed to build reqwest client");

        Self { rt, client }
    }

    fn prewarm(&self) {
        let client = self.client.clone();
        self.rt.block_on(async move {
            let urls = [
                "https://api.wecollege.net",
                "https://vidlink.pro",
                "https://vidcore.io",
                "https://vidsrc.sh",
                "https://vidfast.vc",
            ];
            let futures = urls.iter().map(|url| {
                let client = client.clone();
                async move {
                    let _ = client
                        .head(*url)
                        .timeout(Duration::from_secs(2))
                        .send()
                        .await;
                }
            });
            futures_util::future::join_all(futures).await;
        });

        // Pre-discover and prime the VidCore protocol so all subsequent lookups hit memory cache
        let warmup_lookup = make_lookup(ProviderTitle {
            media_type: MediaType::Movie,
            tmdb_id: TmdbId::try_from(27205).unwrap(),
            imdb_id: "tt1375666".into(),
            my_anime_list_id: None,
            season: None,
            episode: None,
        });
        let _ = stream_provider_adapters::resolve("vidcore", &warmup_lookup, self);
    }
}

async fn execute_request(client: &reqwest::Client, req: Request) -> Result<Response, HostError> {
    let method = match req.method.as_str() {
        "POST" => reqwest::Method::POST,
        _ => reqwest::Method::GET,
    };
    let mut builder = client.request(method, &req.url);
    if req.timeout_ms > 0 {
        builder = builder.timeout(Duration::from_millis(req.timeout_ms as u64));
    }
    for h in req.headers {
        builder = builder.header(&h.name, &h.value);
    }
    if !req.body.is_empty() {
        builder = builder.body(req.body);
    }
    let res = builder.send().await.map_err(|e| {
        if e.is_timeout() {
            HostError::Timeout
        } else {
            HostError::UpstreamFailure
        }
    })?;

    let status = res.status().as_u16();
    let mut headers = Vec::new();
    for (k, v) in res.headers() {
        if let Ok(v_str) = v.to_str() {
            headers.push(Header {
                name: k.as_str().to_string(),
                value: v_str.to_string(),
            });
        }
    }
    let bytes = res.bytes().await.map_err(|_| HostError::UpstreamFailure)?;
    if bytes.len() > req.max_response_bytes as usize {
        return Err(HostError::LimitExceeded);
    }
    Ok(Response {
        url: req.url,
        status,
        headers,
        body: bytes.to_vec(),
    })
}

impl ProviderHost for LiveHost {
    fn fetch(&self, request: Request) -> Result<Response, HostError> {
        let client = self.client.clone();
        self.rt
            .block_on(async move { execute_request(&client, request).await })
    }

    fn fetch_batch(
        &self,
        requests: Vec<Request>,
    ) -> Result<Vec<Result<Response, HostError>>, HostError> {
        let client = self.client.clone();
        Ok(self.rt.block_on(async move {
            let futures = requests.into_iter().map(|req| {
                let client = client.clone();
                async move { execute_request(&client, req).await }
            });
            futures_util::future::join_all(futures).await
        }))
    }

    fn now_unix_ms(&self) -> Result<i64, HostError> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Cancelled)?
            .as_millis() as i64)
    }

    fn random_bytes(&self, length: u32) -> Result<Vec<u8>, HostError> {
        let mut buf = vec![0u8; length as usize];
        getrandom::fill(&mut buf).map_err(|_| HostError::Cancelled)?;
        Ok(buf)
    }
}

fn make_lookup(title: ProviderTitle) -> Lookup {
    Lookup {
        title,
        preferences: QualityPreference::default(),
        device: DeviceCapabilities {
            formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
            codecs: vec!["h264".into(), "aac".into()],
            max_height: 1080,
            allow_unknown_codecs: true,
            allow_unknown_resolution: true,
            supports_encrypted_hls: true,
        },
        context_key: String::new(),
        startup_height: 1080,
        wait_for_complete: false,
        refresh: false,
    }
}

struct TestTarget {
    label: &'static str,
    title: ProviderTitle,
}

fn main() {
    println!("============================================================");
    println!("     LIVE PROVIDER VERIFICATION SUITE ACROSS SHOW TYPES     ");
    println!("============================================================");

    let host = LiveHost::new();
    print!("Pre-warming provider connections and TLS pools... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let prewarm_t0 = std::time::Instant::now();
    host.prewarm();
    println!("ready in {} ms.", prewarm_t0.elapsed().as_millis());

    let targets = [
        TestTarget {
            label: "Movie (Modern Sci-Fi): Inception (2010)",
            title: ProviderTitle {
                media_type: MediaType::Movie,
                tmdb_id: TmdbId::try_from(27205).unwrap(),
                imdb_id: "tt1375666".into(),
                my_anime_list_id: None,
                season: None,
                episode: None,
            },
        },
        TestTarget {
            label: "Movie (Cult Classic Drama): Fight Club (1999)",
            title: ProviderTitle {
                media_type: MediaType::Movie,
                tmdb_id: TmdbId::try_from(550).unwrap(),
                imdb_id: "tt0137523".into(),
                my_anime_list_id: None,
                season: None,
                episode: None,
            },
        },
        TestTarget {
            label: "TV Show (Prestige Drama): Breaking Bad S01E01",
            title: ProviderTitle {
                media_type: MediaType::Tv,
                tmdb_id: TmdbId::try_from(1396).unwrap(),
                imdb_id: "tt0903747".into(),
                my_anime_list_id: None,
                season: Some(1),
                episode: Some(1),
            },
        },
        TestTarget {
            label: "TV Show (Dark Fantasy): Supernatural S01E10",
            title: ProviderTitle {
                media_type: MediaType::Tv,
                tmdb_id: TmdbId::try_from(1622).unwrap(),
                imdb_id: "tt0460681".into(),
                my_anime_list_id: None,
                season: Some(1),
                episode: Some(10),
            },
        },
        TestTarget {
            label: "Anime / Animated: Attack on Titan S01E01",
            title: ProviderTitle {
                media_type: MediaType::Tv,
                tmdb_id: TmdbId::try_from(1429).unwrap(),
                imdb_id: "tt2560140".into(),
                my_anime_list_id: Some(16498),
                season: Some(1),
                episode: Some(1),
            },
        },
    ];

    let providers = ["vidsrc"];

    for target in &targets[..1] {
        println!("\n>>> Testing Target: {} <<<", target.label);
        println!("------------------------------------------------------------");

        let lookup = make_lookup(target.title.clone());

        for provider in &providers {
            print!("  [{:^8}] resolving... ", provider);
            std::io::Write::flush(&mut std::io::stdout()).unwrap();

            let t0 = std::time::Instant::now();
            match resolve(provider, &lookup, &host) {
                Ok(result) => {
                    let elapsed = t0.elapsed();
                    let candidates = result.candidates.unwrap_or_default();
                    let stream_count = candidates.len();
                    println!(
                        "SUCCESS ({} ms) -> {} streams found",
                        elapsed.as_millis(),
                        stream_count
                    );

                    for (idx, stream) in candidates.iter().take(3).enumerate() {
                        println!(
                            "     #{} [{:?}] Server: '{}' (variants: {}, tracks: {}) -> {}\n        FetchContext: {:?}",
                            idx + 1,
                            stream.format,
                            stream.server,
                            stream.resolver_metadata.variants.len(),
                            stream.resolver_metadata.tracks.len(),
                            stream.url,
                            stream.fetch_context,
                        );
                    }
                }
                Err(err) => {
                    let elapsed = t0.elapsed();
                    println!("FAILED ({} ms) -> {:?}", elapsed.as_millis(), err);
                }
            }
        }
    }

    println!("\n============================================================");
    println!("                 ALL LIVE CHECKS COMPLETE                   ");
    println!("============================================================");

    println!("\n>>> Running coordinator health checks... <<<");
    let shutdown = tokio_util::sync::CancellationToken::new();
    let coordinator = stream_media::DiscoveryCoordinator::default_live(shutdown).unwrap();
    let host_rt = tokio::runtime::Runtime::new().unwrap();
    use stream_core::ports::MediaEngine;
    let health = host_rt.block_on(async { coordinator.check_provider_health().await.unwrap() });
    for h in health {
        println!(
            "HEALTH [{}]: working={}, detail={}",
            h.id, h.working, h.detail
        );
    }
}
