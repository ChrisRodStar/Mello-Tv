use crate::{error::Error, session::ByteRange};
use axum::http::{HeaderMap, StatusCode};
pub const SYNTAX: Error = Error(StatusCode::BAD_REQUEST, "invalid_range_syntax");
pub const UNSAT: Error = Error(StatusCode::RANGE_NOT_SATISFIABLE, "range_not_satisfiable");
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Closed(u64, u64),
    Open(u64),
    Suffix(u64),
}
fn number(s: &str) -> Result<u64, Error> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SYNTAX);
    }
    s.parse().map_err(|_| SYNTAX)
}
impl Range {
    pub fn parse(headers: &HeaderMap) -> Result<Option<Self>, Error> {
        let mut values = headers.get_all("range").iter();
        let Some(value) = values.next() else {
            return Ok(None);
        };
        if values.next().is_some() {
            return Err(SYNTAX);
        }
        let text = value
            .to_str()
            .map_err(|_| SYNTAX)?
            .strip_prefix("bytes=")
            .ok_or(SYNTAX)?;
        let (a, b) = text.split_once('-').ok_or(SYNTAX)?;
        let r = if a.is_empty() {
            Self::Suffix(number(b)?)
        } else if b.is_empty() {
            Self::Open(number(a)?)
        } else {
            let (a, b) = (number(a)?, number(b)?);
            if a > b {
                return Err(SYNTAX);
            }
            Self::Closed(a, b)
        };
        Ok(Some(r))
    }
    pub fn wire(self) -> String {
        match self {
            Self::Closed(a, b) => format!("bytes={a}-{b}"),
            Self::Open(a) => format!("bytes={a}-"),
            Self::Suffix(b) => format!("bytes=-{b}"),
        }
    }
    pub fn resolve(self, total: u64) -> Result<(u64, u64), Error> {
        if total == 0 {
            return Err(UNSAT);
        }
        let (a, b) = match self {
            Self::Closed(a, b) => (a, b.min(total - 1)),
            Self::Open(a) => (a, total - 1),
            Self::Suffix(n) => {
                if n == 0 {
                    return Err(UNSAT);
                }
                (total.saturating_sub(n), total - 1)
            }
        };
        if a >= total || a > b {
            return Err(UNSAT);
        }
        Ok((a, b))
    }
    pub fn constrain(self, limit: &ByteRange) -> Result<(), Error> {
        let end = limit
            .offset
            .checked_add(limit.length)
            .and_then(|n| n.checked_sub(1))
            .ok_or(UNSAT)?;
        match self {
            Self::Closed(a, b) if a >= limit.offset && b <= end => Ok(()),
            _ => Err(UNSAT),
        }
    }
}
pub fn content_range(headers: &HeaderMap) -> Result<(u64, u64, u64), Error> {
    if headers.get_all("content-range").iter().count() != 1 {
        return Err(super::MISMATCH);
    }
    let s = headers
        .get("content-range")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes "))
        .ok_or(super::MISMATCH)?;
    let (extent, total) = s.split_once('/').ok_or(super::MISMATCH)?;
    let (a, b) = extent.split_once('-').ok_or(super::MISMATCH)?;
    let (a, b, total) = (
        number(a).map_err(|_| super::MISMATCH)?,
        number(b).map_err(|_| super::MISMATCH)?,
        number(total).map_err(|_| super::MISMATCH)?,
    );
    if a > b || b >= total {
        return Err(super::MISMATCH);
    }
    Ok((a, b, total))
}
