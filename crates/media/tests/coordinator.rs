#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::if_same_then_else,
    clippy::panic
)]

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use semver::Version;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use stream_core::{
    ports::MediaEngine,
    providers::{
        AdapterResult, CandidateStream, ErrorCategory, FetchContext, Lookup, Metadata, ProviderId,
        ProviderTitle, StreamFormat, TmdbId,
    },
};
use stream_media::{
    coordinator::{
        DiscoveryCoordinator, DiscoveryError, DiscoveryLimits, DiscoveryProvider, ProviderCatalog,
    },
    network::{FetchMetrics, FetchRequest, FetchResponse, TransportError},
    prober::{ProbeLimits, ProbeTransport, StreamProber},
};
use tokio_util::sync::CancellationToken;
use url::Url;

// Helper to construct a standard MPEG-TS segment of 100 packets (18,800 bytes)
fn mock_ts_segment() -> Bytes {
    let mut data = Vec::with_capacity(100 * 188);
    for _ in 0..100 {
        let mut packet = [0u8; 188];
        packet[0] = 0x47; // TS sync byte
        packet[1] = 0x00; // No error, not payload start
        packet[2] = 0x00;
        packet[3] = 0x10; // Payload only
        data.extend_from_slice(&packet);
    }
    Bytes::from(data)
}

struct MockTransport;

#[async_trait]
impl ProbeTransport for MockTransport {
    async fn fetch(
        &self,
        request: FetchRequest,
        _cancellation: &CancellationToken,
    ) -> Result<FetchResponse, TransportError> {
        let url = Url::parse(&request.url).map_err(|_| TransportError::InvalidConfig)?;
        let path = url.path();

        if path.ends_with("1080p.m3u8") {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        let (body, content_type) = if path.ends_with("master.m3u8") {
            (
                Bytes::from(
                    "#EXTM3U\n\
                     #EXT-X-VERSION:4\n\
                     #EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080,CODECS=\"avc1.4d401f\"\n\
                     https://cdn.example.com/1080p.m3u8\n\
                     #EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1280x720,CODECS=\"avc1.4d401f\"\n\
                     https://cdn.example.com/720p.m3u8\n",
                ),
                "application/vnd.apple.mpegurl",
            )
        } else if path.ends_with("1080p.m3u8") {
            (
                Bytes::from(
                    "#EXTM3U\n\
                     #EXT-X-VERSION:3\n\
                     #EXT-X-TARGETDURATION:6\n\
                     #EXTINF:6.0,\n\
                     https://cdn.example.com/seg1.ts\n\
                     #EXTINF:6.0,\n\
                     https://cdn.example.com/seg2.ts\n\
                     #EXTINF:6.0,\n\
                     https://cdn.example.com/seg3.ts\n\
                     #EXT-X-ENDLIST\n",
                ),
                "application/vnd.apple.mpegurl",
            )
        } else if path.ends_with("720p.m3u8") {
            (
                Bytes::from(
                    "#EXTM3U\n\
                     #EXT-X-VERSION:3\n\
                     #EXT-X-TARGETDURATION:6\n\
                     #EXTINF:6.0,\n\
                     https://cdn.example.com/seg1.ts\n\
                     #EXTINF:6.0,\n\
                     https://cdn.example.com/seg2.ts\n\
                     #EXTINF:6.0,\n\
                     https://cdn.example.com/seg3.ts\n\
                     #EXT-X-ENDLIST\n",
                ),
                "application/vnd.apple.mpegurl",
            )
        } else if path.ends_with(".ts") {
            (mock_ts_segment(), "video/mp2t")
        } else {
            return Err(TransportError::DestinationDenied);
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            content_type
                .parse()
                .map_err(|_| TransportError::InvalidConfig)?,
        );

        Ok(FetchResponse {
            url,
            status: StatusCode::OK,
            headers,
            body,
            metrics: FetchMetrics {
                admission_wait: Duration::from_millis(1),
                response_headers: Duration::from_millis(5),
                first_byte: Some(Duration::from_millis(10)),
                transfer: Duration::from_millis(2),
                first_chunk_bytes: 1024,
                total: Duration::from_millis(8),
                redirects: 0,
            },
        })
    }
}

struct TestProvider {
    id: ProviderId,
    version: Version,
    generation: u64,
    call_count: Arc<AtomicUsize>,
    delay: Duration,
    fail: bool,
}

#[async_trait]
impl DiscoveryProvider for TestProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }
    fn version(&self) -> &Version {
        &self.version
    }
    fn generation(&self) -> u64 {
        self.generation
    }
    async fn resolve(
        &self,
        lookup: &Lookup,
        cancel: CancellationToken,
    ) -> Result<AdapterResult, ErrorCategory> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::select! {
                _ = cancel.cancelled() => return Err(ErrorCategory::Cancelled),
                _ = tokio::time::sleep(self.delay) => {}
            }
        }
        if self.fail {
            return Err(ErrorCategory::Unavailable);
        }

        let candidate = CandidateStream {
            expires_at: None,
            id: "stream-1".into(),
            server: "server-a".into(),
            selected_variant_url: String::new(),
            url: "https://cdn.example.com/master.m3u8".into(),
            provider: self.id.clone(),
            format: StreamFormat::Hls,
            title: lookup.title.clone(),
            fetch_context: FetchContext::default(),
            resolver_metadata: Metadata::default(),
            content_verified: true,
        };

        Ok(AdapterResult {
            issues: Vec::new(),
            candidates: Some(vec![candidate]),
            resolver_duration: 1_000_000,
        })
    }
}

struct TestCatalog {
    providers: Vec<Arc<dyn DiscoveryProvider>>,
}

impl ProviderCatalog for TestCatalog {
    fn snapshot(&self) -> Result<Vec<Arc<dyn DiscoveryProvider>>, DiscoveryError> {
        Ok(self.providers.clone())
    }
}

fn test_lookup() -> Lookup {
    let mut lookup = Lookup {
        title: ProviderTitle::movie(TmdbId::try_from(12345).unwrap()),
        preferences: Default::default(),
        device: stream_core::providers::DeviceCapabilities {
            formats: vec![StreamFormat::Hls],
            codecs: vec!["avc1".into()],
            max_height: 1080,
            allow_unknown_resolution: true,
            allow_unknown_codecs: true,
            supports_encrypted_hls: true,
        },
        context_key: "test-partition".into(),
        startup_height: 1080,
        wait_for_complete: true,
        refresh: false,
    };
    lookup.preferences.max_height = 1080;
    lookup
}

fn create_coordinator(
    providers: Vec<Arc<dyn DiscoveryProvider>>,
) -> Result<DiscoveryCoordinator, DiscoveryError> {
    let transport = Arc::new(MockTransport);
    let prober = StreamProber::new(transport, ProbeLimits::default()).unwrap();
    let limits = DiscoveryLimits {
        max_flights: 16,
        concurrent_providers: 4,
        round_timeout: Duration::from_secs(5),
        provider_timeout: Duration::from_millis(200),
    };
    DiscoveryCoordinator::new(Arc::new(TestCatalog { providers }), prober, limits)
}

#[tokio::test]
async fn coalesces_concurrent_title_lookups_to_prevent_redundant_requests() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(TestProvider {
        id: ProviderId::new("vidcore").unwrap(),
        version: Version::new(1, 0, 0),
        generation: 1,
        call_count: call_count.clone(),
        delay: Duration::from_millis(50),
        fail: false,
    });

    let coordinator = create_coordinator(vec![provider]).unwrap();
    let cancel = CancellationToken::new();

    let lookup1 = test_lookup();
    let lookup2 = test_lookup();

    let (res1, res2) = tokio::join!(
        coordinator.resolve(lookup1, &cancel),
        coordinator.resolve(lookup2, &cancel)
    );

    let sel1 = res1.expect("lookup 1 should succeed");
    let sel2 = res2.expect("lookup 2 should succeed");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "provider resolve must be called only once"
    );
    assert_eq!(
        sel1.selected.as_ref().unwrap().candidate.source.url,
        sel2.selected.as_ref().unwrap().candidate.source.url
    );
}

#[tokio::test]
async fn provider_timeout_preserves_successful_provider_results() {
    let fast_count = Arc::new(AtomicUsize::new(0));
    let slow_count = Arc::new(AtomicUsize::new(0));

    let fast_provider = Arc::new(TestProvider {
        id: ProviderId::new("vidfast").unwrap(),
        version: Version::new(1, 0, 0),
        generation: 1,
        call_count: fast_count.clone(),
        delay: Duration::from_millis(10),
        fail: false,
    });

    let slow_provider = Arc::new(TestProvider {
        id: ProviderId::new("vidslow").unwrap(),
        version: Version::new(1, 0, 0),
        generation: 1,
        call_count: slow_count.clone(),
        delay: Duration::from_millis(500), // Exceeds 200ms provider_timeout
        fail: false,
    });

    let coordinator = create_coordinator(vec![fast_provider, slow_provider]).unwrap();
    let cancel = CancellationToken::new();

    let result = coordinator
        .resolve(test_lookup(), &cancel)
        .await
        .expect("resolution succeeds via fast provider");

    assert_eq!(fast_count.load(Ordering::SeqCst), 1);
    assert!(result.selected.is_some());
    let statuses = result.providers.as_ref().expect("statuses present");
    let slow_status = statuses
        .iter()
        .find(|s| s.provider.as_str() == "vidslow")
        .expect("slow provider status present");
    assert_eq!(slow_status.failure, Some(ErrorCategory::Timeout));
}

#[tokio::test]
async fn media_engine_trait_returns_ranked_primary_and_backup_streams() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(TestProvider {
        id: ProviderId::new("vidcore").unwrap(),
        version: Version::new(1, 0, 0),
        generation: 1,
        call_count,
        delay: Duration::ZERO,
        fail: false,
    });

    let engine: Arc<dyn MediaEngine> = Arc::new(create_coordinator(vec![provider]).unwrap());
    let result = engine
        .resolve_stream(&test_lookup())
        .await
        .expect("resolution succeeds");

    let primary = result.selected.as_ref().expect("primary stream selected");
    // 1080p variant should rank above 720p variant
    assert_eq!(
        primary
            .candidate
            .observation
            .variant
            .as_ref()
            .unwrap()
            .height,
        Some(1080)
    );

    let backups = result.backups.as_ref().expect("backups present");
    assert!(!backups.is_empty(), "backups should contain the 720p rung");
    assert_eq!(
        backups[0]
            .candidate
            .observation
            .variant
            .as_ref()
            .unwrap()
            .height,
        Some(720)
    );
}

#[tokio::test]
async fn progressive_startup_prefers_startup_rung_and_probes_higher_rungs_afterward() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(TestProvider {
        id: ProviderId::new("vidcore").unwrap(),
        version: Version::new(1, 0, 0),
        generation: 1,
        call_count,
        delay: Duration::ZERO,
        fail: false,
    });

    let coordinator = create_coordinator(vec![provider]).unwrap();
    let cancel = CancellationToken::new();

    let mut progressive_lookup = test_lookup();
    progressive_lookup.wait_for_complete = false;
    progressive_lookup.startup_height = 720;

    let early_result = coordinator
        .resolve(progressive_lookup.clone(), &cancel)
        .await
        .expect("progressive lookup returns early");

    assert!(early_result.selected.is_some());
    assert_eq!(
        early_result
            .selected
            .as_ref()
            .unwrap()
            .candidate
            .observation
            .variant
            .as_ref()
            .unwrap()
            .height,
        Some(720),
        "startup should use the measured 720p rung without waiting for 1080p"
    );

    let mut complete_lookup = progressive_lookup;
    complete_lookup.wait_for_complete = true;
    let complete_result = coordinator
        .resolve(complete_lookup, &cancel)
        .await
        .expect("the same discovery flight completes higher-rung probes");
    assert_eq!(
        complete_result
            .selected
            .as_ref()
            .unwrap()
            .candidate
            .observation
            .variant
            .as_ref()
            .unwrap()
            .height,
        Some(1080),
        "the completed selection should expose the measured 1080p rung"
    );
}

#[tokio::test]
async fn coordinator_rejects_resolution_after_shutdown() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(TestProvider {
        id: ProviderId::new("vidcore").unwrap(),
        version: Version::new(1, 0, 0),
        generation: 1,
        call_count,
        delay: Duration::ZERO,
        fail: false,
    });

    let coordinator = create_coordinator(vec![provider]).unwrap();
    let lookup = test_lookup();

    coordinator.shutdown().await.expect("shutdown succeeds");

    let cancel = CancellationToken::new();
    let err = match coordinator.resolve(lookup, &cancel).await {
        Err(err) => err,
        Ok(_) => panic!("must reject after shutdown"),
    };
    assert_eq!(err, DiscoveryError::Closed);
}
