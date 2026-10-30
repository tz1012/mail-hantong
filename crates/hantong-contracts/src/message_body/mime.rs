//! Bounded text MIME selection. No markup, URL or attachment is executed.
use super::MAX_BODY_BYTES;
use crate::ErrorCode;
struct Text {
    bytes: Vec<u8>,
    html: bool,
}
fn parameter(value: &str, name: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|item| {
        let (k, v) = item.trim().split_once('=')?;
        (k.trim().eq_ignore_ascii_case(name)).then(|| v.trim().trim_matches('"').to_owned())
    })
}
type Headers<'a> = (std::collections::BTreeMap<String, String>, &'a [u8]);
fn headers(bytes: &[u8]) -> Result<Option<Headers<'_>>, ErrorCode> {
    let Some((split, width)) = bytes
        .windows(4)
        .position(|v| v == b"\r\n\r\n")
        .map(|n| (n, 4))
        .or_else(|| bytes.windows(2).position(|v| v == b"\n\n").map(|n| (n, 2)))
    else {
        return Ok(None);
    };
    if split > 65536 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let Ok(head) = std::str::from_utf8(&bytes[..split]) else {
        return Err(ErrorCode::CapabilityUnsupported);
    };
    let mut map = std::collections::BTreeMap::<String, String>::new();
    let mut key = String::new();
    for line in head.lines() {
        if line.len() > 8192 {
            return Err(ErrorCode::QuotaExceeded);
        }
        if line.starts_with([' ', '\t']) {
            let Some(previous) = map.get_mut(&key) else {
                return Ok(None);
            };
            if previous.len() + line.len() > 8192 {
                return Err(ErrorCode::QuotaExceeded);
            }
            previous.push(' ');
            previous.push_str(line.trim());
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Ok(None);
        };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Ok(None);
        }
        key = name.to_ascii_lowercase();
        if map.contains_key(&key)
            && [
                "content-type",
                "content-transfer-encoding",
                "mime-version",
                "content-disposition",
            ]
            .contains(&key.as_str())
        {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        // Repeated routing headers such as Received are normal RFC822 metadata.
        map.insert(key.clone(), value.trim().to_owned());
    }
    if !map.keys().any(|k| {
        [
            "content-type",
            "content-transfer-encoding",
            "mime-version",
            "from",
            "subject",
            "to",
            "date",
        ]
        .contains(&k.as_str())
    }) {
        return Ok(None);
    }
    Ok(Some((map, &bytes[split + width..])))
}
fn base64(bytes: &[u8]) -> Result<Vec<u8>, ErrorCode> {
    let v: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .collect();
    if !v.len().is_multiple_of(4) {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let digit = |b| match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(v.len() / 4 * 3);
    for (i, q) in v.chunks_exact(4).enumerate() {
        let a = digit(q[0]).ok_or(ErrorCode::CapabilityUnsupported)?;
        let b = digit(q[1]).ok_or(ErrorCode::CapabilityUnsupported)?;
        out.push((a << 2) | (b >> 4));
        if q[2] == b'=' {
            if q[3] != b'=' || i + 1 != v.len() / 4 || b & 15 != 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            continue;
        }
        let c = digit(q[2]).ok_or(ErrorCode::CapabilityUnsupported)?;
        out.push((b << 4) | (c >> 2));
        if q[3] == b'=' {
            if i + 1 != v.len() / 4 || c & 3 != 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            continue;
        }
        out.push((c << 6) | digit(q[3]).ok_or(ErrorCode::CapabilityUnsupported)?);
    }
    Ok(out)
}
fn quoted(bytes: &[u8]) -> Result<Vec<u8>, ErrorCode> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'=' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if bytes.get(i + 1..i + 3) == Some(b"\r\n") {
            i += 3;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'\n') {
            i += 2;
            continue;
        }
        let pair = bytes
            .get(i + 1..i + 3)
            .ok_or(ErrorCode::CapabilityUnsupported)?;
        let s = std::str::from_utf8(pair).map_err(|_| ErrorCode::CapabilityUnsupported)?;
        out.push(u8::from_str_radix(s, 16).map_err(|_| ErrorCode::CapabilityUnsupported)?);
        i += 3;
    }
    Ok(out)
}
fn extract(bytes: &[u8], depth: usize, parts: &mut usize) -> Result<Text, ErrorCode> {
    if depth > 8 || bytes.len() > MAX_BODY_BYTES || *parts >= 32 {
        return Err(ErrorCode::QuotaExceeded);
    }
    *parts += 1;
    let Some((head, body)) = headers(bytes)? else {
        return Ok(Text {
            bytes: bytes.to_vec(),
            html: false,
        });
    };
    let content = head
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("text/plain");
    let kind = content
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if head.get("content-disposition").is_some_and(|v| {
        v.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("attachment")
    }) {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    if kind.starts_with("multipart/") {
        let boundary = parameter(content, "boundary").ok_or(ErrorCode::CapabilityUnsupported)?;
        if boundary.is_empty()
            || boundary.len() > 70
            || !boundary.bytes().all(|b| b.is_ascii_graphic() && b != b'"')
        {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        let marker = format!("--{boundary}");
        let close = format!("{marker}--");
        let mut current = None::<Vec<u8>>;
        let mut pieces = Vec::new();
        let mut closed = false;
        for line in body.split_inclusive(|b| *b == b'\n') {
            let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
            let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
            if trimmed == marker.as_bytes() || trimmed == close.as_bytes() {
                if let Some(piece) = current.take() {
                    pieces.push(piece);
                    if pieces.len() > 32 {
                        return Err(ErrorCode::QuotaExceeded);
                    }
                }
                if trimmed == close.as_bytes() {
                    closed = true;
                    break;
                }
                current = Some(Vec::new());
            } else if let Some(piece) = current.as_mut() {
                piece.extend_from_slice(line);
            }
        }
        if !closed {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        let mut html = None;
        for piece in pieces {
            match extract(&piece, depth + 1, parts) {
                Ok(text) if !text.html => return Ok(text),
                Ok(text) => {
                    if html.is_none() {
                        html = Some(text)
                    }
                }
                Err(ErrorCode::CapabilityUnsupported) => {}
                Err(e) => return Err(e),
            }
        }
        return html.ok_or(ErrorCode::CapabilityUnsupported);
    }
    if kind != "text/plain" && kind != "text/html" {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let charset = parameter(content, "charset").unwrap_or_else(|| "us-ascii".into());
    if ![
        "utf-8",
        "utf8",
        "us-ascii",
        "ascii",
        "iso-8859-1",
        "windows-1252",
    ]
    .iter()
    .any(|v| charset.eq_ignore_ascii_case(v))
    {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let cte = head
        .get("content-transfer-encoding")
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "7bit".into());
    let bytes = match cte.as_str() {
        "7bit" | "8bit" | "binary" => body.to_vec(),
        "base64" => base64(body)?,
        "quoted-printable" => quoted(body)?,
        _ => return Err(ErrorCode::CapabilityUnsupported),
    };
    let bytes = if charset.eq_ignore_ascii_case("iso-8859-1") {
        bytes
            .into_iter()
            .map(char::from)
            .collect::<String>()
            .into_bytes()
    } else if charset.eq_ignore_ascii_case("windows-1252") {
        return Err(ErrorCode::CapabilityUnsupported);
    } else {
        bytes
    };
    Ok(Text {
        bytes,
        html: kind == "text/html",
    })
}
fn entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let candidate = &rest[start + 1..];
        let Some(end) = candidate.find(';').filter(|n| *n <= 12) else {
            out.push('&');
            rest = candidate;
            continue;
        };
        let entity = &candidate[..end];
        let value = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|n| u32::from_str_radix(n, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(ch) = value {
            out.push(ch);
            rest = &candidate[end + 1..]
        } else {
            out.push('&');
            rest = candidate
        }
    }
    out.push_str(rest);
    out
}
fn html_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut tag = None::<String>;
    let mut hidden = None::<String>;
    let mut quoted = None;
    for ch in input.chars() {
        if let Some(raw) = tag.as_mut() {
            if let Some(q) = quoted {
                if ch == q {
                    quoted = None
                }
                if raw.len() < 1024 {
                    raw.push(ch)
                }
                continue;
            }
            if ch == '\'' || ch == '"' {
                quoted = Some(ch)
            }
            if ch == '>' {
                let raw = tag.take().unwrap();
                let raw = raw.trim();
                let closing = raw.starts_with('/');
                let name = raw
                    .trim_start_matches('/')
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if hidden.as_ref().is_some_and(|n| n == &name) && closing {
                    hidden = None
                } else if hidden.is_none()
                    && ["script", "style"].contains(&name.as_str())
                    && !closing
                {
                    hidden = Some(name.clone())
                }
                if hidden.is_none()
                    && ["br", "p", "div", "li", "tr", "h1", "h2", "h3", "blockquote"]
                        .contains(&name.as_str())
                    && !out.ends_with('\n')
                {
                    out.push('\n')
                }
            } else if raw.len() < 1024 {
                raw.push(ch)
            }
        } else if ch == '<' {
            tag = Some(String::new())
        } else if hidden.is_none() {
            out.push(ch)
        }
    }
    entities(&out)
}
pub(super) fn render(bytes: &[u8]) -> Result<String, ErrorCode> {
    let text = extract(bytes, 0, &mut 0)?;
    let decoded = String::from_utf8(text.bytes).map_err(|_| ErrorCode::CapabilityUnsupported)?;
    let out = if text.html {
        html_text(&decoded)
    } else {
        decoded
    };
    Ok(super::sanitize_terminal_text(&out))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mail03_mime_plain_and_encoded_korean() {
        assert_eq!(render(b"Subject: Example\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8=").unwrap(),"Hello");
        assert_eq!(render(b"Content-Type: text/plain\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n1 < 2=0Aok").unwrap(),"1 < 2\nok");
    }
    #[test]
    fn mail03_mime_prefers_plain_and_ignores_attachments() {
        let m=b"Content-Type: multipart/alternative; boundary=x\r\n\r\n--x\r\nContent-Type: text/html\r\n\r\n<b>HTML</b>\r\n--x\r\nContent-Type: text/plain\r\n\r\nPlain\r\n--x--\r\n";
        assert_eq!(render(m).unwrap(), "Plain\n");
    }
    #[test]
    fn sec08_mime_never_executes_control_or_binary_and_rejects_bad_encoding() {
        assert_eq!(
            render(b"Content-Type: text/html\r\n\r\n<b>Hello</b>\x1b[2J").unwrap(),
            "Hello"
        );
        assert_eq!(
            render(b"Content-Type: application/octet-stream\r\n\r\nsecret"),
            Err(ErrorCode::CapabilityUnsupported)
        );
        assert_eq!(
            render(b"Content-Type: text/plain\r\nContent-Transfer-Encoding: base64\r\n\r\n!!!!"),
            Err(ErrorCode::CapabilityUnsupported)
        );
    }
}

#[cfg(test)]
mod realistic_tests {
    use super::*;
    #[test]
    fn mail03_korean_multipart_and_html_entities_remain_inert() {
        let raw=b"Received: route-one\r\nReceived: route-two\r\nFrom: test@example.test\r\nSubject: Example\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=bound\r\n\r\n--bound\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<b>HTML</b>\r\n--bound\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n7JWI64WV7ZWY7IS47JqU\r\n--bound--\r\n";
        assert_eq!(render(raw).unwrap(), "안녕하세요");
        assert_eq!(render(b"Content-Type: text/html; charset=utf-8\r\n\r\n<style>secret css</style><b>A &amp; B</b><br>&lt;user@example.test&gt; &#x1b;[2J").unwrap(),"A & B\n<user@example.test> ");
        assert_eq!(
            render(b"Content-Type: text/plain; charset=euc-kr\r\n\r\nlegacy"),
            Err(ErrorCode::CapabilityUnsupported)
        );
    }
}
