#![allow(clippy::unwrap_used)]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ed25519_dalek::{Signer, SigningKey};
use family_stream_relay::{
    auth::Verifier,
    config::Config,
    control,
    session::{self, AuthorizationRecord, AuthorizationStore, Resource},
};
use serde_json::{Value, json};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
const SID: &str = "session_synthetic_001";
const RID: &str = "resource_synthetic_001";
fn config() -> Config {
    Config::parse(include_str!("../config/relay.example.toml")).unwrap()
}
fn verifier() -> Verifier {
    let key = SigningKey::from_bytes(&[7; 32]);
    Verifier::from_json(&config().auth,&serde_json::to_vec(&json!({"keys":[{"kid":"test-key","public_key":B64.encode(key.verifying_key().as_bytes())}]})).unwrap()).unwrap()
}
fn claims(now: i64) -> Value {
    json!({"iss":"family-stream-api","aud":"family-stream-relay","sub":"owner_synthetic_001","sid":SID,"arv":1,"iat":now-10,"nbf":now-10,"exp":now+100,"jti":"request_synthetic_001"})
}
fn sign(header: &str, claims: &str, key: [u8; 32]) -> String {
    let data = format!("{}.{}", B64.encode(header), B64.encode(claims));
    let sig = SigningKey::from_bytes(&key).sign(data.as_bytes());
    format!("{data}.{}", B64.encode(sig.to_bytes()))
}
fn token(claims: &Value) -> String {
    sign(
        r#"{"alg":"EdDSA","kid":"test-key","typ":"JWT"}"#,
        &claims.to_string(),
        [7; 32],
    )
}
#[test]
fn signed_claim_attacks_are_rejected() {
    let now = 2000000000;
    let verifier = verifier();
    assert!(verifier.verify(&token(&claims(now)), now).is_ok());
    for (field, value) in [
        ("iss", json!("attacker")),
        ("aud", json!(["family-stream-relay"])),
        ("sub", json!("")),
        ("sid", json!("other")),
        ("arv", json!(0)),
        ("exp", json!(now - 40)),
        ("exp", json!(now + 21601)),
        ("iat", json!(-1)),
        ("iat", json!(now + 31)),
        ("nbf", json!(now + 31)),
        ("exp", json!("tomorrow")),
        ("exp", json!(2.5)),
    ] {
        let mut c = claims(now);
        c[field] = value;
        assert!(verifier.verify(&token(&c), now).is_err(), "{field}");
    }
    for field in [
        "iss", "aud", "sub", "sid", "arv", "iat", "nbf", "exp", "jti",
    ] {
        let mut c = claims(now);
        c.as_object_mut().unwrap().remove(field);
        assert!(verifier.verify(&token(&c), now).is_err(), "{field}");
    }
    for header in [
        r#"{"alg":"none","kid":"test-key"}"#,
        r#"{"alg":"HS256","kid":"test-key"}"#,
        r#"{"alg":"EdDSA","kid":"unknown"}"#,
        r#"{"alg":"EdDSA","kid":"test-key","jku":"https://evil.example.com"}"#,
        r#"{"alg":"EdDSA","alg":"EdDSA","kid":"test-key"}"#,
    ] {
        assert!(
            verifier
                .verify(&sign(header, &claims(now).to_string(), [7; 32]), now)
                .is_err()
        );
    }
    let raw = claims(now)
        .to_string()
        .replacen('{', "{\"exp\":2000000100,", 1);
    assert!(
        verifier
            .verify(
                &sign(r#"{"alg":"EdDSA","kid":"test-key"}"#, &raw, [7; 32]),
                now
            )
            .is_err()
    );
    assert!(
        verifier
            .verify(
                &sign(
                    r#"{"alg":"EdDSA","kid":"test-key"}"#,
                    &claims(now).to_string(),
                    [8; 32]
                ),
                now
            )
            .is_err()
    );
    assert!(verifier.verify(&"A".repeat(4097), now).is_err());
    for query in [
        "st=one&st=two",
        "%73t=one&st=two",
        "st=one&url=https://evil.example.com",
        "token=one",
    ] {
        assert!(verifier.query(Some(query)).is_err());
    }
}
fn record(resource: Resource, now: i64) -> AuthorizationRecord {
    AuthorizationRecord {
        registration: None,
        session_id: SID.into(),
        owner_id: "owner_synthetic_001".into(),
        auth_revision: 1,
        expires_at: now + 200,
        source_expires_at: Some(now + 50),
        revoked: false,
        resource,
        resource_session_id: SID.into(),
        source_generation: 2,
        resource_generation: 2,
        generation_valid_until: None,
        cancelled: CancellationToken::new(),
        reader: None,
    }
}
#[test]
fn ownership_revisions_resources_generations_and_lease_boundaries() {
    let now = 2000000000;
    let c = verifier().verify(&token(&claims(now)), now).unwrap();
    let r = Resource::Registered(RID.into());
    assert_eq!(
        session::authorize(&c, SID, &r, record(r.clone(), now), now)
            .unwrap()
            .expires_at,
        now + 50
    );
    for attack in 0..10 {
        let mut state = record(r.clone(), now);
        match attack {
            0 => state.owner_id = "other_owner_0001".into(),
            1 => state.auth_revision = 2,
            2 => state.revoked = true,
            3 => state.resource_session_id = "other_session_001".into(),
            4 => state.resource_generation = 1,
            5 => state.expires_at = now,
            6 => state.source_expires_at = Some(now),
            7 => state.resource = Resource::Registered("different_resource_001".into()),
            8 => state.cancelled.cancel(),
            _ => state.session_id = "other_session_001".into(),
        };
        assert!(
            session::authorize(&c, SID, &r, state, now).is_err(),
            "attack {attack}"
        );
    }
    let mut state = record(r.clone(), now);
    state.resource_generation = 1;
    state.generation_valid_until = Some(now + 20);
    assert_eq!(
        session::authorize(&c, SID, &r, state, now)
            .unwrap()
            .expires_at,
        now + 20
    );
    let expired = verifier().verify(&token(&claims(now)), now + 110).unwrap();
    assert!(session::authorize(&expired, SID, &r, record(r.clone(), now), now + 110).is_err());
}
struct FixtureStore {
    lookups: Arc<AtomicUsize>,
    wrong_owner: bool,
}
impl AuthorizationStore for FixtureStore {
    fn lookup<'a>(
        &'a self,
        _: &'a str,
        resource: &'a Resource,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<AuthorizationRecord, family_stream_relay::error::Error>>
                + Send
                + 'a,
        >,
    > {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let mut state = record(resource.clone(), chrono::Utc::now().timestamp());
            if self.wrong_owner {
                state.owner_id = "wrong_owner_0001".into();
            }
            Ok(state)
        })
    }
}
#[tokio::test]
async fn actual_get_head_entry_and_nested_routes_enforce_authorization_before_store() {
    let now = chrono::Utc::now().timestamp();
    let verifier = Arc::new(verifier());
    let lookups = Arc::new(AtomicUsize::new(0));
    let router = control::streaming_with(
        Some(verifier.clone()),
        Arc::new(FixtureStore {
            lookups: lookups.clone(),
            wrong_owner: false,
        }),
    );
    for method in ["GET", "HEAD"] {
        for path in [
            format!("/stream/{SID}/entry"),
            format!("/stream/{SID}/r/{RID}"),
        ] {
            for (query, status) in [
                (String::new(), StatusCode::UNAUTHORIZED),
                ("?st=bad".into(), StatusCode::UNAUTHORIZED),
                ("?st=one&st=two".into(), StatusCode::BAD_REQUEST),
                (
                    format!("?st={}", token(&claims(now))),
                    StatusCode::SERVICE_UNAVAILABLE,
                ),
            ] {
                let before = lookups.load(Ordering::SeqCst);
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(format!("{path}{query}"))
                            .header("authorization", "Bearer ignored-control-or-client")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), status);
                assert_eq!(response.headers()["referrer-policy"], "no-referrer");
                assert_eq!(
                    lookups.load(Ordering::SeqCst) - before,
                    usize::from(status == StatusCode::SERVICE_UNAVAILABLE)
                );
                if method == "HEAD" {
                    assert!(
                        to_bytes(response.into_body(), 4096)
                            .await
                            .unwrap()
                            .is_empty()
                    );
                }
            }
        }
    }
    let signed = token(&claims(now));
    let other = router
        .clone()
        .oneshot(
            Request::get(format!("/stream/other_session_001/entry?st={signed}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/stream/{SID}/entry?st={signed}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["allow"], "GET,HEAD");
    let unavailable =
        control::streaming_with(Some(verifier.clone()), Arc::new(session::Unavailable));
    assert_eq!(
        unavailable
            .oneshot(
                Request::get(format!("/stream/{SID}/entry?st={signed}"))
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let wrong = control::streaming_with(
        Some(verifier),
        Arc::new(FixtureStore {
            lookups,
            wrong_owner: true,
        }),
    );
    assert_eq!(
        wrong
            .oneshot(
                Request::head(format!("/stream/{SID}/r/{RID}?st={signed}"))
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
