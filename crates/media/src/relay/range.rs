use std::{collections::BTreeMap, io};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use reqwest::{Client, StatusCode, header};
use stream_core::providers::FetchContext;
use url::Url;

use super::stream::{ActivityGuard, ByteStream, RelayError, RelayResponse, upstream_error};
use crate::network::request_headers;

const CHUNK_BYTES: u64 = 1024 * 1024;

pub(super) fn open_range_start(range: &str) -> Option<u64> {
    let start = range.strip_prefix("bytes=")?.strip_suffix('-')?;
    if start.is_empty() || !start.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    start.parse().ok()
}

struct RangeSource {
    client: Client,
    url: Url,
    context: FetchContext,
    etag: Option<String>,
}

struct Chunk {
    bytes: Bytes,
    end: u64,
    total: u64,
    content_type: String,
    etag: Option<String>,
}

impl RangeSource {
    async fn chunk(&self, start: u64, expected_total: Option<u64>) -> Result<Chunk, RelayError> {
        let end = start.saturating_add(CHUNK_BYTES - 1);
        let end = expected_total.map_or(end, |total| end.min(total - 1));
        let mut request = BTreeMap::from([("range".into(), format!("bytes={start}-{end}"))]);
        if let Some(etag) = &self.etag {
            request.insert("if-range".into(), etag.clone());
        }
        let headers = request_headers(&self.context, &self.url, &request)
            .map_err(|error| RelayError::Upstream(error.to_string()))?;
        let res = self
            .client
            .get(self.url.clone())
            .headers(headers)
            .send()
            .await
            .map_err(upstream_error)?;
        if res.status() != StatusCode::PARTIAL_CONTENT {
            return Err(RelayError::UpstreamStatus(res.status().as_u16()));
        }
        let invalid_range = || RelayError::Upstream("invalid bounded media range".into());
        let (actual_start, actual_end, total) = res
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range)
            .ok_or_else(invalid_range)?;
        if actual_start != start
            || actual_end != end.min(total.saturating_sub(1))
            || actual_end < start
            || actual_end >= total
            || expected_total.is_some_and(|expected| expected != total)
        {
            return Err(invalid_range());
        }
        let etag = res
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.starts_with('"'))
            .map(str::to_owned);
        if self.etag.is_some() && self.etag != etag {
            return Err(invalid_range());
        }
        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let length = (actual_end - start + 1) as usize;
        let mut bytes = bytes::BytesMut::with_capacity(length);
        let mut body = res.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(upstream_error)?;
            if chunk.len() > length.saturating_sub(bytes.len()) {
                return Err(invalid_range());
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != length {
            return Err(invalid_range());
        }
        Ok(Chunk {
            bytes: bytes.freeze(),
            end: actual_end,
            total,
            content_type,
            etag,
        })
    }
}

fn parse_content_range(header: &str) -> Option<(u64, u64, u64)> {
    let (boundaries, total) = header.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = boundaries.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

pub(super) async fn relay_open_range(
    client: Client,
    url: Url,
    context: FetchContext,
    start: u64,
    activity: ActivityGuard,
) -> Result<RelayResponse, RelayError> {
    let mut source = RangeSource {
        client,
        url,
        context,
        etag: None,
    };
    let first = source.chunk(start, None).await?;
    let total = first.total;
    source.etag = first.etag;
    let trailing = stream::try_unfold((source, first.end + 1), move |(source, next)| async move {
        if next == total {
            return Ok(None);
        }
        let chunk = source
            .chunk(next, Some(total))
            .await
            .map_err(io::Error::other)?;
        Ok(Some((chunk.bytes, (source, chunk.end + 1))))
    });
    let body = stream::once(async move { Ok(first.bytes) }).chain(trailing);
    let stream: ByteStream = Box::pin(body.map(move |result| {
        let _keep_delivery_active = &activity;
        result
    }));
    Ok(RelayResponse::Binary {
        status: StatusCode::PARTIAL_CONTENT.as_u16(),
        content_type: first.content_type,
        content_length: Some(total - start),
        content_range: Some(format!("bytes {start}-{}/{total}", total - 1)),
        accept_ranges: Some("bytes".into()),
        stream,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn relay_retries_open_range_and_streams_contiguous_slices()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::relay::MediaRelay;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let start = 5;
        let total = start + CHUNK_BYTES + 3;
        let server = tokio::spawn(async move {
            for (requested, response, byte, count) in [
                (
                    format!("bytes={start}-"),
                    "HTTP/1.1 428 Precondition Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
                    0,
                    0,
                ),
                (
                    format!("bytes={start}-{}", start + CHUNK_BYTES - 1),
                    partial_response(start, start + CHUNK_BYTES - 1, total),
                    b'A',
                    CHUNK_BYTES,
                ),
                (
                    format!("bytes={}-{}", start + CHUNK_BYTES, total - 1),
                    partial_response(start + CHUNK_BYTES, total - 1, total),
                    b'B',
                    3,
                ),
            ] {
                let (mut socket, _) = listener.accept().await?;
                let mut request = Vec::new();
                let mut byte_buffer = [0; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte_buffer).await?;
                    request.push(byte_buffer[0]);
                }
                assert!(String::from_utf8_lossy(&request).contains(&format!("range: {requested}\r\n")));
                socket.write_all(response.as_bytes()).await?;
                socket.write_all(&vec![byte; count as usize]).await?;
            }
            Ok::<_, io::Error>(())
        });
        let relay = MediaRelay::new();
        let response = relay
            .proxy_url(
                "test",
                "token",
                &format!("http://{address}/media.mp4"),
                &FetchContext::default(),
                Some("bytes=5-"),
            )
            .await?;
        let RelayResponse::Binary {
            status,
            content_range,
            content_length,
            mut stream,
            ..
        } = response
        else {
            return Err("expected binary response".into());
        };
        assert_eq!(status, 206);
        assert_eq!(
            content_range,
            Some(format!("bytes {start}-{}/{total}", total - 1))
        );
        assert_eq!(content_length, Some(CHUNK_BYTES + 3));
        assert_eq!(relay.active_streams(), Some(1));
        let first = stream.next().await.ok_or("missing first slice")??;
        assert_eq!(first.len() as u64, CHUNK_BYTES);
        assert!(first.iter().all(|byte| *byte == b'A'));
        assert_eq!(
            stream.next().await.ok_or("missing final slice")??.as_ref(),
            b"BBB"
        );
        assert!(stream.next().await.is_none());
        server.await??;
        Ok(())
    }

    fn partial_response(start: u64, end: u64, total: u64) -> String {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            end - start + 1,
        )
    }
}
