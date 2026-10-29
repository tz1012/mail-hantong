//! Bounded, typed decoding of authenticated account-display responses.
use hantong_contracts::ErrorCode;
use serde::{Deserialize, Serialize, de};
use std::{collections::BTreeSet, fmt, marker::PhantomData};

const MAX_FRAME: usize = 1_048_576;
const MAX_ACCOUNTS: usize = 128;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct AccountSummary {
    account_id: String,
    provider: String,
    primary_address: Option<String>,
    display_name: Option<String>,
    color: Option<String>,
    account_number: u64,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Vec<String>>,
}
impl fmt::Debug for AccountSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountSummary { <redacted> }")
    }
}
impl AccountSummary {
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn primary_address(&self) -> Option<&str> {
        self.primary_address.as_deref()
    }
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }
    pub fn account_number(&self) -> u64 {
        self.account_number
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn capabilities(&self) -> Option<&[String]> {
        self.capabilities.as_deref()
    }

    fn validate(&self) -> Result<(), ErrorCode> {
        if !valid_id(&self.account_id)
            || !(1..=i64::MAX as u64).contains(&self.account_number)
            || !matches!(
                self.provider.as_str(),
                "gmail" | "graph" | "imap" | "archive"
            )
            || !matches!(
                self.status.as_str(),
                "ready" | "connecting" | "reauth_required" | "offline" | "disabled" | "error"
            )
        {
            return Err(ErrorCode::InvalidInput);
        }
        valid_optional_display(self.primary_address.as_deref(), 320, 1280)?;
        valid_optional_display(self.display_name.as_deref(), 128, 512)?;
        if let Some(color) = &self.color
            && (color.len() != 7
                || !color.starts_with('#')
                || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit))
        {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(caps) = &self.capabilities
            && (caps.len() > 128
                || caps.windows(2).any(|w| w[0] >= w[1])
                || caps.iter().any(|v| {
                    v.is_empty()
                        || v.len() > 96
                        || !v.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-')
                        })
                }))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AccountListResponse {
    result: ResultWire,
}
impl fmt::Debug for AccountListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountListResponse { <redacted> }")
    }
}
impl AccountListResponse {
    pub fn items(&self) -> &[AccountSummary] {
        &self.result.items
    }
    pub fn observed_at(&self) -> &str {
        &self.result.observed_at
    }
    pub fn to_json(&self) -> String {
        // Only bounded strings, integers, booleans, nulls, and arrays are held.
        serde_json::to_string(&self.result).expect("validated account response is serializable")
    }
}

pub fn parse_account_list_response(
    bytes: &[u8],
    expected_id: &str,
    requested_ids: &[String],
    include_capabilities: bool,
) -> Result<AccountListResponse, ErrorCode> {
    if bytes.is_empty()
        || bytes.len() > MAX_FRAME
        || bytes.starts_with(&[0xef, 0xbb, 0xbf])
        || expected_id.is_empty()
        || expected_id.len() > 64
        || !expected_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
        || !(1..=MAX_ACCOUNTS).contains(&requested_ids.len())
        || requested_ids.iter().any(|id| !valid_id(id))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let expected: BTreeSet<&str> = requested_ids.iter().map(String::as_str).collect();
    if expected.len() != requested_ids.len() {
        return Err(ErrorCode::InvalidInput);
    }
    // Decode the original stream: a Value intermediate loses duplicate keys.
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    if envelope.jsonrpc != "2.0" || envelope.id != expected_id {
        return Err(ErrorCode::InvalidInput);
    }
    let result = match envelope.body {
        Body::Error(error) => {
            if error.code != error.message {
                return Err(ErrorCode::InvalidInput);
            }
            return Err(ErrorCode::ALL
                .into_iter()
                .find(|code| code.as_str() == error.code)
                .ok_or(ErrorCode::InvalidInput)?);
        }
        Body::Result(result) => *result,
    };
    if result.schema_version != 1
        || !result.ok
        || result.next_cursor.0.is_some()
        || result.completeness != "complete"
        || !result.warnings.is_empty()
        || result.metadata.source != "local_store"
        || !result.metadata.cached
        || result.items.len() != expected.len()
        || result.account_results.len() != expected.len()
        || !valid_timestamp(&result.observed_at)
    {
        return Err(ErrorCode::InvalidInput);
    }
    let mut seen = BTreeSet::new();
    for (index, item) in result.items.iter().enumerate() {
        item.validate()?;
        if !expected.contains(item.account_id.as_str())
            || !seen.insert(item.account_id.as_str())
            || include_capabilities != item.capabilities.is_some()
        {
            return Err(ErrorCode::InvalidInput);
        }
        if index > 0 {
            let previous = &result.items[index - 1];
            if (previous.account_number, &previous.account_id)
                >= (item.account_number, &item.account_id)
            {
                return Err(ErrorCode::InvalidInput);
            }
        }
        let row = &result.account_results[index];
        if row.account_id != item.account_id
            || row.status != "complete"
            || row.items.len() != 1
            || row.items[0] != *item
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    Ok(AccountListResponse { result })
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn valid_optional_display(
    value: Option<&str>,
    chars: usize,
    bytes: usize,
) -> Result<(), ErrorCode> {
    if let Some(value) = value
        && (value.is_empty()
            || value.len() > bytes
            || value.chars().count() > chars
            || value.trim().is_empty()
            || value.chars().any(char::is_control))
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}
pub(crate) fn valid_timestamp(value: &str) -> bool {
    let b = value.as_bytes();
    if !(b.len() == 20 || (22..=24).contains(&b.len()))
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[b.len() - 1] != b'Z'
        || [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .any(|&i| !b[i].is_ascii_digit())
    {
        return false;
    }
    let pair = |i: usize| u32::from(b[i] - b'0') * 10 + u32::from(b[i + 1] - b'0');
    let year = pair(0) * 100 + pair(2);
    let (month, day, hour, minute, second) = (pair(5), pair(8), pair(11), pair(14), pair(17));
    if year < 1970 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    let days = match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=days).contains(&day) {
        return false;
    }
    let fraction = if b.len() == 20 {
        &[][..]
    } else {
        if b[19] != b'.' {
            return false;
        }
        &b[20..b.len() - 1]
    };
    if !fraction.iter().all(u8::is_ascii_digit) {
        return false;
    }
    (year, month, day, hour, minute, second) != (1970, 1, 1, 0, 0, 0)
        || fraction.iter().any(|b| *b != b'0')
}

/// Derived structs accept sequences too; this boundary permits only maps.
struct Object<T>(T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Visitor<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("object")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(de::value::MapAccessDeserializer::new(map)).map(Object)
            }
        }
        deserializer.deserialize_map(Visitor(PhantomData))
    }
}

/// Missing is an error; only explicit JSON null or a string is accepted.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct NullableString(Option<String>);
impl<'de> Deserialize<'de> for NullableString {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl de::Visitor<'_> for Visitor {
            type Value = NullableString;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("string or null")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(NullableString(None))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(NullableString(Some(value.into())))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(NullableString(Some(value)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
#[derive(Default)]
struct OptionalCapabilities(Option<Vec<String>>);
impl<'de> Deserialize<'de> for OptionalCapabilities {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<String>::deserialize(deserializer).map(|values| Self(Some(values)))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountWire {
    account_id: String,
    provider: String,
    primary_address: NullableString,
    display_name: NullableString,
    color: NullableString,
    account_number: u64,
    status: String,
    #[serde(default)]
    capabilities: OptionalCapabilities,
}
impl<'de> Deserialize<'de> for AccountSummary {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Object(value) = Object::<AccountWire>::deserialize(deserializer)?;
        Ok(Self {
            account_id: value.account_id,
            provider: value.provider,
            primary_address: value.primary_address.0,
            display_name: value.display_name.0,
            color: value.color.0,
            account_number: value.account_number,
            status: value.status,
            capabilities: value.capabilities.0,
        })
    }
}
#[derive(Clone, PartialEq, Eq, Serialize)]
struct AccountResult {
    account_id: String,
    status: String,
    items: Vec<AccountSummary>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountResultWire {
    account_id: String,
    status: String,
    items: Vec<AccountSummary>,
}
impl<'de> Deserialize<'de> for AccountResult {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Object(value) = Object::<AccountResultWire>::deserialize(deserializer)?;
        Ok(Self {
            account_id: value.account_id,
            status: value.status,
            items: value.items,
        })
    }
}
#[derive(Clone, PartialEq, Eq, Serialize)]
struct Metadata {
    source: String,
    cached: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataWire {
    source: String,
    cached: bool,
}
impl<'de> Deserialize<'de> for Metadata {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Object(value) = Object::<MetadataWire>::deserialize(deserializer)?;
        Ok(Self {
            source: value.source,
            cached: value.cached,
        })
    }
}
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResultWire {
    schema_version: u64,
    ok: bool,
    items: Vec<AccountSummary>,
    next_cursor: NullableString,
    account_results: Vec<AccountResult>,
    completeness: String,
    warnings: Vec<()>,
    observed_at: String,
    metadata: Metadata,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorWire {
    code: String,
    message: String,
}
struct Envelope {
    jsonrpc: String,
    id: String,
    body: Body,
}
enum Body {
    Result(Box<ResultWire>),
    Error(ErrorWire),
}
impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Envelope;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON-RPC object")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut jsonrpc, mut id, mut body) = (None, None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "jsonrpc" if jsonrpc.is_none() => {
                            jsonrpc = Some(map.next_value::<String>()?)
                        }
                        "id" if id.is_none() => id = Some(map.next_value::<String>()?),
                        "result" if body.is_none() => {
                            let Object(result) = map.next_value::<Object<ResultWire>>()?;
                            body = Some(Body::Result(Box::new(result)));
                        }
                        "error" if body.is_none() => {
                            let Object(error) = map.next_value::<Object<ErrorWire>>()?;
                            body = Some(Body::Error(error));
                        }
                        _ => {
                            return Err(de::Error::custom(
                                "unknown, duplicate, or conflicting field",
                            ));
                        }
                    }
                }
                Ok(Envelope {
                    jsonrpc: jsonrpc.ok_or_else(|| de::Error::missing_field("jsonrpc"))?,
                    id: id.ok_or_else(|| de::Error::missing_field("id"))?,
                    body: body.ok_or_else(|| de::Error::custom("missing body"))?,
                })
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[cfg(windows)]
pub fn list_default_accounts(include_capabilities: bool) -> Result<AccountListResponse, ErrorCode> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let profile = crate::authenticate_default().await?;
            let ids = profile.accounts().to_vec();
            if ids.is_empty() {
                return Err(ErrorCode::ScopeDenied);
            }
            let request_id = "accounts-list-1";
            let request = serde_json::json!({
                "schema_version":1,"request_id":request_id,"tool":"mail.accounts.list",
                "params":{"account_ids":ids,"include_capabilities":include_capabilities}
            });
            let raw = profile
                .exchange(request_id, "mail.accounts.list", &request)
                .await?;
            parse_account_list_response(&raw, request_id, &ids, include_capabilities)
        })
}
#[cfg(not(windows))]
pub fn list_default_accounts(_: bool) -> Result<AccountListResponse, ErrorCode> {
    Err(ErrorCode::CapabilityUnsupported)
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
