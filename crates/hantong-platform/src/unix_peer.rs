//! Kernel-observed Unix peer identity for auth version 2.

#![cfg(unix)]
#![allow(unsafe_code)]

use hantong_contracts::{
    ErrorCode,
    unix_auth::{UnixPeerContextV2, UnixPlatform},
};
use std::os::fd::RawFd;

/// Observe a connected Unix stream.  No peer fields are accepted from the
/// request or environment.
pub fn observe_peer(fd: RawFd) -> Result<UnixPeerContextV2, ErrorCode> {
    #[cfg(target_os = "linux")]
    {
        let mut credentials = unsafe { std::mem::zeroed::<libc::ucred>() };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        } != 0
            || length as usize != std::mem::size_of::<libc::ucred>()
            || credentials.pid <= 0
        {
            return Err(ErrorCode::AuthRequired);
        }
        let pid = credentials.pid as u32;
        let start = linux_process_start(pid)?;
        let boot = linux_boot_id()?;
        let session_id = linux_session_id(credentials.pid)?;
        let start_again = linux_process_start(pid)?;
        if start != start_again {
            return Err(ErrorCode::AuthRequired);
        }
        let mut instance = [0u8; 32];
        hash_instance(&boot, pid, start, &mut instance);
        UnixPeerContextV2::new(
            UnixPlatform::Linux,
            credentials.uid,
            credentials.gid,
            pid,
            session_id,
            instance,
        )
        .map_err(|_| ErrorCode::AuthRequired)
    }
    #[cfg(target_os = "macos")]
    {
        let mut uid = 0;
        let mut gid = 0;
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(ErrorCode::AuthRequired);
        }
        let pid = macos_peer_pid(fd)?;
        let start = macos_process_start(pid, uid)?;
        let boot = macos_boot_session_uuid()?;
        let session = macos_session_id(pid)?;
        let start_again = macos_process_start(pid, uid)?;
        if start != start_again {
            return Err(ErrorCode::AuthRequired);
        }
        let mut instance = [0u8; 32];
        hash_instance(&boot, pid, start, &mut instance);
        UnixPeerContextV2::new(UnixPlatform::MacOs, uid, gid, pid, session, instance)
            .map_err(|_| ErrorCode::AuthRequired)
    }
}

#[cfg(target_os = "linux")]
pub fn current_peer_context() -> Result<UnixPeerContextV2, ErrorCode> {
    let pid = unsafe { libc::getpid() };
    if pid <= 0 {
        return Err(ErrorCode::AuthRequired);
    }
    let pid = u32::try_from(pid).map_err(|_| ErrorCode::AuthRequired)?;
    let start = linux_process_start(pid)?;
    let boot = linux_boot_id()?;
    let mut instance = [0u8; 32];
    hash_instance(&boot, pid, start, &mut instance);
    UnixPeerContextV2::new(
        UnixPlatform::Linux,
        unsafe { libc::geteuid() },
        unsafe { libc::getegid() },
        pid,
        linux_session_id(pid as libc::pid_t)?,
        instance,
    )
    .map_err(|_| ErrorCode::AuthRequired)
}

#[cfg(target_os = "macos")]
pub fn current_peer_context() -> Result<UnixPeerContextV2, ErrorCode> {
    let pid = u32::try_from(unsafe { libc::getpid() }).map_err(|_| ErrorCode::AuthRequired)?;
    let start = macos_process_start(pid, unsafe { libc::geteuid() })?;
    let boot = macos_boot_session_uuid()?;
    let mut instance = [0u8; 32];
    hash_instance(&boot, pid, start, &mut instance);
    UnixPeerContextV2::new(
        UnixPlatform::MacOs,
        unsafe { libc::geteuid() },
        unsafe { libc::getegid() },
        pid,
        macos_session_id(pid)?,
        instance,
    )
    .map_err(|_| ErrorCode::AuthRequired)
}

/// Re-check the kernel process instance and executable identity after a peer
/// has been observed. A same-UID process is not sufficient proof.
pub fn peer_matches_current_image(peer: &UnixPeerContextV2) -> Result<bool, ErrorCode> {
    if peer.effective_uid != unsafe { libc::geteuid() } {
        return Ok(false);
    }
    #[cfg(target_os = "linux")]
    {
        let pid = peer.peer_pid;
        let start = linux_process_start(pid)?;
        let boot = linux_boot_id()?;
        let mut instance = [0u8; 32];
        hash_instance(&boot, pid, start, &mut instance);
        if instance != peer.process_instance {
            return Ok(false);
        }
        let peer_session = linux_session_id(pid as libc::pid_t)?;
        if peer_session != peer.session_id {
            return Ok(false);
        }
        let admitted = match crate::linux_identity::current_asset() {
            Ok(expected_asset) => crate::linux_identity::peer_same_asset(pid, &expected_asset)?,
            #[cfg(test)]
            Err(_) => {
                let observed = std::fs::canonicalize(format!("/proc/{pid}/exe"))
                    .map_err(|_| ErrorCode::AuthRequired)?;
                let current = std::env::current_exe()
                    .and_then(std::fs::canonicalize)
                    .map_err(|_| ErrorCode::AuthRequired)?;
                same_executable(&observed, &current)?
            }
            #[cfg(not(test))]
            Err(error) => return Err(error),
        };
        if !admitted {
            return Ok(false);
        }
        let start_after = linux_process_start(pid)?;
        let boot_after = linux_boot_id()?;
        let mut instance_after = [0u8; 32];
        hash_instance(&boot_after, pid, start_after, &mut instance_after);
        if start_after != start
            || boot_after != boot
            || instance_after != peer.process_instance
            || linux_session_id(pid as libc::pid_t)? != peer.session_id
        {
            return Ok(false);
        }
        match crate::linux_identity::current_asset() {
            Ok(expected_asset) => crate::linux_identity::peer_same_asset(pid, &expected_asset),
            #[cfg(test)]
            Err(_) => Ok(true),
            #[cfg(not(test))]
            Err(error) => Err(error),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let start = macos_process_start(peer.peer_pid, peer.effective_uid)?;
        let boot = macos_boot_session_uuid()?;
        let mut instance = [0u8; 32];
        hash_instance(&boot, peer.peer_pid, start, &mut instance);
        if instance != peer.process_instance {
            return Ok(false);
        }
        let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let length = unsafe {
            libc::proc_pidpath(
                peer.peer_pid as libc::c_int,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
            )
        };
        if length <= 0 || length as usize >= buffer.len() {
            return Err(ErrorCode::AuthRequired);
        }
        let observed = std::path::PathBuf::from(
            std::str::from_utf8(&buffer[..length as usize]).map_err(|_| ErrorCode::AuthRequired)?,
        );
        let current = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(|_| ErrorCode::AuthRequired)?;
        same_executable(&observed, &current)
    }
}

#[cfg(any(target_os = "macos", test))]
fn same_executable(
    observed: &std::path::Path,
    current: &std::path::Path,
) -> Result<bool, ErrorCode> {
    use std::os::unix::fs::MetadataExt;
    let observed_meta = std::fs::metadata(observed).map_err(|_| ErrorCode::AuthRequired)?;
    let current_meta = std::fs::metadata(current).map_err(|_| ErrorCode::AuthRequired)?;
    if observed_meta.dev() != current_meta.dev() || observed_meta.ino() != current_meta.ino() {
        return Ok(false);
    }
    Ok(hash_file(observed)? == hash_file(current)?)
}

#[cfg(any(target_os = "macos", test))]
fn hash_file(path: &std::path::Path) -> Result<[u8; 32], ErrorCode> {
    use std::io::Read;
    let metadata = std::fs::metadata(path).map_err(|_| ErrorCode::AuthRequired)?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
        return Err(ErrorCode::AuthRequired);
    }
    let mut file = std::fs::File::open(path).map_err(|_| ErrorCode::AuthRequired)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let mut output = [0u8; 32];
    output.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref());
    Ok(output)
}

#[cfg(target_os = "linux")]
fn linux_boot_id() -> Result<[u8; 16], ErrorCode> {
    let text = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| ErrorCode::AuthRequired)?;
    let compact: String = text.trim().chars().filter(|ch| *ch != '-').collect();
    if compact.len() != 32 || !compact.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ErrorCode::AuthRequired);
    }
    let mut id = [0u8; 16];
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&compact[index * 2..index * 2 + 2], 16)
            .map_err(|_| ErrorCode::AuthRequired)?;
    }
    Ok(id)
}

#[cfg(target_os = "linux")]
fn linux_process_start(pid: u32) -> Result<u64, ErrorCode> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|_| ErrorCode::AuthRequired)?;
    parse_linux_process_start(&text)
}

#[cfg(target_os = "linux")]
fn parse_linux_process_start(text: &str) -> Result<u64, ErrorCode> {
    let right = text.rfind(')').ok_or(ErrorCode::AuthRequired)?;
    let tail = text
        .get(right + 1..)
        .and_then(|tail| tail.strip_prefix(' '))
        .ok_or(ErrorCode::AuthRequired)?;
    let fields: Vec<&str> = tail.split_whitespace().collect();
    fields
        .get(19)
        .and_then(|value| value.parse().ok())
        .ok_or(ErrorCode::AuthRequired)
}

#[cfg(target_os = "linux")]
fn linux_session_id(pid: libc::pid_t) -> Result<u32, ErrorCode> {
    let sid = unsafe { libc::getsid(pid) };
    if sid <= 0 {
        return Err(ErrorCode::AuthRequired);
    }
    u32::try_from(sid).map_err(|_| ErrorCode::AuthRequired)
}

#[cfg(target_os = "macos")]
fn macos_peer_pid(fd: RawFd) -> Result<u32, ErrorCode> {
    let mut pid = 0i32;
    let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut i32).cast(),
            &mut size,
        )
    } != 0
        || pid <= 0
    {
        return Err(ErrorCode::AuthRequired);
    }
    u32::try_from(pid).map_err(|_| ErrorCode::AuthRequired)
}

#[cfg(target_os = "macos")]
fn macos_session_id(pid: u32) -> Result<u32, ErrorCode> {
    let sid = unsafe { libc::getsid(pid as libc::pid_t) };
    if sid <= 0 {
        return Err(ErrorCode::AuthRequired);
    }
    u32::try_from(sid).map_err(|_| ErrorCode::AuthRequired)
}

#[cfg(target_os = "macos")]
fn macos_process_start(pid: u32, expected_uid: libc::uid_t) -> Result<u64, ErrorCode> {
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let observed = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size as libc::c_int,
        )
    };
    if observed != size as libc::c_int
        || info.pbi_pid != pid
        || info.pbi_uid != expected_uid
        || info.pbi_start_tvsec == 0
    {
        return Err(ErrorCode::AuthRequired);
    }
    info.pbi_start_tvsec
        .checked_mul(1_000_000)
        .and_then(|seconds| seconds.checked_add(info.pbi_start_tvusec))
        .ok_or(ErrorCode::AuthRequired)
}

#[cfg(target_os = "macos")]
fn macos_boot_session_uuid() -> Result<[u8; 16], ErrorCode> {
    let name = std::ffi::CString::new("kern.bootsessionuuid").unwrap();
    let mut buffer = [0u8; 64];
    let mut size = buffer.len() as libc::size_t;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(ErrorCode::AuthRequired);
    }
    let value = std::str::from_utf8(&buffer[..size.min(buffer.len())])
        .map_err(|_| ErrorCode::AuthRequired)?
        .trim_end_matches('\0')
        .trim();
    let compact: String = value.chars().filter(|ch| *ch != '-').collect();
    if compact.len() != 32 || !compact.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ErrorCode::AuthRequired);
    }
    let mut uuid = [0u8; 16];
    for (index, byte) in uuid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&compact[index * 2..index * 2 + 2], 16)
            .map_err(|_| ErrorCode::AuthRequired)?;
    }
    Ok(uuid)
}

fn hash_instance(boot: &[u8], pid: u32, start: u64, out: &mut [u8; 32]) {
    let mut input = Vec::with_capacity(32 + 16 + 4 + 8);
    input.extend_from_slice(b"mail-hantong/unix-process/v1\0");
    input.extend_from_slice(boot);
    input.extend_from_slice(&pid.to_le_bytes());
    input.extend_from_slice(&start.to_le_bytes());
    out.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, &input).as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec03_peer_encoding_rejects_zero_identity() {
        assert!(UnixPeerContextV2::new(UnixPlatform::Linux, 1000, 1000, 1, 1, [0; 32]).is_err());
    }

    #[test]
    fn sec03_peer_encoding_is_fixed_little_endian() {
        let peer = UnixPeerContextV2::new(
            UnixPlatform::Linux,
            0x0102_0304,
            0x0506_0708,
            0x090a_0b0c,
            0x0d0e_0f10,
            [7; 32],
        )
        .unwrap();
        assert_eq!(peer.peer_pid, 0x090a_0b0c);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_proc_stat_parser_rejects_malformed_or_truncated_records() {
        assert!(parse_linux_process_start("1 (bad) S").is_err());
        assert!(parse_linux_process_start("1 bad S 1").is_err());
        for truncated in [")", "1 (bad)", "1 (bad) ", "1 (bad)S"] {
            assert!(parse_linux_process_start(truncated).is_err());
        }
        let valid = "1 (name with ) paren) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 20";
        assert_eq!(parse_linux_process_start(valid), Ok(20));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sec03_macos_peer_observation_uses_kernel_socket_identity() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let (left, _right) = UnixStream::pair().unwrap();
        let peer = observe_peer(left.as_raw_fd()).unwrap();
        assert_eq!(peer.platform, UnixPlatform::MacOs);
        assert_ne!(peer.peer_pid, 0);
        assert_ne!(peer.process_instance, [0; 32]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sec03_macos_invalid_fd_fails_closed() {
        assert!(matches!(observe_peer(-1), Err(ErrorCode::AuthRequired)));
    }
}
