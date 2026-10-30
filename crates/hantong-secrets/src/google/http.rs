use super::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Host {
    Token,
    Identity,
    Gmail,
}
impl Host {
    #[cfg(windows)]
    fn name(self) -> &'static str {
        match self {
            Self::Token => "oauth2.googleapis.com",
            Self::Identity => "openidconnect.googleapis.com",
            Self::Gmail => "gmail.googleapis.com",
        }
    }
}
pub(super) struct Request<'a> {
    pub host: Host,
    pub path: &'a str,
    pub post: Option<&'a [u8]>,
    pub bearer: Option<&'a str>,
    pub limit: usize,
    pub deadline: Instant,
    pub cancel: &'a AtomicBool,
}
pub(super) struct Response {
    pub status: u32,
    pub body: SensitiveBuffer,
}
pub(super) trait Transport {
    fn request(&self, request: Request<'_>) -> Result<Response, ErrorCode>;
}
pub(super) fn check(deadline: Instant, cancel: &AtomicBool) -> Result<(), ErrorCode> {
    if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
        Err(ErrorCode::Offline)
    } else {
        Ok(())
    }
}
pub(super) fn success(status: u32) -> Result<(), ErrorCode> {
    match status {
        200 => Ok(()),
        400 | 401 => Err(ErrorCode::AuthRequired),
        403 => Err(ErrorCode::ScopeDenied),
        404 => Err(ErrorCode::NotFound),
        429 => Err(ErrorCode::RateLimited),
        _ => Err(ErrorCode::Offline),
    }
}
fn validate(r: &Request<'_>) -> Result<(), ErrorCode> {
    check(r.deadline, r.cancel)?;
    if r.limit == 0
        || r.limit > 2 * 1024 * 1024
        || r.path.len() > 8192
        || !r.path.is_ascii()
        || r.path.bytes().any(|b| b <= 32 || b == 127)
        || r.path.contains('#')
        || r.bearer.is_some_and(|b| {
            b.is_empty() || b.len() > 16384 || !b.bytes().all(|b| (0x21..=0x7e).contains(&b))
        })
    {
        return Err(ErrorCode::InvalidInput);
    }
    let valid = match r.host {
        Host::Token => {
            r.path == "/token" && r.post.is_some_and(|b| b.len() <= 65536) && r.bearer.is_none()
        }
        Host::Identity => r.path == "/v1/userinfo" && r.post.is_none() && r.bearer.is_some(),
        Host::Gmail => {
            let route = r.path.split('?').next().unwrap_or("");
            let read_path = route == "/gmail/v1/users/me/labels"
                || route == "/gmail/v1/users/me/messages"
                || route
                    .strip_prefix("/gmail/v1/users/me/messages/")
                    .is_some_and(|id| {
                        !id.is_empty()
                            && id.len() <= 256
                            && id
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    });
            read_path && r.post.is_none() && r.bearer.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(ErrorCode::InvalidInput)
    }
}
pub(super) struct NativeTransport;
impl Transport for NativeTransport {
    fn request(&self, r: Request<'_>) -> Result<Response, ErrorCode> {
        validate(&r)?;
        #[cfg(windows)]
        {
            native::request(r)
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use super::*;
    use std::{ffi::c_void, ptr};
    use windows_sys::Win32::Networking::WinHttp::*;
    struct Handle(*mut c_void);
    struct SecretWide(Vec<u16>);
    impl Drop for SecretWide {
        fn drop(&mut self) {
            for item in &mut self.0 {
                unsafe {
                    ptr::write_volatile(item, 0);
                }
            }
            std::sync::atomic::compiler_fence(Ordering::SeqCst);
        }
    }
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
    fn owned(raw: *mut c_void) -> Result<Handle, ErrorCode> {
        if raw.is_null() {
            Err(ErrorCode::Offline)
        } else {
            Ok(Handle(raw))
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn require(v: i32) -> Result<(), ErrorCode> {
        if v == 0 {
            Err(ErrorCode::Offline)
        } else {
            Ok(())
        }
    }
    fn timeouts(h: &Handle, r: &Request<'_>) -> Result<(), ErrorCode> {
        check(r.deadline, r.cancel)?;
        let ms = r
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .saturating_div(4)
            .clamp(1, 2500) as i32;
        unsafe { require(WinHttpSetTimeouts(h.0, ms, ms, ms, ms)) }
    }
    pub(super) fn request(r: Request<'_>) -> Result<Response, ErrorCode> {
        let agent = wide("Hantong/1");
        let host = wide(r.host.name());
        let path = wide(r.path);
        let verb = wide(if r.post.is_some() { "POST" } else { "GET" });
        // No callback handles or unchecked certificate flags. WinHTTP validates
        // the fixed hostname and platform TLS chain; redirects are disabled.
        let session = owned(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        })?;
        timeouts(&session, &r)?;
        let retries = 1u32;
        unsafe {
            require(WinHttpSetOption(
                session.0,
                WINHTTP_OPTION_CONNECT_RETRIES,
                (&retries as *const u32).cast(),
                4,
            ))?;
        }
        let conn = owned(unsafe { WinHttpConnect(session.0, host.as_ptr(), 443, 0) })?;
        let req = owned(unsafe {
            WinHttpOpenRequest(
                conn.0,
                verb.as_ptr(),
                path.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE | WINHTTP_FLAG_REFRESH,
            )
        })?;
        let disable =
            WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
        unsafe {
            require(WinHttpSetOption(
                req.0,
                WINHTTP_OPTION_DISABLE_FEATURE,
                (&disable as *const u32).cast(),
                4,
            ))?;
        }
        let header_limit = 16384u32;
        unsafe {
            require(WinHttpSetOption(
                req.0,
                WINHTTP_OPTION_MAX_RESPONSE_HEADER_SIZE,
                (&header_limit as *const u32).cast(),
                4,
            ))?;
        }
        let mut header = SensitiveBuffer(Vec::with_capacity(33000));
        if let Some(token) = r.bearer {
            header.0.extend_from_slice(b"Authorization: Bearer ");
            header.0.extend_from_slice(token.as_bytes());
            header.0.extend_from_slice(b"\r\n");
        }
        if r.post.is_some() {
            header
                .0
                .extend_from_slice(b"Content-Type: application/x-www-form-urlencoded\r\n");
        }
        let wide_header = SecretWide(
            header
                .0
                .iter()
                .map(|b| u16::from(*b))
                .chain(Some(0))
                .collect(),
        );
        let body = r.post.unwrap_or(&[]);
        timeouts(&req, &r)?;
        let sent = unsafe {
            WinHttpSendRequest(
                req.0,
                wide_header.0.as_ptr(),
                (wide_header.0.len() - 1) as u32,
                if body.is_empty() {
                    ptr::null()
                } else {
                    body.as_ptr().cast()
                },
                body.len() as u32,
                body.len() as u32,
                0,
            )
        };
        // Headers contain bearer material and are wiped before any error return.
        drop(wide_header);
        require(sent)?;
        timeouts(&req, &r)?;
        unsafe {
            require(WinHttpReceiveResponse(req.0, ptr::null_mut()))?;
        }
        let (mut status, mut length) = (0u32, 4u32);
        unsafe {
            require(WinHttpQueryHeaders(
                req.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut length,
                ptr::null_mut(),
            ))?;
        }
        let mut body = SensitiveBuffer(vec![0; r.limit + 1]);
        let mut used = 0usize;
        loop {
            timeouts(&req, &r)?;
            let mut read = 0u32;
            unsafe {
                require(WinHttpReadData(
                    req.0,
                    body.0[used..].as_mut_ptr().cast(),
                    ((r.limit + 1 - used).min(8192)) as u32,
                    &mut read,
                ))?;
            }
            if read == 0 {
                break;
            }
            used += read as usize;
            if used > r.limit {
                return Err(ErrorCode::QuotaExceeded);
            }
        }
        body.0.truncate(used);
        Ok(Response { status, body })
    }
}
