use serde::{Deserialize, Serialize};

use super::{ProviderTitle, StreamFormat, ValidationError, bounded, nonnegative, strings};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityPreference {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_languages: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtitle_languages: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_height: i64,
    #[serde(default)]
    pub require_audio: bool,
    #[serde(default)]
    pub require_subtitles: bool,
}

pub(super) fn is_zero(value: &i64) -> bool {
    *value == 0
}
pub(super) fn is_false(value: &bool) -> bool {
    !value
}

impl QualityPreference {
    pub fn validate(&self) -> Result<(), ValidationError> {
        nonnegative(self.max_height, "preferences.max_height")?;
        strings(&self.audio_languages, 32, "audio_languages")?;
        strings(&self.subtitle_languages, 32, "subtitle_languages")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceCapabilities {
    pub formats: Vec<StreamFormat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codecs: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_height: i64,
    #[serde(default)]
    pub allow_unknown_codecs: bool,
    #[serde(default)]
    pub allow_unknown_resolution: bool,
    #[serde(default)]
    pub supports_encrypted_hls: bool,
}

impl DeviceCapabilities {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.formats.is_empty()
            || self.formats.len() > 4
            || self.formats.contains(&StreamFormat::Unknown)
        {
            return Err(ValidationError::new(
                "device.formats",
                "one to four supported formats required",
            ));
        }
        nonnegative(self.max_height, "device.max_height")?;
        strings(&self.codecs, 32, "device.codecs")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lookup {
    pub title: ProviderTitle,
    #[serde(default)]
    pub preferences: QualityPreference,
    #[serde(default)]
    pub device: DeviceCapabilities,
    /// Credential partition supplied by trusted ingress; never accepted from JSON.
    #[serde(skip)]
    pub context_key: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub startup_height: i64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub wait_for_complete: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub refresh: bool,
}

impl Lookup {
    /// Payload validation mirrors Go; ingress additionally validates its context key.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.title.validate()?;
        self.preferences.validate()?;
        self.device.validate()?;
        if !(0..=16384).contains(&self.startup_height) {
            return Err(ValidationError::new(
                "startup_height",
                "must be in 0..=16384",
            ));
        }
        Ok(())
    }

    pub fn validate_context(&self) -> Result<(), ValidationError> {
        bounded(&self.context_key, 1, 128, "context_key")
    }
}
