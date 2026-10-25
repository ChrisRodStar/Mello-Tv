use serde::{Deserialize, Serialize};

pub use crate::accounts::Profile;

/// Validated route/storage identity; serialized as the existing opaque string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProfileId(String);

impl ProfileId {
    pub fn new(value: impl Into<String>) -> Result<Self, crate::error::DomainError> {
        let value = value.into();
        if !crate::accounts::is_safe_identifier(&value) {
            return Err(crate::error::DomainError::Validation(
                "invalid profile identifier".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProfileId {
    type Error = crate::error::DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProfileId> for String {
    fn from(value: ProfileId) -> Self {
        value.0
    }
}
