//! Bounded, display-only RFC 2047 decoding for an already parsed Subject.

const MAX_INPUT: usize = 16 * 1024;
const MAX_OUTPUT: usize = 32 * 1024;
const MAX_WORDS: usize = 256;
const MAX_WORD: usize = 75;

pub(crate) fn decode_subject(input: &[u8]) -> String {
    let truncated_input = input.len() > MAX_INPUT;
    let input = &input[..input.len().min(MAX_INPUT)];
    let mut output = String::new();
    let mut i = 0;
    let mut words = 0;
    let mut previous_word = false;
    let mut pending_space = &input[..0];
    while i < input.len() {
        if input[i] == b' ' || input[i] == b'\t' {
            let start = i;
            while i < input.len() && (input[i] == b' ' || input[i] == b'\t') {
                i += 1;
            }
            pending_space = &input[start..i];
            continue;
        }
        let start = i;
        while i < input.len() && input[i] != b' ' && input[i] != b'\t' {
            i += 1;
        }
        let decoded = parse_word(&input[start..i]);
        if decoded.is_some() && words == MAX_WORDS {
            append_raw(&mut output, pending_space);
            append_raw(&mut output, &input[start..]);
            append_marker(&mut output);
            return output;
        }
        let separator = if previous_word && decoded.is_some() {
            &input[..0]
        } else {
            pending_space
        };
        if !append_raw(&mut output, separator) {
            return output;
        }
        pending_space = &input[..0];
        previous_word = decoded.is_some();
        if let Some(text) = decoded {
            words += 1;
            if !append_raw(&mut output, text.as_bytes()) {
                return output;
            }
        } else if !append_raw(&mut output, &input[start..i]) {
            return output;
        }
    }
    append_raw(&mut output, pending_space);
    if truncated_input {
        append_marker(&mut output);
    }
    output
}

fn parse_word(input: &[u8]) -> Option<String> {
    if input.len() > MAX_WORD
        || !input.starts_with(b"=?")
        || !input.ends_with(b"?=")
        || input.len() < 9
    {
        return None;
    }
    let word = &input[..input.len() - 2];
    let q1 = (2..word.len()).find(|&n| word[n] == b'?')?;
    let q2 = (q1 + 1..word.len()).find(|&n| word[n] == b'?')?;
    if q1 == 2 || q2 != q1 + 2 || !word[2..q1].iter().all(u8::is_ascii) {
        return None;
    }
    let payload = &word[q2 + 1..];
    if payload.is_empty()
        || payload
            .iter()
            .any(|&b| b == b'?' || b.is_ascii_whitespace())
    {
        return None;
    }
    let charset = String::from_utf8_lossy(&word[2..q1]).to_ascii_lowercase();
    let bytes = match word[q1 + 1] {
        b'b' | b'B' => base64(payload)?,
        b'q' | b'Q' => quoted_printable(payload)?,
        _ => return None,
    };
    let text = match charset.as_str() {
        "utf-8" => String::from_utf8(bytes).ok()?,
        "us-ascii" if bytes.iter().all(|b| b.is_ascii()) => String::from_utf8(bytes).ok()?,
        "iso-8859-1" => bytes.into_iter().map(char::from).collect(),
        _ => return None,
    };
    Some(text)
}

fn base64(value: &[u8]) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 4 * 3);
    for (index, chunk) in value.chunks_exact(4).enumerate() {
        let last = index + 1 == value.len() / 4;
        let a = six(chunk[0])?;
        let b = six(chunk[1])?;
        let c = if chunk[2] == b'=' { 0 } else { six(chunk[2])? };
        let d = if chunk[3] == b'=' { 0 } else { six(chunk[3])? };
        let padded = chunk[3] == b'=';
        if !last && padded {
            return None;
        }
        if chunk[2] == b'=' && (!padded || b & 0x0f != 0) {
            return None;
        }
        if padded && c & 0x03 != 0 {
            return None;
        }
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c << 6) | d);
        }
    }
    Some(out)
}

fn six(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn quoted_printable(value: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        match value[i] {
            b'_' => out.push(b' '),
            b'=' if i + 2 < value.len() => {
                out.push(hex(value[i + 1])? << 4 | hex(value[i + 2])?);
                i += 2;
            }
            b'!'..=b'~' if value[i] != b'?' && value[i] != b'=' => out.push(value[i]),
            _ => return None,
        }
        i += 1;
    }
    Some(out)
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn append_raw(output: &mut String, bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let remaining = MAX_OUTPUT.saturating_sub(output.len());
    if text.len() <= remaining {
        output.push_str(&text);
        return true;
    }
    while output.len() > MAX_OUTPUT - '…'.len_utf8() {
        output.pop();
    }
    let mut end = MAX_OUTPUT - '…'.len_utf8() - output.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&text[..end]);
    output.push('…');
    false
}
fn append_marker(output: &mut String) {
    if !output.ends_with('…') {
        while output.len() > MAX_OUTPUT - '…'.len_utf8() {
            output.pop();
        }
        output.push('…');
    }
}

#[cfg(test)]
mod tests {
    use super::decode_subject;
    #[test]
    fn mail01_subject_words_preserve_boundaries_invalid_data_and_literal_unicode() {
        for (input, expected) in [
            ("", ""),
            (" \t ", " \t "),
            ("답장: 한글 제목", "답장: 한글 제목"),
            ("=?uTf-8?b?7ZWc7Ya1?=", "한통"),
            ("=?iso-8859-1?q?caf=E9?=", "café"),
            ("=?US-ASCII?Q?hello_world?=", "hello world"),
            ("Re: =?UTF-8?Q?hello?= world", "Re: hello world"),
            ("=?UTF-8?Q?hello?= \t =?UTF-8?Q?_world?=", "hello world"),
            (
                "=?UTF-8?Q?hello?= \r\n =?UTF-8?Q?world?=",
                "hello \r\n world",
            ),
            (
                "=?UTF-8?Q?hello?==?UTF-8?Q?world?=",
                "=?UTF-8?Q?hello?==?UTF-8?Q?world?=",
            ),
            ("prefix=?UTF-8?Q?hello?=", "prefix=?UTF-8?Q?hello?="),
            ("=?UTF-8?Q?hello?=suffix", "=?UTF-8?Q?hello?=suffix"),
            ("=?UTF-8?Q?raw space?=", "=?UTF-8?Q?raw space?="),
            ("=?UTF-8?Q?trailing=?=", "=?UTF-8?Q?trailing=?="),
            ("=?UTF-8?Q?=ED=95?=", "=?UTF-8?Q?=ED=95?="),
            ("=?US-ASCII?Q?=FF?=", "=?US-ASCII?Q?=FF?="),
            ("=?UTF-8?B?QR==?=", "=?UTF-8?B?QR==?="),
            ("=?UTF-8?B?QUJ=?=", "=?UTF-8?B?QUJ=?="),
            ("=?UTF-8?B?QQ=Q?=", "=?UTF-8?B?QQ=Q?="),
            ("=?UTF-8?B?QQ==QQ==?=", "=?UTF-8?B?QQ==QQ==?="),
            (
                "=?UTF-8?Q?=3D=3FUTF-8=3FQ=3Fnested=3F=3D?=",
                "=?UTF-8?Q?nested?=",
            ),
            ("=?UTF-8?Q?=00=E2=80=AE?=", "\0\u{202e}"),
        ] {
            assert_eq!(decode_subject(input.as_bytes()), expected, "{input:?}");
        }
        let word = b"=?UTF-8?Q?=ED=95=9C?=";
        for end in 0..word.len() {
            assert_eq!(
                decode_subject(&word[..end]),
                String::from_utf8_lossy(&word[..end])
            );
        }
        let maximum = format!("=?UTF-8?Q?{}?=", "a".repeat(63));
        assert_eq!(maximum.len(), 75);
        assert_eq!(decode_subject(maximum.as_bytes()), "a".repeat(63));
        let over = maximum.replace("?=", "a?=");
        assert_eq!(decode_subject(over.as_bytes()), over);
    }
    #[test]
    fn mail01_decodes_b_q_and_raw_korean() {
        assert_eq!(
            decode_subject(b"=?US-ASCII?B?SGk=?=\t=?UTF-8?Q?=ED=95=9C?=\t=?utf-8?Q?=EA=B8=80?="),
            "Hi한글"
        );
        assert_eq!(decode_subject("한글".as_bytes()), "한글");
    }
    #[test]
    fn mail01_preserves_malformed_unsupported_and_suffix() {
        assert_eq!(
            decode_subject(b"x =?UTF-8?B?bad?= !"),
            "x =?UTF-8?B?bad?= !"
        );
        assert_eq!(
            decode_subject(b"=?X-UNKNOWN?B?SGk=?="),
            "=?X-UNKNOWN?B?SGk=?="
        );
        assert_eq!(decode_subject(b"=?UTF-8?Q?=ZZ?="), "=?UTF-8?Q?=ZZ?=");
        assert_eq!(decode_subject(b"=?UTF-8?Q?x?=,"), "=?UTF-8?Q?x?=,");
    }
    #[test]
    fn ops07_bounds_input_and_word_budget_with_markers() {
        assert!(decode_subject(&vec![b'a'; 40_000]).ends_with('…'));
        let many = " =?US-ASCII?B?QQ==?=".repeat(257);
        assert!(decode_subject(many.as_bytes()).contains('…'));
        let mut exact_limit = vec![0xff; 10_922];
        exact_limit.extend_from_slice(b"aa b");
        let output = decode_subject(&exact_limit);
        assert!(output.len() <= super::MAX_OUTPUT);
        assert!(output.ends_with('…'));
        for byte in 0..=255 {
            let output = decode_subject(&vec![byte; super::MAX_INPUT + 1]);
            assert!(output.len() <= super::MAX_OUTPUT);
            assert!(output.ends_with('…'));
        }
    }
}
