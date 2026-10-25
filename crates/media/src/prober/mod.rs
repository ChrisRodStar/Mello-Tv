//! Fresh, bounded media evidence. Accessibility is byte/container evidence, not
//! decoded playback or independent content-identity verification.
mod bmff;
mod budget;
mod mp4;

use crate::{
    hls::{self, ByteRange, MediaSample, Playlist, PlaylistKind},
    network::{FetchRequest, FetchResponse, HttpClient, TransportError},
};
use async_trait::async_trait;
use budget::ByteBudget;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream::FuturesUnordered};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime},
};
use stream_core::providers::{
    CandidateStream, ErrorCategory, Metadata, ProbeObservation, ProbedStream, StreamFormat, Variant,
};
use thiserror::Error;
use tokio::{
    sync::{Semaphore, mpsc},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait ProbeTransport: Send + Sync {
    async fn fetch(
        &self,
        request: FetchRequest,
        cancellation: &CancellationToken,
    ) -> Result<FetchResponse, TransportError>;
}
#[async_trait]
impl ProbeTransport for HttpClient {
    async fn fetch(
        &self,
        request: FetchRequest,
        cancellation: &CancellationToken,
    ) -> Result<FetchResponse, TransportError> {
        HttpClient::fetch(self, request, cancellation).await
    }
}

#[derive(Clone, Debug)]
pub struct ProbeLimits {
    pub concurrent: usize,
    pub samples_per_variant: usize,
    pub sample_bytes: usize,
    pub total_sample_bytes: usize,
    pub total_playlist_bytes: usize,
    pub source_timeout: Duration,
    pub max_variants: usize,
}
impl Default for ProbeLimits {
    fn default() -> Self {
        Self {
            concurrent: 4,
            samples_per_variant: 3,
            sample_bytes: 128 * 1024,
            total_sample_bytes: 4 * 1024 * 1024,
            total_playlist_bytes: 4 * 1024 * 1024,
            source_timeout: Duration::from_secs(6),
            max_variants: 6,
        }
    }
}
#[derive(Debug, Error)]
#[error("probe limits exceed supported bounds")]
pub struct ProbeConfigError;

pub const MAX_ROUND_CANDIDATES: usize = 1024;
pub const MAX_ROUND_RESULTS: usize = MAX_ROUND_CANDIDATES * 6;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ProbeRoundError {
    #[error("probe round candidate limit exceeded")]
    CandidateLimit,
    #[error("probe round result limit exceeded")]
    ResultLimit,
    #[error("probe options require a positive height ceiling")]
    InvalidOptions,
    #[error("probe round cancelled")]
    Cancelled,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProbeOptions {
    pub max_height: Option<i64>,
}
pub type ProbeProgress = Arc<dyn Fn(&[ProbedStream]) + Send + Sync>;

#[derive(Clone)]
pub struct StreamProber {
    transport: Arc<dyn ProbeTransport>,
    limits: ProbeLimits,
    slots: Arc<Semaphore>,
}

#[derive(Clone)]
struct Round {
    segments: ByteBudget,
    playlists: ByteBudget,
    options: ProbeOptions,
}
impl Round {
    fn new(limits: &ProbeLimits, options: ProbeOptions) -> Self {
        Self {
            segments: ByteBudget::new(limits.total_sample_bytes),
            playlists: ByteBudget::new(limits.total_playlist_bytes),
            options,
        }
    }
}

#[derive(Clone, Copy, Debug, Error)]
enum ProbeFailure {
    #[error("probe byte budget exhausted")]
    Budget,
    #[error("unsupported media layout or protection")]
    Unsupported,
    #[error("invalid media response")]
    Protocol,
    #[error("upstream media unavailable")]
    Upstream,
    #[error("media probe timed out")]
    Timeout,
    #[error("media probe cancelled")]
    Cancelled,
}
type ProbeResult<T> = Result<T, ProbeFailure>;
impl ProbeFailure {
    fn category(self) -> ErrorCategory {
        match self {
            Self::Budget => ErrorCategory::Unavailable,
            Self::Unsupported => ErrorCategory::Unsupported,
            Self::Protocol => ErrorCategory::ProtocolChange,
            Self::Upstream => ErrorCategory::UpstreamFailure,
            Self::Timeout => ErrorCategory::Timeout,
            Self::Cancelled => ErrorCategory::Cancelled,
        }
    }
}
impl From<TransportError> for ProbeFailure {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Cancelled => Self::Cancelled,
            TransportError::Timeout => Self::Timeout,
            TransportError::Capacity | TransportError::ResponseLimit => Self::Budget,
            TransportError::DestinationDenied
            | TransportError::InvalidConfig
            | TransportError::InvalidContext
            | TransportError::RedirectDenied
            | TransportError::HeaderLimit => Self::Protocol,
            TransportError::Dns | TransportError::Request(_) => Self::Upstream,
        }
    }
}

impl StreamProber {
    pub fn new(
        transport: Arc<dyn ProbeTransport>,
        limits: ProbeLimits,
    ) -> Result<Self, ProbeConfigError> {
        if !(1..=4).contains(&limits.concurrent)
            || !(1..=3).contains(&limits.samples_per_variant)
            || !(1..=128 * 1024).contains(&limits.sample_bytes)
            || !(1..=4 * 1024 * 1024).contains(&limits.total_sample_bytes)
            || !(1..=4 * 1024 * 1024).contains(&limits.total_playlist_bytes)
            || limits.source_timeout.is_zero()
            || limits.source_timeout > Duration::from_secs(6)
            || !(1..=6).contains(&limits.max_variants)
        {
            return Err(ProbeConfigError);
        }
        Ok(Self {
            transport,
            slots: Arc::new(Semaphore::new(limits.concurrent)),
            limits,
        })
    }

    pub async fn probe_all(
        &self,
        candidates: Vec<CandidateStream>,
        cancel: &CancellationToken,
    ) -> Result<Vec<ProbedStream>, ProbeRoundError> {
        self.probe_all_with_options(candidates, ProbeOptions::default(), cancel)
            .await
    }

    pub async fn probe_all_with_options(
        &self,
        candidates: Vec<CandidateStream>,
        options: ProbeOptions,
        cancel: &CancellationToken,
    ) -> Result<Vec<ProbedStream>, ProbeRoundError> {
        validate_options(options)?;
        if candidates.len() > MAX_ROUND_CANDIDATES {
            return Err(ProbeRoundError::CandidateLimit);
        }
        let round = Round::new(&self.limits, options);
        let operation = futures_util::stream::iter(candidates)
            .map(|candidate| self.probe_owned(candidate, round.clone(), cancel.clone(), None))
            .buffer_unordered(self.limits.concurrent)
            .collect::<Vec<_>>();
        let completed = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ProbeRoundError::Cancelled),
            completed = operation => completed,
        };
        let results: Vec<_> = completed.into_iter().flatten().collect();
        if results.len() > MAX_ROUND_RESULTS {
            return Err(ProbeRoundError::ResultLimit);
        }
        Ok(results)
    }

    /// Starts candidates as they arrive. The callback runs serially after each
    /// completed source; dropping/cancelling this method drops all owned futures.
    pub async fn probe_stream(
        &self,
        mut candidates: mpsc::Receiver<CandidateStream>,
        options: ProbeOptions,
        cancel: &CancellationToken,
        progress: Option<ProbeProgress>,
    ) -> Result<Vec<ProbedStream>, ProbeRoundError> {
        validate_options(options)?;
        let round = Round::new(&self.limits, options);
        let mut pending = FuturesUnordered::new();
        let (rendition_tx, mut rendition_rx) =
            mpsc::channel(self.limits.concurrent * self.limits.max_variants);
        let mut closed = false;
        let mut results = Vec::new();
        let mut positions = BTreeMap::new();
        let mut admitted = 0usize;
        loop {
            if closed && pending.is_empty() {
                break;
            }
            tokio::select! {
                biased;
                _=cancel.cancelled()=>{candidates.close();return Err(ProbeRoundError::Cancelled);},
                rendition=rendition_rx.recv(),if !pending.is_empty()=>{
                    if let Some(rendition)=rendition {
                        merge_results(&mut results,&mut positions,std::iter::once(rendition))?;
                        if let Some(callback)=&progress {callback(&results);}
                    }
                }
                completed=pending.next(),if !pending.is_empty()=>{
                    if let Some(completed)=completed {
                        merge_results(&mut results,&mut positions,completed)?;
                        if let Some(callback)=&progress {callback(&results);}
                    }
                }
                next=candidates.recv(),if !closed && pending.len()<self.limits.concurrent=>{
                    match next {
                        Some(candidate)=>{
                            admitted+=1;
                            if admitted>MAX_ROUND_CANDIDATES {candidates.close();return Err(ProbeRoundError::CandidateLimit);}
                            pending.push(self.probe_owned(candidate,round.clone(),cancel.clone(),Some(rendition_tx.clone())));
                        },
                        None=>closed=true,
                    }
                }
            }
        }
        Ok(results)
    }

    async fn probe_owned(
        &self,
        candidate: CandidateStream,
        round: Round,
        cancel: CancellationToken,
        progress: Option<mpsc::Sender<ProbedStream>>,
    ) -> Vec<ProbedStream> {
        let child = cancel.child_token();
        let _cancel_on_drop = child.clone().drop_guard();
        let deadline = Instant::now() + self.limits.source_timeout;
        let work = async {
            let _permit = tokio::time::timeout_at(deadline, self.slots.acquire())
                .await
                .map_err(|_| ProbeFailure::Timeout)?
                .map_err(|_| ProbeFailure::Budget)?;
            candidate.validate().map_err(|_| ProbeFailure::Protocol)?;
            let now: DateTime<Utc> = SystemTime::now().into();
            if candidate
                .expires_at
                .is_some_and(|expiry| expiry <= now + chrono::Duration::seconds(5))
            {
                return Err(ProbeFailure::Budget);
            }
            match candidate.format {
                StreamFormat::Mp4 => Ok(vec![
                    tokio::time::timeout_at(
                        deadline,
                        self.probe_mp4(candidate.clone(), &round, &child),
                    )
                    .await
                    .unwrap_or_else(|_| failure(candidate.clone(), ProbeFailure::Timeout)),
                ]),
                StreamFormat::Hls | StreamFormat::Unknown => {
                    self.probe_hls(candidate.clone(), &round, &child, deadline, progress)
                        .await
                }
                _ => Err(ProbeFailure::Unsupported),
            }
        };
        let result = tokio::select! {biased;_=child.cancelled()=>Err(ProbeFailure::Cancelled),result=work=>result};
        match result {
            Ok(items) => items,
            Err(error) => vec![failure(candidate, error)],
        }
    }

    async fn fetch_playlist(
        &self,
        candidate: &CandidateStream,
        url: &str,
        round: &Round,
        cancel: &CancellationToken,
    ) -> ProbeResult<Playlist> {
        let reservation = round.playlists.reserve(1024 * 1024, false, cancel).await?;
        let mut request = FetchRequest::get(url);
        request.context = candidate.fetch_context.clone();
        request.max_response_bytes = reservation.bytes;
        let response = self.transport.fetch(request, cancel).await?;
        reservation.finish(response.body.len())?;
        if !response.status.is_success() {
            return Err(ProbeFailure::Upstream);
        }
        hls::parse_playlist(&response.body, &response.url).map_err(|error| match error {
            hls::HlsError::UnsupportedFeature(_) | hls::HlsError::ChangingInitialization => {
                ProbeFailure::Unsupported
            }
            _ => ProbeFailure::Protocol,
        })
    }

    async fn probe_hls(
        &self,
        candidate: CandidateStream,
        round: &Round,
        cancel: &CancellationToken,
        deadline: Instant,
        progress: Option<mpsc::Sender<ProbedStream>>,
    ) -> ProbeResult<Vec<ProbedStream>> {
        let playlist = tokio::time::timeout_at(
            deadline,
            self.fetch_playlist(&candidate, &candidate.url, round, cancel),
        )
        .await
        .map_err(|_| ProbeFailure::Timeout)??;
        if playlist.kind == PlaylistKind::Media {
            let result = tokio::time::timeout_at(
                deadline,
                self.sample_playlist(candidate.clone(), playlist, None, round, cancel),
            )
            .await
            .unwrap_or_else(|_| failure(candidate, ProbeFailure::Timeout));
            return Ok(vec![result]);
        }
        let mut variants = playlist.metadata.variants.clone();
        variants.retain(|variant| {
            round
                .options
                .max_height
                .filter(|height| *height > 0)
                .is_none_or(|ceiling| variant.height.is_none_or(|height| height <= ceiling))
        });
        variants.sort_by_key(|variant| std::cmp::Reverse(variant.height.unwrap_or(0)));
        let mut results = Vec::new();
        for (index, variant) in variants
            .into_iter()
            .take(self.limits.max_variants)
            .enumerate()
        {
            if cancel.is_cancelled() {
                return Err(ProbeFailure::Cancelled);
            }
            if Instant::now() >= deadline {
                break;
            }
            let mut source = candidate.clone();
            source.id = format!("{}:variant:{index}", source.id);
            if source.id.len() > 256 {
                return Err(ProbeFailure::Protocol);
            }
            source.selected_variant_url = variant.url.clone();
            source.format = StreamFormat::Hls;
            let fetched = tokio::time::timeout_at(
                deadline,
                self.fetch_playlist(&candidate, &variant.url, round, cancel),
            )
            .await
            .unwrap_or(Err(ProbeFailure::Timeout));
            let mut media = match fetched {
                Ok(media) if media.kind == PlaylistKind::Media => media,
                Ok(_) => {
                    let mut item = failure(source, ProbeFailure::Unsupported);
                    item.observation.variant = Some(variant);
                    record_rendition(&progress, &mut results, item, cancel);
                    continue;
                }
                Err(error) => {
                    let mut item = failure(source, error);
                    item.observation.variant = Some(variant);
                    record_rendition(&progress, &mut results, item, cancel);
                    continue;
                }
            };
            media
                .metadata
                .tracks
                .extend(playlist.metadata.tracks.iter().cloned());
            media.metadata.tracks_known = playlist.metadata.tracks_known;
            media.encrypted |= playlist.encrypted;
            media.unsupported_protection |= playlist.unsupported_protection;
            let mut item = tokio::time::timeout_at(
                deadline,
                self.sample_playlist(source.clone(), media, Some(variant.clone()), round, cancel),
            )
            .await
            .unwrap_or_else(|_| {
                let mut failed = failure(source, ProbeFailure::Timeout);
                failed.observation.variant = Some(variant);
                failed
            });
            item.observation.playlist_metadata.variants = playlist.metadata.variants.clone();
            record_rendition(&progress, &mut results, item, cancel);
        }
        if results.is_empty() {
            return Err(if Instant::now() >= deadline {
                ProbeFailure::Timeout
            } else {
                ProbeFailure::Budget
            });
        }
        Ok(results)
    }

    async fn sample_playlist(
        &self,
        candidate: CandidateStream,
        playlist: Playlist,
        variant: Option<Variant>,
        round: &Round,
        cancel: &CancellationToken,
    ) -> ProbedStream {
        let mut result = failure(candidate, ProbeFailure::Protocol);
        result.observation.format = StreamFormat::Hls;
        result.source.format = StreamFormat::Hls;
        result.observation.variant = variant;
        result.observation.playlist_metadata = playlist.metadata.clone();
        result.observation.encrypted = playlist.encrypted;
        result.observation.protection_known = !playlist.unsupported_protection;
        let work = async {
            if playlist.unsupported_protection {
                return Err(ProbeFailure::Unsupported);
            }
            let mut measured_variant = None;
            if let Some(initialization) = &playlist.initialization {
                if playlist.encrypted {
                    return Err(ProbeFailure::Unsupported);
                }
                let response = self
                    .fetch_sample(
                        &result.source,
                        initialization,
                        256 * 1024,
                        &round.playlists,
                        true,
                        cancel,
                    )
                    .await?;
                let mut parsed = Variant {
                    url: if result.source.selected_variant_url.is_empty() {
                        result.source.url.clone()
                    } else {
                        result.source.selected_variant_url.clone()
                    },
                    ..Variant::default()
                };
                bmff::inspect(&response.body, &mut parsed)?;
                if parsed.height.is_none() || parsed.codecs.is_empty() {
                    return Err(ProbeFailure::Unsupported);
                }
                measured_variant = Some(parsed);
            }
            if let Some(measured) = measured_variant {
                if let Some(declared) = &mut result.observation.variant {
                    if declared
                        .width
                        .zip(measured.width)
                        .is_some_and(|(a, b)| a != b)
                        || declared
                            .height
                            .zip(measured.height)
                            .is_some_and(|(a, b)| a != b)
                    {
                        return Err(ProbeFailure::Protocol);
                    }
                    declared.width = measured.width;
                    declared.height = measured.height;
                    declared.codecs = measured.codecs;
                } else {
                    result.observation.variant = Some(measured);
                }
            }
            let count = self.limits.samples_per_variant.min(playlist.samples.len());
            let samples = (0..count)
                .map(|index| {
                    let position = if count == 1 {
                        0
                    } else {
                        index * (playlist.samples.len() - 1) / (count - 1)
                    };
                    &playlist.samples[position]
                })
                .collect::<Vec<_>>();
            if samples.is_empty() {
                return Err(ProbeFailure::Protocol);
            }
            let mut all_speeds = true;
            for sample in samples {
                let response = self
                    .fetch_sample(
                        &result.source,
                        sample,
                        self.limits.sample_bytes,
                        &round.segments,
                        false,
                        cancel,
                    )
                    .await?;
                let textual = response
                    .headers
                    .get("content-type")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|kind| {
                        let kind = kind.to_ascii_lowercase();
                        kind.contains("text/html")
                            || kind.contains("json")
                            || kind.contains("mpegurl")
                    });
                let is_ts = bmff::transport_stream(&response.body);
                let is_fragment = bmff::fragment(&response.body);
                if response
                    .headers
                    .get_all("content-encoding")
                    .iter()
                    .any(|value| value.as_bytes() != b"identity")
                {
                    return Err(ProbeFailure::Unsupported);
                }
                if response.body.is_empty()
                    || (textual && !is_ts && !is_fragment)
                    || masquerades_as_text(&response.body)
                    || (!playlist.encrypted && !is_ts && !is_fragment)
                {
                    return Err(ProbeFailure::Protocol);
                }
                // A fragment header cannot establish clear sample entries or
                // codec configuration without its mandatory initialization map.
                if !playlist.encrypted && is_fragment && playlist.initialization.is_none() {
                    return Err(ProbeFailure::Unsupported);
                }
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

    async fn fetch_sample(
        &self,
        candidate: &CandidateStream,
        sample: &MediaSample,
        wanted: usize,
        budget: &ByteBudget,
        exact: bool,
        cancel: &CancellationToken,
    ) -> ProbeResult<FetchResponse> {
        let desired = sample
            .byte_range
            .map(|range| {
                usize::try_from(range.length)
                    .unwrap_or(usize::MAX)
                    .min(wanted)
            })
            .unwrap_or(wanted);
        let reservation = budget.reserve(desired, exact, cancel).await?;
        let start = sample.byte_range.map(|range| range.start).unwrap_or(0);
        let length = i64::try_from(reservation.bytes).map_err(|_| ProbeFailure::Protocol)?;
        let range = ByteRange::new(start, length).map_err(|_| ProbeFailure::Protocol)?;
        let mut request = FetchRequest::get(&sample.url);
        request.context = candidate.fetch_context.clone();
        request.max_response_bytes = reservation.bytes;
        request.sample = true;
        request.headers.insert(
            "Range".into(),
            range.header_value().map_err(|_| ProbeFailure::Protocol)?,
        );
        let response = self.transport.fetch(request, cancel).await?;
        reservation.finish(response.body.len())?;
        if !matches!(response.status.as_u16(), 200 | 206) {
            return Err(ProbeFailure::Upstream);
        }
        if start > 0 && response.status.as_u16() != 206 {
            return Err(ProbeFailure::Unsupported);
        }
        if response.status.as_u16() == 206 {
            validate_range(&response, start, length, None)?;
        }
        Ok(response)
    }
}

fn validate_options(options: ProbeOptions) -> Result<(), ProbeRoundError> {
    if options.max_height.is_some_and(|height| height <= 0) {
        return Err(ProbeRoundError::InvalidOptions);
    }
    Ok(())
}

fn masquerades_as_text(body: &[u8]) -> bool {
    let prefix = &body[..body.len().min(64)];
    let prefix = prefix
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(prefix)
        .trim_ascii_start();
    prefix.starts_with(b"#EXTM3U")
        || prefix
            .get(..5)
            .is_some_and(|value| value.eq_ignore_ascii_case(b"<html"))
        || prefix
            .get(..9)
            .is_some_and(|value| value.eq_ignore_ascii_case(b"<!doctype"))
}

fn merge_results(
    results: &mut Vec<ProbedStream>,
    positions: &mut BTreeMap<String, usize>,
    incoming: impl IntoIterator<Item = ProbedStream>,
) -> Result<(), ProbeRoundError> {
    for item in incoming {
        if let Some(position) = positions.get(&item.source.id) {
            let stored = results
                .get_mut(*position)
                .ok_or(ProbeRoundError::ResultLimit)?;
            *stored = item;
        } else {
            if results.len() >= MAX_ROUND_RESULTS {
                return Err(ProbeRoundError::ResultLimit);
            }
            positions.insert(item.source.id.clone(), results.len());
            results.push(item);
        }
    }
    Ok(())
}

fn record_rendition(
    progress: &Option<mpsc::Sender<ProbedStream>>,
    results: &mut Vec<ProbedStream>,
    item: ProbedStream,
    cancel: &CancellationToken,
) {
    // Progress is bounded and best-effort; final completion always returns the
    // full evidence. No task is detached just to notify a slow observer.
    if !cancel.is_cancelled()
        && let Some(progress) = progress
    {
        let _ = progress.try_send(item.clone());
    }
    results.push(item);
}

fn observation(format: StreamFormat) -> ProbeObservation {
    ProbeObservation {
        at: SystemTime::now().into(),
        delivery_bps: None,
        reliability: None,
        variant: None,
        first_byte: None,
        sample_bytes: 0,
        format,
        failure: None,
        playlist_metadata: Metadata::default(),
        samples_completed: 0,
        accessible: false,
        encrypted: false,
        protection_known: false,
    }
}
fn failure(candidate: CandidateStream, error: ProbeFailure) -> ProbedStream {
    let mut value = observation(candidate.format);
    value.failure = Some(error.category());
    ProbedStream {
        source: candidate,
        observation: value,
    }
}
fn finalize(mut result: ProbedStream, work: ProbeResult<()>) -> ProbedStream {
    match work {
        Ok(()) => {
            result.observation.accessible = true;
            result.observation.failure = None;
        }
        Err(error) => {
            result.observation.accessible = false;
            result.observation.delivery_bps = None;
            if matches!(error, ProbeFailure::Unsupported) {
                result.observation.protection_known = false;
            }
            result.observation.failure = Some(error.category());
        }
    }
    result
}

fn measure(
    observation: &mut ProbeObservation,
    response: &FetchResponse,
    all_speeds: &mut bool,
) -> ProbeResult<()> {
    observation.sample_bytes = observation
        .sample_bytes
        .checked_add(i64::try_from(response.body.len()).map_err(|_| ProbeFailure::Protocol)?)
        .ok_or(ProbeFailure::Protocol)?;
    observation.samples_completed += 1;
    if let Some(delay) = response
        .metrics
        .first_byte
        .and_then(|delay| i64::try_from(delay.as_nanos()).ok())
    {
        observation.first_byte = Some(
            observation
                .first_byte
                .map(|previous| previous.max(delay))
                .unwrap_or(delay),
        );
    }
    // A buffered chunk's post-first-byte read interval can approach zero. Charge
    // the whole request except admission to avoid optimistic throughput claims.
    let elapsed = response
        .metrics
        .total
        .checked_sub(response.metrics.admission_wait)
        .ok_or(ProbeFailure::Protocol)?;
    if response.body.len() < 16 * 1024 || elapsed < Duration::from_millis(1) {
        *all_speeds = false;
        return Ok(());
    }
    let speed = response.body.len() as f64 * 8.0 / elapsed.as_secs_f64();
    if !speed.is_finite() || speed <= 0.0 {
        *all_speeds = false;
        return Ok(());
    }
    observation.delivery_bps = Some(
        observation
            .delivery_bps
            .map(|previous| previous.min(speed))
            .unwrap_or(speed),
    );
    Ok(())
}

fn validate_range(
    response: &FetchResponse,
    start: i64,
    length: i64,
    expected_total: Option<i64>,
) -> ProbeResult<Option<i64>> {
    if response.headers.get_all("content-range").iter().count() != 1 {
        return Err(ProbeFailure::Protocol);
    }
    let value = response
        .headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .ok_or(ProbeFailure::Protocol)?;
    let (positions, total) = value
        .strip_prefix("bytes ")
        .and_then(|value| value.split_once('/'))
        .ok_or(ProbeFailure::Protocol)?;
    let (first, last) = positions.split_once('-').ok_or(ProbeFailure::Protocol)?;
    let first = first.parse::<i64>().map_err(|_| ProbeFailure::Protocol)?;
    let last = last.parse::<i64>().map_err(|_| ProbeFailure::Protocol)?;
    let total = if total == "*" {
        None
    } else {
        Some(total.parse::<i64>().map_err(|_| ProbeFailure::Protocol)?)
    };
    if first != start
        || last < first
        || last
            .checked_sub(first)
            .and_then(|bytes| bytes.checked_add(1))
            != i64::try_from(response.body.len()).ok()
        || response.body.len() as u64 > length as u64
        || total.is_some_and(|total| total <= last)
        || expected_total.is_some_and(|expected| total != Some(expected))
    {
        return Err(ProbeFailure::Protocol);
    }
    Ok(total)
}
