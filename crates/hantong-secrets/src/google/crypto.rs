//! Platform SHA-256 and bounded URL-safe base64 helpers.

#[cfg(windows)]
#[allow(unsafe_code)]
pub(super) fn sha256(input: &[u8]) -> Result<[u8; 32], super::ErrorCode> {
    use windows_sys::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
    let length = u32::try_from(input.len()).map_err(|_| super::ErrorCode::InvalidInput)?;
    let mut digest = [0; 32];
    let status = unsafe {
        BCryptHash(
            BCRYPT_SHA256_ALG_HANDLE,
            std::ptr::null(),
            0,
            input.as_ptr(),
            length,
            digest.as_mut_ptr(),
            32,
        )
    };
    if status < 0 {
        return Err(super::ErrorCode::InternalError);
    }
    Ok(digest)
}

#[cfg(unix)]
pub(super) fn sha256(input: &[u8]) -> Result<[u8; 32], super::ErrorCode> {
    use ring::digest::{SHA256, digest};
    digest(&SHA256, input)
        .as_ref()
        .try_into()
        .map_err(|_| super::ErrorCode::InternalError)
}

pub(super) fn base64url(input: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity((input.len() * 4).div_ceil(3));
    for c in input.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..(c.len() + 1) {
            s.push(A[((n >> (18 - i * 6)) & 63) as usize] as char);
        }
    }
    s
}

pub(super) fn decode64(input: &str, max: usize) -> Result<Vec<u8>, super::ErrorCode> {
    if input.len() > max.saturating_mul(4).div_ceil(3) + 4 {
        return Err(super::ErrorCode::QuotaExceeded);
    }
    let input = input.trim_end_matches('=');
    if input.len() % 4 == 1 {
        return Err(super::ErrorCode::InvalidInput);
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u8);
    for b in input.bytes() {
        let n = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(super::ErrorCode::InvalidInput),
        };
        acc = (acc << 6) | u32::from(n);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            if out.len() > max {
                return Err(super::ErrorCode::QuotaExceeded);
            }
        }
    }
    if bits > 0 && acc & ((1 << bits) - 1) != 0 {
        return Err(super::ErrorCode::InvalidInput);
    }
    Ok(out)
}
