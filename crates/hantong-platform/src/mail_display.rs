//! Bounded presentation of untrusted mail headers. Never an identity proof.
use serde_json::Value;

const MAX_HEADER: usize = 8192;

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
fn base64(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let digit = |b| match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    for (index, c) in bytes.chunks_exact(4).enumerate() {
        let a = digit(c[0])?;
        let b = digit(c[1])?;
        out.push((a << 2) | (b >> 4));
        if c[2] == b'=' {
            if c[3] != b'=' || index + 1 != bytes.len() / 4 || b & 15 != 0 {
                return None;
            }
        } else {
            let d = digit(c[2])?;
            out.push((b << 4) | (d >> 2));
            if c[3] == b'=' {
                if index + 1 != bytes.len() / 4 || d & 3 != 0 {
                    return None;
                }
            } else {
                out.push((d << 6) | digit(c[3])?);
            }
        }
    }
    Some(out)
}
fn quoted(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'_' => out.push(b' '),
            b'=' => {
                out.push((hex(*bytes.get(i + 1)?)? << 4) | hex(*bytes.get(i + 2)?)?);
                i += 2;
            }
            b if b.is_ascii() && !b.is_ascii_control() => out.push(b),
            _ => return None,
        }
        i += 1;
    }
    Some(out)
}
fn word(bytes: &[u8]) -> Option<(String, usize)> {
    if !bytes.starts_with(b"=?") {
        return None;
    }
    let charset_end = bytes[2..].iter().position(|b| *b == b'?')? + 2;
    if !(3..=42).contains(&charset_end) {
        return None;
    }
    let charset = std::str::from_utf8(&bytes[2..charset_end]).ok()?;
    let encoding = *bytes.get(charset_end + 1)?;
    if bytes.get(charset_end + 2) != Some(&b'?') {
        return None;
    }
    let start = charset_end + 3;
    let end = bytes.get(start..)?.windows(2).position(|v| v == b"?=")? + start;
    let decoded = match encoding {
        b'B' | b'b' => base64(&bytes[start..end])?,
        b'Q' | b'q' => quoted(&bytes[start..end])?,
        _ => return None,
    };
    Some((
        crate::mail_charset::decode_mail_charset(&decoded, charset)?,
        end + 2,
    ))
}
pub fn decode_header(bytes: &[u8]) -> String {
    if bytes.len() > MAX_HEADER {
        return String::new();
    }
    let mut out = String::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let Some((decoded, len)) = word(&bytes[cursor..]) {
            out.push_str(&decoded);
            cursor += len;
            let mut next = cursor;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if word(&bytes[next..]).is_some() {
                cursor = next;
            }
        } else {
            let next = bytes[cursor + 1..]
                .windows(2)
                .position(|v| v == b"=?")
                .map_or(bytes.len(), |n| cursor + 1 + n);
            out.push_str(&String::from_utf8_lossy(&bytes[cursor..next]));
            cursor = next;
        }
    }
    single_line(&out)
}
pub fn single_line(text: &str) -> String {
    text.chars().map(|ch| match ch {
        '\r'|'\n'|'\t'|'\u{2028}'|'\u{2029}'=>' ',
        ch if ch.is_control() || matches!(ch as u32,0x061c|0x200b..=0x200f|0x202a..=0x202e|0x2060..=0x2069|0xfeff)=>' ',
        ch=>ch,
    }).collect()
}
pub fn decode_hex_header(value: Option<&Value>) -> String {
    let Some(s) = value.and_then(Value::as_str) else {
        return String::new();
    };
    if s.len() > MAX_HEADER * 2 || !s.len().is_multiple_of(2) {
        return String::new();
    }
    let bytes: Option<Vec<u8>> = s
        .as_bytes()
        .chunks_exact(2)
        .map(|v| Some(hex(v[0])? * 16 + hex(v[1])?))
        .collect();
    bytes.map_or_else(String::new, |v| decode_header(&v))
}
fn addresses(value: &Value) -> String {
    let Some(rows) = value.as_array() else {
        return String::new();
    };
    rows.iter()
        .take(128)
        .map(|address| {
            let name = decode_hex_header(address.get("name_hex"));
            let mailbox = decode_hex_header(address.get("mailbox_hex"));
            let host = decode_hex_header(address.get("host_hex"));
            let email = if host.is_empty() {
                mailbox
            } else {
                format!("{mailbox}@{host}")
            };
            if name.is_empty() {
                email
            } else if email.is_empty() {
                name
            } else {
                format!("{name} <{email}>")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
pub fn reading_text(response: &Value, fallback: &Value, ko: bool) -> Option<String> {
    let body = response
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| response.pointer("/body/text").and_then(Value::as_str))?;
    let headers = response
        .get("headers")
        .or_else(|| response.pointer("/body/headers"))
        .filter(|v| v.is_object())
        .unwrap_or(fallback);
    let resource = if ko {
        include_str!("../resources/mail-display-ko.txt")
    } else {
        include_str!("../resources/mail-display-en.txt")
    };
    let label = |key: &str| {
        resource
            .lines()
            .find_map(|line| {
                line.split_once('=')
                    .filter(|(k, _)| *k == key)
                    .map(|(_, v)| v)
            })
            .expect("mail display resource")
    };
    let missing = label("missing");
    let mut out = String::new();
    for key in ["subject", "from", "to", "cc", "date"] {
        let value = headers.get(key).or_else(|| fallback.get(key));
        let mut display = match value {
            Some(Value::String(s)) => decode_header(s.as_bytes()),
            Some(Value::Array(_)) => addresses(value.unwrap()),
            _ => String::new(),
        };
        if display.is_empty() && key == "subject" {
            display = decode_hex_header(fallback.get("subject_hex"));
        }
        if display.is_empty() && key == "from" {
            display = fallback
                .get("sender")
                .and_then(Value::as_str)
                .map_or_else(String::new, |s| decode_header(s.as_bytes()));
        }
        if display.is_empty() {
            display = missing.into();
        }
        out.push_str(&format!("{}: {}\n", label(key), display));
    }
    out.push('\n');
    if body.is_empty() {
        out.push_str(label("empty"));
    } else {
        out.push_str(
            &body
                .split('\n')
                .map(single_line)
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    Some(out)
}

/// Relabel only the five generated envelope rows; mail body is never translated.
pub fn relanguage_reading_text(text: &str, ko: bool) -> String {
    let en = include_str!("../resources/mail-display-en.txt");
    let kr = include_str!("../resources/mail-display-ko.txt");
    let mut lines = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
    for (index, key) in ["subject", "from", "to", "cc", "date"].iter().enumerate() {
        let lookup = |source: &'static str, wanted: &str| {
            source
                .lines()
                .find_map(|line| {
                    line.split_once('=')
                        .filter(|(k, _)| k == &wanted)
                        .map(|(_, v)| v)
                })
                .expect("mail display resource")
        };
        let Some(line) = lines.get_mut(index) else {
            break;
        };
        let Some((label, value)) = line.split_once(": ") else {
            continue;
        };
        if label != lookup(en, key) && label != lookup(kr, key) {
            continue;
        }
        let source = if ko { kr } else { en };
        let value = if value == lookup(en, "missing") || value == lookup(kr, "missing") {
            lookup(source, "missing")
        } else {
            value
        };
        *line = format!("{}: {}", lookup(source, key), value);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli16_mime_words_decode_folded_utf8_and_q_without_terminal_controls() {
        assert_eq!(decode_header(b"=?UTF-8?B?7ZWc6riA?="), "한글");
        assert_eq!(
            decode_header(b"=?UTF-8?B?7ZWc?=\r\n =?utf-8?B?6riA?="),
            "한글"
        );
        assert_eq!(
            decode_header(b"Re: =?UTF-8?Q?hello_world?="),
            "Re: hello world"
        );
        assert_eq!(decode_header(b"=?UTF-8?Q?=1B=5B31m?="), " [31m");
        assert_eq!(decode_header(b"=?unknown?B?YWJj?="), "=?unknown?B?YWJj?=");
        assert_eq!(decode_header(b"=?UTF-8?B?Y===?="), "=?UTF-8?B?Y===?=");
    }
    #[test]
    fn cli16_reading_screen_has_envelope_and_explicit_empty_or_missing_fields() {
        let value = serde_json::json!({"text":"Body","headers":{"subject":"Hello","from":"A <a@example.invalid>","to":"b@example.invalid","cc":"c@example.invalid","date":"Tue, 3 Nov 2026"}});
        let screen = reading_text(&value, &Value::Null, false).unwrap();
        for text in [
            "Subject: Hello",
            "From: A <a@example.invalid>",
            "To: b@example.invalid",
            "Cc: c@example.invalid",
            "Date: Tue, 3 Nov 2026",
            "\n\nBody",
        ] {
            assert!(screen.contains(text));
        }
        assert!(
            reading_text(&serde_json::json!({"text":""}), &Value::Null, false)
                .unwrap()
                .contains("(Empty body)")
        );
        assert!(
            reading_text(&serde_json::json!({"state":"pending"}), &Value::Null, false).is_none()
        );
    }

    #[test]
    fn cli16_body_line_endings_and_tabs_do_not_emit_control_glyphs() {
        let rendered = reading_text(
            &serde_json::json!({"text":"one\r\ntwo\tthree\u{1b}"}),
            &Value::Null,
            false,
        )
        .unwrap();
        assert!(!rendered.contains('\r'));
        assert!(!rendered.contains('\t'));
        assert!(!rendered.contains('\u{1b}'));
        assert!(rendered.contains("one \ntwo three "));
    }
    #[cfg(windows)]
    #[test]
    fn cli16_legacy_korean_header_is_readable() {
        assert_eq!(decode_header(b"=?ks_c_5601-1987?Q?=C7=D1=B1=DB?="), "한글");
    }
}
