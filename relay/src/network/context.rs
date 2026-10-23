use crate::{
    contracts::providers::FetchContext,
    error::{Error, INVALID},
};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use url::Url;
fn matches(path: &str, prefix: &str) -> bool {
    prefix == "/"
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| prefix.ends_with('/') || rest.starts_with('/'))
}
pub fn headers(context: &FetchContext, target: &Url) -> Result<HeaderMap, Error> {
    crate::validation::context(context)?;
    let mut headers = HeaderMap::new();
    if let Some(public) = &context.public_resources {
        for (name, value) in [("origin", &public.origin), ("referer", &public.referer)] {
            if let Some(value) = value {
                headers.insert(
                    HeaderName::from_static(name),
                    HeaderValue::from_str(value).map_err(|_| INVALID)?,
                );
            }
        }
    }
    // Encoded separators/dot segments have server-dependent interpretation: never attach credentials.
    let ambiguous = target.path().contains('%') || target.path().contains("//");
    for scope in context.scopes.iter().flatten() {
        let origin = super::destination::destination(&scope.origin)?;
        if origin.origin() != target.origin()
            || ambiguous
            || !matches(target.path(), scope.path_prefix.as_deref().unwrap_or("/"))
        {
            continue;
        }
        for (name, value) in scope.headers.iter().flatten() {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| INVALID)?;
            let value = HeaderValue::from_str(value).map_err(|_| INVALID)?;
            if headers.get(&name).is_some_and(|old| old != value) {
                return Err(INVALID);
            }
            headers.insert(name, value);
        }
        if let Some(cookie) = &scope.cookies
            && matches(target.path(), scope.cookie_path.as_deref().ok_or(INVALID)?)
        {
            let value = HeaderValue::from_str(cookie).map_err(|_| INVALID)?;
            if headers.get("cookie").is_some_and(|old| old != value) {
                return Err(INVALID);
            }
            headers.insert("cookie", value);
        }
    }
    if headers.len() > 64
        || headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.len())
            .sum::<usize>()
            > 16384
    {
        return Err(INVALID);
    }
    Ok(headers)
}
