//! Paged, bounded fast-start MP4 inspection; large timing tables stay remote.

use super::{
    ProbeFailure, ProbeResult, Round, StreamProber, bmff, failure, finalize, measure,
    validate_range,
};
use crate::{
    hls::{ByteRange, MediaSample},
    network::FetchResponse,
};
use std::collections::BTreeMap;
use stream_core::providers::{CandidateStream, ProbedStream, StreamFormat, Variant};
use tokio_util::sync::CancellationToken;

impl StreamProber {
    #[allow(clippy::too_many_arguments)]
    async fn mp4_range(
        &self,
        candidate: &CandidateStream,
        start: i64,
        length: usize,
        total: Option<i64>,
        round: &Round,
        metadata: bool,
        cancel: &CancellationToken,
    ) -> ProbeResult<(FetchResponse, i64)> {
        let length = i64::try_from(length).map_err(|_| ProbeFailure::Protocol)?;
        let sample = MediaSample {
            url: candidate.url.clone(),
            byte_range: Some(ByteRange::new(start, length).map_err(|_| ProbeFailure::Protocol)?),
        };
        let budget = if metadata {
            &round.playlists
        } else {
            &round.segments
        };
        let response = self
            .fetch_sample(candidate, &sample, length as usize, budget, true, cancel)
            .await?;
        if response.status.as_u16() != 206 {
            return Err(ProbeFailure::Unsupported);
        }
        let actual =
            validate_range(&response, start, length, total)?.ok_or(ProbeFailure::Protocol)?;
        let remaining = actual.checked_sub(start).ok_or(ProbeFailure::Protocol)?;
        if remaining <= 0 || i64::try_from(response.body.len()).ok() != Some(length.min(remaining))
        {
            return Err(ProbeFailure::Protocol);
        }
        Ok((response, actual))
    }

    pub(super) async fn probe_mp4(
        &self,
        candidate: CandidateStream,
        round: &Round,
        cancel: &CancellationToken,
    ) -> ProbedStream {
        let mut result = failure(candidate, ProbeFailure::Protocol);
        result.observation.format = StreamFormat::Mp4;
        result.source.format = StreamFormat::Mp4;
        let work = async {
            // The metadata and sample paths share reservations with the rest of
            // this round. No whole-file fetch or synchronous network I/O occurs.
            let (first, total) = self
                .mp4_range(
                    &result.source,
                    0,
                    self.limits.sample_bytes,
                    None,
                    round,
                    false,
                    cancel,
                )
                .await?;
            if first.body.len() < 32 || first.body.get(4..8) != Some(b"ftyp") {
                return Err(ProbeFailure::Protocol);
            }
            let (ftyp_size, _) = bmff::box_header(&first.body, total as u64)?;
            let ftyp_size = usize::try_from(ftyp_size).map_err(|_| ProbeFailure::Protocol)?;
            let moov_prefix = first
                .body
                .get(ftyp_size..)
                .ok_or(ProbeFailure::Unsupported)?;
            if ftyp_size < 16 || moov_prefix.get(4..8) != Some(b"moov") {
                return Err(ProbeFailure::Unsupported);
            }
            let available = (total as u64)
                .checked_sub(ftyp_size as u64)
                .ok_or(ProbeFailure::Protocol)?;
            let (moov_size, header) = bmff::box_header(moov_prefix, available)?;
            let start = (ftyp_size as u64)
                .checked_add(header as u64)
                .ok_or(ProbeFailure::Protocol)?;
            let end = (ftyp_size as u64)
                .checked_add(moov_size)
                .ok_or(ProbeFailure::Protocol)?;
            let mut reader = MetadataReader {
                prober: self,
                candidate: &result.source,
                round,
                cancel,
                initial: &first.body,
                total,
                pages: BTreeMap::new(),
                fetches: 0,
            };
            let mut variant = Variant {
                url: result.source.url.clone(),
                ..Variant::default()
            };
            reader.inspect(start, end, &mut variant).await?;
            if variant.height.is_none() || variant.codecs.is_empty() {
                return Err(ProbeFailure::Unsupported);
            }
            result.observation.variant = Some(variant);
            result.observation.protection_known = true;
            let mut all_speeds = true;
            measure(&mut result.observation, &first, &mut all_speeds)?;
            for index in 1..self.limits.samples_per_variant {
                let length = self
                    .limits
                    .sample_bytes
                    .min(usize::try_from(total).map_err(|_| ProbeFailure::Protocol)?);
                let length_i64 = i64::try_from(length).map_err(|_| ProbeFailure::Protocol)?;
                let divisor = i64::try_from(self.limits.samples_per_variant - 1)
                    .map_err(|_| ProbeFailure::Protocol)?;
                let start = (total - length_i64) / divisor
                    * i64::try_from(index).map_err(|_| ProbeFailure::Protocol)?;
                let (response, _) = self
                    .mp4_range(
                        &result.source,
                        start,
                        length,
                        Some(total),
                        round,
                        false,
                        cancel,
                    )
                    .await?;
                measure(&mut result.observation, &response, &mut all_speeds)?;
            }
            if !all_speeds {
                result.observation.delivery_bps = None;
            }
            Ok(())
        }
        .await;
        finalize(result, work)
    }
}

struct MetadataReader<'a> {
    prober: &'a StreamProber,
    candidate: &'a CandidateStream,
    round: &'a Round,
    cancel: &'a CancellationToken,
    initial: &'a [u8],
    total: i64,
    pages: BTreeMap<u64, Vec<u8>>,
    fetches: usize,
}

impl MetadataReader<'_> {
    async fn read(&mut self, start: u64, length: u64) -> ProbeResult<Vec<u8>> {
        let end = start.checked_add(length).ok_or(ProbeFailure::Protocol)?;
        if end > self.total as u64 || length > 256 * 1024 {
            return Err(ProbeFailure::Unsupported);
        }
        let length = usize::try_from(length).map_err(|_| ProbeFailure::Protocol)?;
        if end <= self.initial.len() as u64 {
            return Ok(self
                .initial
                .get(start as usize..end as usize)
                .ok_or(ProbeFailure::Protocol)?
                .to_vec());
        }
        let mut output = Vec::with_capacity(length);
        while output.len() < length {
            let position = start
                .checked_add(output.len() as u64)
                .ok_or(ProbeFailure::Protocol)?;
            let page_start = position / 8192 * 8192;
            if !self.pages.contains_key(&page_start) {
                if self.fetches >= 32 {
                    return Err(ProbeFailure::Unsupported);
                }
                let page_length = 8192.min((self.total as u64) - page_start) as usize;
                let (response, _) = self
                    .prober
                    .mp4_range(
                        self.candidate,
                        i64::try_from(page_start).map_err(|_| ProbeFailure::Protocol)?,
                        page_length,
                        Some(self.total),
                        self.round,
                        true,
                        self.cancel,
                    )
                    .await?;
                self.fetches += 1;
                self.pages.insert(page_start, response.body.to_vec());
            }
            let page = self.pages.get(&page_start).ok_or(ProbeFailure::Protocol)?;
            let offset =
                usize::try_from(position - page_start).map_err(|_| ProbeFailure::Protocol)?;
            let available = page.get(offset..).ok_or(ProbeFailure::Protocol)?;
            let take = available.len().min(length - output.len());
            if take == 0 {
                return Err(ProbeFailure::Protocol);
            }
            output.extend_from_slice(&available[..take]);
        }
        Ok(output)
    }

    async fn inspect(&mut self, start: u64, end: u64, variant: &mut Variant) -> ProbeResult<()> {
        let mut stack = vec![(start, end, 0_u8)];
        let mut boxes = 0usize;
        while let Some((mut position, end, depth)) = stack.pop() {
            if depth > 8 {
                return Err(ProbeFailure::Unsupported);
            }
            while position < end {
                boxes += 1;
                if boxes > 4096 || end - position < 8 {
                    return Err(ProbeFailure::Unsupported);
                }
                let mut header = self.read(position, 8).await?;
                if bmff::u32_at(&header, 0)? == 1 {
                    header = self.read(position, 16).await?;
                }
                let (size, prefix) = bmff::box_header(&header, end - position)?;
                let kind = header.get(4..8).ok_or(ProbeFailure::Protocol)?;
                let next = position.checked_add(size).ok_or(ProbeFailure::Protocol)?;
                match kind {
                    b"trak" | b"mdia" | b"minf" | b"stbl" => {
                        stack.push((position + prefix as u64, next, depth + 1))
                    }
                    b"stsd" => {
                        let description = self.read(position, size).await?;
                        bmff::inspect(&description, variant)?;
                    }
                    b"pssh" | b"sinf" | b"senc" | b"saiz" | b"saio" => {
                        return Err(ProbeFailure::Unsupported);
                    }
                    _ => {}
                }
                position = next;
            }
        }
        Ok(())
    }
}
