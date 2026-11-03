//! Display-only preferences shared by the terminal clients.
//!
//! This file deliberately contains no account credentials, provider settings,
//! grants, or policy state.  Aliases are presentation labels only.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_NAME: &str = "terminal-ui.json";
const LOCK_NAME: &str = "terminal-ui.json.lock";
const MAX_BYTES: usize = 32 * 1024;
const MAX_ACCOUNTS: usize = 128;
const MAX_ALIAS_BYTES: usize = 128;

#[derive(Clone, Eq, PartialEq, Default)]
pub struct Preferences {
    pub revision: u64,
    aliases: BTreeMap<String, String>,
}

impl std::fmt::Debug for Preferences {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Preferences")
            .field("revision", &self.revision)
            .field("alias_count", &self.aliases.len())
            .finish()
    }
}

impl Preferences {
    pub fn aliases(&self) -> &BTreeMap<String, String> {
        &self.aliases
    }

    pub fn alias(&self, account_id: &str) -> Option<&str> {
        self.aliases.get(account_id).map(String::as_str)
    }

    pub fn set(&mut self, account_id: &str, alias: &str) -> Result<(), String> {
        validate_account_id(account_id)?;
        if alias.is_empty() {
            self.aliases.remove(account_id);
        } else {
            let alias = alias.trim();
            validate_alias(alias)?;
            if !self.aliases.contains_key(account_id) && self.aliases.len() >= MAX_ACCOUNTS {
                return Err("too many account aliases".into());
            }
            self.aliases.insert(account_id.to_owned(), alias.to_owned());
        }
        Ok(())
    }
}

pub fn load() -> Result<Preferences, String> {
    let path = fixed_path()?;
    match File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::with_capacity(MAX_BYTES.min(4096));
            file.take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "ui preferences read failed".to_owned())?;
            decode(&bytes)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(_) => Err("ui preferences read failed".into()),
    }
}

/// Compare-and-swap save. The expected value must be the result of `load()`;
/// a concurrent writer therefore cannot silently overwrite another UI change.
pub fn save(
    expected: &Preferences,
    aliases: &BTreeMap<String, String>,
) -> Result<Preferences, String> {
    let path = fixed_path()?;
    let lock_path = path.with_file_name(LOCK_NAME);
    let _lock = open_lock(&lock_path)?;
    (|| {
        let current = load()?;
        if &current != expected {
            return Err("ui preferences changed concurrently".into());
        }
        let next = Preferences {
            revision: expected
                .revision
                .checked_add(1)
                .ok_or("ui preferences revision overflow")?,
            aliases: aliases.clone(),
        };
        validate_preferences(&next)?;
        let bytes = encode(&next)?;
        save_locked(&path, &bytes)?;
        Ok(next)
    })()
}

/// Convenience CAS update for one account alias.
pub fn set(expected: &Preferences, account_id: &str, alias: &str) -> Result<Preferences, String> {
    let mut aliases = expected.aliases.clone();
    if alias.is_empty() {
        aliases.remove(account_id);
    } else {
        validate_account_id(account_id)?;
        validate_alias(alias)?;
        if !aliases.contains_key(account_id) && aliases.len() >= MAX_ACCOUNTS {
            return Err("too many account aliases".into());
        }
        aliases.insert(account_id.to_owned(), alias.trim().to_owned());
    }
    save(expected, &aliases)
}

fn save_locked(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock error")?
        .as_nanos();
    let temp = path.with_file_name(format!("{FILE_NAME}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| "ui preferences temp failed".to_owned())?;
        file.write_all(bytes)
            .map_err(|_| "ui preferences write failed".to_owned())?;
        file.sync_all()
            .map_err(|_| "ui preferences flush failed".to_owned())?;
        fs::rename(&temp, path).map_err(|_| "ui preferences replace failed".to_owned())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn decode(bytes: &[u8]) -> Result<Preferences, String> {
    if bytes.len() > MAX_BYTES {
        return Err("ui preferences too large".into());
    }
    reject_duplicate_keys(bytes)?;
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid ui preferences JSON")?;
    let object = value
        .as_object()
        .ok_or("ui preferences must be an object")?;
    if object.len() != 3
        || !object.contains_key("format_version")
        || !object.contains_key("revision")
        || !object.contains_key("aliases")
    {
        return Err("unknown or missing ui preferences fields".into());
    }
    if object["format_version"].as_u64() != Some(1) {
        return Err("unsupported ui preferences version".into());
    }
    let revision = object["revision"]
        .as_u64()
        .ok_or("invalid ui preferences revision")?;
    let map = object["aliases"]
        .as_object()
        .ok_or("invalid ui preferences aliases")?;
    if map.len() > MAX_ACCOUNTS {
        return Err("too many account aliases".into());
    }
    let mut aliases = BTreeMap::new();
    for (account, value) in map {
        let alias = value.as_str().ok_or("alias must be a string")?;
        validate_account_id(account)?;
        validate_alias(alias)?;
        aliases.insert(account.clone(), alias.to_owned());
    }
    Ok(Preferences { revision, aliases })
}

pub fn encode(preferences: &Preferences) -> Result<Vec<u8>, String> {
    validate_preferences(preferences)?;
    let mut aliases = serde_json::Map::new();
    for (account, alias) in &preferences.aliases {
        aliases.insert(account.clone(), serde_json::Value::String(alias.clone()));
    }
    let mut object = serde_json::Map::new();
    object.insert("format_version".into(), serde_json::Value::from(1));
    object.insert(
        "revision".into(),
        serde_json::Value::from(preferences.revision),
    );
    object.insert("aliases".into(), serde_json::Value::Object(aliases));
    let bytes = serde_json::to_vec(&serde_json::Value::Object(object))
        .map_err(|_| "ui preferences encode failed")?;
    if bytes.len() > MAX_BYTES {
        return Err("ui preferences too large".into());
    }
    Ok(bytes)
}

fn validate_preferences(preferences: &Preferences) -> Result<(), String> {
    if preferences.aliases.len() > MAX_ACCOUNTS {
        return Err("too many account aliases".into());
    }
    for (account, alias) in &preferences.aliases {
        validate_account_id(account)?;
        validate_alias(alias)?;
    }
    Ok(())
}
fn validate_account_id(value: &str) -> Result<(), String> {
    if !(1..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    {
        return Err("invalid account id".into());
    }
    Ok(())
}
fn validate_alias(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.chars().count() > 32
        || value.len() > MAX_ALIAS_BYTES
        || value.chars().any(|c| {
            c.is_control()
                || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200F}')
        })
    {
        return Err("invalid account alias".into());
    }
    Ok(())
}

// serde_json::Value keeps the last duplicate object key. Scan object keys
// first so a repeated key cannot silently change the interpreted preferences.
fn reject_duplicate_keys(bytes: &[u8]) -> Result<(), String> {
    let mut objects: Vec<std::collections::BTreeSet<String>> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                let start = index;
                index += 1;
                let mut escaped = false;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if byte == b'\\' {
                        escaped = true;
                        continue;
                    }
                    if byte == b'"' {
                        break;
                    }
                }
                let token = &bytes[start..index];
                let mut look = index;
                while look < bytes.len() && bytes[look].is_ascii_whitespace() {
                    look += 1;
                }
                if look < bytes.len() && bytes[look] == b':' {
                    let key: String =
                        serde_json::from_slice(token).map_err(|_| "invalid ui preferences JSON")?;
                    if let Some(current) = objects.last_mut()
                        && !current.insert(key)
                    {
                        return Err("duplicate ui preferences field".into());
                    }
                }
            }
            b'{' => objects.push(std::collections::BTreeSet::new()),
            b'}' => {
                objects.pop();
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}

fn open_lock(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
        .open(path)
        .map_err(|_| "ui preferences lock failed".into())
}

fn fixed_path() -> Result<PathBuf, String> {
    let profiles = crate::app_directories()
        .map_err(|_| "ui preferences unavailable")?
        .profiles;
    if profiles.file_name().and_then(|n| n.to_str()) != Some("profiles") || profiles.is_symlink() {
        return Err("invalid profiles path".into());
    }
    if !profiles.is_dir() {
        return Err("invalid profiles path".into());
    }
    let path = profiles.join(FILE_NAME);
    if path.is_symlink() {
        return Err("invalid ui preferences file".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec_round_trip_and_clear() {
        let mut p = Preferences::default();
        p.set("acct-a", "개인").unwrap();
        let q = decode(&encode(&p).unwrap()).unwrap();
        assert_eq!(q.alias("acct-a"), Some("개인"));
        p.set("acct-a", "").unwrap();
        assert!(p.alias("acct-a").is_none());
    }
    #[test]
    fn codec_rejects_unknown_fields_and_terminal_spoofing() {
        assert!(decode(br#"{"format_version":1,"revision":0,"aliases":{},"x":1}"#).is_err());
        assert!(
            decode(br#"{"format_version":1,"format_version":1,"revision":0,"aliases":{}}"#)
                .is_err()
        );
        assert!(validate_alias("a\u{202E}b").is_err());
        assert!(validate_alias("x\u{200B}y").is_err());
        assert!(validate_account_id("account.id").is_err());
    }
    #[test]
    fn account_limit_is_bounded() {
        let mut p = Preferences::default();
        for n in 0..MAX_ACCOUNTS {
            p.set(&format!("a{n}"), "x").unwrap();
        }
        assert!(p.set("overflow", "x").is_err());
    }
}
