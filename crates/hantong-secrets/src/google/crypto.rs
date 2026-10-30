//! Windows CNG SHA-256 and bounded URL-safe base64 helpers.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(super) fn sha256(input: &[u8]) -> Result<[u8; 32], super::ErrorCode> {
    use windows_sys::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
    let length = u32::try_from(input.len()).map_err(|_| super::ErrorCode::InvalidInput)?;
    let mut digest = [0; 32];
    // Windows 11 supports the CNG SHA-256 pseudo-handle. All buffers remain
    // borrowed for this synchronous call; no secret key is supplied.
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
#[cfg(all(test, not(windows)))]
pub(super) fn sha256(input: &[u8]) -> Result<[u8; 32], super::ErrorCode> {
    Ok(portable_test_sha256(input))
}
#[cfg(all(test, not(windows)))]
fn portable_test_sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut padded = Vec::with_capacity(input.len() + 72);
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((input.len() as u64) * 8).to_be_bytes());
    for block in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(chunk.try_into().unwrap());
        }
        for i in 16..64 {
            let a = w[i - 15];
            let b = w[i - 2];
            w[i] = w[i - 16]
                .wrapping_add(a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3))
                .wrapping_add(w[i - 7])
                .wrapping_add(b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10));
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let t1 = h
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22))
                .wrapping_add((a & b) ^ (a & c) ^ (b & c));
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
        // The schedule can contain PKCE verifier material.
        wipe_words(&mut w);
    }
    super::wipe(&mut padded);
    let mut out = [0u8; 32];
    for (chunk, v) in out.chunks_exact_mut(4).zip(state) {
        chunk.copy_from_slice(&v.to_be_bytes());
    }
    out
}
#[cfg(all(test, not(windows)))]
#[allow(unsafe_code)]
fn wipe_words(words: &mut [u32]) {
    for word in words {
        // SAFETY: each word is a uniquely borrowed initialized u32.
        unsafe {
            std::ptr::write_volatile(word, 0);
        }
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
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
