use hantong_contracts::ErrorCode;
use std::fmt;
use std::path::PathBuf;
#[cfg(windows)]
use std::path::{Component, Path, Prefix};

/// Fixed application roots beneath the current Windows user's LocalAppData.
/// The paths are discovered only; this API never creates or opens them.
#[derive(Clone, Eq, PartialEq)]
pub struct AppDirectories {
    pub data: PathBuf,
    pub secrets: PathBuf,
    pub profiles: PathBuf,
}

impl fmt::Debug for AppDirectories {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppDirectories")
            .field("data", &"<redacted>")
            .field("secrets", &"<redacted>")
            .field("profiles", &"<redacted>")
            .finish()
    }
}

pub fn app_directories() -> Result<AppDirectories, ErrorCode> {
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    {
        let base = known_local_app_data()?;
        compose(base)
    }
}

#[cfg(windows)]
fn compose(base: PathBuf) -> Result<AppDirectories, ErrorCode> {
    validate_base(&base)?;
    let product = base.join("mail-hantong");
    // Keep roots fixed and sibling-separated. Secrets performs the final
    // reparse-point and handle checks when it opens these roots.
    Ok(AppDirectories {
        data: product.join("data"),
        secrets: product.join("secrets"),
        profiles: product.join("profiles"),
    })
}

#[cfg(windows)]
fn validate_base(path: &Path) -> Result<(), ErrorCode> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) => {}
        _ => return Err(ErrorCode::CapabilityUnsupported),
    }
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    if components.any(|component| !matches!(component, Component::Normal(_))) {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    if path.as_os_str().is_empty() {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    Ok(())
}

#[cfg(windows)]
fn known_local_app_data() -> Result<PathBuf, ErrorCode> {
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::S_OK;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath};

    let mut raw = ptr::null_mut();
    // SAFETY: SHGetKnownFolderPath allocates a null-terminated UTF-16 path;
    // the returned pointer is released exactly once with CoTaskMemFree below.
    let status =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, ptr::null_mut(), &mut raw) };
    struct TaskMem(*mut u16);
    impl Drop for TaskMem {
        fn drop(&mut self) {
            // SAFETY: pointer came from SHGetKnownFolderPath and is freed once.
            unsafe { CoTaskMemFree(self.0.cast()) }
        }
    }
    let owned = TaskMem(raw);
    // Microsoft requires freeing the output even when the call fails.
    if status != S_OK || owned.0.is_null() {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let mut length = 0usize;
    // SAFETY: the API guarantees a null-terminated PWSTR on success.
    unsafe {
        loop {
            if length > 32_767 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            if *owned.0.add(length) == 0 {
                break;
            }
            length += 1;
        }
        let slice = std::slice::from_raw_parts(owned.0, length);
        Ok(std::ffi::OsString::from_wide(slice).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn inst01_fixed_children_are_siblings_and_no_path_input_is_reflected() {
        let base = PathBuf::from(r"C:\Users\fixture\AppData\Local");
        let directories = compose(base).unwrap();
        assert_eq!(
            directories.data,
            directories.secrets.parent().unwrap().join("data")
        );
        assert_eq!(directories.data.parent(), directories.secrets.parent());
        assert_eq!(
            directories.profiles,
            directories.data.parent().unwrap().join("profiles")
        );
        let debug = format!("{directories:?}");
        assert!(!debug.contains("fixture"));
    }

    #[cfg(windows)]
    #[test]
    fn inst01_rejects_unc_relative_parent_and_device_paths() {
        for value in [
            r"\\server\share\Local",
            r"relative\Local",
            r"C:\Users\..\Local",
            r"\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy1",
        ] {
            assert_eq!(
                compose(PathBuf::from(value)),
                Err(ErrorCode::CapabilityUnsupported)
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn inst01_non_windows_does_not_use_fake_paths() {
        assert_eq!(app_directories(), Err(ErrorCode::CapabilityUnsupported));
    }

    #[cfg(windows)]
    #[test]
    fn inst01_known_folder_discovery_ignores_environment_override() {
        let directories = app_directories().unwrap();
        if let Some(expected) = std::env::var_os("HANTONG_TEST_EXPECTED_DATA_ROOT") {
            assert_eq!(directories.data, PathBuf::from(expected));
            return;
        }
        assert!(directories.data.ends_with(Path::new(r"mail-hantong\data")));
        assert!(
            directories
                .secrets
                .ends_with(Path::new(r"mail-hantong\secrets"))
        );
        // Change only the child's environment; never mutate process-global
        // environment while other Rust tests may be running.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "paths::tests::inst01_known_folder_discovery_ignores_environment_override",
            ])
            .env("LOCALAPPDATA", r"C:\untrusted-environment-override")
            .env("HANTONG_TEST_EXPECTED_DATA_ROOT", &directories.data)
            .output()
            .unwrap();
        assert!(output.status.success(), "known-folder child test failed");
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}
