//! Production TUF transport using the shared guarded egress policy. Large
//! application artifacts stream to staging instead of buffering in memory.

use async_trait::async_trait;
use futures_util::StreamExt;
use std::{fmt, sync::Arc};
use stream_core::packages::AllowedOrigin;
use stream_media::network::{FetchRequest, HttpClient};
use tokio_util::sync::CancellationToken;
use tough::{Transport, TransportError, TransportErrorKind, TransportStream};
use url::Url;

#[derive(Clone)]
pub struct UpdateTransport {
    client: HttpClient,
    origins: Arc<[AllowedOrigin]>,
    metadata: Url,
}
impl fmt::Debug for UpdateTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateTransport").finish_non_exhaustive()
    }
}
impl UpdateTransport {
    /// Configure a separate pooled client for updates. Its response cap may be
    /// up to256MiB while discovery clients retain their much smaller defaults.
    pub fn new(
        client: HttpClient,
        metadata: Url,
        origins: Vec<AllowedOrigin>,
    ) -> Result<Self, super::UpdateError> {
        if origins.is_empty()
            || origins.len() > 32
            || metadata.scheme() != "https"
            || !metadata.path().ends_with('/')
            || !origins
                .iter()
                .any(|origin| origin.as_str() == metadata.origin().ascii_serialization())
        {
            return Err(super::UpdateError::Invalid);
        }
        Ok(Self {
            client,
            metadata,
            origins: origins.into(),
        })
    }
}
#[async_trait]
impl Transport for UpdateTransport {
    async fn fetch(&self, url: Url) -> Result<TransportStream, TransportError> {
        let mut request = FetchRequest::get(url.as_str());
        request.allowed_origins = Some(self.origins.to_vec());
        let metadata = url.origin() == self.metadata.origin()
            && url.path().starts_with(self.metadata.path())
            && url.path().ends_with(".json");
        request.max_response_bytes = if metadata {
            1024 * 1024
        } else {
            self.client.max_response_bytes()
        };
        let response = self
            .client
            .fetch_stream(request, &CancellationToken::new())
            .await
            .map_err(|error| {
                TransportError::new_with_cause(
                    TransportErrorKind::Other,
                    "configured update repository",
                    error,
                )
            })?;
        if response.status.as_u16() != 200 {
            let kind = if matches!(response.status.as_u16(), 404 | 410) {
                TransportErrorKind::FileNotFound
            } else {
                TransportErrorKind::Other
            };
            return Err(TransportError::new(kind, "configured update repository"));
        }
        Ok(Box::pin(response.body.map(|chunk| {
            chunk.map_err(|error| {
                TransportError::new_with_cause(
                    TransportErrorKind::Other,
                    "configured update repository",
                    error,
                )
            })
        })))
    }
}
