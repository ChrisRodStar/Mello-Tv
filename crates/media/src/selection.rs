//! Deterministic, measured-source selection. All clock decisions use the
//! caller's trusted observation time; sorting never fetches or reads a clock.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use chrono::{DateTime, Duration, Utc};
use stream_core::providers::{Lookup, ProbedStream, RankedSource, Selection, StreamFormat, Track};
use thiserror::Error;

pub const ADAPTIVE_REASON: &str = "verified same-source adaptive rendition available";
const MAX_PROBED_SOURCES: usize = 32 * 32 * 6;
const MAX_BACKUPS: usize = 128;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SelectionError {
    #[error("invalid source lookup")]
    InvalidLookup,
    #[error("source selection input exceeds its bound")]
    Capacity,
    #[error("source identities must be unique within a probe round")]
    DuplicateSource,
}

fn tracks(item: &ProbedStream) -> impl Iterator<Item = &Track> {
    item.source
        .resolver_metadata
        .tracks
        .iter()
        .chain(item.observation.playlist_metadata.tracks.iter())
        .filter(|track| {
            item.observation.variant.as_ref().is_none_or(|variant| {
                track.group.is_empty()
                    || match track.kind.as_str() {
                        "audio" => track.group == variant.audio_group,
                        "subtitles" => track.group == variant.subtitle_group,
                        _ => true,
                    }
            })
        })
}

fn language_rank(preferred: &[String], item: &ProbedStream, kind: &str) -> (usize, bool) {
    for (rank, language) in preferred.iter().enumerate() {
        if tracks(item).any(|track| {
            track.kind == kind
                && !track.language.is_empty()
                && track.language.eq_ignore_ascii_case(language)
        }) {
            return (rank, true);
        }
    }
    (preferred.len(), false)
}

fn height(item: &ProbedStream) -> i64 {
    item.observation
        .variant
        .as_ref()
        .and_then(|variant| variant.height)
        .unwrap_or(0)
}

fn established(item: &ProbedStream) -> bool {
    item.observation
        .variant
        .as_ref()
        .and_then(|variant| variant.bitrate)
        .is_some_and(|rate| rate > 0)
}

/// A structural or eligibility failure is never promoted to measured evidence.
pub fn eligible(
    lookup: &Lookup,
    item: &ProbedStream,
    now: DateTime<Utc>,
) -> Result<(), &'static str> {
    if !lookup.title.compatible_with(&item.source.title) {
        return Err("title or episode mismatch");
    }
    if item
        .source
        .expires_at
        .is_some_and(|expiry| expiry.signed_duration_since(now) <= Duration::seconds(5))
    {
        return Err("source expired or near expiry");
    }
    let observation = &item.observation;
    if !observation.accessible || observation.failure.is_some() {
        return Err("media sample unavailable");
    }
    if !lookup.device.formats.contains(&observation.format) {
        return Err("device format unsupported");
    }
    if !observation.protection_known
        || (observation.encrypted && !lookup.device.supports_encrypted_hls)
    {
        return Err("encryption unsupported or unknown");
    }
    if lookup.preferences.require_audio
        && !language_rank(&lookup.preferences.audio_languages, item, "audio").1
    {
        return Err("required audio language not established");
    }
    if lookup.preferences.require_subtitles
        && !language_rank(&lookup.preferences.subtitle_languages, item, "subtitles").1
    {
        return Err("required subtitles not established");
    }
    let variant = observation.variant.as_ref();
    match variant.and_then(|variant| variant.height) {
        None if !lookup.device.allow_unknown_resolution => return Err("resolution unknown"),
        Some(height) if height <= 0 => return Err("invalid resolution height"),
        Some(height) if lookup.device.max_height > 0 && height > lookup.device.max_height => {
            return Err("device resolution limit");
        }
        Some(height)
            if lookup.preferences.max_height > 0 && height > lookup.preferences.max_height =>
        {
            return Err("preferred resolution limit");
        }
        _ => {}
    }
    match variant.filter(|variant| !variant.codecs.is_empty()) {
        None if !lookup.device.allow_unknown_codecs => return Err("codecs unknown"),
        Some(variant) => {
            for codec in &variant.codecs {
                let codec = codec.to_ascii_lowercase();
                if !lookup.device.codecs.iter().any(|allowed| {
                    let allowed = allowed.to_ascii_lowercase();
                    codec == allowed
                        || codec
                            .strip_prefix(&allowed)
                            .is_some_and(|suffix| suffix.starts_with('.'))
                }) {
                    return Err("codec unsupported");
                }
            }
        }
        _ => {}
    }
    let Some(delivery) = observation
        .delivery_bps
        .filter(|delivery| delivery.is_finite() && *delivery > 0.0)
    else {
        return Err("delivery speed not measured");
    };
    if let Some(bitrate) = variant.and_then(|variant| variant.bitrate) {
        if bitrate <= 0 {
            return Err("declared bitrate invalid");
        }
        if delivery < bitrate as f64 * 1.5 {
            return Err("insufficient estimated delivery headroom");
        }
    }
    if item.validate().is_err() {
        return Err("invalid source or measurement structure");
    }
    Ok(())
}

fn codec_families(codecs: &[String]) -> Vec<String> {
    let mut families: Vec<_> = codecs
        .iter()
        .map(|codec| codec.split('.').next().unwrap_or("").to_ascii_lowercase())
        .collect();
    families.sort_unstable();
    families
}

fn compatible_codecs(first: &[String], second: &[String]) -> bool {
    !first.is_empty()
        && first.len() == second.len()
        && codec_families(first) == codec_families(second)
}

fn fresh(item: &ProbedStream, now: DateTime<Utc>) -> bool {
    let age = now.signed_duration_since(item.observation.at);
    age >= Duration::zero() && age <= Duration::seconds(15)
}

fn same_ladder(higher: &ProbedStream, lower: &ProbedStream) -> bool {
    let (Some(top), Some(bottom)) = (&higher.observation.variant, &lower.observation.variant)
    else {
        return false;
    };
    lower.observation.format == StreamFormat::Hls
        && lower.source.url == higher.source.url
        && lower.source.provider == higher.source.provider
        && lower.source.fetch_context == higher.source.fetch_context
        && height(lower) < height(higher)
        && compatible_codecs(&top.codecs, &bottom.codecs)
        && top.audio_group == bottom.audio_group
        && top.subtitle_group == bottom.subtitle_group
}

fn adaptive(item: &ProbedStream, alternatives: &[RankedSource], now: DateTime<Utc>) -> bool {
    if item.observation.format != StreamFormat::Hls
        || item.observation.samples_completed < 3
        || item
            .observation
            .variant
            .as_ref()
            .is_none_or(|variant| variant.width.is_none() || variant.height.is_none())
    {
        return false;
    }
    alternatives.iter().any(|alternative| {
        let lower = &alternative.candidate;
        same_ladder(item, lower)
            && lower.observation.samples_completed >= 3
            && fresh(lower, now)
            && lower
                .observation
                .variant
                .as_ref()
                .is_some_and(|variant| variant.width.is_some() && variant.height.is_some())
    })
}

fn rank(lookup: &Lookup, item: ProbedStream) -> RankedSource {
    let mut reasons = vec![
        "requested title coordinates match resolver assignment".into(),
        "bounded media sample accessible".into(),
        "device eligibility satisfied".into(),
    ];
    reasons.push(
        if item.source.content_verified {
            "content independently verified"
        } else {
            "content identity has not been independently verified"
        }
        .into(),
    );
    for (kind, preferences) in [
        ("audio", &lookup.preferences.audio_languages),
        ("subtitles", &lookup.preferences.subtitle_languages),
    ] {
        if language_rank(preferences, &item, kind).1 {
            reasons.push(format!("{kind} preference available"));
        }
    }
    let observation = &item.observation;
    let mut score = if let (Some(bitrate), Some(delivery)) = (
        observation
            .variant
            .as_ref()
            .and_then(|variant| variant.bitrate),
        observation.delivery_bps,
    ) {
        reasons.push("estimated delivery exceeds declared bitrate with headroom".into());
        (delivery / bitrate as f64).min(4.0) * 10.0
    } else {
        reasons.push("declared bitrate unknown; sustainable delivery unconfirmed".into());
        5.0
    };
    if let Some(reliability) = observation.reliability {
        score += reliability * 25.0;
        reasons.push("recent local probe reliability considered".into());
    } else {
        reasons.push("historical reliability unknown".into());
    }
    if observation
        .variant
        .as_ref()
        .is_some_and(|variant| variant.height.is_some())
    {
        reasons.push("resolution prioritized within language and delivery tiers".into());
    }
    RankedSource {
        candidate: item,
        reasons: Some(reasons),
        score,
    }
}

/// Seven ordered criteria mirror the current Go policy, including verified
/// adaptive rendition availability. Score cannot outrank language or resolution.
pub fn select(
    lookup: &Lookup,
    items: Vec<ProbedStream>,
    now: DateTime<Utc>,
) -> Result<Selection, SelectionError> {
    lookup
        .validate()
        .map_err(|_| SelectionError::InvalidLookup)?;
    if items.len() > MAX_PROBED_SOURCES {
        return Err(SelectionError::Capacity);
    }
    let mut identities = BTreeSet::new();
    let mut rejected = BTreeMap::new();
    let mut ranked = Vec::with_capacity(items.len());
    for item in items {
        if !identities.insert(item.source.id.clone()) {
            return Err(SelectionError::DuplicateSource);
        }
        match eligible(lookup, &item, now) {
            Ok(()) => ranked.push(rank(lookup, item)),
            Err(reason) => {
                if rejected.len() < 1024 {
                    rejected.insert(item.source.id, reason.into());
                }
            }
        }
    }
    let adaptive_flags: Vec<_> = ranked
        .iter()
        .map(|item| adaptive(&item.candidate, &ranked, now))
        .collect();
    for (ranked, has_adaptive) in ranked.iter_mut().zip(adaptive_flags) {
        if has_adaptive {
            ranked
                .reasons
                .get_or_insert_with(Vec::new)
                .push(ADAPTIVE_REASON.into());
        }
    }
    ranked.sort_by_cached_key(|ranked| SortKey::new(lookup, ranked));
    let mut ranked = ranked.into_iter();
    let selected = ranked.next();
    // Keep wire payloads bounded even when every provider offers many renditions.
    let backups = ranked.take(MAX_BACKUPS).collect();
    Ok(Selection {
        rejected: Some(rejected),
        selected,
        backups: Some(backups),
        ..Selection::default()
    })
}

struct SortKey {
    audio: usize,
    subtitles: usize,
    established: bool,
    height: i64,
    adaptive: bool,
    score: f64,
    delay: Option<i64>,
    id: String,
}
impl SortKey {
    fn new(lookup: &Lookup, item: &RankedSource) -> Self {
        Self {
            audio: language_rank(
                &lookup.preferences.audio_languages,
                &item.candidate,
                "audio",
            )
            .0,
            subtitles: language_rank(
                &lookup.preferences.subtitle_languages,
                &item.candidate,
                "subtitles",
            )
            .0,
            established: established(&item.candidate),
            height: height(&item.candidate),
            adaptive: item
                .reasons
                .as_ref()
                .is_some_and(|reasons| reasons.iter().any(|reason| reason == ADAPTIVE_REASON)),
            score: item.score,
            delay: item.candidate.observation.first_byte,
            id: item.candidate.source.id.clone(),
        }
    }
}
impl Eq for SortKey {}
impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.audio
            .cmp(&other.audio)
            .then_with(|| self.subtitles.cmp(&other.subtitles))
            .then_with(|| other.established.cmp(&self.established))
            .then_with(|| other.height.cmp(&self.height))
            .then_with(|| other.adaptive.cmp(&self.adaptive))
            .then_with(|| other.score.total_cmp(&self.score))
            .then_with(|| self.delay.is_none().cmp(&other.delay.is_none()))
            .then_with(|| self.delay.cmp(&other.delay))
            .then_with(|| self.id.cmp(&other.id))
    }
}

/// Early startup is stricter than eventual selection: recent three-sample
/// evidence, the top requested languages, and verified same-source lower rungs.
pub fn startup_ready(lookup: &Lookup, result: &Selection, now: DateTime<Utc>) -> bool {
    if lookup.startup_height <= 0
        || lookup.refresh
        || lookup.validate().is_err()
        || result.validate().is_err()
    {
        return false;
    }
    let Some(selected) = &result.selected else {
        return false;
    };
    let item = &selected.candidate;
    if !fresh(item, now)
        || eligible(lookup, item, now).is_err()
        || !established(item)
        || item.observation.samples_completed < 3
    {
        return false;
    }
    let target = [lookup.device.max_height, lookup.preferences.max_height]
        .into_iter()
        .filter(|value| *value > 0)
        .fold(lookup.startup_height, i64::min);
    if height(item) < target {
        return false;
    }
    for (kind, preferences) in [
        ("audio", &lookup.preferences.audio_languages),
        ("subtitles", &lookup.preferences.subtitle_languages),
    ] {
        if !preferences.is_empty() && language_rank(preferences, item, kind) != (0, true) {
            return false;
        }
    }
    if item.observation.format == StreamFormat::Hls
        && let Some(variant) = &item.observation.variant
    {
        let needs_lower = item
            .observation
            .playlist_metadata
            .variants
            .iter()
            .any(|other| {
                other
                    .height
                    .is_some_and(|height| height < self::height(item))
                    && compatible_codecs(&other.codecs, &variant.codecs)
                    && other.audio_group == variant.audio_group
                    && other.subtitle_group == variant.subtitle_group
            });
        if needs_lower {
            return result.backups.iter().flatten().any(|ranked| {
                same_ladder(item, &ranked.candidate)
                    && ranked.candidate.observation.samples_completed >= 3
                    && fresh(&ranked.candidate, now)
                    && eligible(lookup, &ranked.candidate, now).is_ok()
            });
        }
    }
    true
}
