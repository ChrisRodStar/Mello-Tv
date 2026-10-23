//! Versioned authenticated row envelopes. No sensitive value is a SQL parameter.
use crate::{
    error::{Error, STORE},
    secret,
};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, path::Path};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyFile {
    active_key_id: String,
    keys: Vec<Key>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Key {
    kid: String,
    key: String,
}
pub(super) struct Crypto {
    active: String,
    keys: BTreeMap<String, Aes256Gcm>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    kid: String,
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}
impl Crypto {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let bytes = secret::read_private(path, 8192)?;
        let mut file: KeyFile = serde_json::from_slice(&bytes).map_err(|_| STORE)?;
        if file.keys.is_empty() || file.keys.len() > 8 {
            return Err(STORE);
        }
        let mut keys = BTreeMap::new();
        for mut key in file.keys.drain(..) {
            use zeroize::Zeroize;
            let decoded = B64.decode(&key.key);
            key.key.zeroize();
            let decoded = Zeroizing::new(decoded.map_err(|_| STORE)?);
            if !crate::auth::opaque(&key.kid) || decoded.len() != 32 {
                return Err(STORE);
            }
            let cipher = Aes256Gcm::new_from_slice(&decoded).map_err(|_| STORE)?;
            if keys.insert(key.kid, cipher).is_some() {
                return Err(STORE);
            }
        }
        if !keys.contains_key(&file.active_key_id) {
            return Err(STORE);
        }
        Ok(Self {
            active: file.active_key_id,
            keys,
        })
    }
    pub fn seal<T: Serialize>(&self, aad: &str, value: &T) -> Result<Vec<u8>, Error> {
        let plaintext = Zeroizing::new(serde_json::to_vec(value).map_err(|_| STORE)?);
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).map_err(|_| STORE)?;
        let aad = format!("relay-store:1:{}:{aad}", self.active);
        let cipher = self.keys.get(&self.active).ok_or(STORE)?;
        let ciphertext = cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: &plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| STORE)?;
        serde_json::to_vec(&Envelope {
            version: 1,
            kid: self.active.clone(),
            nonce,
            ciphertext,
        })
        .map_err(|_| STORE)
    }
    pub fn open<T: DeserializeOwned>(&self, aad: &str, bytes: &[u8]) -> Result<T, Error> {
        let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| STORE)?;
        if envelope.version != 1 {
            return Err(STORE);
        }
        let cipher = self.keys.get(&envelope.kid).ok_or(STORE)?;
        let aad = format!("relay-store:1:{}:{aad}", envelope.kid);
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    &Nonce::from(envelope.nonce),
                    Payload {
                        msg: &envelope.ciphertext,
                        aad: aad.as_bytes(),
                    },
                )
                .map_err(|_| STORE)?,
        );
        serde_json::from_slice(&plaintext).map_err(|_| STORE)
    }
}
pub(super) fn random_id() -> Result<String, Error> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| STORE)?;
    Ok(B64.encode(bytes))
}
