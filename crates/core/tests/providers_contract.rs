//! Contract fixtures are transcribed from Go model field declarations and test
//! vectors; no test calls providers, DNS, sockets, or the Go toolchain.
use std::error::Error;

use serde_json::{Value, json};
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, DeviceCapabilities, ErrorCategory, FetchContext,
        FetchScope, Lookup, ProbeObservation, ProviderFailure, ProviderId, ProviderTitle,
        QualityPreference, Selection, StreamFormat, TmdbId,
    },
};

type TestResult = Result<(), Box<dyn Error>>;

fn selection() -> Result<Selection, serde_json::Error> {
    serde_json::from_str(include_str!("fixtures/providers-selection-v1.json"))
}

fn candidate() -> Result<CandidateStream, Box<dyn Error>> {
    selection()?
        .selected
        .map(|ranked| ranked.candidate.source)
        .ok_or_else(|| "fixture lacks selected candidate".into())
}

fn observation() -> Result<ProbeObservation, Box<dyn Error>> {
    selection()?
        .selected
        .map(|ranked| ranked.candidate.observation)
        .ok_or_else(|| "fixture lacks observation".into())
}

#[test]
fn go_title_validation_vectors() -> TestResult {
    let vectors = [
        (json!({"type":"movie","tmdb_id":100}), true),
        (
            json!({"type":"movie","tmdb_id":100,"imdb_id":"tt1234567","my_anime_list_id":456}),
            true,
        ),
        (json!({"type":"movie","tmdb_id":100,"season":1}), false),
        (json!({"type":"movie","tmdb_id":100,"episode":1}), false),
        (json!({"type":"movie","tmdb_id":0}), false),
        (json!({"type":"movie","tmdb_id":-10}), false),
        (json!({"type":"anime","tmdb_id":100}), false),
        (
            json!({"type":"tv","tmdb_id":200,"season":1,"episode":1}),
            true,
        ),
        (
            json!({"type":"tv","tmdb_id":200,"season":0,"episode":1}),
            true,
        ),
        (json!({"type":"tv","tmdb_id":200,"episode":1}), false),
        (json!({"type":"tv","tmdb_id":200,"season":1}), false),
        (
            json!({"type":"tv","tmdb_id":200,"season":-1,"episode":1}),
            false,
        ),
        (
            json!({"type":"tv","tmdb_id":200,"season":0,"episode":0}),
            false,
        ),
        (
            json!({"type":"tv","tmdb_id":200,"season":1,"episode":-1}),
            false,
        ),
        (
            json!({"type":"movie","tmdb_id":100,"my_anime_list_id":-5}),
            false,
        ),
        (
            json!({"type":"movie","tmdb_id":100,"imdb_id":"tt123456789012345678901234567890123"}),
            false,
        ),
        (
            json!({"type":"movie","tmdb_id":100,"imdb_id":" tt1234567 "}),
            false,
        ),
    ];
    for (wire, valid) in vectors {
        let accepted = serde_json::from_value::<ProviderTitle>(wire.clone())
            .is_ok_and(|title| title.validate().is_ok());
        assert_eq!(accepted, valid, "title vector: {wire}");
    }
    Ok(())
}

#[test]
fn title_identity_distinguishes_coordinates_and_preserves_cross_ids() -> TestResult {
    let id = TmdbId::try_from(300)?;
    let first = ProviderTitle::episode(id, 1, 1)?;
    assert!(first.same_content(&first.clone()));
    assert!(!first.same_content(&ProviderTitle::episode(id, 1, 2)?));
    assert!(!first.same_content(&ProviderTitle::episode(id, 2, 1)?));
    assert!(!first.same_content(&ProviderTitle::movie(id)));
    let mut cross_a = first.clone();
    cross_a.imdb_id = "tt0460681".to_owned();
    let mut cross_b = cross_a.clone();
    cross_b.imdb_id = "tt1234567".to_owned();
    assert!(cross_a.same_content(&cross_b));
    assert!(!cross_a.compatible_with(&cross_b));
    let encoded = serde_json::to_value(&cross_a)?;
    assert_eq!(encoded["tmdb_id"], json!(300));
    assert_eq!(encoded["type"], json!("tv"));
    assert_eq!(encoded["imdb_id"], json!("tt0460681"));
    assert!(encoded.get("media_type").is_none());
    Ok(())
}

#[test]
fn positive_identifier_is_enforced_during_deserialization() -> TestResult {
    for raw in ["0", "-1", "9223372036854775808", "1.5", "\"123\""] {
        assert!(serde_json::from_str::<TmdbId>(raw).is_err());
    }
    assert_eq!(
        serde_json::from_str::<TmdbId>("9223372036854775807")?.get(),
        i64::MAX
    );
    Ok(())
}

#[test]
fn provider_ids_are_extensible_but_cannot_be_paths() -> TestResult {
    for name in ProviderId::BUILT_IN
        .into_iter()
        .chain(["community-provider_2"])
    {
        let provider = ProviderId::new(name)?;
        assert_eq!(provider.as_str(), name);
        assert_eq!(serde_json::from_value::<ProviderId>(json!(name))?, provider);
    }
    for name in [
        "",
        "VidCore",
        "../vidcore",
        "foo/bar",
        "foo\\bar",
        "-leading",
        "éxample",
        "test\n",
    ] {
        assert!(ProviderId::new(name).is_err());
        assert!(serde_json::from_value::<ProviderId>(json!(name)).is_err());
    }
    assert!(ProviderId::new("a".repeat(65)).is_err());
    Ok(())
}

#[test]
fn complete_selection_round_trips_wire_names_nulls_and_nanoseconds() -> TestResult {
    let original: Value =
        serde_json::from_str(include_str!("fixtures/providers-selection-v1.json"))?;
    let decoded = selection()?;
    decoded.validate()?;
    let encoded = serde_json::to_value(&decoded)?;
    assert_eq!(encoded, original);
    assert_eq!(
        encoded["selected"]["candidate"]["observation"]["first_byte"],
        json!(125000001)
    );
    assert_eq!(encoded["providers"][0]["duration"], json!(500000000));
    assert_eq!(
        encoded["selected"]["candidate"]["observation"]["at"],
        json!("2026-10-25T15:00:00.123456789Z")
    );
    Ok(())
}

#[test]
fn required_nullable_result_fields_do_not_disappear() -> TestResult {
    let empty = Selection::default();
    empty.validate()?;
    let wire = serde_json::to_value(&empty)?;
    assert_eq!(
        wire,
        json!({"version":"providers.v1","backups":null,"rejected":null,"providers":null})
    );
    for field in ["backups", "rejected", "providers"] {
        let mut missing = wire.clone();
        if let Some(object) = missing.as_object_mut() {
            object.remove(field);
        }
        assert!(serde_json::from_value::<Selection>(missing).is_err());
    }
    Ok(())
}

#[test]
fn lookup_context_is_trusted_out_of_band_and_never_serialized() -> TestResult {
    let mut lookup: Lookup = serde_json::from_value(json!({
        "title":{"type":"tv","tmdb_id":100,"season":1,"episode":1},
        "device":{"formats":["hls"]}, "startup_height":1080
    }))?;
    lookup.validate()?;
    assert!(lookup.context_key.is_empty());
    assert!(lookup.validate_context().is_err());
    lookup.context_key = "household-123".to_owned();
    lookup.validate_context()?;
    assert!(serde_json::to_value(&lookup)?.get("context_key").is_none());
    for height in [-5, 16385] {
        lookup.startup_height = height;
        assert!(lookup.validate().is_err());
    }
    lookup.startup_height = 0;
    lookup.preferences.max_height = -1;
    assert!(lookup.validate().is_err());
    Ok(())
}

#[test]
fn request_decoder_rejects_unknown_fields_and_forged_context() -> TestResult {
    let wire = json!({
        "title":{"type":"tv","tmdb_id":100,"season":1,"episode":1},
        "preferences":{}, "device":{"formats":["hls"]}
    });
    let mut forged = wire.clone();
    forged["context_key"] = json!("attacker-supplied");
    assert!(serde_json::from_value::<Lookup>(forged).is_err());
    for field in ["title", "preferences", "device"] {
        let mut unknown_nested = wire.clone();
        unknown_nested[field]["unexpected"] = json!(true);
        assert!(serde_json::from_value::<Lookup>(unknown_nested).is_err());
    }
    serde_json::from_value::<Lookup>(wire)?.validate()?;
    Ok(())
}

#[test]
fn device_and_preferences_bound_unknown_inputs() -> TestResult {
    let mut device = DeviceCapabilities::default();
    assert!(device.validate().is_err());
    device.formats = vec![StreamFormat::Hls, StreamFormat::Mp4];
    device.validate()?;
    device.formats.push(StreamFormat::Unknown);
    assert!(device.validate().is_err());
    device.formats = vec![StreamFormat::Hls];
    device.codecs = vec!["avc1".to_owned(); 33];
    assert!(device.validate().is_err());
    let mut preferences = QualityPreference {
        audio_languages: vec!["en".to_owned(); 33],
        ..QualityPreference::default()
    };
    assert!(preferences.validate().is_err());
    preferences.audio_languages = vec!["".to_owned()];
    assert!(preferences.validate().is_err());
    preferences.audio_languages = vec!["é".repeat(33)];
    assert!(preferences.validate().is_err());
    Ok(())
}

#[test]
fn scoped_credentials_reject_ambiguity_and_header_injection() -> TestResult {
    let scope = FetchScope {
        origin: "https://media.example.com".to_owned(),
        path_prefix: "/media".to_owned(),
        cookies: "session=secret".to_owned(),
        cookie_path: "/media".to_owned(),
        ..FetchScope::default()
    };
    let mut context = FetchContext {
        scopes: vec![scope],
        public_resources: None,
    };
    context.validate()?;
    let Some(first) = context.scopes.first_mut() else {
        return Err("fixture scope missing".into());
    };
    first.headers.insert(
        "Authorization".to_owned(),
        "secret\r\nInjected: yes".to_owned(),
    );
    assert!(context.validate().is_err());
    let Some(first) = context.scopes.first_mut() else {
        return Err("fixture scope missing".into());
    };
    first.headers.clear();
    first
        .headers
        .insert("Authorization".to_owned(), "one".to_owned());
    first
        .headers
        .insert("authorization".to_owned(), "two".to_owned());
    assert!(context.validate().is_err());
    let Some(first) = context.scopes.first_mut() else {
        return Err("fixture scope missing".into());
    };
    first.headers.clear();
    first.cookie_path.clear();
    assert!(context.validate().is_err());
    let Some(first) = context.scopes.first_mut() else {
        return Err("fixture scope missing".into());
    };
    first.cookies.clear();
    for path in [
        "relative",
        "/media%2fprivate",
        "/media/../private",
        "/media//private",
        "/media?x=1",
        "/media\\private",
    ] {
        first.path_prefix = path.to_owned();
        assert!(
            FetchContext {
                scopes: vec![first.clone()],
                public_resources: None
            }
            .validate()
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn candidate_diagnostics_do_not_expose_urls_cookies_or_headers() -> TestResult {
    let candidate = candidate()?;
    let diagnostic = format!("{candidate:?} {:?}", candidate.fetch_context);
    for secret in [
        "secret-token",
        "secret-cookie",
        "Authorization",
        "media.example.com",
    ] {
        assert!(!diagnostic.contains(secret));
    }
    assert!(diagnostic.contains("REDACTED"));
    Ok(())
}

#[test]
fn candidate_validation_rejects_invalid_urls_and_ownership() -> TestResult {
    let mut candidate = candidate()?;
    candidate.validate_for(&ProviderId::new("vidlink")?, &candidate.title)?;
    assert!(
        candidate
            .validate_for(&ProviderId::new("videasy")?, &candidate.title)
            .is_err()
    );
    assert!(
        candidate
            .validate_for(
                &candidate.provider,
                &ProviderTitle::movie(TmdbId::try_from(603)?)
            )
            .is_err()
    );
    for url in [
        "file:///etc/passwd",
        "https://user:password@example.com/video",
        "https://example.com/video#fragment",
        "https://example.com\\video",
        "https://example.com/video\r\n",
    ] {
        candidate.url = url.to_owned();
        assert!(candidate.validate().is_err());
    }
    Ok(())
}

#[test]
fn malformed_measurements_never_become_valid_evidence() -> TestResult {
    let original = observation()?;
    for throughput in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let mut observation = original.clone();
        observation.delivery_bps = Some(throughput);
        assert!(observation.validate().is_err());
    }
    for reliability in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut observation = original.clone();
        observation.reliability = Some(reliability);
        assert!(observation.validate().is_err());
    }
    let mut invalid = original.clone();
    invalid.first_byte = Some(-1);
    assert!(invalid.validate().is_err());
    invalid = original.clone();
    invalid.sample_bytes = -1;
    assert!(invalid.validate().is_err());
    invalid = original;
    invalid.samples_completed = -1;
    assert!(invalid.validate().is_err());
    Ok(())
}

#[test]
fn nonfinite_measurements_fail_serialization_instead_of_becoming_null() -> TestResult {
    let original = observation()?;
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut throughput = original.clone();
        throughput.delivery_bps = Some(invalid);
        assert!(serde_json::to_string(&throughput).is_err());
        let mut reliability = original.clone();
        reliability.reliability = Some(invalid);
        assert!(serde_json::to_string(&reliability).is_err());
        let mut result = selection()?;
        let Some(ranked) = &mut result.selected else {
            return Err("fixture lacks selected result".into());
        };
        ranked.score = invalid;
        assert!(serde_json::to_string(&result).is_err());
    }
    Ok(())
}

#[test]
fn adapter_results_enforce_caps_and_unique_identity() -> TestResult {
    let candidate = candidate()?;
    let provider = candidate.provider.clone();
    let title = candidate.title.clone();
    let mut result = AdapterResult {
        candidates: Some(vec![candidate.clone()]),
        issues: vec![],
        resolver_duration: 1000000,
    };
    result.validate_for(&provider, &title, 1)?;
    assert!(result.validate_for(&provider, &title, 0).is_err());
    result.candidates = Some(vec![candidate.clone(), candidate]);
    assert!(result.validate_for(&provider, &title, 2).is_err());
    result.candidates = None;
    result.resolver_duration = -1;
    assert!(result.validate_for(&provider, &title, 2).is_err());
    Ok(())
}

#[test]
fn selection_rejects_unknown_versions_nonfinite_scores_and_duplicate_backups() -> TestResult {
    let mut result = selection()?;
    result.version = "providers.v2".to_owned();
    assert!(result.validate().is_err());
    result.version = "providers.v1".to_owned();
    let Some(mut ranked) = result.selected.clone() else {
        return Err("selected fixture missing".into());
    };
    result.backups = Some(vec![ranked.clone()]);
    assert!(result.validate().is_err());
    result.backups = None;
    ranked.score = f64::NAN;
    result.selected = Some(ranked);
    assert!(result.validate().is_err());
    Ok(())
}

#[test]
fn error_categories_retain_cancellation_spelling_and_sanitized_validation() -> TestResult {
    let failure = ProviderFailure {
        category: ErrorCategory::Unavailable,
        stage: "probe".to_owned(),
        detail: "all mirrors timed out".to_owned(),
    };
    assert_eq!(
        failure.to_string(),
        "unavailable: probe: all mirrors timed out"
    );
    assert_eq!(
        serde_json::to_value(ErrorCategory::Cancelled)?,
        json!("cancellation")
    );
    assert!(serde_json::from_value::<ErrorCategory>(json!("cancelled")).is_err());
    let title = ProviderTitle::movie(TmdbId::try_from(100)?);
    assert_eq!(title.media_type, MediaType::Movie);
    Ok(())
}

#[test]
fn duplicate_identity_fields_are_rejected_across_contract_boundaries() {
    for wire in [
        r#"{"type":"movie","type":"tv","tmdb_id":100,"season":0,"episode":1}"#,
        r#"{"type":"movie","tmdb_id":100,"tmdb_id":200}"#,
    ] {
        assert!(serde_json::from_str::<ProviderTitle>(wire).is_err());
    }
    // A flattened catalog identity must also reject ambiguous duplicate keys.
    for wire in [
        r#"{"type":"movie","type":"tv","id":100,"title":"Example","overview":"","rating":0,"details":true}"#,
        r#"{"type":"movie","id":100,"id":200,"title":"Example","overview":"","rating":0,"details":true}"#,
    ] {
        assert!(serde_json::from_str::<stream_core::catalog::Title>(wire).is_err());
    }
}
