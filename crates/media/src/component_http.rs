//! Adapts sandboxed provider HTTP calls to the same destination-checked, pooled
//! transport used by native probes. Guests never receive an unrestricted client.

use std::collections::BTreeMap;

use async_trait::async_trait;
use stream_core::providers::{FetchContext, FetchScope};
use stream_provider_runtime::http::{
    HttpError, HttpPolicy, HttpRequest, HttpResponse, HttpTransport,
};
use tokio_util::sync::CancellationToken;

use crate::network::{FetchRequest, HttpClient, Method, TransportError, parse_destination_url};

#[derive(Clone, Debug)]
pub struct ComponentHttpTransport {
    client: HttpClient,
}

impl ComponentHttpTransport {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }
}

fn map_error(error: TransportError) -> HttpError {
    match error {
        TransportError::DestinationDenied | TransportError::RedirectDenied => HttpError::Denied,
        TransportError::InvalidConfig | TransportError::InvalidContext => HttpError::InvalidRequest,
        TransportError::Capacity | TransportError::ResponseLimit | TransportError::HeaderLimit => {
            HttpError::LimitExceeded
        }
        TransportError::Cancelled => HttpError::Cancelled,
        TransportError::Timeout => HttpError::Timeout,
        TransportError::Dns | TransportError::Request(_) => HttpError::UpstreamFailure,
    }
}

fn prepare(request: HttpRequest, policy: &HttpPolicy) -> Result<FetchRequest, HttpError> {
    let url = parse_destination_url(&request.url).map_err(map_error)?;
    let method =
        Method::from_bytes(request.method.as_bytes()).map_err(|_| HttpError::InvalidRequest)?;
    let mut scoped = BTreeMap::new();
    let mut internal = BTreeMap::new();
    let mut cookies = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in request.headers {
        let name = name.to_ascii_lowercase();
        if !seen.insert(name.clone()) {
            return Err(HttpError::InvalidRequest);
        }
        match name.as_str() {
            "content-type" | "range" | "if-range" => {
                internal.insert(name, value);
            }
            "accept-encoding" if value == "identity" => {
                internal.insert(name, value);
            }
            "cookie" => cookies = value,
            "user-agent"
            | "accept"
            | "accept-language"
            | "referer"
            | "origin"
            | "authorization"
            | "x-playback-environment"
            | "x-csrf-token" => {
                scoped.insert(name, value);
            }
            _ => return Err(HttpError::InvalidRequest),
        }
    }
    let path = url.path().to_owned();
    let context = FetchContext {
        scopes: vec![FetchScope {
            origin: url.origin().ascii_serialization(),
            path_prefix: path.clone(),
            headers: scoped,
            cookie_path: if cookies.is_empty() {
                String::new()
            } else {
                path
            },
            cookies,
        }],
        public_resources: None,
    };
    context.validate().map_err(|_| HttpError::InvalidRequest)?;
    let mut outgoing = FetchRequest::get(request.url);
    outgoing.method = method;
    outgoing.body = request.body.into();
    outgoing.context = context;
    outgoing.headers = internal;
    outgoing.max_response_bytes = policy.max_response_bytes;
    outgoing.allowed_origins = Some(policy.allowed_origins.to_vec());
    Ok(outgoing)
}

#[async_trait]
impl HttpTransport for ComponentHttpTransport {
    async fn fetch(
        &self,
        request: HttpRequest,
        policy: HttpPolicy,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        if policy.max_response_bytes == 0 {
            return Err(HttpError::LimitExceeded);
        }
        let mut outgoing = prepare(request, &policy)?;
        outgoing.max_response_bytes = outgoing
            .max_response_bytes
            .min(self.client.max_response_bytes());
        let response =
            tokio::time::timeout_at(policy.deadline, self.client.fetch(outgoing, &cancellation))
                .await
                .map_err(|_| HttpError::Timeout)?
                .map_err(map_error)?;
        let mut headers = Vec::with_capacity(response.headers.len());
        for (name, value) in &response.headers {
            headers.push((
                name.as_str().to_owned(),
                value
                    .to_str()
                    .map_err(|_| HttpError::UpstreamFailure)?
                    .to_owned(),
            ));
        }
        Ok(HttpResponse {
            url: response.url.into(),
            status: response.status.as_u16(),
            headers,
            body: response.body.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Duration};
    use stream_core::packages::AllowedOrigin;

    fn policy() -> Result<HttpPolicy, Box<dyn std::error::Error>> {
        Ok(HttpPolicy {
            allowed_origins: Arc::from([AllowedOrigin::new("https://example.com")?]),
            context_key: Arc::from("partition"),
            deadline: tokio::time::Instant::now() + Duration::from_secs(1),
            max_response_bytes: 1024,
        })
    }
    fn request(headers: Vec<(String, String)>) -> HttpRequest {
        HttpRequest {
            url: "https://example.com/api/resolve".into(),
            method: "POST".into(),
            headers,
            body: Vec::new(),
        }
    }
    #[test]
    fn credentials_are_bound_to_the_requested_origin_and_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = prepare(
            request(vec![
                ("Authorization".into(), "Bearer fixture".into()),
                ("Cookie".into(), "session=fixture".into()),
                ("Content-Type".into(), "application/json".into()),
            ]),
            &policy()?,
        )?;
        let scope = request.context.scopes.first().ok_or("missing scope")?;
        assert_eq!(scope.origin, "https://example.com");
        assert_eq!(scope.path_prefix, "/api/resolve");
        assert_eq!(scope.cookie_path, "/api/resolve");
        assert_eq!(
            request.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert!(request.allowed_origins.is_some());
        assert!(!format!("{request:?}").contains("fixture"));
        Ok(())
    }
    #[test]
    fn duplicate_and_hop_by_hop_headers_cannot_cross_host_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        for headers in [
            vec![("Host".into(), "local".into())],
            vec![
                ("Authorization".into(), "a".into()),
                ("authorization".into(), "b".into()),
            ],
            vec![("Accept-Encoding".into(), "gzip".into())],
        ] {
            assert!(prepare(request(headers), &policy()?).is_err());
        }
        Ok(())
    }
}
