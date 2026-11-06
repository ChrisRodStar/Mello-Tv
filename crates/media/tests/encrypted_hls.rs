//! Encrypted sources must prove decrypted media, not just return opaque bytes.
use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, StatusCode};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use stream_core::providers::{CandidateStream, ErrorCategory};
use stream_media::{
    hls::parse_playlist,
    network::{FetchMetrics, FetchRequest, FetchResponse, TransportError},
    prober::{ProbeLimits, ProbeTransport, StreamProber},
};
use tokio_util::sync::CancellationToken;
use url::Url;

const KEY: &[u8; 16] = b"0123456789abcdef";
#[derive(Clone, Copy)]
enum KeyResponse {
    Valid,
    Wrong,
    Missing,
    Short,
}
struct EncryptedTransport {
    response: KeyResponse,
    key_calls: AtomicUsize,
}
#[async_trait]
impl ProbeTransport for EncryptedTransport {
    async fn fetch(
        &self,
        request: FetchRequest,
        _: &CancellationToken,
    ) -> Result<FetchResponse, TransportError> {
        let url = Url::parse(&request.url).map_err(|_| TransportError::InvalidConfig)?;
        let mut status = StatusCode::OK;
        let (body, mime) = if url.path() == "/media.m3u8" {
            (Bytes::from_static(b"#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:6,\n7.ts\n#EXTINF:6,\n8.ts\n#EXT-X-ENDLIST\n"), "application/vnd.apple.mpegurl")
        } else if url.path() == "/key.bin" {
            self.key_calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.max_response_bytes, 16);
            let bytes: &[u8] = match self.response {
                KeyResponse::Valid => KEY,
                KeyResponse::Wrong => b"fedcba9876543210",
                KeyResponse::Missing => {
                    status = StatusCode::FORBIDDEN;
                    b""
                }
                KeyResponse::Short => b"short",
            };
            (Bytes::copy_from_slice(bytes), "application/octet-stream")
        } else {
            assert!(request.sample);
            assert!(request.headers.contains_key("Range"));
            let sequence = if url.path() == "/7.ts" { 7u128 } else { 8u128 };
            let mut plaintext = vec![0; 188 * 128];
            for packet in plaintext.chunks_mut(188) {
                packet[..4].copy_from_slice(&[0x47, 0x01, 0, 0x10]);
            }
            let encrypted =
                stream_provider_codecs::aes_cbc_encrypt(KEY, &sequence.to_be_bytes(), &plaintext)
                    .map_err(|_| TransportError::InvalidConfig)?;
            (Bytes::from(encrypted), "text/html")
        };
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static(mime));
        Ok(FetchResponse {
            url,
            status,
            headers,
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
        serde_json::json!({"id":"encrypted", "provider":"vixsrc", "url":"https://cdn.example.com/media.m3u8", "format":"hls", "title":{"type":"movie","tmdb_id":27205},"fetch_context":{},"resolver_metadata":{},"content_verified":false}),
    )
}
#[tokio::test]
async fn encrypted_text_mime_is_accepted_only_after_valid_key_decryption()
-> Result<(), Box<dyn std::error::Error>> {
    for (response, expected) in [
        (KeyResponse::Valid, None),
        (KeyResponse::Wrong, Some(ErrorCategory::ProtocolChange)),
        (KeyResponse::Missing, Some(ErrorCategory::UpstreamFailure)),
        (KeyResponse::Short, Some(ErrorCategory::ProtocolChange)),
    ] {
        let transport = Arc::new(EncryptedTransport {
            response,
            key_calls: AtomicUsize::new(0),
        });
        let prober = StreamProber::new(transport.clone(), ProbeLimits::default())?;
        let streams = prober
            .probe_all(vec![candidate()?], &CancellationToken::new())
            .await?;
        let stream = streams.first().ok_or("missing result")?;
        assert_eq!(stream.observation.failure, expected);
        assert_eq!(stream.observation.accessible, expected.is_none());
        assert!(stream.observation.encrypted);
        assert_eq!(transport.key_calls.load(Ordering::SeqCst), 1);
        if expected.is_none() {
            assert_eq!(stream.observation.samples_completed, 2);
            assert!(stream.observation.delivery_bps.is_some());
        }
    }
    Ok(())
}
#[test]
fn key_rotation_sequence_iv_and_clear_segments_keep_their_associations()
-> Result<(), Box<dyn std::error::Error>> {
    let base = Url::parse("https://cdn.example.com/media.m3u8")?;
    let playlist = parse_playlist(b"#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"first.key\"\n#EXTINF:6,\n7.ts\n#EXTINF:6,\n8.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"second.key\",IV=0xabc\n#EXTINF:6,\n9.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:6,\n10.ts\n", &base)?;
    assert_eq!(
        playlist.samples[0].encryption.as_ref().map(|e| e.iv),
        Some(7u128.to_be_bytes())
    );
    assert_eq!(
        playlist.samples[1].encryption.as_ref().map(|e| e.iv),
        Some(8u128.to_be_bytes())
    );
    assert_eq!(
        playlist.samples[2].encryption.as_ref().map(|e| e.iv),
        Some(0xabcu128.to_be_bytes())
    );
    assert_eq!(
        playlist.samples[2]
            .encryption
            .as_ref()
            .map(|e| e.key_url.as_ref()),
        Some("https://cdn.example.com/second.key")
    );
    assert!(playlist.samples[3].encryption.is_none());
    assert!(parse_playlist(b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"http://127.0.0.1/private.key\"\n#EXTINF:6,\na.ts\n", &base).is_err());
    assert!(
        parse_playlist(
            b"#EXTM3U\n#EXTINF:6,\na.ts\n#EXT-X-MEDIA-SEQUENCE:7\n",
            &base
        )
        .is_err()
    );
    Ok(())
}
