#![allow(unsafe_code)]
//! Fixed per-user startup registration. No shell or caller-selected command.
use hantong_contracts::platform::{AutoStart, AutoStartState, PlatformError};
use std::{
    path::{Component, Path, Prefix},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW,
    },
};

const RUN_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "mail-hantong";
const VALUE_MAX_BYTES: usize = 522;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this object exclusively owns the returned registry handle.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
}

/// Constructed only from the running core's executable. Merely constructing or
/// inspecting this adapter never registers startup or starts another process.
pub struct WindowsAutoStart {
    value: Vec<u16>,
    run_path: Vec<u16>,
}
impl WindowsAutoStart {
    pub fn for_current_core() -> Result<Self, PlatformError> {
        let path = std::env::current_exe().map_err(|_| PlatformError::Denied)?;
        Ok(Self {
            value: Self::command_for_path(&path)?,
            run_path: wide(RUN_PATH),
        })
    }

    fn command_for_path(path: &Path) -> Result<Vec<u16>, PlatformError> {
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or(PlatformError::Denied)?;
        let absolute = path.to_str().ok_or(PlatformError::Denied)?;
        if !path.is_absolute()
            || !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
            || path
                .components()
                .any(|value| matches!(value, Component::ParentDir))
            || !name.eq_ignore_ascii_case("mail-hantong-core.exe")
            || absolute
                .chars()
                .any(|c| c.is_control() || c == '"' || c == '%')
        {
            return Err(PlatformError::Denied);
        }
        let command = format!("\"{absolute}\" --supervise");
        if command.encode_utf16().count() > 260 {
            return Err(PlatformError::Denied);
        }
        Ok(wide(&command))
    }

    fn open(&self, write: bool, create: bool) -> Result<Option<Key>, PlatformError> {
        let mut raw = ptr::null_mut();
        let access = KEY_QUERY_VALUE | if write { KEY_SET_VALUE } else { 0 };
        // SAFETY: fixed/validated NUL-terminated path, valid out-pointer, no
        // inherited handle; the successful result is immediately owned by Key.
        let status = unsafe {
            if create {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    self.run_path.as_ptr(),
                    0,
                    ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    access,
                    ptr::null(),
                    &mut raw,
                    ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    self.run_path.as_ptr(),
                    0,
                    access,
                    &mut raw,
                )
            }
        };
        let key = Key(raw);
        match status {
            ERROR_SUCCESS if !key.0.is_null() => Ok(Some(key)),
            ERROR_FILE_NOT_FOUND if !create => Ok(None),
            _ => Err(PlatformError::Denied),
        }
    }

    fn state_for_key(&self, key: &Key) -> Result<AutoStartState, PlatformError> {
        let name = wide(VALUE_NAME);
        let mut bytes = [0u8; VALUE_MAX_BYTES];
        let mut size = bytes.len() as u32;
        let mut kind = 0;
        // SAFETY: owned live key, valid NUL-terminated name and bounded output
        // buffers. ERROR_MORE_DATA is never decoded or retried unboundedly.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        match status {
            ERROR_FILE_NOT_FOUND => return Ok(AutoStartState::Disabled),
            ERROR_MORE_DATA => return Ok(AutoStartState::OwnershipConflict),
            ERROR_SUCCESS => {}
            _ => return Err(PlatformError::Denied),
        }
        if size as usize > bytes.len() || kind != REG_SZ {
            return Ok(AutoStartState::OwnershipConflict);
        }
        // Byte equality also rejects malformed UTF-16, embedded/missing NULs,
        // odd length and extra arguments; expected bytes came from valid Rust str.
        let expected: Vec<u8> = self
            .value
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        Ok(if bytes[..size as usize] == expected {
            AutoStartState::Enabled
        } else {
            AutoStartState::OwnershipConflict
        })
    }
}
impl AutoStart for WindowsAutoStart {
    fn state(&self) -> Result<AutoStartState, PlatformError> {
        match self.open(false, false)? {
            Some(key) => self.state_for_key(&key),
            None => Ok(AutoStartState::Disabled),
        }
    }

    fn set_enabled(&mut self, enabled: bool) -> Result<(), PlatformError> {
        // A no-op needs only query access. Recheck after acquiring write access
        // below so a changed/foreign value is never treated as owned.
        match (enabled, self.state()?) {
            (_, AutoStartState::OwnershipConflict) => return Err(PlatformError::Denied),
            (true, AutoStartState::Enabled) | (false, AutoStartState::Disabled) => return Ok(()),
            _ => {}
        }
        let key = match self.open(true, enabled)? {
            Some(key) => key,
            None => return Ok(()), // disabling a missing key does not create it
        };
        let current = self.state_for_key(&key)?;
        match (enabled, current) {
            (_, AutoStartState::OwnershipConflict) => return Err(PlatformError::Denied),
            (true, AutoStartState::Enabled) | (false, AutoStartState::Disabled) => return Ok(()),
            _ => {}
        }
        let name = wide(VALUE_NAME);
        let status = if enabled {
            let bytes: Vec<u8> = self
                .value
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            // SAFETY: validated bounded REG_SZ includes its NUL; API copies it
            // synchronously. Only the fixed product value is touched.
            unsafe {
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                )
            }
        } else {
            // SAFETY: the live key was just checked for the exact owned value.
            unsafe { RegDeleteValueW(key.0, name.as_ptr()) }
        };
        if status != ERROR_SUCCESS {
            return Err(PlatformError::Denied);
        }
        let expected = if enabled {
            AutoStartState::Enabled
        } else {
            AutoStartState::Disabled
        };
        if self.state_for_key(&key)? != expected {
            return Err(PlatformError::Denied);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
