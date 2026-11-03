//! Bounded, display-only decoding for legacy mail header/body charsets.
//!
//! Mail data is untrusted input.  This module never guesses a system ANSI
//! code page and never returns a replacement string for an invalid encoding.

#![allow(unsafe_code)]

const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// Decode a mail field using its declared charset.
///
/// The result is intended for display only.  Unknown charsets, malformed
/// input, and inputs above the bound return `None`.
pub fn decode_mail_charset(bytes: &[u8], charset: &str) -> Option<String> {
    if bytes.len() > MAX_INPUT_BYTES {
        return None;
    }

    match normalize_charset(charset).as_str() {
        "utf-8" | "utf8" => String::from_utf8(bytes.to_vec()).ok(),
        "us-ascii" | "ascii" => bytes
            .iter()
            .all(|byte| *byte < 0x80)
            .then(|| bytes.iter().map(|byte| *byte as char).collect()),
        "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" => {
            Some(bytes.iter().map(|byte| *byte as char).collect())
        }
        "windows-1252" | "cp1252" => {
            if bytes
                .iter()
                .any(|b| matches!(b, 0x81 | 0x8d | 0x8f | 0x90 | 0x9d))
            {
                return None;
            }
            decode_windows(bytes, 1252)
        }
        "euc-kr" | "ks_c_5601-1987" | "ksc5601" | "cp949" | "windows-949" | "ms949" => {
            decode_windows(bytes, 949)
        }
        _ => None,
    }
}

fn normalize_charset(charset: &str) -> String {
    charset
        .trim()
        .trim_matches(['"', '\''])
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn decode_windows(bytes: &[u8], code_page: u32) -> Option<String> {
    use windows_sys::Win32::Globalization::{MB_ERR_INVALID_CHARS, MultiByteToWideChar};

    if bytes.is_empty() {
        return Some(String::new());
    }
    let input_len = i32::try_from(bytes.len()).ok()?;
    let flags = MB_ERR_INVALID_CHARS;

    // SAFETY: `bytes` remains alive and is passed with its exact bounded byte
    // length.  The first call requests the required UTF-16 length.
    let needed = unsafe {
        MultiByteToWideChar(
            code_page,
            flags,
            bytes.as_ptr(),
            input_len,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 {
        return None;
    }
    let mut wide = vec![0u16; usize::try_from(needed).ok()?];

    // SAFETY: `wide` has exactly the capacity reported by the sizing call;
    // pointers and lengths are valid for the duration of this call.
    let written = unsafe {
        MultiByteToWideChar(
            code_page,
            flags,
            bytes.as_ptr(),
            input_len,
            wide.as_mut_ptr(),
            needed,
        )
    };
    if written != needed {
        return None;
    }
    String::from_utf16(&wide).ok()
}

#[cfg(not(windows))]
fn decode_windows(_bytes: &[u8], _code_page: u32) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::decode_mail_charset;

    #[test]
    fn mail01_decodes_utf8_ascii_and_latin1() {
        assert_eq!(
            decode_mail_charset("한글".as_bytes(), "UTF-8"),
            Some("한글".into())
        );
        assert_eq!(
            decode_mail_charset(b"Subject", "us-ascii"),
            Some("Subject".into())
        );
        assert_eq!(decode_mail_charset(&[0xe9], "ISO-8859-1"), Some("é".into()));
        assert_eq!(decode_mail_charset(&[0xff], "us-ascii"), None);
    }

    #[test]
    fn mail01_rejects_unknown_invalid_and_oversized_input() {
        assert_eq!(decode_mail_charset(b"x", "x-unknown"), None);
        assert_eq!(decode_mail_charset(&[0xff], "utf-8"), None);
        assert_eq!(
            decode_mail_charset(&vec![b'x'; 1024 * 1024 + 1], "utf-8"),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn mail01_decodes_cp949_korean_and_cp1252() {
        // CP949 bytes for "한글".
        assert_eq!(
            decode_mail_charset(&[0xc7, 0xd1, 0xb1, 0xdb], "euc-kr"),
            Some("한글".into())
        );
        assert_eq!(
            decode_mail_charset(&[0x80], "windows-1252"),
            Some("€".into())
        );
        assert_eq!(decode_mail_charset(&[0x81], "windows-1252"), None);
    }
}
