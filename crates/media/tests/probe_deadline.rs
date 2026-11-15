//! A cold master and rendition can each be healthy while exceeding the old
//! six-second combined budget. Virtual time keeps this regression deterministic.
use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use std::{sync::Arc, time::Duration};
use stream_core::providers::{CandidateStream, ErrorCategory};
use stream_media::{
    network::{FetchMetrics, FetchRequest, FetchResponse, TransportError},
    prober::{ProbeLimits, ProbeTransport, StreamProber},
};
use tokio_util::sync::CancellationToken;
use url::Url;

struct ColdPlaylists {
    stalled: bool,
    long_rendition: bool,
}
#[async_trait]
impl ProbeTransport for ColdPlaylists {
    async fn fetch(
        &self,
        request: FetchRequest,
        _: &CancellationToken,
    ) -> Result<FetchResponse, TransportError> {
        let url = Url::parse(&request.url).map_err(|_| TransportError::InvalidConfig)?;
        let body = match url.path() {
            "/master.m3u8" => {
                tokio::time::sleep(if self.stalled {
                    Duration::from_secs(30)
                } else {
                    Duration::from_millis(3500)
                })
                .await;
                Bytes::from_static(b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080\n1080.m3u8\n")
            }
            "/1080.m3u8" => {
                tokio::time::sleep(Duration::from_millis(3500)).await;
                let mut playlist = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:6\n");
                if self.long_rendition {
                    let query = "a".repeat(340);
                    for segment in 0..1_800 {
                        use std::fmt::Write;
                        writeln!(playlist, "#EXTINF:6,\nsegment-{segment}.ts?token={query}")
                            .map_err(|_| TransportError::InvalidConfig)?;
                    }
                } else {
                    playlist.push_str("#EXTINF:6,\nsegment.ts\n");
                }
                playlist.push_str("#EXT-X-ENDLIST\n");
                Bytes::from(playlist)
            }
            _ => {
                let mut bytes = vec![0; 188 * 10];
                for packet in bytes.chunks_mut(188) {
                    packet[0] = 0x47;
                    packet[1] = 0x01;
                    packet[2] = 0x00;
                    packet[3] = 0x10;
                }
                Bytes::from(bytes)
            }
        };
        if body.len() > request.max_response_bytes {
            return Err(TransportError::ResponseLimit);
        }
        Ok(FetchResponse {
            url,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body,
            metrics: FetchMetrics {
                admission_wait: Duration::ZERO,
                response_headers: Duration::from_millis(10),
                first_byte: Some(Duration::from_millis(10)),
                transfer: Duration::from_millis(100),
                first_chunk_bytes: 188,
                total: Duration::from_millis(110),
                redirects: 0,
            },
        })
    }
}
fn candidate() -> Result<CandidateStream, serde_json::Error> {
    serde_json::from_value(
        serde_json::json!({"id":"cold", "provider":"vidrock", "url":"https://cdn.example.com/master.m3u8", "format":"hls", "title":{"type":"tv","tmdb_id":1399,"season":2,"episode":5},"fetch_context":{},"resolver_metadata":{},"content_verified":false}),
    )
}
#[tokio::test(start_paused = true)]
async fn cold_playlists_reach_media_samples_within_default_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let prober = StreamProber::new(
        Arc::new(ColdPlaylists {
            stalled: false,
            long_rendition: false,
        }),
        ProbeLimits::default(),
    )?;
    let streams = prober
        .probe_all(vec![candidate()?], &CancellationToken::new())
        .await?;
    let stream = streams.first().ok_or("missing result")?;
    assert!(stream.observation.accessible);
    assert!(stream.observation.sample_bytes > 0);
    assert_eq!(
        stream.observation.variant.as_ref().and_then(|v| v.height),
        Some(1080)
    );
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn stalled_playlist_still_stops_at_the_source_deadline()
-> Result<(), Box<dyn std::error::Error>> {
    let limits = ProbeLimits::default();
    let deadline = limits.source_timeout;
    let prober = StreamProber::new(
        Arc::new(ColdPlaylists {
            stalled: true,
            long_rendition: false,
        }),
        limits,
    )?;
    let started = tokio::time::Instant::now();
    let streams = prober
        .probe_all(vec![candidate()?], &CancellationToken::new())
        .await?;
    assert_eq!(
        streams.first().ok_or("missing result")?.observation.failure,
        Some(ErrorCategory::Timeout)
    );
    assert_eq!(started.elapsed(), deadline);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn long_signed_movie_playlist_reaches_media_sampling()
-> Result<(), Box<dyn std::error::Error>> {
    let prober = StreamProber::new(
        Arc::new(ColdPlaylists {
            stalled: false,
            long_rendition: true,
        }),
        ProbeLimits::default(),
    )?;
    let streams = prober
        .probe_all(vec![candidate()?], &CancellationToken::new())
        .await?;
    let stream = streams.first().ok_or("missing result")?;
    assert!(stream.observation.accessible);
    assert!(stream.observation.sample_bytes > 0);
    Ok(())
}
