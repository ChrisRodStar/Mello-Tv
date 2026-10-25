use chrono::{TimeZone, Utc};
use serde_json::json;
use stream_core::accounts::{
    AuthorizedPlayback, Device, PlaybackGrantClaims, PlaybackMutation, PlaybackPreferences,
    PlaybackRequest, Preferences, Profile, ProfileInput, Progress, ProgressUpdate, RelayReady,
    Target, is_safe_identifier,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn profile_wire_matches_public_go_contract() -> TestResult {
    let wire = json!({
        "id": "profile_1", "name": "Zoë", "color": "#446e88",
        "preferences": {"completed": true, "format": "both", "genres": ["scifi", "animation"]}
    });
    let profile: Profile = serde_json::from_value(wire.clone())?;
    profile.validate()?;
    assert_eq!(serde_json::to_value(profile)?, wire);
    Ok(())
}

#[test]
fn profile_validation_uses_utf8_byte_limits() -> TestResult {
    let mut input = ProfileInput {
        name: "é".repeat(40),
        color: "blue".into(),
    };
    input.validate()?;
    input.name.push('é');
    assert!(input.validate().is_err());
    input.name = " \n\t".into();
    assert!(input.validate().is_err());
    input.name = "Name".into();
    input.color.clear();
    assert!(input.validate().is_err());
    assert!(
        serde_json::from_value::<ProfileInput>(json!({"name":"a","color":"b","admin":true}))
            .is_err()
    );
    Ok(())
}

#[test]
fn preferences_reject_duplicates_and_unknown_slugs() -> TestResult {
    for genres in [json!(["drama", "drama"]), json!(["unknown"])] {
        let preferences: Preferences =
            serde_json::from_value(json!({"completed":false,"format":"movie","genres":genres}))?;
        assert!(preferences.validate().is_err());
    }
    assert!(
        serde_json::from_value::<Preferences>(json!({"completed":false,"format":"both"})).is_err()
    );
    assert!(
        serde_json::from_value::<Preferences>(
            json!({"completed":false,"format":"all","genres":[]})
        )
        .is_err()
    );
    Preferences::default().validate()?;
    Ok(())
}

#[test]
fn go_nullable_slices_normalize_to_empty_without_omitting_required_keys() -> TestResult {
    let preferences: Preferences = serde_json::from_value(json!({
        "completed":false,"format":"both","genres":null
    }))?;
    preferences.validate()?;
    assert!(preferences.genres.is_empty());
    let request: PlaybackRequest = serde_json::from_value(json!({
        "type":"movie","id":603,
        "device":{"formats":["hls"],"codecs":null},
        "preferences":{"audio_languages":null,"subtitle_languages":null}
    }))?;
    request.validate()?;
    let mut ready = serde_json::to_value(relay_ready()?)?;
    ready["available_heights"] = serde_json::Value::Null;
    let ready: RelayReady = serde_json::from_value(ready)?;
    ready.validate()?;
    assert!(ready.available_heights.is_empty());
    Ok(())
}

#[test]
fn target_serializes_flat_coordinates_and_accepts_specials() -> TestResult {
    let wire = json!({"type":"tv","id":1622,"season":0,"episode":1});
    let target: Target = serde_json::from_value(wire.clone())?;
    assert_eq!(target.key(), "tv:1622:0:1");
    assert_eq!(target.season(), Some(0));
    assert_eq!(target.episode(), Some(1));
    assert_eq!(serde_json::to_value(target)?, wire);
    let movie: Target = serde_json::from_value(json!({"type":"movie","id":603}))?;
    assert_eq!(movie.key(), "movie:603");
    Ok(())
}

#[test]
fn invalid_coordinates_are_rejected_during_deserialization() {
    for wire in [
        json!({"type":"movie","id":0}),
        json!({"type":"movie","id":-1}),
        json!({"type":"movie","id":603,"season":0}),
        json!({"type":"tv","id":1622}),
        json!({"type":"tv","id":1622,"season":-1,"episode":1}),
        json!({"type":"tv","id":1622,"season":10001,"episode":1}),
        json!({"type":"tv","id":1622,"season":1,"episode":0}),
        json!({"type":"tv","id":1622,"season":1,"episode":100001}),
        json!({"type":"anime","id":1622}),
    ] {
        assert!(serde_json::from_value::<Target>(wire).is_err());
    }
}

#[test]
fn playback_request_preserves_flat_target_and_optional_fields() -> TestResult {
    let wire = json!({
        "type":"tv","id":1622,"season":0,"episode":1,"imdb_id":"tt2861424","mal_id":1,
        "device":{"formats":["hls"],"max_height":2160,"allow_unknown_codecs":false,
                  "allow_unknown_resolution":false,"supports_encrypted_hls":true},
        "preferences":{"audio_languages":["en"],"require_audio":true,"require_subtitles":false}
    });
    let request: PlaybackRequest = serde_json::from_value(wire.clone())?;
    request.validate()?;
    assert_eq!(serde_json::to_value(request)?, wire);
    let simple: PlaybackRequest = serde_json::from_value(json!({"type":"movie","id":603}))?;
    simple.validate()?;
    assert_eq!(
        serde_json::to_value(simple)?,
        json!({"type":"movie","id":603})
    );
    Ok(())
}

#[test]
fn playback_request_rejects_invalid_cross_ids_and_device_bounds() -> TestResult {
    let mut request: PlaybackRequest = serde_json::from_value(json!({"type":"movie","id":603}))?;
    for id in ["tt", "TT123", "tt12x", "tt１２", "tt123 "] {
        request.imdb_id = id.into();
        assert!(request.validate().is_err());
    }
    request.imdb_id.clear();
    request.mal_id = Some(0);
    assert!(request.validate().is_err());
    let device: Device = serde_json::from_value(json!({"formats":[],"max_height":-1}))?;
    assert!(device.validate().is_err());
    let device: Device = serde_json::from_value(json!({"formats":["hls"],"max_height":16385}))?;
    assert!(device.validate().is_err());
    assert!(serde_json::from_value::<Device>(json!({"formats":["dash"]})).is_err());
    let preferences = PlaybackPreferences {
        audio_languages: vec!["en".into(); 17],
        ..PlaybackPreferences::default()
    };
    assert!(preferences.validate().is_err());
    Ok(())
}

fn progress_update() -> ProgressUpdate {
    ProgressUpdate {
        playback_id: "playback_1".into(),
        sequence: 1,
        position_seconds: 30.0,
        duration_seconds: 120.0,
        watched_seconds: 30.0,
    }
}

#[test]
fn progress_request_requires_all_fields_and_rejects_nonfinite_values() -> TestResult {
    let update = progress_update();
    update.validate()?;
    let wire = serde_json::to_value(&update)?;
    assert_eq!(wire["sequence"], 1);
    assert_eq!(wire["position_seconds"], 30.0);
    assert!(
        serde_json::from_value::<ProgressUpdate>(json!({"playback_id":"p","sequence":1})).is_err()
    );
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let mut update = progress_update();
        update.position_seconds = invalid;
        assert!(update.validate().is_err());
        assert!(serde_json::to_value(&update).is_err());
        update.position_seconds = 1.0;
        update.duration_seconds = invalid;
        assert!(update.validate().is_err());
        update.duration_seconds = 1.0;
        update.watched_seconds = invalid;
        assert!(update.validate().is_err());
    }
    Ok(())
}

#[test]
fn progress_bounds_match_go_and_do_not_forbid_seeking() -> TestResult {
    let mut update = progress_update();
    update.position_seconds = 86_400.0;
    update.duration_seconds = 0.0;
    update.watched_seconds = 604_800.0;
    update.validate()?;
    update.watched_seconds += 1.0;
    assert!(update.validate().is_err());
    update.watched_seconds = 1.0;
    update.position_seconds += 1.0;
    assert!(update.validate().is_err());
    update.position_seconds = 1.0;
    update.sequence = 0;
    assert!(update.validate().is_err());
    // Stored aggregate history may legitimately exceed a single playback cap.
    let progress: Progress = serde_json::from_value(json!({
        "type":"movie","id":603,"position_seconds":100,"duration_seconds":120,
        "watched_seconds":1_000_000,"completed":true,"updated_at":"2026-10-25T12:00:00Z"
    }))?;
    progress.validate()?;
    let mut bad_progress = progress.clone();
    bad_progress.updated_at = chrono::DateTime::from_timestamp(-1, 0).ok_or("valid pre-epoch timestamp")?;
    assert!(bad_progress.validate().is_err());
    bad_progress.updated_at = chrono::DateTime::from_timestamp(-62_135_596_800, 0).ok_or("valid go-zero timestamp")?;
    assert!(bad_progress.validate().is_err());
    Ok(())
}

fn relay_ready() -> Result<RelayReady, serde_json::Error> {
    serde_json::from_value(json!({
        "contract_version":"relay.v1","session_id":"session_1","entry_path":"/stream/session_1/entry",
        "format":"hls","expires_at":"2026-10-25T12:00:00Z","revision":2,"auth_revision":1,
        "source_generation":3,"state":"active","resume_required":false,"available_heights":[1080,720]
    }))
}

#[test]
fn relay_ready_validates_local_entry_and_independent_revisions() -> TestResult {
    let mut ready = relay_ready()?;
    ready.validate()?;
    assert_ne!(ready.revision, ready.auth_revision);
    ready.entry_path = "https://untrusted.example/stream/session_1/entry".into();
    assert!(ready.validate().is_err());
    ready.entry_path = "/stream/session_1/entry".into();
    ready.auth_revision = 0;
    assert!(ready.validate().is_err());
    ready.auth_revision = 1;
    ready.expires_at = Utc
        .with_ymd_and_hms(1, 1, 1, 0, 0, 0)
        .single()
        .ok_or("invalid timestamp")?;
    assert!(ready.validate().is_err());
    Ok(())
}

#[test]
fn relay_height_validation_rejects_duplicates_ranges_and_mp4_ladders() -> TestResult {
    let mut ready = relay_ready()?;
    for heights in [
        vec![720, 720],
        vec![0],
        vec![16_385],
        vec![-1],
        (1..=33).collect(),
    ] {
        ready.available_heights = heights;
        assert!(ready.validate().is_err());
    }
    ready.available_heights = vec![720];
    ready.format = serde_json::from_value(json!("mp4"))?;
    assert!(ready.validate().is_err());
    ready.available_heights.clear();
    ready.validate()?;
    Ok(())
}

#[test]
fn identifiers_reject_non_ascii_and_path_characters() {
    assert!(is_safe_identifier("ABC_def-123"));
    for value in ["", "../profile", "a/b", "a?b", "é", " a", "a b"] {
        assert!(!is_safe_identifier(value));
    }
    assert!(is_safe_identifier(&"a".repeat(128)));
    assert!(!is_safe_identifier(&"a".repeat(129)));
}

#[test]
fn playback_debug_redacts_grant_and_claims_keep_auth_revision() -> TestResult {
    let playback: AuthorizedPlayback = serde_json::from_value(json!({
        "id":"playback_1","title":{"type":"movie","id":603},
        "url":"/stream/session_1/entry?st=super-secret-token","format":"hls",
        "available_heights":[1080],"expires_at":"2026-10-25T12:00:00Z","revision":3,
        "source_generation":2,"reload_required":false,"resume_seconds":15
    }))?;
    let debug = format!("{playback:?}");
    assert!(!debug.contains("super-secret-token"));
    assert!(debug.contains("[REDACTED]"));
    let wire = json!({"iss":"family-stream-api","aud":"family-stream-relay","sub":"profile_1",
        "sid":"session_1","arv":1,"iat":1792900000,"nbf":1792900000,"exp":1792900300,"jti":"grant_1"});
    let claims: PlaybackGrantClaims = serde_json::from_value(wire.clone())?;
    assert_eq!(claims.auth_revision, 1);
    assert_eq!(serde_json::to_value(claims)?, wire);
    assert!(
        PlaybackMutation {
            expected_revision: 0
        }
        .validate()
        .is_err()
    );
    PlaybackMutation {
        expected_revision: 1,
    }
    .validate()?;
    Ok(())
}
