use super::TransportError;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::BTreeMap;
use stream_core::providers::FetchContext;
use url::Url;

pub(super) fn request_headers(
    context: &FetchContext,
    target: &Url,
    internal: &BTreeMap<String, String>,
) -> Result<HeaderMap, TransportError> {
    context
        .validate()
        .map_err(|_| TransportError::InvalidContext)?;
    let mut headers = HeaderMap::new();
    headers.insert("accept-encoding", HeaderValue::from_static("identity"));
    if let Some(public) = &context.public_resources {
        for (name, value) in [("origin", &public.origin), ("referer", &public.referer)] {
            if !value.is_empty() {
                insert(&mut headers, name, value, true)?;
            }
        }
    }
    let ambiguous = target.path().contains('%') || target.path().contains("//");
    for scope in &context.scopes {
        let origin = Url::parse(&scope.origin).map_err(|_| TransportError::InvalidContext)?;
        let prefix = if scope.path_prefix.is_empty() {
            "/"
        } else {
            &scope.path_prefix
        };
        if ambiguous || origin.origin() != target.origin() || !path_matches(target.path(), prefix) {
            continue;
        }
        for (name, value) in &scope.headers {
            insert(&mut headers, name, value, true)?;
        }
        if !scope.cookies.is_empty() && path_matches(target.path(), &scope.cookie_path) {
            insert(&mut headers, "cookie", &scope.cookies, true)?;
        }
    }
    if internal.len() > 16 {
        return Err(TransportError::InvalidContext);
    }
    for (name, value) in internal {
        if !matches!(
            name.to_ascii_lowercase().as_str(),
            "accept" | "accept-encoding" | "content-type" | "range" | "if-range"
        ) {
            return Err(TransportError::InvalidContext);
        }
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransportError::InvalidContext)?;
        let value = HeaderValue::from_str(value).map_err(|_| TransportError::InvalidContext)?;
        headers.insert(name, value);
    }
    if headers.len() > 64
        || headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.len())
            .sum::<usize>()
            > 16_384
    {
        return Err(TransportError::InvalidContext);
    }
    Ok(headers)
}
fn insert(
    headers: &mut HeaderMap,
    name: &str,
    value: &str,
    sensitive: bool,
) -> Result<(), TransportError> {
    let name =
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransportError::InvalidContext)?;
    let mut value = HeaderValue::from_str(value).map_err(|_| TransportError::InvalidContext)?;
    value.set_sensitive(sensitive);
    if headers.get(&name).is_some_and(|previous| previous != value) {
        return Err(TransportError::InvalidContext);
    }
    headers.insert(name, value);
    Ok(())
}
fn path_matches(path: &str, prefix: &str) -> bool {
    prefix == "/"
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| prefix.ends_with('/') || rest.starts_with('/'))
}
