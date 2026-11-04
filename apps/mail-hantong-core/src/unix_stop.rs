//! Verified Unix Core stop entrypoint used during self-update.
//!
//! This command has no provider, Store, signer, or secret access.  It only
//! proves the installed image and asks the current Core socket which kernel
//! peer is the Core process to stop.

use hantong_contracts::ErrorCode;

pub(crate) fn run() -> Result<(), ErrorCode> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    runtime.block_on(run_async())
}

pub(crate) async fn run_async() -> Result<(), ErrorCode> {
    let image = hantong_platform::unix_release::verified_installed_core("mail-hantong-core")?;
    let Some(connection) = connect_if_running().await? else {
        return Ok(());
    };
    let peer = connection.peer_context().clone();
    hantong_platform::unix_lifecycle::stop_verified_core(&peer, &image)
}

/// An updater may legitimately find that the old Core already exited.  Treat
/// only a missing endpoint as that state; an existing malformed or hostile
/// endpoint must still go through the normal transport admission checks.
async fn connect_if_running()
-> Result<Option<hantong_platform::unix_transport::UnixLocalConnection>, ErrorCode> {
    let path = hantong_platform::unix_paths::socket_path()?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ErrorCode::CoreUnavailable),
        Ok(_) => {}
    }
    match hantong_platform::unix_transport::connect().await {
        Ok(connection) => Ok(Some(connection)),
        Err(ErrorCode::CoreUnavailable) if !path.exists() => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn isolated_missing_socket() -> bool {
        if std::env::var_os("HANTONG_STOP_CHILD").is_some() {
            return false;
        }
        let root = std::env::temp_dir().join(format!(
            "hantong-stop-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("runtime root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("private runtime root");
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("unix_stop::tests::cli07_missing_core_socket_is_a_noop")
            .arg("--nocapture")
            .env("HANTONG_STOP_CHILD", "1")
            .env("XDG_RUNTIME_DIR", &root)
            .status()
            .expect("child test");
        let _ = std::fs::remove_dir_all(root);
        assert!(status.success());
        true
    }

    #[tokio::test]
    async fn cli07_missing_core_socket_is_a_noop() {
        if isolated_missing_socket() {
            return;
        }
        let path = hantong_platform::unix_paths::socket_path().expect("socket path");
        assert!(!path.exists());
        assert!(super::connect_if_running().await.unwrap().is_none());
    }
}
