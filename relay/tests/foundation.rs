#![allow(clippy::unwrap_used)]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{DateTime, Utc};
use family_stream_relay::{
    config::Config,
    contracts::{CreateSession, providers::Selection},
    control::{self, App},
    validation,
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn config() -> Config {
    Config::parse(include_str!("../config/relay.example.toml")).unwrap()
}
fn now() -> DateTime<Utc> {
    "2030-01-01T00:00:00Z".parse().unwrap()
}
fn fixture(name: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(format!("tests/fixtures/providers/{name}.json")).unwrap(),
    )
    .unwrap()
}
fn create_value() -> Value {
    json!({"contract_version":"relay.v1","owner_id":"fixture-household-0001","expires_at":"2030-01-01T06:00:00Z","selection":fixture("renditions")})
}
#[test]
fn actual_go_envelopes_round_trip_without_changing_units_unknowns_or_order() {
    for name in [
        "movie-null",
        "tv-special",
        "renditions",
        "encrypted-hls",
        "progressive-mp4",
        "ordered-backups",
        "no-selection",
    ] {
        let original = fixture(name);
        let selection: Selection = serde_json::from_value(original.clone()).unwrap();
        assert!(
            same_json(&serde_json::to_value(&selection).unwrap(), &original),
            "{name}"
        );
        let valid = validation::selection(&selection, &config(), now());
        assert_eq!(valid.is_ok(), name != "no-selection", "{name}");
    }
    let selection: Selection = serde_json::from_value(fixture("movie-null")).unwrap();
    let observation = &selection.selected.as_ref().unwrap().candidate.observation;
    assert_eq!(observation.first_byte, Some(100_000_000));
    assert_eq!(observation.delivery_bps, Some(24_000_000.0));
    assert!(!observation.playlist_metadata.tracks_known);
    assert!(observation.playlist_metadata.tracks.is_none());
    let opaque = &selection
        .selected
        .as_ref()
        .unwrap()
        .candidate
        .source
        .resolver_metadata
        .provider_data
        .as_ref()
        .unwrap()["unknown"];
    assert_eq!(opaque["big"].to_string(), "18446744073709551617");
    assert_eq!(
        opaque["decimal"].to_string(),
        "0.12345678901234567890123456789"
    );
    let selection: Selection = serde_json::from_value(fixture("ordered-backups")).unwrap();
    assert_eq!(
        selection
            .backups
            .as_ref()
            .unwrap()
            .iter()
            .map(|v| v.candidate.source.provider.as_str())
            .collect::<Vec<_>>(),
        ["vidlink", "vidsrc"]
    );
}
#[test]
fn unknown_fields_are_retained_privately_at_every_level() {
    let mut value = create_value();
    for pointer in [
        "/selection",
        "/selection/selected",
        "/selection/selected/candidate",
        "/selection/selected/candidate/source",
        "/selection/selected/candidate/source/title",
        "/selection/selected/candidate/observation",
        "/selection/selected/candidate/observation/playlist_metadata",
    ] {
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("future_field".into(), json!({"secret":"synthetic"}));
    }
    let request: CreateSession = serde_json::from_value(value.clone()).unwrap();
    assert!(same_json(&serde_json::to_value(request).unwrap(), &value));
}
#[test]
fn rejects_invalid_identity_protection_formats_metrics_and_contexts() {
    for (pointer, replacement) in [
        ("/selection/version", json!("providers.v2")),
        ("/selection/selected", Value::Null),
        (
            "/selection/selected/candidate/source/title/tmdb_id",
            json!(0),
        ),
        (
            "/selection/selected/candidate/source/title/season",
            json!(0),
        ),
        (
            "/selection/selected/candidate/observation/accessible",
            json!(false),
        ),
        (
            "/selection/selected/candidate/observation/protection_known",
            json!(false),
        ),
        (
            "/selection/selected/candidate/observation/format",
            json!("dash"),
        ),
        (
            "/selection/selected/candidate/observation/first_byte",
            json!(-1),
        ),
        ("/selection/selected/candidate/source/format", json!("mp4")),
        (
            "/selection/selected/candidate/source/url",
            json!("https://user:secret@media.example.com/a"),
        ),
        (
            "/selection/selected/candidate/source/fetch_context",
            json!({"scopes":[{"origin":"https://media.example.com/","headers":{"Host":"evil"}}]}),
        ),
        (
            "/selection/selected/candidate/source/fetch_context",
            json!({"scopes":[{"origin":"https://media.example.com/","headers":{"Accept":"x","accept":"y"}}]}),
        ),
        (
            "/selection/selected/candidate/source/fetch_context",
            json!({"scopes":[{"origin":"https://media.example.com/","cookies":"secret"}]}),
        ),
        (
            "/selection/selected/candidate/source/fetch_context",
            json!({"public_resources":{"referer":"https://media.example.com/?secret=fixture"}}),
        ),
        ("/owner_id", json!("too-short")),
        ("/expires_at", json!("2030-01-02T00:00:00Z")),
    ] {
        let mut value = create_value();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        value
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), replacement);
        let request: CreateSession = serde_json::from_value(value).unwrap();
        assert!(
            validation::create(&request, &config(), now()).is_err(),
            "{pointer}"
        );
    }
}
#[test]
fn backups_must_match_coordinates_and_cross_ids_and_fit_budget() {
    for (key, value) in [
        ("tmdb_id", json!(1)),
        ("imdb_id", json!("tt0000001")),
        ("type", json!("tv")),
    ] {
        let mut selection = fixture("ordered-backups");
        selection["backups"][0]["candidate"]["source"]["title"][key] = value;
        let selection: Selection = serde_json::from_value(selection).unwrap();
        assert!(validation::selection(&selection, &config(), now()).is_err());
    }
    let mut value = create_value();
    let selected = value["selection"]["selected"].clone();
    value["selection"]["backups"] = Value::Array(vec![selected; 128]);
    let request: CreateSession = serde_json::from_value(value).unwrap();
    assert!(validation::create(&request, &config(), now()).is_err());
    let mut value = create_value();
    value["selection"]["private-extension"] =
        json!("x".repeat(config().state.max_snapshot_bytes as usize));
    let request: CreateSession = serde_json::from_value(value).unwrap();
    assert_eq!(
        validation::create(&request, &config(), now())
            .unwrap_err()
            .1,
        "import_too_large"
    );
}
#[test]
fn config_rejects_public_alias_listeners_unknown_options_and_inconsistent_limits() {
    let example = include_str!("../config/relay.example.toml");
    for changed in [
        example.replace("127.0.0.1:8091", "0.0.0.0:8091"),
        example.replace("127.0.0.1:8092", "127.0.0.1:8091"),
        example.replace(
            "max_control_concurrency_count = 4",
            "max_control_concurrency_count = 0",
        ),
        example.replace(
            "max_sources_per_session_count = 128",
            "max_sources_per_session_count = 129",
        ),
        example.replace(
            "max_grant_lifetime_s = 21600",
            "max_grant_lifetime_s = 21601",
        ),
        example.replace("guard_mode = \"local-mullvad\"", "guard_mode = \"unsafe\""),
        example.to_owned() + "\nallow_unsafe = true\n",
    ] {
        assert!(Config::parse(&changed).is_err());
    }
}
fn app() -> App {
    App::new(config(), b"synthetic-bearer-01234567890123456789012345")
}
fn request(body: Value, auth: bool) -> Request<Body> {
    let mut request = Request::post("/internal/v1/sessions")
        .header("content-type", "application/json")
        .header("idempotency-key", "fixture-1");
    if auth {
        request = request.header(
            "authorization",
            "Bearer synthetic-bearer-01234567890123456789012345",
        );
    }
    request.body(Body::from(body.to_string())).unwrap()
}
#[tokio::test]
async fn private_auth_is_required_before_parsing_and_errors_never_echo_secrets() {
    let router = control::control(app());
    let response = router
        .clone()
        .oneshot(request(json!({"secret":"synthetic-source-url"}), false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-source-url"));
    let mut duplicate = request(create_value(), true);
    duplicate.headers_mut().append(
        "authorization",
        "Bearer synthetic-bearer-01234567890123456789012345"
            .parse()
            .unwrap(),
    );
    assert_eq!(
        router.oneshot(duplicate).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn actual_create_route_validates_but_never_issues_non_durable_urls() {
    let mut value = create_value();
    value["expires_at"] = json!((Utc::now() + chrono::Duration::hours(1)).to_rfc3339());
    let response = control::control(app())
        .oneshot(request(value, true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(body["code"], "state_store_unavailable");
    assert!(body.get("entry_path").is_none());
}
#[tokio::test]
async fn admission_body_and_idempotency_are_bounded() {
    let mut config = config();
    config.limits.max_control_body_bytes = 128;
    let router = control::control(App::new(
        config,
        b"synthetic-bearer-01234567890123456789012345",
    ));
    assert_eq!(
        router
            .clone()
            .oneshot(request(create_value(), true))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut req = request(json!({}), true);
    req.headers_mut().remove("idempotency-key");
    assert_eq!(
        router.oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn stream_listener_cannot_expose_control_and_head_does_not_fetch() {
    for path in [
        "/stream/fixture/entry?st=synthetic",
        "/internal/v1/sessions",
        "/health/live",
    ] {
        let response = control::streaming()
            .oneshot(Request::head(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if path.starts_with("/stream") {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::NOT_FOUND
            }
        );
    }
}
#[test]
fn secret_files_require_private_regular_files_and_reject_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bearer");
    std::fs::write(&path, b"synthetic-bearer-01234567890123456789012345\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(family_stream_relay::secret::bearer(&path).is_ok());
    let link = directory.path().join("link");
    symlink(&path, &link).unwrap();
    assert!(family_stream_relay::secret::bearer(&link).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(family_stream_relay::secret::bearer(&path).is_err());
}

// JSON numbers do not distinguish 40 from 40.0; keep integer/integer comparisons exact.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) if x.is_f64() || y.is_f64() => {
            x.as_f64() == y.as_f64()
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, value)| y.get(key).is_some_and(|v| same_json(value, v)))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same_json(a, b))
        }
        _ => a == b,
    }
}

#[test]
fn required_nullable_go_fields_accept_null_but_reject_missing() {
    for field in ["backups", "rejected", "providers"] {
        let mut value = fixture("movie-null");
        value.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Selection>(value).is_err(),
            "{field}"
        );
    }
}

#[tokio::test]
async fn request_admission_rejects_excess_without_waiting_for_body() {
    use axum::body::{Bytes, HttpBody};
    use std::{
        convert::Infallible,
        pin::Pin,
        sync::Arc,
        task::{Context, Poll},
    };
    struct PendingBody(Arc<tokio::sync::Notify>);
    impl HttpBody for PendingBody {
        type Data = Bytes;
        type Error = Infallible;
        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<hyper::body::Frame<Bytes>, Self::Error>>> {
            self.0.notify_one();
            Poll::Pending
        }
    }
    let mut config = config();
    config.limits.max_control_concurrency_count = 1;
    let router = control::control(App::new(
        config,
        b"synthetic-bearer-01234567890123456789012345",
    ));
    let polled = Arc::new(tokio::sync::Notify::new());
    let mut first = request(json!({}), true);
    *first.body_mut() = Body::new(PendingBody(polled.clone()));
    let waiting = tokio::spawn(router.clone().oneshot(first));
    polled.notified().await;
    let response = router.oneshot(request(json!({}), true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    waiting.abort();
    let _ = waiting.await;
}

#[test]
fn cross_ids_must_agree_between_backups_when_selected_omits_them() {
    for (key, first, second) in [
        ("imdb_id", json!("tt0000001"), json!("tt0000002")),
        ("my_anime_list_id", json!(1001), json!(1002)),
    ] {
        let mut value = fixture("ordered-backups");
        value["selected"]["candidate"]["source"]["title"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        value["backups"][0]["candidate"]["source"]["title"][key] = first.clone();
        value["backups"][1]["candidate"]["source"]["title"][key] = second;
        let selection: Selection = serde_json::from_value(value.clone()).unwrap();
        assert!(
            validation::selection(&selection, &config(), now()).is_err(),
            "{key}"
        );
        value["backups"][1]["candidate"]["source"]["title"][key] = first;
        let selection: Selection = serde_json::from_value(value).unwrap();
        assert!(
            validation::selection(&selection, &config(), now()).is_ok(),
            "{key}"
        );
        let serialized = serde_json::to_value(selection).unwrap();
        assert!(
            serialized["selected"]["candidate"]["source"]["title"]
                .get(key)
                .is_none()
        );
    }
}

#[test]
fn opaque_json_numbers_retain_precision_in_metadata_and_flattened_unknowns() {
    let exact = r#"{"big":18446744073709551617,"negative":-18446744073709551619,"decimal":0.12345678901234567890123456789,"nested":[9007199254740993,1.234567890123456789e+400]}"#;
    let opaque: Value = serde_json::from_str(exact).unwrap();
    let mut request = create_value();
    request["selection"]["selected"]["candidate"]["source"]["resolver_metadata"]["provider_data"] =
        opaque.clone();
    for pointer in [
        "/selection",
        "/selection/selected/candidate/source/title",
        "/selection/selected/candidate/observation/playlist_metadata",
    ] {
        request
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown_precision".into(), opaque.clone());
    }
    // Exercise the same bytes -> typed model -> bytes path used by relayctl/control.
    let wire = serde_json::to_vec(&request).unwrap();
    let typed: CreateSession = serde_json::from_slice(&wire).unwrap();
    let round_trip: Value = serde_json::from_slice(&serde_json::to_vec(&typed).unwrap()).unwrap();
    assert_eq!(
        round_trip["selection"]["selected"]["candidate"]["source"]["resolver_metadata"]["provider_data"],
        opaque
    );
    for pointer in [
        "/selection",
        "/selection/selected/candidate/source/title",
        "/selection/selected/candidate/observation/playlist_metadata",
    ] {
        assert_eq!(
            round_trip.pointer(pointer).unwrap()["unknown_precision"],
            opaque
        );
    }
    assert_eq!(opaque["big"].to_string(), "18446744073709551617");
    assert_eq!(
        opaque["decimal"].to_string(),
        "0.12345678901234567890123456789"
    );
}

#[test]
fn stage_two_configurations_get_safe_store_budget_defaults() {
    let legacy = include_str!("../config/relay.example.toml")
        .replace("max_database_bytes = 1073741824\n", "")
        .replace("min_free_space_bytes = 67108864\n", "")
        .replace("max_idempotency_count = 256\n", "");
    let config = Config::parse(&legacy).unwrap();
    assert_eq!(config.state.max_database_bytes, 1_073_741_824);
    assert_eq!(config.state.min_free_space_bytes, 67_108_864);
    assert_eq!(config.state.max_idempotency_count, 256);
    for name in [
        "max_database_bytes",
        "min_free_space_bytes",
        "max_idempotency_count",
    ] {
        let invalid = legacy.replace("[state]", &format!("[state]\n{name} = 0"));
        assert!(Config::parse(&invalid).is_err());
    }
}
