//! Pinned Ed25519 JWTs. No key URLs, algorithm negotiation or network callbacks.
use crate::{
    config::Auth,
    error::{CONFIG, Error},
    secret,
};
use axum::http::StatusCode;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};
pub const INVALID_GRANT: Error = Error(StatusCode::UNAUTHORIZED, "invalid_grant");
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    kid: String,
    #[serde(default)]
    typ: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub sid: String,
    pub arv: u64,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub jti: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Key {
    kid: String,
    public_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFile {
    keys: Vec<Key>,
}
#[derive(Clone)]
pub struct Verifier {
    keys: BTreeMap<String, VerifyingKey>,
    skew: i64,
    lifetime: i64,
    max: usize,
}
pub fn opaque(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn decode(value: &str) -> Result<Vec<u8>, Error> {
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| INVALID_GRANT)?;
    if URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err(INVALID_GRANT);
    }
    Ok(bytes)
}
impl Verifier {
    pub fn load(auth: &Auth) -> Result<Self, Error> {
        let bytes = secret::read_private(Path::new(&auth.api_verification_keys_file), 16384)?;
        Self::from_json(auth, &bytes)
    }
    pub fn from_json(auth: &Auth, bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > 16384 {
            return Err(CONFIG);
        }
        let file: KeyFile = serde_json::from_slice(bytes).map_err(|_| CONFIG)?;
        if file.keys.is_empty() || file.keys.len() > 16 {
            return Err(CONFIG);
        }
        let mut keys = BTreeMap::new();
        for key in file.keys {
            if key.kid.is_empty()
                || key.kid.len() > 64
                || !key
                    .kid
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err(CONFIG);
            }
            let bytes: [u8; 32] = decode(&key.public_key)
                .map_err(|_| CONFIG)?
                .try_into()
                .map_err(|_| CONFIG)?;
            let public = VerifyingKey::from_bytes(&bytes).map_err(|_| CONFIG)?;
            if public.is_weak() || keys.insert(key.kid, public).is_some() {
                return Err(CONFIG);
            }
        }
        Ok(Self {
            keys,
            skew: auth.clock_skew_s as i64,
            lifetime: auth.max_grant_lifetime_s as i64,
            max: auth.max_grant_bytes as usize,
        })
    }
    pub fn verify(&self, token: &str, now: i64) -> Result<Claims, Error> {
        if token.len() > self.max {
            return Err(INVALID_GRANT);
        }
        let parts: Vec<_> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(INVALID_GRANT);
        }
        let header: Header =
            serde_json::from_slice(&decode(parts[0])?).map_err(|_| INVALID_GRANT)?;
        if header.alg != "EdDSA" || header.typ.as_deref().is_some_and(|typ| typ != "JWT") {
            return Err(INVALID_GRANT);
        }
        let key = self.keys.get(&header.kid).ok_or(INVALID_GRANT)?;
        let signature = Signature::from_slice(&decode(parts[2])?).map_err(|_| INVALID_GRANT)?;
        key.verify_strict(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .map_err(|_| INVALID_GRANT)?;
        // Derived struct visitors reject duplicate fields, including timestamps.
        let claims: Claims =
            serde_json::from_slice(&decode(parts[1])?).map_err(|_| INVALID_GRANT)?;
        if claims.iss != "family-stream-api"
            || claims.aud != "family-stream-relay"
            || !opaque(&claims.sub)
            || !opaque(&claims.sid)
            || !opaque(&claims.jti)
            || claims.arv == 0
            || claims.iat < 0
            || claims.nbf < 0
            || claims.exp <= claims.nbf
            || claims.exp <= claims.iat
            || claims
                .exp
                .checked_sub(claims.iat)
                .is_none_or(|duration| duration > self.lifetime)
            || claims.iat > now.saturating_add(self.skew)
            || claims.nbf > now.saturating_add(self.skew)
        {
            return Err(INVALID_GRANT);
        }
        if claims.exp <= now.saturating_sub(self.skew) {
            return Err(Error(StatusCode::UNAUTHORIZED, "expired_grant"));
        }
        Ok(claims)
    }
    pub fn query(&self, query: Option<&str>) -> Result<String, Error> {
        let query = query.ok_or(INVALID_GRANT)?;
        if query.len() > self.max * 3 + 8 {
            return Err(INVALID_GRANT);
        }
        let pairs: Vec<_> = url::form_urlencoded::parse(query.as_bytes()).collect();
        if pairs.iter().filter(|(k, _)| k == "st").count() > 1 {
            return Err(Error(StatusCode::BAD_REQUEST, "duplicate_grant"));
        }
        if pairs.len() != 1 || pairs[0].0 != "st" {
            return Err(INVALID_GRANT);
        }
        Ok(pairs[0].1.to_string())
    }
}
