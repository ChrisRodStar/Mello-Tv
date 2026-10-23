//! Shared HLS and progressive MP4 delivery primitives. Only manifests, keys and a prefix are collected.
pub mod connection;
pub mod range;
mod recovery;
mod validators;
use crate::{
    config::Config,
    error::{Error, STORE},
    hls,
    network::{Fetch, NetError, Transport, Upstream},
    session::{Authorized, ResourceKind, Store},
};
use axum::{
    body::Body,
    http::{HeaderMap, Method, Response, StatusCode},
    response::IntoResponse,
};
use hyper::body::{Bytes, Frame};
use range::Range;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::Instant;
const REJECTED: Error = Error(StatusCode::BAD_GATEWAY, "upstream_rejected");
pub const MISMATCH: Error = Error(StatusCode::BAD_GATEWAY, "range_mismatch");
struct Evidence {
    offset: u64,
    total: u64,
    bytes: Vec<u8>,
    validators: HeaderMap,
}
fn prefix_need(kind: &ResourceKind) -> usize {
    if *kind == ResourceKind::Segment {
        380
    } else {
        16
    }
}
#[derive(Clone)]
pub struct Delivery {
    pub config: Arc<Config>,
    pub store: Store,
    pub transport: Arc<Transport>,
}
fn network(error: NetError) -> Error {
    match error {
        NetError::Guard => Error(StatusCode::SERVICE_UNAVAILABLE, "vpn_unavailable"),
        NetError::Timeout => Error(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"),
        NetError::Limit => Error(StatusCode::TOO_MANY_REQUESTS, "request_limit"),
        NetError::Cancelled => Error(StatusCode::REQUEST_TIMEOUT, "stream_cancelled"),
        _ => REJECTED,
    }
}
pub fn lease_deadline(expires_at: i64) -> Result<Instant, Error> {
    let remaining = chrono::DateTime::from_timestamp(expires_at, 0)
        .ok_or(Error(StatusCode::GONE, "session_expired"))?
        .signed_duration_since(chrono::Utc::now())
        .to_std()
        .map_err(|_| Error(StatusCode::GONE, "session_expired"))?;
    Ok(Instant::now() + remaining)
}
impl Delivery {
    pub async fn serve(
        &self,
        sid: &str,
        grant: &str,
        authorized: Authorized,
        method: Method,
        headers: &HeaderMap,
    ) -> Result<axum::response::Response, Error> {
        let _time = crate::health::Timer::start(crate::health::Stage::Delivery);
        if authorized.cancelled.is_cancelled() {
            return Err(Error(StatusCode::GONE, "session_expired"));
        }
        let deadline = lease_deadline(authorized.expires_at)?;
        let cancel = authorized.cancelled.clone();
        // Transport waits observe cancellation. Let a queued store transaction
        // settle: poisoning cancels readers before its safe error is delivered.
        let result = tokio::time::timeout_at(
            deadline,
            self.serve_inner(sid, grant, authorized, method, headers, deadline),
        )
        .await
        .map_err(|_| Error(StatusCode::GONE, "session_expired"))?;
        if result.is_ok() && (cancel.is_cancelled() || Instant::now() >= deadline) {
            return Err(Error(StatusCode::GONE, "session_expired"));
        }
        result
    }
    async fn serve_inner(
        &self,
        sid: &str,
        grant: &str,
        authorized: Authorized,
        method: Method,
        headers: &HeaderMap,
        deadline: Instant,
    ) -> Result<axum::response::Response, Error> {
        let registration = authorized.registration.as_ref().ok_or(STORE)?;
        let mut requested = Range::parse(headers)?;
        let mp4 = registration.kind == ResourceKind::Mp4;
        let manifest = registration.kind == ResourceKind::Manifest;
        let key = registration.kind == ResourceKind::Key;
        let deadline = deadline.min(
            Instant::now()
                + Duration::from_secs(if manifest {
                    self.config.network.playlist_timeout_s
                } else if mp4 {
                    self.config.network.mp4_max_transfer_s
                } else {
                    self.config.network.segment_timeout_s
                }),
        );
        // A player may send bytes=0- while opening any HTTP input. Rewritten
        // manifests and small validated keys are complete representations: ignore
        // a syntactically valid Range rather than forward it or reject startup.
        if manifest || key {
            requested = None;
        }
        let conditional = validators::if_range(headers, requested.is_some())?;
        let mut discovered_total = None;
        let mut discovered_validators = None;
        if let Some(limit) = &registration.range {
            if let Some(r) = requested {
                let r = if matches!(r, Range::Open(_) | Range::Suffix(_)) {
                    let (total, metadata) = self
                        .metadata(registration, sid, &authorized, deadline)
                        .await?;
                    let Ok((a, b)) = r.resolve(total) else {
                        return unsatisfiable(total);
                    };
                    if Range::Closed(a, b).constrain(limit).is_err() {
                        return unsatisfiable(total);
                    }
                    discovered_total = Some(total);
                    discovered_validators = Some(metadata);
                    Range::Closed(a, b)
                } else {
                    r
                };
                r.constrain(limit)?;
                requested = Some(r);
            } else if method == Method::GET {
                return Err(Error(StatusCode::BAD_REQUEST, "range_required"));
            }
        }
        let range = requested.map(Range::wire);
        let metadata_only =
            method == Method::HEAD && registration.range.is_some() && requested.is_none();
        let timeout = if manifest {
            self.config.network.playlist_timeout_s
        } else if mp4 {
            self.config.network.mp4_max_transfer_s
        } else {
            self.config.network.segment_timeout_s
        };
        let needs_evidence = !registration.encrypted && requested.is_some_and(|range| {
            mp4 || registration.kind == ResourceKind::Subtitle || matches!(range, Range::Closed(0,end) if end < (prefix_need(&registration.kind) - 1) as u64)
        });
        let mut evidence = if needs_evidence && conditional.is_none() {
            Some(
                self.evidence(registration, sid, &authorized, deadline)
                    .await?,
            )
        } else {
            None
        };
        let mut upstream = self
            .transport
            .fetch(Fetch {
                url: &registration.uri,
                method: if manifest || key || !metadata_only {
                    Method::GET
                } else {
                    method.clone()
                },
                context: &registration.context,
                session_id: sid,
                deadline: deadline.min(Instant::now() + Duration::from_secs(timeout)),
                cancel: authorized.cancelled.clone(),
                range: range.as_deref(),
                if_range: conditional.as_ref(),
            })
            .await
            .map_err(network)?;
        identity(&upstream.headers)?;
        if manifest {
            if upstream.status != StatusCode::OK {
                return Err(REJECTED);
            }
            let final_url = upstream.url.clone();
            let bytes = collect(
                &mut upstream,
                self.config.limits.max_playlist_bytes as usize,
            )
            .await?;
            drop(upstream);
            let _manifest_time = crate::health::Timer::start(crate::health::Stage::Manifest);
            let plan = hls::parse(
                &bytes,
                &final_url,
                registration,
                self.config.limits.max_playlist_depth_count as usize,
                self.config.limits.max_manifest_registrations_count as usize,
                self.config.state.max_snapshot_bytes as usize,
            )?;
            // A single store transaction is the publication barrier for every reference.
            let ids = if plan.registrations.is_empty() {
                vec![]
            } else {
                self.store
                    .register(
                        sid.into(),
                        authorized.generation,
                        plan.registrations.clone(),
                    )
                    .await?
            };
            let references = plan.registrations.clone();
            let output = plan.publish(
                &ids,
                sid,
                grant,
                self.config.limits.max_playlist_bytes as usize,
            )?;
            redact(&output, registration, &final_url, &references)?;
            if authorized.cancelled.is_cancelled() || Instant::now() >= deadline {
                return Err(Error(StatusCode::GONE, "session_expired"));
            }
            return response(
                StatusCode::OK,
                "application/vnd.apple.mpegurl",
                output.len() as u64,
                if method == Method::HEAD {
                    Body::empty()
                } else {
                    Body::from(output)
                },
                None,
            );
        }
        if key {
            if upstream.status != StatusCode::OK {
                return Err(REJECTED);
            }
            let bytes = collect(
                &mut upstream,
                self.config.limits.max_key_bytes.min(16) as usize,
            )
            .await?;
            if bytes.len() != 16 {
                return Err(REJECTED);
            }
            return response(
                StatusCode::OK,
                "application/octet-stream",
                16,
                if method == Method::HEAD {
                    Body::empty()
                } else {
                    Body::from(bytes)
                },
                None,
            );
        }
        if needs_evidence && conditional.is_some() && upstream.status == StatusCode::PARTIAL_CONTENT
        {
            evidence = Some(
                self.evidence(registration, sid, &authorized, deadline)
                    .await?,
            );
        }
        // Conditional 206 responses may omit Last-Modified (RFC 9110 15.3.7).
        // Enrich only our comparison context from bounded evidence, never the
        // published response headers or the transport's trailer comparison state.
        let mut comparison = upstream.headers.clone();
        if upstream.status == StatusCode::PARTIAL_CONTENT && conditional.is_some() {
            let reference = if let Some(e) = &evidence {
                Some((e.total, e.validators.clone()))
            } else if let (Some(total), Some(headers)) = (discovered_total, &discovered_validators)
            {
                Some((total, headers.clone()))
            } else if conditional
                .as_ref()
                .is_some_and(|c| validators::needs_date_evidence(c, &comparison))
            {
                Some(
                    self.metadata(registration, sid, &authorized, deadline)
                        .await?,
                )
            } else {
                None
            };
            if let Some((total, headers)) = reference {
                if range::content_range(&upstream.headers)?.2 != total {
                    return Err(MISMATCH);
                }
                for name in ["last-modified", "date"] {
                    if !comparison.contains_key(name)
                        && let Some(value) = headers.get(name)
                    {
                        comparison.insert(name, value.clone());
                    }
                }
                if !validators::consistent(&headers, &comparison) {
                    return Err(MISMATCH);
                }
            }
        }
        if let Some(condition) = &conditional {
            let matches = validators::matches(condition, &comparison);
            if upstream.status == StatusCode::PARTIAL_CONTENT && !matches {
                return Err(MISMATCH);
            }
            if upstream.status == StatusCode::OK && requested.is_some() {
                if registration.range.is_some() || matches {
                    return Err(MISMATCH);
                }
                requested = None;
                evidence = None;
            }
        }
        if upstream.status == StatusCode::RANGE_NOT_SATISFIABLE {
            if upstream.headers.get_all("content-range").iter().count() != 1 {
                return Err(MISMATCH);
            }
            let value = upstream
                .headers
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .ok_or(MISMATCH)?;
            let total = value
                .strip_prefix("bytes */")
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or(MISMATCH)?;
            if requested.is_none_or(|r| r.resolve(total).is_ok())
                || evidence.as_ref().is_some_and(|e| e.total != total)
                || discovered_total.is_some_and(|n| n != total)
            {
                return Err(MISMATCH);
            }
            if let Some(condition) = &conditional
                && !validators::matches(condition, &upstream.headers)
            {
                // 416 need not carry representation validators (RFC 9110 15.5.17).
                // Release its reader/slot before bounded independent discovery.
                let reported = upstream.headers.clone();
                drop(upstream);
                let (current_total, metadata) = self
                    .metadata(registration, sid, &authorized, deadline)
                    .await?;
                if current_total != total
                    || !validators::matches(condition, &metadata)
                    || ["etag", "last-modified"].iter().any(|name| {
                        reported
                            .get(*name)
                            .is_some_and(|value| metadata.get(*name) != Some(value))
                    })
                {
                    return Err(MISMATCH);
                }
            }
            return unsatisfiable(total);
        }
        let length = length(&upstream.headers)?;
        let (start, expected, content_range) = if let Some(r) = requested {
            if upstream.status != StatusCode::PARTIAL_CONTENT {
                return Err(MISMATCH);
            }
            let (a, b, total) = range::content_range(&upstream.headers)?;
            if evidence.as_ref().is_some_and(|e| {
                e.total != total || !validators::consistent(&e.validators, &comparison)
            }) {
                return Err(MISMATCH);
            }
            if discovered_total.is_some_and(|n| n != total)
                || discovered_validators
                    .as_ref()
                    .is_some_and(|h| !validators::consistent(h, &comparison))
            {
                return Err(MISMATCH);
            }
            if r.resolve(total)? != (a, b) {
                return Err(MISMATCH);
            }
            let n = b
                .checked_sub(a)
                .and_then(|n| n.checked_add(1))
                .ok_or(MISMATCH)?;
            if length.is_some_and(|l| l != n) {
                return Err(MISMATCH);
            }
            (a, Some(n), Some(format!("bytes {a}-{b}/{total}")))
        } else {
            if upstream.status != StatusCode::OK || upstream.headers.contains_key("content-range") {
                return Err(REJECTED);
            }
            (0, length, None)
        };
        let status = upstream.status;
        let mime = match registration.kind {
            ResourceKind::Segment => "video/mp2t",
            ResourceKind::Init | ResourceKind::Fragment | ResourceKind::Mp4 => "video/mp4",
            ResourceKind::Subtitle => "text/vtt",
            _ => return Err(REJECTED),
        };
        // HEAD uses the same bounded prefix evidence, closing without consuming
        // media. A constrained HEAD without Range is metadata-only, as specified.
        let prefix = if metadata_only {
            Bytes::new()
        } else {
            let mut bytes = Vec::new();
            let need = if evidence.is_some() {
                expected
                    .ok_or(MISMATCH)?
                    .min(prefix_need(&registration.kind) as u64) as usize
            } else if registration.encrypted || start > 0 {
                1
            } else {
                prefix_need(&registration.kind)
            };
            let cap = self.config.limits.max_sniff_bytes as usize;
            if need > cap {
                return Err(REJECTED);
            }
            while bytes.len() < need {
                let Some(mut chunk) = upstream.chunk().await.map_err(network)? else {
                    break;
                };
                let count = chunk.len().min(need - bytes.len());
                bytes.extend_from_slice(&chunk.split_to(count));
                upstream.replay(chunk);
            }
            if let Some(e) = &evidence {
                let first = start.max(e.offset);
                let last = start
                    .checked_add(bytes.len() as u64)
                    .ok_or(MISMATCH)?
                    .min(e.offset.checked_add(e.bytes.len() as u64).ok_or(MISMATCH)?);
                if first < last
                    && bytes[(first - start) as usize..(last - start) as usize]
                        != e.bytes[(first - e.offset) as usize..(last - e.offset) as usize]
                {
                    return Err(MISMATCH);
                }
            } else {
                validate_prefix(&bytes, &registration.kind, registration.encrypted, start)?;
            }
            Bytes::from(bytes)
        };
        if expected.is_some_and(|n| prefix.len() as u64 > n) {
            return Err(MISMATCH);
        }
        let mut safe_headers = validators::safe(&upstream.headers);
        let private = private_values(registration, &upstream.url, &[])?;
        if safe_headers.get("etag").is_some_and(|tag| {
            tag.to_str().map_or(true, |value| {
                value.contains("://")
                    || upstream
                        .url
                        .host_str()
                        .is_some_and(|host| value.contains(host))
                    || private.iter().any(|secret| private_match(value, secret))
            })
        }) {
            safe_headers.remove("etag");
        }
        let body = if method == Method::HEAD {
            Body::empty()
        } else {
            Body::new(StreamBody {
                next: None,
                upstream: Some(upstream),
                prefix: Some(prefix),
                count: 0,
                expected,

                deadline,

                done: false,
                final_chunk: None,
                utf8: if registration.kind == ResourceKind::Subtitle && requested.is_none() {
                    Some(Utf8::default())
                } else {
                    None
                },
                authorized: Some(authorized),
            })
        };
        let mut response = Response::builder()
            .status(status)
            .header("content-type", mime)
            .header("accept-ranges", "bytes");
        if let Some(n) = expected {
            response = response.header("content-length", n);
        }
        if let Some(value) = content_range {
            response = response.header("content-range", value);
        }
        let mut response = response.body(body).map_err(|_| REJECTED)?;
        response.headers_mut().extend(safe_headers);
        Ok(response)
    }
    async fn metadata(
        &self,
        registration: &crate::session::Registration,
        sid: &str,
        authorized: &Authorized,
        deadline: Instant,
    ) -> Result<(u64, HeaderMap), Error> {
        let metadata = self
            .transport
            .fetch(Fetch {
                url: &registration.uri,
                method: Method::HEAD,
                context: &registration.context,
                session_id: sid,
                deadline,
                cancel: authorized.cancelled.clone(),
                range: None,
                if_range: None,
            })
            .await
            .map_err(network)?;
        if metadata.status == StatusCode::OK && !metadata.headers.contains_key("content-range") {
            identity(&metadata.headers)?;
            if let Some(total) = length(&metadata.headers)? {
                return Ok((total, metadata.headers.clone()));
            }
        }
        drop(metadata);
        // HEAD-unavailable discovery consumes exactly one byte inside the
        // capability; no full GET, prefix draining or container classification.
        let offset = registration.range.as_ref().map_or(0, |r| r.offset);
        let wire = Range::Closed(offset, offset).wire();
        let mut metadata = self
            .transport
            .fetch(Fetch {
                url: &registration.uri,
                method: Method::GET,
                context: &registration.context,
                session_id: sid,
                deadline,
                cancel: authorized.cancelled.clone(),
                range: Some(&wire),
                if_range: None,
            })
            .await
            .map_err(network)?;
        identity(&metadata.headers)?;
        if metadata.status != StatusCode::PARTIAL_CONTENT {
            return Err(MISMATCH);
        }
        let (a, b, total) = range::content_range(&metadata.headers)?;
        if (a, b) != (offset, offset)
            || length(&metadata.headers)?.is_some_and(|n| n != 1)
            || collect(&mut metadata, 1).await?.len() != 1
        {
            return Err(MISMATCH);
        }
        Ok((total, metadata.headers.clone()))
    }
    async fn evidence(
        &self,
        registration: &crate::session::Registration,
        sid: &str,
        authorized: &Authorized,
        deadline: Instant,
    ) -> Result<Evidence, Error> {
        let need = prefix_need(&registration.kind);
        if need > self.config.limits.max_sniff_bytes as usize {
            return Err(REJECTED);
        }
        let offset = registration.range.as_ref().map_or(0, |r| r.offset);
        let size = registration
            .range
            .as_ref()
            .map_or(need as u64, |r| r.length.min(need as u64));
        let end = offset
            .checked_add(size)
            .and_then(|v| v.checked_sub(1))
            .ok_or(MISMATCH)?;
        let range = Range::Closed(offset, end);
        if let Some(limit) = &registration.range {
            range.constrain(limit)?;
        }
        let wire = range.wire();
        let mut upstream = self
            .transport
            .fetch(Fetch {
                url: &registration.uri,
                method: Method::GET,
                context: &registration.context,
                session_id: sid,
                deadline,
                cancel: authorized.cancelled.clone(),
                range: Some(&wire),
                if_range: None,
            })
            .await
            .map_err(network)?;
        identity(&upstream.headers)?;
        if upstream.status != StatusCode::PARTIAL_CONTENT {
            return Err(MISMATCH);
        }
        let (a, b, total) = range::content_range(&upstream.headers)?;
        if range.resolve(total)? != (a, b)
            || length(&upstream.headers)?.is_some_and(|n| n != b - a + 1)
        {
            return Err(MISMATCH);
        }
        let validators = upstream.headers.clone();
        let bytes = collect(&mut upstream, need).await?;
        if bytes.len() as u64 != b - a + 1 {
            return Err(MISMATCH);
        }
        validate_prefix(&bytes, &registration.kind, false, 0)?;
        Ok(Evidence {
            offset: a,
            total,
            bytes,
            validators,
        })
    }
}
fn unsatisfiable(total: u64) -> Result<axum::response::Response, Error> {
    let mut response = range::UNSAT.into_response();
    response.headers_mut().insert(
        "content-range",
        format!("bytes */{total}").parse().map_err(|_| MISMATCH)?,
    );
    Ok(response)
}
fn response(
    status: StatusCode,
    mime: &str,
    len: u64,
    body: Body,
    range: Option<String>,
) -> Result<axum::response::Response, Error> {
    let mut r = Response::builder()
        .status(status)
        .header("content-type", mime)
        .header("content-length", len);
    if let Some(range) = range {
        r = r.header("content-range", range);
    }
    r.body(body).map_err(|_| REJECTED)
}
fn identity(h: &HeaderMap) -> Result<(), Error> {
    validators::validate_duplicates(h)?;
    if h.get_all("content-type").iter().count() > 1
        || h.get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("multipart/"))
    {
        return Err(REJECTED);
    }
    if h.get_all("content-encoding").iter().count() > 1
        || h.get("content-encoding").is_some_and(|v| v != "identity")
    {
        return Err(REJECTED);
    }
    Ok(())
}
fn length(h: &HeaderMap) -> Result<Option<u64>, Error> {
    if h.get_all("content-length").iter().count() > 1 {
        return Err(MISMATCH);
    }
    h.get("content-length")
        .map(|s| {
            s.to_str()
                .ok()
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse().ok())
                .ok_or(MISMATCH)
        })
        .transpose()
}
async fn collect(upstream: &mut Upstream, max: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    while let Some(chunk) = upstream.chunk().await.map_err(network)? {
        if bytes.len().checked_add(chunk.len()).is_none_or(|n| n > max) {
            return Err(REJECTED);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn validate_prefix(
    bytes: &[u8],
    kind: &ResourceKind,
    encrypted: bool,
    start: u64,
) -> Result<(), Error> {
    if bytes.is_empty() {
        return Err(REJECTED);
    }
    if encrypted || start > 0 {
        return Ok(());
    }
    // Error pages are never media, even when a provider assigned a media suffix.
    let trim = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let p = &bytes[trim..];
    if p.starts_with(b"<")
        || p.starts_with(b"{")
        || p.starts_with(b"[")
        || p.starts_with(b"#EXTM3U")
    {
        return Err(REJECTED);
    }
    if encrypted || start > 0 {
        return Ok(());
    }
    let valid = match kind {
        ResourceKind::Segment => {
            bytes.len() >= 188
                && [0, 188, 376]
                    .iter()
                    .filter(|i| **i + 4 <= bytes.len())
                    .all(|i| {
                        bytes[*i] == 0x47 && bytes[*i + 1] & 0x80 == 0 && bytes[*i + 3] & 0x30 != 0
                    })
        }
        ResourceKind::Init | ResourceKind::Fragment | ResourceKind::Mp4 => {
            bytes.len() >= 8
                && match u32::from_be_bytes(bytes[..4].try_into().map_err(|_| REJECTED)?) {
                    1 => {
                        bytes.len() >= 16
                            && u64::from_be_bytes(bytes[8..16].try_into().map_err(|_| REJECTED)?)
                                >= 16
                    }
                    size => size >= 8,
                }
                && if matches!(kind, ResourceKind::Init | ResourceKind::Mp4) {
                    &bytes[4..8] == b"ftyp"
                } else {
                    [b"styp", b"moof", b"sidx", b"emsg"]
                        .iter()
                        .any(|tag| tag.as_slice() == &bytes[4..8])
                }
        }
        ResourceKind::Subtitle => {
            let p = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
            p.starts_with(b"WEBVTT")
                && p.get(6)
                    .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                && std::str::from_utf8(p)
                    .err()
                    .is_none_or(|e| e.error_len().is_none())
        }
        _ => false,
    };
    if valid { Ok(()) } else { Err(REJECTED) }
}
// Free metadata has a deliberately narrow grammar. Also reject exact source
// query/credential values, including values too ordinary for URI detection.
fn redact(
    output: &str,
    r: &crate::session::Registration,
    base: &url::Url,
    references: &[crate::session::Registration],
) -> Result<(), Error> {
    let metadata = hls::metadata_values(output)?;
    let secrets = private_values(r, base, references)?;
    if secrets
        .iter()
        .any(|secret| metadata.iter().any(|value| private_match(value, secret)))
    {
        return Err(hls::INVALID);
    }
    Ok(())
}
fn private_match(value: &str, secret: &str) -> bool {
    !secret.is_empty()
        && if secret.len() < 4 {
            value
                .split(|c: char| !c.is_alphanumeric() && !"_-.".contains(c))
                .any(|word| word == secret)
        } else {
            value.contains(secret)
        }
}
fn private_values(
    r: &crate::session::Registration,
    base: &url::Url,
    references: &[crate::session::Registration],
) -> Result<std::collections::BTreeSet<String>, Error> {
    let mut secrets = std::collections::BTreeSet::new();
    for raw in [&r.uri, base.as_str()]
        .into_iter()
        .chain(references.iter().map(|r| r.uri.as_str()))
    {
        let url = url::Url::parse(raw).map_err(|_| hls::INVALID)?;
        for (name, value) in url.query_pairs() {
            if !value.is_empty() && !public_query(&name, &value) {
                secrets.insert(value.into_owned());
            }
            if secrets.len() > 4096 {
                return Err(hls::INVALID);
            }
        }
    }
    // Every generated registration uses this same parent fetch context.
    for scope in r.context.scopes.iter().flatten() {
        for value in scope.headers.iter().flatten().map(|(_, v)| v) {
            secrets.insert(value.clone());
            if let Some(token) = value.strip_prefix("Bearer ") {
                secrets.insert(token.to_owned());
            }
        }
        if let Some(cookie) = &scope.cookies {
            secrets.insert(cookie.clone());
            for item in cookie.split(';') {
                if let Some((_, value)) = item.split_once('=') {
                    secrets.insert(value.trim().to_owned());
                }
            }
        }
    }
    if secrets.len() > 4096 {
        return Err(hls::INVALID);
    }
    Ok(secrets)
}
fn public_query(name: &str, value: &str) -> bool {
    match name.to_ascii_lowercase().as_str() {
        "v" | "ver" | "version" => {
            value.len() <= 16
                && value
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        }
        "lang" | "language" | "locale" => {
            let mut parts = value.split('-');
            parts.next().is_some_and(|s| {
                (2..=3).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphabetic())
            }) && parts.clone().count() <= 2
                && parts.all(|s| {
                    (2..=4).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphabetic())
                })
        }
        _ => false,
    }
}

type NextChunk = Pin<Box<dyn Future<Output = (Upstream, Result<Option<Bytes>, Error>)> + Send>>;
struct StreamBody {
    upstream: Option<Upstream>,
    next: Option<NextChunk>,
    prefix: Option<Bytes>,
    count: u64,
    expected: Option<u64>,
    authorized: Option<Authorized>,
    deadline: Instant,

    done: bool,
    final_chunk: Option<Bytes>,
    utf8: Option<Utf8>,
}
impl StreamBody {
    fn finish(&mut self) {
        self.done = true;
        self.upstream = None;
        self.next = None;
        self.prefix = None;
        self.final_chunk = None;
        self.authorized = None;
    }
}
impl Drop for StreamBody {
    fn drop(&mut self) {
        if !self.done {
            crate::health::failure("stream_cancelled");
        }
    }
}
impl hyper::body::Body for StreamBody {
    type Data = Bytes;
    type Error = Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
        if self.done {
            return Poll::Ready(None);
        }
        if Instant::now() >= self.deadline
            || self
                .authorized
                .as_ref()
                .is_none_or(|a| a.cancelled.is_cancelled())
        {
            self.finish();
            crate::health::failure("stream_cancelled");
            return Poll::Ready(Some(Err(Error(StatusCode::GONE, "stream_cancelled"))));
        }
        let data = if let Some(prefix) = self.prefix.take() {
            Some(prefix)
        } else {
            if self.next.is_none() {
                let Some(mut upstream) = self.upstream.take() else {
                    self.finish();
                    return Poll::Ready(None);
                };
                self.next = Some(Box::pin(async move {
                    let result = upstream.chunk().await.map_err(network);
                    (upstream, result)
                }));
            }
            let Some(next) = self.next.as_mut() else {
                return Poll::Ready(None);
            };
            let (upstream, result) = match next.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => result,
            };
            self.next = None;
            self.upstream = Some(upstream);
            match result {
                Ok(data) => data,
                Err(error) => {
                    self.finish();
                    crate::health::failure(error.1);
                    return Poll::Ready(Some(Err(error)));
                }
            }
        };
        match data {
            Some(data) => {
                if self.final_chunk.is_some() {
                    self.finish();
                    crate::health::failure(MISMATCH.1);
                    return Poll::Ready(Some(Err(MISMATCH)));
                }
                if self.utf8.as_mut().is_some_and(|v| v.push(&data).is_err()) {
                    self.finish();
                    return Poll::Ready(Some(Err(REJECTED)));
                }
                let Some(count) = self.count.checked_add(data.len() as u64) else {
                    self.finish();
                    crate::health::failure(MISMATCH.1);
                    return Poll::Ready(Some(Err(MISMATCH)));
                };
                if self.expected.is_some_and(|n| count > n) {
                    self.finish();
                    crate::health::failure(MISMATCH.1);
                    return Poll::Ready(Some(Err(MISMATCH)));
                }
                self.count = count;
                if self.expected == Some(count) {
                    // Confirm EOF before releasing the last advertised bytes;
                    // Hyper may otherwise stop polling at Content-Length.
                    self.final_chunk = Some(data);
                    return self.poll_frame(cx);
                }
                crate::health::bytes(&crate::health::DELIVERED_BYTES, data.len());
                Poll::Ready(Some(Ok(Frame::data(data))))
            }
            None => {
                let short = self.expected.is_some_and(|n| n != self.count)
                    || self.utf8.as_ref().is_some_and(|v| !v.tail.is_empty());
                let final_chunk = self.final_chunk.take();
                self.finish();
                if short {
                    crate::health::failure(MISMATCH.1);
                    Poll::Ready(Some(Err(MISMATCH)))
                } else if let Some(data) = final_chunk {
                    crate::health::bytes(&crate::health::DELIVERED_BYTES, data.len());
                    Poll::Ready(Some(Ok(Frame::data(data))))
                } else {
                    Poll::Ready(None)
                }
            }
        }
    }
}

#[derive(Default)]
struct Utf8 {
    tail: Vec<u8>,
}
impl Utf8 {
    fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let mut start = 0;
        while !self.tail.is_empty() && start < bytes.len() {
            self.tail.push(bytes[start]);
            start += 1;
            match std::str::from_utf8(&self.tail) {
                Ok(_) => self.tail.clear(),
                Err(e) if e.error_len().is_none() && self.tail.len() < 4 => {}
                Err(_) => return Err(REJECTED),
            }
        }
        match std::str::from_utf8(&bytes[start..]) {
            Ok(_) => Ok(()),
            Err(e) if e.error_len().is_none() => {
                self.tail
                    .extend_from_slice(&bytes[start + e.valid_up_to()..]);
                Ok(())
            }
            Err(_) => Err(REJECTED),
        }
    }
}

#[cfg(test)]
mod tests;
