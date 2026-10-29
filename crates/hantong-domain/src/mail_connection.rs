//! Validated provider-neutral IMAP/SMTP connection settings.

use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr, str::FromStr};

const MAX_JSON_BYTES: usize = 16 * 1024;
const MAX_VERSION: u64 = i64::MAX as u64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    ImplicitTls,
    StarttlsRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    AppPassword,
    Oauth2,
}

#[derive(Clone, Eq, PartialEq)]
struct CommonEndpointConfig {
    settings_version: u64,
    host: String,
    port: u16,
    tls: TlsMode,
    login_name: String,
    authentication: Authentication,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapEndpointConfig {
    common: CommonEndpointConfig,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SmtpEndpointConfig {
    common: CommonEndpointConfig,
}

#[derive(Clone, Eq, PartialEq)]
pub struct MailConnectionConfig {
    schema_version: u32,
    account_id: String,
    imap: ImapEndpointConfig,
    smtp: Option<SmtpEndpointConfig>,
}

impl fmt::Debug for ImapEndpointConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapEndpointConfig { <redacted> }")
    }
}
impl fmt::Debug for SmtpEndpointConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SmtpEndpointConfig { <redacted> }")
    }
}
impl fmt::Debug for MailConnectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MailConnectionConfig { <redacted> }")
    }
}

#[derive(Serialize)]
struct EndpointWire<'a> {
    settings_version: u64,
    host: &'a str,
    port: u16,
    tls: TlsMode,
    login_name: &'a str,
    authentication: Authentication,
}

impl Serialize for ImapEndpointConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EndpointWire::from(&self.common).serialize(serializer)
    }
}
impl Serialize for SmtpEndpointConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EndpointWire::from(&self.common).serialize(serializer)
    }
}
impl<'a> From<&'a CommonEndpointConfig> for EndpointWire<'a> {
    fn from(value: &'a CommonEndpointConfig) -> Self {
        Self {
            settings_version: value.settings_version,
            host: &value.host,
            port: value.port,
            tls: value.tls,
            login_name: &value.login_name,
            authentication: value.authentication,
        }
    }
}

#[derive(Serialize)]
struct ConfigWire<'a> {
    schema_version: u32,
    account_id: &'a str,
    imap: &'a ImapEndpointConfig,
    smtp: Option<&'a SmtpEndpointConfig>,
}
impl Serialize for MailConnectionConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ConfigWire {
            schema_version: self.schema_version,
            account_id: &self.account_id,
            imap: &self.imap,
            smtp: self.smtp.as_ref(),
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointWireIn {
    settings_version: u64,
    host: String,
    port: u16,
    tls: TlsMode,
    login_name: String,
    authentication: Authentication,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigWireIn {
    schema_version: u32,
    account_id: String,
    imap: EndpointWireIn,
    #[serde(default)]
    smtp: Option<EndpointWireIn>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailConnectionError {
    Invalid,
    TooLarge,
}

impl fmt::Display for MailConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "mail connection settings are invalid",
            Self::TooLarge => "mail connection settings are too large",
        })
    }
}
impl std::error::Error for MailConnectionError {}

impl MailConnectionConfig {
    pub fn from_json(input: &[u8]) -> Result<Self, MailConnectionError> {
        if input.len() > MAX_JSON_BYTES {
            return Err(MailConnectionError::TooLarge);
        }
        let wire: ConfigWireIn =
            serde_json::from_slice(input).map_err(|_| MailConnectionError::Invalid)?;
        if wire.schema_version != 1 || !valid_account_id(&wire.account_id) {
            return Err(MailConnectionError::Invalid);
        }
        Ok(Self {
            schema_version: wire.schema_version,
            account_id: wire.account_id,
            imap: ImapEndpointConfig {
                common: validate_endpoint(wire.imap)?,
            },
            smtp: wire
                .smtp
                .map(validate_endpoint)
                .transpose()?
                .map(|common| SmtpEndpointConfig { common }),
        })
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn imap(&self) -> &ImapEndpointConfig {
        &self.imap
    }
    pub fn smtp(&self) -> Option<&SmtpEndpointConfig> {
        self.smtp.as_ref()
    }
}

macro_rules! endpoint_getters {
    ($type:ty) => {
        impl $type {
            pub fn settings_version(&self) -> u64 {
                self.common.settings_version
            }
            pub fn host(&self) -> &str {
                &self.common.host
            }
            pub fn port(&self) -> u16 {
                self.common.port
            }
            pub fn tls(&self) -> TlsMode {
                self.common.tls
            }
            pub fn login_name(&self) -> &str {
                &self.common.login_name
            }
            pub fn authentication(&self) -> Authentication {
                self.common.authentication
            }
        }
    };
}
endpoint_getters!(ImapEndpointConfig);
endpoint_getters!(SmtpEndpointConfig);

fn validate_endpoint(value: EndpointWireIn) -> Result<CommonEndpointConfig, MailConnectionError> {
    if value.settings_version == 0 || value.settings_version > MAX_VERSION || value.port == 0 {
        return Err(MailConnectionError::Invalid);
    }
    let host = normalize_host(&value.host)?;
    if !valid_login(&value.login_name) {
        return Err(MailConnectionError::Invalid);
    }
    Ok(CommonEndpointConfig {
        settings_version: value.settings_version,
        host,
        port: value.port,
        tls: value.tls,
        login_name: value.login_name,
        authentication: value.authentication,
    })
}

fn valid_account_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn valid_login(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1280
        && value.chars().count() <= 320
        && value == value.trim()
        && !value.trim().is_empty()
        && !value.chars().any(char::is_control)
}

fn normalize_host(value: &str) -> Result<String, MailConnectionError> {
    if value.is_empty() || value.len() > 253 || value != value.trim() || value.ends_with('.') {
        return Err(MailConnectionError::Invalid);
    }
    if value.contains('%')
        || value.contains('/')
        || value.contains('@')
        || value.contains('[')
        || value.contains(']')
    {
        return Err(MailConnectionError::Invalid);
    }
    if let Ok(ip) = IpAddr::from_str(value) {
        return Ok(ip.to_string());
    }
    if !value.is_ascii() {
        return Err(MailConnectionError::Invalid);
    }
    let labels = value.split('.').collect::<Vec<_>>();
    let legacy_numeric = labels.iter().all(|label| {
        !label.is_empty()
            && (label.bytes().all(|b| b.is_ascii_digit())
                || (label.len() > 2
                    && label[..2].eq_ignore_ascii_case("0x")
                    && label[2..].bytes().all(|b| b.is_ascii_hexdigit())))
    });
    if legacy_numeric {
        return Err(MailConnectionError::Invalid);
    }
    for label in labels {
        if label.is_empty()
            || label.len() > 63
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(MailConnectionError::Invalid);
        }
        if !label.as_bytes()[0].is_ascii_alphanumeric()
            || !label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
        {
            return Err(MailConnectionError::Invalid);
        }
    }
    Ok(value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests;
