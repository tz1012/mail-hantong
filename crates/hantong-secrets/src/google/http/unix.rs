//! Strict bounded HTTP/1 response parsing for the Unix Google transport.

use super::super::*;
use crate::SensitiveBuffer;

const MAX_HEADER_BYTES: usize = 16 * 1024;

pub(super) fn parse_response(
    bytes: &[u8],
    limit: usize,
) -> Result<(u32, SensitiveBuffer), ErrorCode> {
    if limit == 0 || limit > 2 * 1024 * 1024 {
        return Err(ErrorCode::InvalidInput);
    }
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .ok_or(ErrorCode::Offline)?;
    if end > MAX_HEADER_BYTES {
        return Err(ErrorCode::QuotaExceeded);
    }
    let mut lines = bytes[..end - 2].split_inclusive(|b| *b == b'\n');
    let first = lines
        .next()
        .and_then(|line| line.strip_suffix(b"\n"))
        .ok_or(ErrorCode::Offline)?;
    let status = parse_status(first)?;
    let mut length = None;
    let mut chunked = false;
    for raw in lines {
        let raw = raw.strip_suffix(b"\n").ok_or(ErrorCode::Offline)?;
        let line = raw.strip_suffix(b"\r").ok_or(ErrorCode::Offline)?;
        if line.is_empty() || line[0] == b' ' || line[0] == b'\t' {
            return Err(ErrorCode::Offline);
        }
        let colon = line
            .iter()
            .position(|b| *b == b':')
            .ok_or(ErrorCode::Offline)?;
        let (name, value) = line.split_at(colon);
        let value = &value[1..];
        if name.is_empty() || !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-') {
            return Err(ErrorCode::Offline);
        }
        let value = trim_ows(value);
        if value
            .iter()
            .any(|b| *b == 0 || *b == 127 || *b < 0x20 && *b != b'\t')
        {
            return Err(ErrorCode::Offline);
        }
        if name.eq_ignore_ascii_case(b"content-length") {
            if length.is_some() || value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
                return Err(ErrorCode::Offline);
            }
            let n = std::str::from_utf8(value)
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or(ErrorCode::Offline)?;
            if n > limit {
                return Err(ErrorCode::QuotaExceeded);
            }
            length = Some(n);
        } else if name.eq_ignore_ascii_case(b"transfer-encoding") {
            if chunked || !value.eq_ignore_ascii_case(b"chunked") {
                return Err(ErrorCode::Offline);
            }
            chunked = true;
        } else if name.eq_ignore_ascii_case(b"content-encoding") {
            return Err(ErrorCode::Offline);
        }
    }
    if chunked && length.is_some() {
        return Err(ErrorCode::Offline);
    }
    let body = if chunked {
        decode_chunked(&bytes[end..], limit)?
    } else if let Some(n) = length {
        if bytes[end..].len() != n {
            return Err(ErrorCode::Offline);
        }
        let mut out = SensitiveBuffer(Vec::with_capacity(n));
        out.0.extend_from_slice(&bytes[end..]);
        out
    } else if bytes[end..].is_empty() {
        SensitiveBuffer(Vec::new())
    } else {
        return Err(ErrorCode::Offline);
    };
    Ok((status, body))
}

fn parse_status(line: &[u8]) -> Result<u32, ErrorCode> {
    let line = line.strip_suffix(b"\r").ok_or(ErrorCode::Offline)?;
    // `HTTP/1.1 ` is nine bytes, followed by exactly three status digits
    // and a separating space.  Requiring that separator prevents accepting
    // an overlong/non-delimited status token as a valid response.
    if line.len() < 13 || !line.starts_with(b"HTTP/1.1 ") || line[12] != b' ' {
        return Err(ErrorCode::Offline);
    }
    if !line[9..12].iter().all(u8::is_ascii_digit) {
        return Err(ErrorCode::Offline);
    }
    let n = std::str::from_utf8(&line[9..12]).unwrap().parse().unwrap();
    if !(100..=599).contains(&n) {
        return Err(ErrorCode::Offline);
    }
    Ok(n)
}

fn trim_ows(mut v: &[u8]) -> &[u8] {
    while v.first().is_some_and(|b| *b == b' ' || *b == b'\t') {
        v = &v[1..];
    }
    while v.last().is_some_and(|b| *b == b' ' || *b == b'\t') {
        v = &v[..v.len() - 1];
    }
    v
}

fn decode_chunked(mut input: &[u8], limit: usize) -> Result<SensitiveBuffer, ErrorCode> {
    let mut out = SensitiveBuffer(Vec::with_capacity(limit.min(4096)));
    loop {
        let line_end = input
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(ErrorCode::Offline)?;
        let size = &input[..line_end];
        if size.is_empty()
            || size
                .iter()
                .any(|b| *b == b';' || *b == b' ' || !b.is_ascii_hexdigit())
        {
            return Err(ErrorCode::Offline);
        }
        let n = usize::from_str_radix(
            std::str::from_utf8(size).map_err(|_| ErrorCode::Offline)?,
            16,
        )
        .map_err(|_| ErrorCode::Offline)?;
        input = &input[line_end + 2..];
        if n == 0 {
            if input != b"\r\n" {
                return Err(ErrorCode::Offline);
            }
            return Ok(out);
        }
        let needed = n.checked_add(2).ok_or(ErrorCode::QuotaExceeded)?;
        if n > limit.saturating_sub(out.0.len()) || input.len() < needed {
            return Err(ErrorCode::QuotaExceeded);
        }
        if &input[n..needed] != b"\r\n" {
            return Err(ErrorCode::Offline);
        }
        out.0.extend_from_slice(&input[..n]);
        input = &input[needed..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sec03_unix_http_parser_accepts_chunked_body() {
        let response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nmail\r\n0\r\n\r\n";
        let (status, body) = parse_response(response, 64).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body.0, b"mail");
    }
    #[test]
    fn sec03_unix_http_parser_rejects_ambiguous_or_truncated_frames() {
        let duplicate = b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\na";
        assert!(parse_response(duplicate, 64).is_err());
        let both = b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n0\r\n\r\n";
        assert!(parse_response(both, 64).is_err());
        let truncated = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\na";
        assert!(parse_response(truncated, 64).is_err());
    }

    #[test]
    fn sec03_unix_http_parser_rejects_invalid_status_and_header_values() {
        let bad_status = b"HTTP/1.1 20 OK\r\nContent-Length: 0\r\n\r\n";
        assert!(parse_response(bad_status, 64).is_err());
        let plus_length = b"HTTP/1.1 200 OK\r\nContent-Length: +1\r\n\r\na";
        assert!(parse_response(plus_length, 64).is_err());
        let folded = b"HTTP/1.1 200 OK\r\nX-Test: one\r\n two\r\nContent-Length: 0\r\n\r\n";
        assert!(parse_response(folded, 64).is_err());
        let encoded = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n";
        assert!(parse_response(encoded, 64).is_err());
    }

    #[test]
    fn sec03_unix_http_parser_rejects_chunk_extensions_and_bad_crlf() {
        let extension =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;foo\r\na\r\n0\r\n\r\n";
        assert!(parse_response(extension, 64).is_err());
        let bad_crlf = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\n0\r\n\r\n";
        assert!(parse_response(bad_crlf, 64).is_err());
    }
}
