//! Bounded HLS discovery for probing, without relay rewriting or network I/O.
//!
//! Preserves provider metadata and the Go parser's byte-range/sample semantics.
//! Master and media tags cannot be mixed. A single initialization map is
//! supported; changing maps are explicitly unsupported until probes can retain
//! per-segment initialization associations. Unknown extension tags are ignored.
//! DNS approval and destination pinning still belong to the shared transport.

mod attributes;
mod parser;

use stream_core::providers::Metadata;
use thiserror::Error;

pub use parser::parse_playlist;

pub const MAX_PLAYLIST_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 16 * 1024;
pub const MAX_VARIANTS: usize = 32;
pub const MAX_TRACKS: usize = 64;
pub const MAX_SEGMENTS: usize = 16_384;
pub const MAX_RESOLVED_URI_BYTES: usize = 8 * 1024 * 1024;

/// Error details contain only static fields, never signed URLs or key material.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum HlsError {
    #[error("HLS input limit exceeded: {0}")]
    LimitExceeded(&'static str),
    #[error("HLS manifest is not valid UTF-8")]
    InvalidUtf8,
    #[error("HLS manifest has an invalid header")]
    InvalidHeader,
    #[error("malformed HLS field: {0}")]
    Malformed(&'static str),
    #[error("duplicate HLS field: {0}")]
    Duplicate(&'static str),
    #[error("HLS manifest mixes master and media tags")]
    MixedPlaylist,
    #[error("HLS manifest ends with an incomplete tag or URI association")]
    Incomplete,
    #[error("HLS manifest contains no usable variant or media segment")]
    Empty,
    #[error("HLS resource URL is invalid or nonpublic")]
    InvalidResource,
    #[error("HLS implicit byte range requires a prior range on the same resource")]
    MissingRangePredecessor,
    #[error("HLS byte range overflows the signed 64-bit resource range")]
    RangeOverflow,
    #[error("HLS initialization map changes are not supported by this prober")]
    ChangingInitialization,
    #[error("unsupported HLS probing feature: {0}")]
    UnsupportedFeature(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaylistKind {
    Master,
    Media,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    pub start: i64,
    pub length: i64,
}

impl ByteRange {
    /// Exclusive end remains representable, preserving Go's overflow boundary.
    pub fn new(start: i64, length: i64) -> Result<Self, HlsError> {
        if start < 0 || length <= 0 {
            return Err(HlsError::Malformed("byte range"));
        }
        start.checked_add(length).ok_or(HlsError::RangeOverflow)?;
        Ok(Self { start, length })
    }

    pub fn end_exclusive(self) -> Result<i64, HlsError> {
        Self::new(self.start, self.length)?;
        self.start
            .checked_add(self.length)
            .ok_or(HlsError::RangeOverflow)
    }

    pub fn header_value(self) -> Result<String, HlsError> {
        let end = self
            .end_exclusive()?
            .checked_sub(1)
            .ok_or(HlsError::RangeOverflow)?;
        Ok(format!("bytes={}-{end}", self.start))
    }
}

/// A segment or initialization resource, with its exact optional byte range.
/// Debug is deliberately redacted because URLs commonly contain signed tokens.
#[derive(Clone, Eq, PartialEq)]
pub struct MediaSample {
    pub url: String,
    pub byte_range: Option<ByteRange>,
}

impl std::fmt::Debug for MediaSample {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaSample")
            .field("url", &"[REDACTED]")
            .field("byte_range", &self.byte_range)
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct Playlist {
    pub kind: PlaylistKind,
    pub metadata: Metadata,
    pub samples: Vec<MediaSample>,
    pub initialization: Option<MediaSample>,
    pub encrypted: bool,
    pub unsupported_protection: bool,
    pub end_list: bool,
}

impl Playlist {
    /// The same evenly spaced sample positions as Go: first, midpoint, last.
    /// For even counts the midpoint rounds down, and short playlists do not
    /// repeat a segment. Selection borrows rather than cloning signed URLs.
    pub fn probe_samples(&self) -> impl Iterator<Item = &MediaSample> {
        let count = self.samples.len();
        let indices = match count {
            0 => [None, None, None],
            1 => [Some(0), None, None],
            2 => [Some(0), Some(1), None],
            _ => [Some(0), Some((count - 1) / 2), Some(count - 1)],
        };
        indices
            .into_iter()
            .flatten()
            .filter_map(|index| self.samples.get(index))
    }
}
