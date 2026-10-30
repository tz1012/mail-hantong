//! Minimal display preferences read through the authenticated app boundary.
use crate::accounts::Object;
use hantong_contracts::{
    ErrorCode,
    display_settings::{CliDisplayLanguage, CliDisplaySettings},
};
use serde::{Deserialize, de::DeserializeOwned};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success<T> {
    jsonrpc: String,
    id: String,
    result: Object<T>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    jsonrpc: String,
    id: String,
    error: Object<ErrorWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorWire {
    code: String,
    message: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Rpc<T> {
    Success(Object<Success<T>>),
    Failure(Object<Failure>),
}

// Nullable fields still have to appear. Option<T> would also accept omission.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Nullable<T> {
    Present(T),
    Null(()),
}
impl<T> Nullable<T> {
    pub(crate) fn into_option(self) -> Option<T> {
        match self {
            Self::Present(value) => Some(value),
            Self::Null(()) => None,
        }
    }
}

pub(crate) fn decode_app_response<T: DeserializeOwned>(
    bytes: &[u8],
    expected_id: &str,
    maximum: usize,
) -> Result<T, ErrorCode> {
    if bytes.is_empty()
        || bytes.len() > maximum
        || bytes.starts_with(&[0xef, 0xbb, 0xbf])
        || expected_id.is_empty()
        || expected_id.len() > 64
        || !expected_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    // Object<T> forces maps; typed deserialization rejects duplicates before
    // any Value intermediary can erase them.
    let rpc: Rpc<T> = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    match rpc {
        Rpc::Success(value) => {
            if value.0.jsonrpc != "2.0" || value.0.id != expected_id {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(value.0.result.0)
        }
        Rpc::Failure(value) => {
            if value.0.jsonrpc != "2.0"
                || value.0.id != expected_id
                || value.0.error.0.code != value.0.error.0.message
            {
                return Err(ErrorCode::InvalidInput);
            }
            Err(ErrorCode::ALL
                .into_iter()
                .find(|code| code.as_str() == value.0.error.0.code)
                .ok_or(ErrorCode::InvalidInput)?)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayWire {
    schema_version: u32,
    ok: bool,
    cli_language: Nullable<String>,
}

pub fn parse_display_settings_response(
    bytes: &[u8],
    expected_rpc_id: &str,
) -> Result<CliDisplaySettings, ErrorCode> {
    let result: DisplayWire = decode_app_response(bytes, expected_rpc_id, 4096)?;
    if result.schema_version != 1 || !result.ok {
        return Err(ErrorCode::InvalidInput);
    }
    let language = result
        .cli_language
        .into_option()
        .map(|value| CliDisplayLanguage::parse(&value))
        .transpose()?;
    Ok(CliDisplaySettings::new(language))
}

/// The optional lookup owns one 250ms async budget, including authentication.
pub fn get_default() -> Result<CliDisplaySettings, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .block_on(get_default_async())
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}

pub async fn get_default_async() -> Result<CliDisplaySettings, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::time::timeout(std::time::Duration::from_millis(250), async {
            use hantong_contracts::auth::AppReadTool;
            let profile =
                crate::authenticate_default_app_read(AppReadTool::DisplaySettingsGet).await?;
            let id = "display-settings-1";
            let tool = AppReadTool::DisplaySettingsGet.as_str();
            let envelope =
                serde_json::json!({"schema_version":1,"request_id":id,"tool":tool,"params":{}});
            let raw = profile.exchange(id, tool, &envelope).await?;
            parse_display_settings_response(&raw, id)
        })
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
#[path = "display_settings_tests.rs"]
mod tests;
