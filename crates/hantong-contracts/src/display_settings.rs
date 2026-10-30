//! Minimal CLI display preference, without account or configuration metadata.
use crate::ErrorCode;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliDisplayLanguage {
    En,
    Ko,
}
impl CliDisplayLanguage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ko => "ko",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "en" => Ok(Self::En),
            "ko" => Ok(Self::Ko),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct CliDisplaySettings {
    language: Option<CliDisplayLanguage>,
}
impl CliDisplaySettings {
    pub const fn new(language: Option<CliDisplayLanguage>) -> Self {
        Self { language }
    }
    pub const fn language(&self) -> Option<CliDisplayLanguage> {
        self.language
    }
}
impl fmt::Debug for CliDisplaySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CliDisplaySettings(<redacted>)")
    }
}
