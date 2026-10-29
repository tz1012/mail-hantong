use serde::{Deserialize, Serialize};
use std::fmt;

use crate::timezone::{self, TimezoneError};

const MAX_CONFIG_BYTES: usize = 64 * 1024;
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Ko,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    System,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CliConfig {
    pub language: Language,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuiConfig {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_language"
    )]
    pub language: Option<Language>,
    pub theme: Theme,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleConfig {
    pub timezone_id: String,
}
impl fmt::Debug for LocaleConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocaleConfig { timezone_id: <redacted> }")
    }
}

fn present_language<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Language>, D::Error> {
    Language::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    #[serde(default)]
    pub external_ai_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub settings_version: u64,
    pub cli: CliConfig,
    pub gui: GuiConfig,
    pub locale: LocaleConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    InvalidJson,
    TooLarge,
    InvalidSchema,
    InvalidTimezone,
    StaleVersion,
    SecurityProtected,
    VersionExhausted,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidJson => "configuration input is invalid",
            Self::TooLarge => "configuration input is too large",
            Self::InvalidSchema => "configuration schema is invalid",
            Self::InvalidTimezone => "configuration timezone is invalid",
            Self::StaleVersion => "configuration version conflicts",
            Self::SecurityProtected => "protected configuration requires an approved path",
            Self::VersionExhausted => "configuration version is exhausted",
        })
    }
}
impl std::error::Error for ConfigError {}

impl Config {
    pub fn defaults(timezone_id: impl Into<String>) -> Result<Self, ConfigError> {
        let timezone_id = timezone_id.into();
        timezone::validate_id(&timezone_id)?;
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            settings_version: 1,
            cli: CliConfig {
                language: Language::En,
            },
            gui: GuiConfig {
                language: None,
                theme: Theme::System,
            },
            locale: LocaleConfig { timezone_id },
            security: SecurityConfig::default(),
        })
    }

    pub fn initial_gui_language(&self, os_is_korean: bool) -> Language {
        self.gui.language.unwrap_or(if os_is_korean {
            Language::Ko
        } else {
            Language::En
        })
    }

    pub fn from_json(input: &[u8]) -> Result<Self, ConfigError> {
        if input.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::TooLarge);
        }
        let candidate: Self =
            serde_json::from_slice(input).map_err(|_| ConfigError::InvalidJson)?;
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != SCHEMA_VERSION
            || self.settings_version == 0
            || self.settings_version > i64::MAX as u64
        {
            return Err(ConfigError::InvalidSchema);
        }
        Ok(timezone::validate_id(&self.locale.timezone_id)?)
    }

    pub fn validate_unprivileged_replacement(
        &self,
        previous: &Self,
        expected_version: u64,
    ) -> Result<Self, ConfigError> {
        previous.validate()?;
        self.validate()?;
        if expected_version != previous.settings_version
            || self.settings_version != expected_version
        {
            return Err(ConfigError::StaleVersion);
        }
        if self.security != previous.security {
            return Err(ConfigError::SecurityProtected);
        }
        let next = expected_version
            .checked_add(1)
            .ok_or(ConfigError::VersionExhausted)?;
        if next > i64::MAX as u64 {
            return Err(ConfigError::VersionExhausted);
        }
        let mut accepted = self.clone();
        accepted.settings_version = next;
        Ok(accepted)
    }
}

impl From<TimezoneError> for ConfigError {
    fn from(_: TimezoneError) -> Self {
        Self::InvalidTimezone
    }
}
