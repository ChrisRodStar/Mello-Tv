//! Two secrets protect the server:
//! - the API key (admin: library management, search, diagnostics), and
//! - the play key, derived from the API key, which can only start playback.
//!
//! The play key is embedded in every `.strm` URL. Jellyfin may hand those URLs
//! to its clients for direct play, so they must never carry admin rights.

use std::{fmt::Write as _, path::Path};

use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct Keys {
    api_key: String,
    play_key: String,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys { [REDACTED] }")
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub fn random_hex(len_bytes: usize) -> std::io::Result<String> {
    let mut bytes = vec![0u8; len_bytes];
    getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
    Ok(hex(&bytes))
}

fn valid_key(key: &str) -> bool {
    (16..=256).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

impl Keys {
    pub fn from_api_key(api_key: String) -> Self {
        let digest = Sha256::digest(format!("mello-play-key:{api_key}").as_bytes());
        Self {
            play_key: hex(&digest[..16]),
            api_key,
        }
    }

    /// Uses `configured` when set, otherwise loads or creates `<data>/api-key`.
    pub async fn load(configured: Option<String>, data_dir: &Path) -> std::io::Result<Self> {
        if let Some(key) = configured {
            if !valid_key(&key) {
                return Err(std::io::Error::other(
                    "MELLO_API_KEY must be 16-256 characters of letters, digits, '-', '_' or '.'",
                ));
            }
            return Ok(Self::from_api_key(key));
        }
        let path = data_dir.join("api-key");
        match tokio::fs::read_to_string(&path).await {
            Ok(existing) if valid_key(existing.trim()) => {
                Ok(Self::from_api_key(existing.trim().to_owned()))
            }
            Ok(_) => Err(std::io::Error::other(format!(
                "{} is not a valid key; delete it to generate a new one",
                path.display()
            ))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let key = random_hex(24)?;
                write_private(&path, key.as_bytes()).await?;
                tracing::warn!(
                    path = %path.display(),
                    "generated a new API key; read it from this file to manage the library"
                );
                Ok(Self::from_api_key(key))
            }
            Err(error) => Err(error),
        }
    }

    pub fn play_key(&self) -> &str {
        &self.play_key
    }

    pub fn is_play_key(&self, candidate: &str) -> bool {
        bool::from(self.play_key.as_bytes().ct_eq(candidate.as_bytes()))
    }

    pub fn is_api_key(&self, candidate: &str) -> bool {
        bool::from(self.api_key.as_bytes().ct_eq(candidate.as_bytes()))
    }

    /// Accepts `Authorization: Bearer <key>`, `X-Api-Key: <key>` or `?key=`.
    pub fn authorize_api(&self, headers: &HeaderMap, query_key: Option<&str>) -> bool {
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        let header_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());
        [bearer, header_key, query_key]
            .into_iter()
            .flatten()
            .any(|candidate| self.is_api_key(candidate.trim()))
    }
}

/// Writes a file readable only by the current user.
pub async fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        let tmp_clone = tmp.clone();
        let data = contents.to_vec();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut file = options.open(&tmp_clone)?;
            file.write_all(&data)?;
            file.sync_all()
        })
        .await
        .map_err(std::io::Error::other)??;
    }
    #[cfg(not(unix))]
    tokio::fs::write(&tmp, contents).await?;
    tokio::fs::rename(&tmp, path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_key_is_derived_and_cannot_act_as_api_key() {
        let keys = Keys::from_api_key("0123456789abcdef0123".into());
        assert!(keys.is_api_key("0123456789abcdef0123"));
        assert!(keys.is_play_key(keys.play_key()));
        assert!(!keys.is_api_key(keys.play_key()));
        assert_eq!(keys.play_key().len(), 32);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer 0123456789abcdef0123"
                .parse()
                .unwrap_or_else(|_| unreachable!()),
        );
        assert!(keys.authorize_api(&headers, None));
        assert!(!keys.authorize_api(&HeaderMap::new(), Some(keys.play_key())));
    }

    #[tokio::test]
    async fn generated_key_persists() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let first = Keys::load(None, dir.path()).await?;
        let second = Keys::load(None, dir.path()).await?;
        assert_eq!(first.play_key(), second.play_key());
        Ok(())
    }
}
