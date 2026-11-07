#![allow(unsafe_code)]

//! Linux-only primitives for the R2 sealed execution contract.
//!
//! This module is deliberately not wired into the platform crate by itself.  The
//! caller must establish the release pin and the process-instance checks around
//! these primitives.  In particular, a digest or an fd name is never treated as
//! a trust anchor here.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use hantong_contracts::ErrorCode;
use ring::digest::{Context, SHA256};

const MFD_CLOEXEC: libc::c_uint = 0x0001;
const MFD_ALLOW_SEALING: libc::c_uint = 0x0002;
const MFD_NOEXEC_SEAL: libc::c_uint = 0x0008;
const MFD_EXEC: libc::c_uint = 0x0010;
const F_SEAL_SEAL: libc::c_int = 0x0001;
const F_SEAL_SHRINK: libc::c_int = 0x0002;
const F_SEAL_GROW: libc::c_int = 0x0004;
const F_SEAL_WRITE: libc::c_int = 0x0008;
const F_SEAL_EXEC: libc::c_int = 0x0020;
const REQUIRED_SEALS: libc::c_int = F_SEAL_SEAL | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE;
const ROLE_ROOT: RawFd = 197;
const ROLE_IMAGE: RawFd = 198;
const ROLE_CONTEXT: RawFd = 199;
const ROLE_MANIFEST: RawFd = 200;
const ROLE_DESCRIPTOR: RawFd = 201;
const ROLE_FIRST_TEMP: RawFd = 202;
const MAX_IMAGE: u64 = 64 * 1024 * 1024;
const MAX_CONTEXT: u64 = 4096;
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
const MAX_DESCRIPTOR: u64 = 256 * 1024;
const PR_SET_NO_NEW_PRIVS: libc::c_int = 38;
const PR_SET_SECCOMP: libc::c_int = 22;
const SECCOMP_MODE_FILTER: libc::c_ulong = 2;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
const SECCOMP_DATA_NR: u32 = 0;
const SECCOMP_DATA_ARCH: u32 = 4;
const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

fn invalid() -> ErrorCode {
    ErrorCode::InvalidInput
}

fn io_error(error: io::Error) -> ErrorCode {
    match error.raw_os_error() {
        Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP) => ErrorCode::CapabilityUnsupported,
        Some(libc::EACCES | libc::EPERM) => ErrorCode::PolicyBlocked,
        _ => ErrorCode::InternalError,
    }
}

fn exec_filter() -> [libc::sock_filter; 9] {
    [
        libc::sock_filter {
            code: (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            jt: 0,
            jf: 0,
            k: SECCOMP_DATA_ARCH,
        },
        libc::sock_filter {
            code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            jt: 1,
            jf: 0,
            k: AUDIT_ARCH_X86_64,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ERRNO | libc::EPERM as u32,
        },
        libc::sock_filter {
            code: (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            jt: 0,
            jf: 0,
            k: SECCOMP_DATA_NR,
        },
        libc::sock_filter {
            code: (libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K) as u16,
            jt: 3,
            jf: 0,
            k: X32_SYSCALL_BIT,
        },
        libc::sock_filter {
            code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            jt: 2,
            jf: 0,
            k: libc::SYS_execve as u32,
        },
        libc::sock_filter {
            code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            jt: 1,
            jf: 0,
            k: libc::SYS_execveat as u32,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ALLOW,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ERRNO | libc::EPERM as u32,
        },
    ]
}

/// Install the helper-only execution guard. Call before creating threads or
/// an async runtime; this process-local filter denies both exec syscall forms
/// and x32 syscall numbers while allowing all other syscalls.
pub fn forbid_helper_exec() -> Result<(), ErrorCode> {
    if !cfg!(target_arch = "x86_64") {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let mut filter = exec_filter();
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: prctl receives scalar values and a pointer to a live immutable
    // filter for the duration of the call. The filter is process-local.
    if unsafe { libc::prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    // SAFETY: the BPF program is fully initialized and remains live here.
    if unsafe { libc::prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    Ok(())
}

fn sha256_file(file: &mut File, size: u64) -> Result<[u8; 32], ErrorCode> {
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut context = Context::new(&SHA256);
    let mut buf = [0u8; 64 * 1024];
    let mut left = size;
    while left != 0 {
        let want = usize::try_from(left.min(buf.len() as u64)).map_err(|_| invalid())?;
        let count = file.read(&mut buf[..want]).map_err(io_error)?;
        if count == 0 {
            return Err(invalid());
        }
        context.update(&buf[..count]);
        left -= count as u64;
    }
    let digest = context.finish();
    let mut result = [0u8; 32];
    result.copy_from_slice(digest.as_ref());
    Ok(result)
}

fn memfd(name: &[u8], executable: bool) -> Result<File, ErrorCode> {
    let mut name_buf = [0u8; 249];
    let name_len = name.len().min(name_buf.len() - 1);
    name_buf[..name_len].copy_from_slice(&name[..name_len]);
    let flags = MFD_CLOEXEC
        | MFD_ALLOW_SEALING
        | if executable {
            MFD_EXEC
        } else {
            MFD_NOEXEC_SEAL
        };
    // SAFETY: the pointer is NUL-terminated and syscall arguments are plain integers.
    let fd = unsafe { libc::syscall(libc::SYS_memfd_create, name_buf.as_ptr(), flags) };
    if fd < 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    // SAFETY: syscall returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd as RawFd) })
}

fn seals(fd: RawFd) -> Result<libc::c_int, ErrorCode> {
    // SAFETY: fcntl reads immutable descriptor state.
    let result = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
    if result < 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    Ok(result)
}

fn add_seals(fd: RawFd) -> Result<(), ErrorCode> {
    // SAFETY: fcntl changes only this newly-created private memfd.
    let result = unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, REQUIRED_SEALS) };
    if result < 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    if seals(fd)? & REQUIRED_SEALS != REQUIRED_SEALS {
        return Err(invalid());
    }
    Ok(())
}

fn validate_elf(file: &mut File, size: u64) -> Result<(), ErrorCode> {
    if size < 64 {
        return Err(invalid());
    }
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header).map_err(io_error)?;
    let elf_type = u16::from_le_bytes([header[16], header[17]]);
    let machine = u16::from_le_bytes([header[18], header[19]]);
    if header[..4] != *b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || !matches!(elf_type, 2 | 3)
        || machine != 0x3e
    {
        return Err(invalid());
    }
    Ok(())
}

/// Copy a bounded source into a fresh sealed memfd and measure the sealed bytes.
pub fn seal_copy(
    source: &mut File,
    expected_size: u64,
    expected_sha: [u8; 32],
    executable: bool,
) -> Result<File, ErrorCode> {
    let limit = if executable { MAX_IMAGE } else { MAX_MANIFEST };
    if expected_size == 0 || expected_size > limit {
        return Err(invalid());
    }
    let mut target = memfd(
        if executable {
            b"hantong-image"
        } else {
            b"hantong-metadata"
        },
        executable,
    )?;
    source.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut left = expected_size;
    let mut buf = [0u8; 64 * 1024];
    while left != 0 {
        let want = usize::try_from(left.min(buf.len() as u64)).map_err(|_| invalid())?;
        let count = source.read(&mut buf[..want]).map_err(io_error)?;
        if count == 0 {
            return Err(invalid());
        }
        target.write_all(&buf[..count]).map_err(io_error)?;
        left -= count as u64;
    }
    let mut extra = [0u8; 1];
    if source.read(&mut extra).map_err(io_error)? != 0 {
        return Err(invalid());
    }
    if executable {
        validate_elf(&mut target, expected_size)?;
        // SAFETY: target is a newly-created private memfd owned by this call.
        if unsafe { libc::fchmod(target.as_raw_fd(), 0o755) } != 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
    } else if unsafe { libc::fchmod(target.as_raw_fd(), 0o600) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    add_seals(target.as_raw_fd())?;
    validate_sealed(&mut target, expected_size, expected_sha, executable)?;
    Ok(target)
}

/// Validate a sealed object without trusting its name or path.
pub fn validate_sealed(
    file: &mut File,
    expected_size: u64,
    expected_sha: [u8; 32],
    executable: bool,
) -> Result<(), ErrorCode> {
    let limit = if executable { MAX_IMAGE } else { MAX_MANIFEST };
    if expected_size == 0 || expected_size > limit {
        return Err(invalid());
    }
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.file_type().is_file() || metadata.len() != expected_size {
        return Err(invalid());
    }
    if executable {
        if metadata.permissions().mode() & 0o111 != 0o111 {
            return Err(invalid());
        }
    } else if metadata.permissions().mode() & 0o111 != 0 {
        return Err(invalid());
    }
    let current_seals = seals(file.as_raw_fd())?;
    if current_seals & REQUIRED_SEALS != REQUIRED_SEALS {
        return Err(invalid());
    }
    if !executable && current_seals & F_SEAL_EXEC == 0 {
        return Err(invalid());
    }
    if executable {
        validate_elf(file, expected_size)?;
    }
    if sha256_file(file, expected_size)? != expected_sha {
        return Err(invalid());
    }
    Ok(())
}

fn stat_fd(fd: RawFd) -> Result<libc::stat, ErrorCode> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: stat is an out pointer to valid uninitialized storage.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    // SAFETY: fstat initialized the structure on success.
    Ok(unsafe { stat.assume_init() })
}

fn same_object(left: &libc::stat, right: &libc::stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

/// Compare `/proc/PID/exe` with a held image by device/inode and required seals.
pub fn proc_image_matches(pid: u32, image: &File) -> Result<bool, ErrorCode> {
    let path = PathBuf::from(format!("/proc/{pid}/exe"));
    let peer = File::open(path).map_err(io_error)?;
    let peer_stat = stat_fd(peer.as_raw_fd())?;
    let image_stat = stat_fd(image.as_raw_fd())?;
    if image_stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(invalid());
    }
    if !same_object(&peer_stat, &image_stat) {
        return Ok(false);
    }
    let image_path = format!("/proc/self/fd/{}", image.as_raw_fd());
    let mut image_for_validation = File::open(image_path).map_err(io_error)?;
    validate_elf(&mut image_for_validation, image_stat.st_size as u64)?;
    let current_seals = seals(image.as_raw_fd())?;
    Ok(current_seals & REQUIRED_SEALS == REQUIRED_SEALS
        && current_seals & F_SEAL_EXEC == 0
        && image.metadata().map_err(io_error)?.permissions().mode() & 0o111 == 0o111)
}

fn open_proc_role(pid: u32, role: RawFd) -> Result<File, ErrorCode> {
    let path = format!("/proc/{pid}/fd/{role}");
    let flags = if role == ROLE_ROOT {
        libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC
    } else {
        libc::O_RDONLY | libc::O_CLOEXEC
    };
    // SAFETY: path is an owned NUL-free Rust string and flags are constants.
    let fd = std::ffi::CString::new(path)
        .map_err(|_| invalid())
        .and_then(|path| {
            // SAFETY: CString supplies a valid NUL-terminated path.
            let fd = unsafe { libc::open(path.as_ptr(), flags) };
            if fd < 0 {
                Err(io_error(io::Error::last_os_error()))
            } else {
                // SAFETY: open returned an owned descriptor.
                Ok(unsafe { File::from_raw_fd(fd) })
            }
        })?;
    Ok(fd)
}

/// Open and validate the five retained role descriptors of a peer process.
pub fn checked_role_files(pid: u32) -> Result<[File; 5], ErrorCode> {
    let mut files = [
        open_proc_role(pid, ROLE_ROOT)?,
        open_proc_role(pid, ROLE_IMAGE)?,
        open_proc_role(pid, ROLE_CONTEXT)?,
        open_proc_role(pid, ROLE_MANIFEST)?,
        open_proc_role(pid, ROLE_DESCRIPTOR)?,
    ];
    let root_stat = stat_fd(files[0].as_raw_fd())?;
    if root_stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(invalid());
    }
    let limits = [MAX_IMAGE, MAX_CONTEXT, MAX_MANIFEST, MAX_DESCRIPTOR];
    let mut stats = Vec::with_capacity(4);
    for (index, (file, limit)) in files.iter_mut().skip(1).zip(limits).enumerate() {
        let stat = stat_fd(file.as_raw_fd())?;
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG
            || stat.st_size < 1
            || stat.st_size as u64 > limit
        {
            return Err(invalid());
        }
        let current_seals = seals(file.as_raw_fd())?;
        if current_seals & REQUIRED_SEALS != REQUIRED_SEALS {
            return Err(invalid());
        }
        if index == 0 {
            if current_seals & F_SEAL_EXEC != 0 {
                return Err(invalid());
            }
            if stat.st_mode & 0o111 != 0o111 {
                return Err(invalid());
            }
            validate_elf(file, stat.st_size as u64)?;
        } else if current_seals & F_SEAL_EXEC == 0 {
            return Err(invalid());
        } else if stat.st_mode & 0o111 != 0 {
            return Err(invalid());
        }
        stats.push(stat);
    }
    if stats.iter().any(|stat| same_object(&root_stat, stat)) {
        return Err(invalid());
    }
    for left in 0..stats.len() {
        for right in (left + 1)..stats.len() {
            if same_object(&stats[left], &stats[right]) {
                return Err(invalid());
            }
        }
    }
    Ok(files)
}

fn duplicate_for_child(file: &File) -> io::Result<File> {
    // SAFETY: F_DUPFD_CLOEXEC creates a new descriptor owned by this process.
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, ROLE_FIRST_TEMP) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: fcntl returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn reserved_roles_occupied() -> Result<bool, ErrorCode> {
    let mut any = false;
    for role in [
        ROLE_ROOT,
        ROLE_IMAGE,
        ROLE_CONTEXT,
        ROLE_MANIFEST,
        ROLE_DESCRIPTOR,
    ] {
        // SAFETY: F_GETFD only reads descriptor state.
        let result = unsafe { libc::fcntl(role, libc::F_GETFD) };
        if result >= 0 {
            any = true;
        } else if io::Error::last_os_error().raw_os_error() != Some(libc::EBADF) {
            return Err(io_error(io::Error::last_os_error()));
        }
    }
    Ok(any)
}

fn verify_owned_reserved_roles(roles: &[File; 5]) -> Result<(), ErrorCode> {
    let checked = checked_role_files(std::process::id())?;
    if !proc_image_matches(std::process::id(), &checked[1])? {
        return Err(invalid());
    }
    // Existing role slots belong to the current verified process.  They are
    // deliberately independent from the new child's image/context/descriptor;
    // only the root/manifest trust is checked by the caller's release binding.
    let _ = roles;
    Ok(())
}

/// Arrange role descriptors for a child without overwriting the parent's roles.
pub fn configure_role_inheritance(command: &mut Command, roles: &[File; 5]) -> io::Result<()> {
    let limit = unsafe {
        let mut value = MaybeUninit::<libc::rlimit>::uninit();
        if libc::getrlimit(libc::RLIMIT_NOFILE, value.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        value.assume_init().rlim_cur
    };
    if limit <= ROLE_DESCRIPTOR as libc::rlim_t {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "role fd limit"));
    }
    if reserved_roles_occupied()
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "reserved role fd"))?
    {
        verify_owned_reserved_roles(roles)
            .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "reserved role fd"))?;
    }
    let mut temporary = Vec::with_capacity(5);
    for role in roles {
        temporary.push(duplicate_for_child(role)?);
    }
    // SAFETY: the closure only calls async-signal-safe dup2/fcntl/close operations.
    unsafe {
        command.pre_exec(move || {
            for (temp, target) in temporary.iter().map(AsRawFd::as_raw_fd).zip([
                ROLE_ROOT,
                ROLE_IMAGE,
                ROLE_CONTEXT,
                ROLE_MANIFEST,
                ROLE_DESCRIPTOR,
            ]) {
                if libc::dup2(temp, target) < 0 {
                    return Err(io::Error::last_os_error());
                }
                let flags = libc::fcntl(target, libc::F_GETFD);
                if flags < 0 || libc::fcntl(target, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(())
}

/// Restore close-on-exec on retained role descriptors after child validation.
pub fn mark_roles_cloexec() -> Result<(), ErrorCode> {
    for role in [
        ROLE_ROOT,
        ROLE_IMAGE,
        ROLE_CONTEXT,
        ROLE_MANIFEST,
        ROLE_DESCRIPTOR,
    ] {
        let flags = unsafe { libc::fcntl(role, libc::F_GETFD) };
        if flags < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        if unsafe { libc::fcntl(role, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec03_rejects_non_elf_executable() {
        let mut source = File::open("/dev/null").expect("/dev/null");
        let result = seal_copy(&mut source, 4, *b"01234567890123456789012345678901", true);
        assert!(matches!(result, Err(ErrorCode::InvalidInput)));
    }

    #[test]
    fn sec03_rejects_wrong_architecture_elf() {
        let path =
            std::env::temp_dir().join(format!("hantong-sealed-wrong-arch-{}", std::process::id()));
        let mut bytes = [0u8; 64];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&0x28u16.to_le_bytes());
        std::fs::write(&path, bytes).expect("write fixture");
        let mut source = File::open(&path).expect("fixture");
        let result = seal_copy(&mut source, 64, [0; 32], true);
        let _ = std::fs::remove_file(path);
        assert!(matches!(result, Err(ErrorCode::InvalidInput)));
    }

    #[test]
    fn sec03_rejects_short_source() {
        let path = std::env::temp_dir().join(format!("hantong-sealed-{}", std::process::id()));
        std::fs::write(&path, b"short").expect("write fixture");
        let mut source = File::open(&path).expect("open fixture");
        let result = seal_copy(&mut source, 64, [0; 32], false);
        let _ = std::fs::remove_file(path);
        assert!(matches!(result, Err(ErrorCode::InvalidInput)));
    }

    #[test]
    fn sec03_hashing_does_not_accept_wrong_bytes() {
        let path = std::env::temp_dir().join(format!("hantong-sealed-zero-{}", std::process::id()));
        std::fs::write(&path, [0u8]).expect("write fixture");
        let mut source = File::open(&path).expect("fixture");
        let mut hash = Context::new(&SHA256);
        hash.update(&[0]);
        let mut expected = [0u8; 32];
        expected.copy_from_slice(hash.finish().as_ref());
        let mut image = seal_copy(&mut source, 1, expected, false).expect("sealed metadata");
        let _ = std::fs::remove_file(path);
        assert!(validate_sealed(&mut image, 1, expected, false).is_ok());
        assert!(matches!(
            image.write_all(b"x"),
            Err(error) if matches!(error.raw_os_error(), Some(libc::EPERM | libc::EACCES))
        ));
    }

    fn child_exec_is_denied(execveat: bool) -> libc::c_int {
        // SAFETY: the child performs no shared Rust cleanup and exits through
        // _exit after the isolated filter test.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork");
        if pid == 0 {
            assert!(forbid_helper_exec().is_ok());
            assert!(unsafe { libc::getpid() } > 0);
            let path = std::ffi::CString::new("/usr/bin/true").unwrap();
            let arg0 = path.as_ptr();
            let argv = [arg0, std::ptr::null()];
            let envp = [std::ptr::null()];
            let result = if execveat {
                let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
                if fd < 0 {
                    unsafe { libc::_exit(12) };
                }
                // SAFETY: all pointers refer to NUL-terminated or terminated
                // arrays that remain live through the syscall.
                unsafe {
                    libc::syscall(
                        libc::SYS_execveat,
                        fd,
                        std::ptr::null::<libc::c_char>(),
                        argv.as_ptr(),
                        envp.as_ptr(),
                        libc::AT_EMPTY_PATH,
                    )
                }
            } else {
                // SAFETY: argv/envp are terminated and the child owns the call.
                unsafe { libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr()) as libc::c_long }
            };
            assert_eq!(result, -1);
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
            unsafe { libc::_exit(0) };
        }
        let mut status = 0;
        // SAFETY: pid came from fork and status points to writable storage.
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        status
    }

    #[test]
    fn sec03_helper_filter_denies_execve_and_execveat_but_allows_benign_syscalls() {
        assert!(libc::WIFEXITED(child_exec_is_denied(false)));
        assert_eq!(libc::WEXITSTATUS(child_exec_is_denied(false)), 0);
        assert!(libc::WIFEXITED(child_exec_is_denied(true)));
        assert_eq!(libc::WEXITSTATUS(child_exec_is_denied(true)), 0);
    }

    #[test]
    fn sec03_helper_filter_has_x86_arch_guard_and_x32_rejection() {
        let filter = exec_filter();
        assert_eq!(filter[1].k, AUDIT_ARCH_X86_64);
        assert_eq!(filter[4].k, X32_SYSCALL_BIT);
        assert_eq!(filter[8].k, SECCOMP_RET_ERRNO | libc::EPERM as u32);
    }
}
