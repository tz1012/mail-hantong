//! Canonical lowercase hexadecimal for bounded wire fields.

use crate::ErrorCode;

pub fn encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub fn decode_fixed<const N: usize>(text: &str) -> Result<[u8; N], ErrorCode> {
    let expected = N.checked_mul(2).ok_or(ErrorCode::InvalidInput)?;
    if text.len() != expected {
        return Err(ErrorCode::InvalidInput);
    }
    let decoded = decode_bounded(text, N)?;
    decoded.try_into().map_err(|_| ErrorCode::InvalidInput)
}

pub fn decode_bounded(text: &str, max_bytes: usize) -> Result<Vec<u8>, ErrorCode> {
    if !text.len().is_multiple_of(2) || text.len() / 2 > max_bytes {
        return Err(ErrorCode::InvalidInput);
    }
    let mut output = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks_exact(2) {
        let high = nibble(pair[0]).ok_or(ErrorCode::InvalidInput)?;
        let low = nibble(pair[1]).ok_or(ErrorCode::InvalidInput)?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_hex_roundtrips_endpoints_and_empty() {
        for (bytes, text) in [
            (&[][..], ""),
            (&[0x00][..], "00"),
            (&[0x7f][..], "7f"),
            (&[0x80][..], "80"),
            (&[0xff][..], "ff"),
        ] {
            assert_eq!(encode(bytes), text);
            assert_eq!(decode_bounded(text, bytes.len()).unwrap(), bytes);
        }
        assert_eq!(decode_fixed::<0>(""), Ok([]));
        assert_eq!(
            decode_fixed::<4>("007f80ff").unwrap(),
            [0, 0x7f, 0x80, 0xff]
        );
    }

    #[test]
    fn wire_hex_rejects_wrong_lengths_and_limits() {
        assert!(decode_fixed::<2>("00").is_err());
        assert!(decode_fixed::<1>("0000").is_err());
        assert!(decode_bounded("00", 0).is_err());
        assert!(decode_bounded("0001", 1).is_err());
        assert!(decode_bounded("", 0).is_ok());
    }

    #[test]
    fn wire_hex_rejects_odd_uppercase_unicode_and_nonhex() {
        for value in ["0", "ABCDEF", "aB", "한글", "0g", "--"] {
            assert!(decode_bounded(value, usize::MAX).is_err());
        }
        assert_eq!(decode_bounded("abcdef", 3).unwrap(), vec![0xab, 0xcd, 0xef]);
    }
}
