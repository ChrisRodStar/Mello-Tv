//! Validator grammar and unchanged representation header policy (RFC 9110).
use super::{MISMATCH, range::SYNTAX};
use crate::error::Error;
use axum::http::{HeaderMap, HeaderValue};
use std::time::SystemTime;

fn etag(value: &HeaderValue) -> bool {
    let bytes = value.as_bytes();
    let bytes = bytes.strip_prefix(b"W/").unwrap_or(bytes);
    bytes.len() >= 2
        && bytes[0] == b'"'
        && bytes[bytes.len() - 1] == b'"'
        && bytes[1..bytes.len() - 1]
            .iter()
            .all(|b| *b == 0x21 || (0x23..=0x7e).contains(b) || *b >= 0x80)
}
fn date(value: &HeaderValue) -> Option<SystemTime> {
    httpdate::parse_http_date(value.to_str().ok()?).ok()
}
fn single<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a HeaderValue> {
    (headers.get_all(name).iter().count() == 1)
        .then(|| headers.get(name))
        .flatten()
}
pub(super) fn if_range(headers: &HeaderMap, ranged: bool) -> Result<Option<HeaderValue>, Error> {
    // RFC 9110 13.1.5: ignore If-Range without Range.
    if !ranged {
        return Ok(None);
    }
    if headers.get_all("if-range").iter().count() > 1 {
        return Err(SYNTAX);
    }
    let Some(value) = headers.get("if-range") else {
        return Ok(None);
    };
    if !etag(value) && date(value).is_none() {
        return Err(SYNTAX);
    }
    Ok(Some(value.clone()))
}
pub(super) fn needs_date_evidence(value: &HeaderValue, headers: &HeaderMap) -> bool {
    date(value).is_some()
        && (!headers.contains_key("last-modified") || !headers.contains_key("date"))
}
pub(super) fn matches(value: &HeaderValue, headers: &HeaderMap) -> bool {
    if etag(value) {
        return !value.as_bytes().starts_with(b"W/")
            && single(headers, "etag").is_some_and(|tag| etag(tag) && tag == value);
    }
    let Some(modified) = single(headers, "last-modified").and_then(date) else {
        return false;
    };
    // As an intermediary, use the RFC's date strength rule. Origin dates from
    // the same response are compared; exact equality, never <=, is required.
    date(value) == Some(modified)
        && single(headers, "date")
            .and_then(date)
            .is_some_and(|now| now.duration_since(modified).is_ok_and(|d| d.as_secs() >= 1))
}
pub(super) fn consistent(first: &HeaderMap, second: &HeaderMap) -> bool {
    ["etag", "last-modified"]
        .iter()
        .all(|name| first.get_all(*name).iter().eq(second.get_all(*name).iter()))
}
pub(super) fn safe(headers: &HeaderMap) -> HeaderMap {
    let mut output = HeaderMap::new();
    if let Some(value) = single(headers, "etag").filter(|v| etag(v)) {
        output.insert("etag", value.clone());
    }
    for name in ["last-modified", "date"] {
        if let Some(value) = single(headers, name).filter(|v| date(v).is_some()) {
            output.insert(name, value.clone());
        }
    }
    // RFC 9110 7.6.1: nominated fields are hop-by-hop even if allowlisted.
    for connection in headers.get_all("connection") {
        if let Ok(value) = connection.to_str() {
            for name in value.split(',').map(str::trim) {
                output.remove(name);
            }
        }
    }
    output
}
// Evidence checks deliberately compare even invalid validators, but those
// values are never published as validators to clients.
pub(super) fn validate_duplicates(headers: &HeaderMap) -> Result<(), Error> {
    for name in ["etag", "last-modified", "date"] {
        if headers.get_all(name).iter().count() > 1 {
            return Err(MISMATCH);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn entity_tag_grammar_strength_dates_and_header_allowlist() {
        let mut headers = HeaderMap::new();
        for tag in ["\"\"", "\"a\\b\"", "W/\"v1\""] {
            headers.insert("etag", tag.parse().unwrap());
            assert_eq!(safe(&headers)["etag"], tag);
            assert_eq!(
                matches(&HeaderValue::from_str(tag).unwrap(), &headers),
                !tag.starts_with("W/")
            );
        }
        for tag in ["secret", "w/\"tag\"", "\"has space\"", "\"one\",\"two\""] {
            headers.insert("etag", tag.parse().unwrap());
            assert!(!safe(&headers).contains_key("etag"));
            assert!(!matches(&HeaderValue::from_str(tag).unwrap(), &headers));
        }
        headers.clear();
        headers.insert(
            "last-modified",
            "Sun, 06 Nov 1994 08:49:37 GMT".parse().unwrap(),
        );
        headers.insert("date", "Sun, 06 Nov 1994 08:49:38 GMT".parse().unwrap());
        assert!(matches(
            &HeaderValue::from_static("Sunday, 06-Nov-94 08:49:37 GMT"),
            &headers
        ));
        assert!(matches(
            &HeaderValue::from_static("Sun Nov  6 08:49:37 1994"),
            &headers
        ));
        assert!(!matches(
            &HeaderValue::from_static("Sun, 06 Nov 1994 08:49:38 GMT"),
            &headers
        ));
        headers.insert("date", "Sun, 06 Nov 1994 08:49:37 GMT".parse().unwrap());
        assert!(!matches(
            &HeaderValue::from_static("Sun, 06 Nov 1994 08:49:37 GMT"),
            &headers
        ));
        headers.append("date", "Sun, 06 Nov 1994 08:49:39 GMT".parse().unwrap());
        assert!(validate_duplicates(&headers).is_err());
        assert!(!safe(&headers).contains_key("date"));
        let mut request = HeaderMap::new();
        request.append("if-range", "\"a\"".parse().unwrap());
        request.append("if-range", "\"b\"".parse().unwrap());
        assert!(if_range(&request, true).is_err());
        assert!(if_range(&request, false).unwrap().is_none());
    }
}
