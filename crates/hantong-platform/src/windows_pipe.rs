#![allow(unsafe_code)]

use hantong_contracts::ErrorCode;
use hantong_contracts::auth::PeerContext;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::ptr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, RevertToSelf, TOKEN_GROUPS, TOKEN_QUERY, TOKEN_USER, TokenGroups,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows_sys::Win32::System::SystemServices::SE_GROUP_LOGON_ID;
use windows_sys::Win32::System::Threading::{
    CreateMutexExW, GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

const PIPE_PREFIX: &str = r"\\.\pipe\mail-hantong-v1-";
const PREFACE: u8 = 0x01;
// CreateFile requires READ_ATTRIBUTES for a named-pipe client too. No append /
// FILE_CREATE_PIPE_INSTANCE bit is granted or requested.
const CLIENT_RIGHTS: u32 = 0x0010_0083;

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

// The guard is confined to synchronous code. Unwinding also reverts the token.
struct Impersonation;
impl Drop for Impersonation {
    fn drop(&mut self) {
        if unsafe { RevertToSelf() } == 0 {
            std::process::abort();
        }
    }
}

struct Identity {
    user: String,
    logon: String,
}
impl Identity {
    fn current() -> Result<Self, ErrorCode> {
        Ok(Self {
            user: current_user_sid().ok_or(ErrorCode::CapabilityUnsupported)?,
            logon: current_logon_sid().ok_or(ErrorCode::CapabilityUnsupported)?,
        })
    }

    fn context(&self) -> Result<PeerContext, ErrorCode> {
        PeerContext::new(&self.user, &self.logon).map_err(|_| ErrorCode::CapabilityUnsupported)
    }
}

/// Context read from this process's Windows token, never from request fields.
pub fn current_peer_context() -> Result<PeerContext, ErrorCode> {
    Identity::current()?.context()
}

fn endpoint_name() -> Result<String, ErrorCode> {
    let sid = current_user_sid().ok_or(ErrorCode::CapabilityUnsupported)?;
    if sid.is_empty() || sid.len() > 184 || !sid.starts_with("S-") {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    Ok(format!("{PIPE_PREFIX}{sid}"))
}

unsafe fn token_sid(
    token: HANDLE,
    class: windows_sys::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Option<String> {
    unsafe {
        let mut length = 0u32;
        let _ = GetTokenInformation(token, class, ptr::null_mut(), 0, &mut length);
        if length == 0 || length > 64 * 1024 {
            return None;
        }
        let words = (length as usize).div_ceil(std::mem::size_of::<usize>());
        let mut buffer = vec![0usize; words];
        if GetTokenInformation(
            token,
            class,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        ) == 0
        {
            return None;
        }
        let sid = if class == TokenUser {
            if (length as usize) < std::mem::size_of::<TOKEN_USER>() {
                return None;
            }
            (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid
        } else {
            let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
            if (length as usize) < std::mem::size_of::<TOKEN_GROUPS>() {
                return None;
            }
            let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
            let count = groups.GroupCount as usize;
            if count
                > (length as usize - offset)
                    / std::mem::size_of::<windows_sys::Win32::Security::SID_AND_ATTRIBUTES>()
            {
                return None;
            }
            (0..groups.GroupCount as usize).find_map(|i| {
                let group = &*groups.Groups.as_ptr().add(i);
                (group.Attributes & SE_GROUP_LOGON_ID as u32 == SE_GROUP_LOGON_ID as u32)
                    .then_some(group.Sid)
            })?
        };
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 || text.is_null() {
            return None;
        }
        let mut size = 0usize;
        while size < 256 && *text.add(size) != 0 {
            size += 1;
        }
        let sid = if size < 256 {
            Some(
                OsString::from_wide(std::slice::from_raw_parts(text, size))
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        };
        let _ = LocalFree(text.cast());
        sid
    }
}

fn current_user_sid() -> Option<String> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let token = Token(token);
        token_sid(token.0, TokenUser)
    }
}

fn current_logon_sid() -> Option<String> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let token = Token(token);
        token_sid(token.0, TokenGroups)
    }
}

fn verify_peer(handle: HANDLE, expected: &Identity) -> Result<PeerContext, ErrorCode> {
    unsafe {
        if ImpersonateNamedPipeClient(handle) == 0 {
            return Err(ErrorCode::ScopeDenied);
        }
        let revert = Impersonation;
        let mut token: HANDLE = std::ptr::null_mut();
        let observed = if OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) != 0 {
            let token = Token(token);
            let user = token_sid(token.0, TokenUser);
            let logon = token_sid(token.0, TokenGroups);
            let user_ok = user.as_deref() == Some(expected.user.as_str());
            let logon_ok = logon.as_deref() == Some(expected.logon.as_str());
            if user_ok && logon_ok {
                user.zip(logon)
                    .and_then(|(user, logon)| PeerContext::new(&user, &logon).ok())
            } else {
                None
            }
        } else {
            None
        };
        drop(revert);
        observed.ok_or(ErrorCode::ScopeDenied)
    }
}

struct SecurityDescriptor(*mut core::ffi::c_void);
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(self.0);
        }
    }
}

fn security_attributes() -> Result<
    (
        SecurityDescriptor,
        windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    ),
    ErrorCode,
> {
    attributes_for(CLIENT_RIGHTS)
}

fn attributes_for(
    rights: u32,
) -> Result<
    (
        SecurityDescriptor,
        windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    ),
    ErrorCode,
> {
    let sid = current_logon_sid().ok_or(ErrorCode::CapabilityUnsupported)?;
    let sddl = format!("D:P(A;;0x{rights:x};;;{sid})");
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    let mut size = 0u32;
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            &mut size,
        ) == 0
            || descriptor.is_null()
        {
            return Err(ErrorCode::CapabilityUnsupported);
        }
    }
    let holder = SecurityDescriptor(descriptor);
    let attrs = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    Ok((holder, attrs))
}

pub struct LocalPipeServer {
    inner: Option<NamedPipeServer>,
    endpoint: String,
    expected: Identity,
    // Existence, not thread ownership, reserves one process across pipe renewal.
    _instance: Token,
}

pub async fn connect() -> Result<NamedPipeClient, ErrorCode> {
    let endpoint = endpoint_name()?;
    connect_endpoint(&endpoint).await
}

async fn connect_endpoint(endpoint: &str) -> Result<NamedPipeClient, ErrorCode> {
    let wide: Vec<u16> = endpoint.encode_utf16().chain(std::iter::once(0)).collect();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let raw = loop {
            let raw = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    CLIENT_RIGHTS,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT,
                    ptr::null_mut(),
                )
            };
            if raw != INVALID_HANDLE_VALUE && !raw.is_null() {
                break raw;
            }
            match unsafe { GetLastError() } {
                2 | 231 => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
                _ => return Err(ErrorCode::CoreUnavailable),
            }
        };
        // Tokio assumes ownership immediately, including failure to register IO.
        let mut client = unsafe { NamedPipeClient::from_raw_handle(raw as _) }
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        client
            .write_all(&[PREFACE])
            .await
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        Ok(client)
    })
    .await
    .map_err(|_| ErrorCode::CoreUnavailable)?
}

fn reserve_instance(endpoint: &str) -> Result<Token, ErrorCode> {
    let local = endpoint
        .strip_prefix(r"\\.\pipe\")
        .ok_or(ErrorCode::InvalidInput)?;
    let name: Vec<u16> = format!(r"Global\{local}-instance")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let (_descriptor, attrs) = attributes_for(0x0010_0001)?;
    // No initial owner and no waits: only creator status and object lifetime
    // are used, so async thread migration cannot abandon mutex ownership.
    let handle = unsafe { CreateMutexExW(&attrs, name.as_ptr(), 0, 0x0010_0001) };
    let status = unsafe { GetLastError() };
    if handle.is_null() {
        return Err(if status == 5 {
            ErrorCode::Conflict
        } else {
            ErrorCode::CoreUnavailable
        });
    }
    let owned = Token(handle);
    if status == ERROR_ALREADY_EXISTS {
        return Err(ErrorCode::Conflict);
    }
    Ok(owned)
}

fn create_pipe(endpoint: &str) -> Result<NamedPipeServer, ErrorCode> {
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(true)
        .max_instances(1)
        .reject_remote_clients(true);
    let (_descriptor, mut attrs) = security_attributes()?;
    // Descriptor and attributes outlive this synchronous CreateNamedPipe call.
    unsafe { options.create_with_security_attributes_raw(endpoint, &mut attrs as *mut _ as *mut _) }
        .map_err(|error| match error.raw_os_error() {
            Some(5 | 231) => ErrorCode::Conflict,
            _ => ErrorCode::CoreUnavailable,
        })
}

impl LocalPipeServer {
    pub fn bind() -> Result<Self, ErrorCode> {
        let endpoint = endpoint_name()?;
        Self::bind_endpoint(&endpoint)
    }

    fn bind_endpoint(endpoint: &str) -> Result<Self, ErrorCode> {
        let expected = Identity::current()?;
        let instance = reserve_instance(endpoint)?;
        let inner = Some(create_pipe(endpoint)?);
        Ok(Self {
            inner,
            expected,
            endpoint: endpoint.to_owned(),
            _instance: instance,
        })
    }

    pub async fn accept(&mut self) -> Result<LocalPipeConnection<'_>, ErrorCode> {
        if self.inner.is_none() {
            let renewed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    match create_pipe(&self.endpoint) {
                        Ok(pipe) => return Ok(pipe),
                        // IOCP cancellation can retain the previous handle briefly.
                        // FIRST_PIPE_INSTANCE stays set, including every retry.
                        Err(ErrorCode::Conflict) => {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await
                        }
                        Err(error) => return Err(error),
                    }
                }
            })
            .await
            .map_err(|_| ErrorCode::Conflict)??;
            self.inner = Some(renewed);
        }
        self.inner
            .as_ref()
            .expect("pipe renewed")
            .connect()
            .await
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let mut connection = LocalPipeConnection {
            server: self,
            observed: None,
        };
        let mut preface = [0u8; 1];
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            connection.read_exact(&mut preface),
        )
        .await;
        if !matches!(read, Ok(Ok(1))) || preface[0] != PREFACE {
            return Err(ErrorCode::ScopeDenied);
        }
        connection.observed = Some(verify_peer(
            connection
                .server
                .inner
                .as_ref()
                .expect("connected pipe")
                .as_raw_handle() as HANDLE,
            &connection.server.expected,
        )?);
        Ok(connection)
    }
}

pub struct LocalPipeConnection<'a> {
    server: &'a mut LocalPipeServer,
    observed: Option<PeerContext>,
}

impl LocalPipeConnection<'_> {
    /// Available only after accept verified the pipe impersonation token. Both
    /// user and logon SID must match the listening process's OS identity.
    pub fn peer_context(&self) -> Result<PeerContext, ErrorCode> {
        self.observed.clone().ok_or(ErrorCode::ScopeDenied)
    }
}

impl<'a> tokio::io::AsyncRead for LocalPipeConnection<'a> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(
            self.get_mut()
                .server
                .inner
                .as_mut()
                .expect("connected pipe"),
        )
        .poll_read(cx, buf)
    }
}

impl<'a> tokio::io::AsyncWrite for LocalPipeConnection<'a> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(
            self.get_mut()
                .server
                .inner
                .as_mut()
                .expect("connected pipe"),
        )
        .poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(
            self.get_mut()
                .server
                .inner
                .as_mut()
                .expect("connected pipe"),
        )
        .poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(
            self.get_mut()
                .server
                .inner
                .as_mut()
                .expect("connected pipe"),
        )
        .poll_shutdown(cx)
    }
}

impl Drop for LocalPipeConnection<'_> {
    fn drop(&mut self) {
        if let Some(pipe) = self.server.inner.take() {
            let _ = pipe.disconnect();
        }
    }
}

#[cfg(test)]
#[path = "pipe_tests.rs"]
mod tests;
