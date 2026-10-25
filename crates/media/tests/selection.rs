use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use stream_core::providers::{Lookup, ProbedStream, Selection, Track};
use stream_media::selection::{ADAPTIVE_REASON, SelectionError, eligible, select, startup_ready};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn now() -> Result<DateTime<Utc>, chrono::ParseError> {
    "2026-10-25T12:00:00Z".parse()
}
fn lookup() -> Result<Lookup, serde_json::Error> {
    serde_json::from_value(
        json!({"title":{"type":"tv","tmdb_id":60625,"season":1,"episode":1},"preferences":{},"device":{"formats":["hls"],"codecs":["avc1"]}}),
    )
}
fn source(id: &str, height: i64, delivery: f64) -> Result<ProbedStream, serde_json::Error> {
    serde_json::from_value(json!({
        "source":{"id":id,"provider":"vidcore","url":"https://cdn.example/master.m3u8","format":"hls","title":{"type":"tv","tmdb_id":60625,"season":1,"episode":1},"fetch_context":{},"resolver_metadata":{"tracks_known":false},"content_verified":false},
        "observation":{"at":"2026-10-25T12:00:00Z","delivery_bps":delivery,"variant":{"url":format!("https://cdn.example/{height}.m3u8"),"width":1920,"height":height,"bitrate":6000000,"codecs":["avc1.4d401f"]},"sample_bytes":393216,"format":"hls","playlist_metadata":{"tracks_known":true},"samples_completed":3,"accessible":true,"encrypted":false,"protection_known":true}
    }))
}
fn ids(selection: &Selection) -> Vec<&str> {
    selection
        .selected
        .iter()
        .chain(selection.backups.iter().flatten())
        .map(|ranked| ranked.candidate.source.id.as_str())
        .collect()
}

#[test]
fn preserves_go_quality_first_order_under_input_permutations() -> TestResult {
    let mut sources = vec![
        source("480", 480, 60_000_000.0)?,
        source("1080-backup", 1080, 10_000_000.0)?,
        source("720", 720, 30_000_000.0)?,
        source("1080-primary", 1080, 16_400_000.0)?,
    ];
    for _ in 0..sources.len() {
        let result = select(&lookup()?, sources.clone(), now()?)?;
        assert_eq!(
            ids(&result),
            vec!["1080-primary", "1080-backup", "720", "480"]
        );
        result.validate()?;
        sources.rotate_left(1);
    }
    Ok(())
}

#[test]
fn language_precedes_quality_and_variant_groups_scope_tracks() -> TestResult {
    let mut request = lookup()?;
    request.preferences.audio_languages = vec!["en".into(), "ja".into()];
    let mut high = source("high", 1080, 12_000_000.0)?;
    high.observation.playlist_metadata.tracks = vec![Track {
        kind: "audio".into(),
        language: "ja".into(),
        ..Track::default()
    }];
    let mut medium = source("medium", 720, 12_000_000.0)?;
    medium.observation.playlist_metadata.tracks = vec![Track {
        kind: "audio".into(),
        language: "EN".into(),
        group: "en-group".into(),
        ..Track::default()
    }];
    if let Some(variant) = &mut medium.observation.variant {
        variant.audio_group = "en-group".into();
    }
    assert_eq!(
        ids(&select(
            &request,
            vec![high.clone(), medium.clone()],
            now()?
        )?),
        vec!["medium", "high"]
    );
    if let Some(variant) = &mut medium.observation.variant {
        variant.audio_group = "other-group".into();
    }
    assert_eq!(
        ids(&select(&request, vec![high, medium], now()?)?),
        vec!["high", "medium"]
    );
    Ok(())
}

#[test]
fn established_bitrate_precedes_unknown_resolution_advantage() -> TestResult {
    let known = source("known", 720, 12_000_000.0)?;
    let mut unknown = source("unknown", 1080, 30_000_000.0)?;
    if let Some(variant) = &mut unknown.observation.variant {
        variant.bitrate = None;
    }
    assert_eq!(
        ids(&select(&lookup()?, vec![unknown, known], now()?)?),
        vec!["known", "unknown"]
    );
    Ok(())
}

#[test]
fn rejects_invalid_measurements_headroom_expiry_and_coordinates() -> TestResult {
    let request = lookup()?;
    for delivery in [0.0, -1.0, f64::NAN, f64::INFINITY, 8_000_000.0] {
        let mut item = source("bad", 1080, 12_000_000.0)?;
        item.observation.delivery_bps = Some(delivery);
        assert!(select(&request, vec![item], now()?)?.selected.is_none());
    }
    let mut item = source("bad", 1080, 12_000_000.0)?;
    item.source.expires_at = Some(now()? + Duration::seconds(5));
    assert_eq!(
        eligible(&request, &item, now()?),
        Err("source expired or near expiry")
    );
    item.source.expires_at = None;
    item.source.title.episode = Some(2);
    assert_eq!(
        eligible(&request, &item, now()?),
        Err("title or episode mismatch")
    );
    Ok(())
}

#[test]
fn known_latency_and_id_break_ties_deterministically() -> TestResult {
    let mut first = source("a", 1080, 12_000_000.0)?;
    let mut second = source("b", 1080, 12_000_000.0)?;
    first.observation.first_byte = Some(500_000_000);
    second.observation.first_byte = Some(100_000_000);
    assert_eq!(
        ids(&select(
            &lookup()?,
            vec![first.clone(), second.clone()],
            now()?
        )?),
        vec!["b", "a"]
    );
    first.observation.first_byte = None;
    assert_eq!(
        ids(&select(
            &lookup()?,
            vec![first.clone(), second.clone()],
            now()?
        )?),
        vec!["b", "a"]
    );
    second.observation.first_byte = None;
    assert_eq!(
        ids(&select(&lookup()?, vec![second, first], now()?)?),
        vec!["a", "b"]
    );
    Ok(())
}

#[test]
fn adaptive_tier_requires_verified_same_master_and_context() -> TestResult {
    let with_lower = source("adaptive", 1080, 12_000_000.0)?;
    let mut without_lower = source("standalone", 1080, 24_000_000.0)?;
    without_lower.source.url = "https://other.example/master.m3u8".into();
    let mut lower = source("lower", 720, 12_000_000.0)?;
    let result = select(
        &lookup()?,
        vec![with_lower.clone(), without_lower.clone(), lower.clone()],
        now()?,
    )?;
    assert_eq!(ids(&result).first(), Some(&"adaptive"));
    assert!(
        result
            .selected
            .as_ref()
            .and_then(|item| item.reasons.as_ref())
            .is_some_and(|reasons| reasons.iter().any(|reason| reason == ADAPTIVE_REASON))
    );
    lower.observation.at = now()? - Duration::seconds(16);
    assert_eq!(
        ids(&select(
            &lookup()?,
            vec![with_lower, without_lower, lower],
            now()?
        )?)
        .first(),
        Some(&"standalone")
    );
    Ok(())
}

#[test]
fn startup_requires_recent_samples_and_an_advertised_compatible_lower_rung() -> TestResult {
    let mut request = lookup()?;
    request.startup_height = 1080;
    let mut high = source("high", 1080, 12_000_000.0)?;
    let lower = source("lower", 720, 12_000_000.0)?;
    high.observation.playlist_metadata.variants =
        lower.observation.variant.iter().cloned().collect();
    let incomplete = select(&request, vec![high.clone()], now()?)?;
    assert!(!startup_ready(&request, &incomplete, now()?));
    let complete = select(&request, vec![high.clone(), lower], now()?)?;
    assert!(startup_ready(&request, &complete, now()?));
    assert!(!startup_ready(
        &request,
        &complete,
        now()? + Duration::seconds(16)
    ));
    request.refresh = true;
    assert!(!startup_ready(&request, &complete, now()?));
    request.refresh = false;
    high.observation.samples_completed = 2;
    assert!(!startup_ready(
        &request,
        &select(&request, vec![high], now()?)?,
        now()?
    ));
    Ok(())
}

#[test]
fn duplicate_sources_and_invalid_lookup_fail_without_ambiguous_results() -> TestResult {
    let item = source("same", 1080, 12_000_000.0)?;
    assert!(matches!(
        select(&lookup()?, vec![item.clone(), item], now()?),
        Err(SelectionError::DuplicateSource)
    ));
    let mut request = lookup()?;
    request.device.formats.clear();
    assert!(matches!(
        select(&request, Vec::new(), now()?),
        Err(SelectionError::InvalidLookup)
    ));
    Ok(())
}
