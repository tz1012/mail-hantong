#![cfg(target_os = "linux")]

use hantong_contracts::ErrorCode;
use std::collections::BTreeMap;
use std::str;

const MAX_PACKAGE: usize = 256 * 1024 * 1024;
const MAX_SOURCE: usize = 128 * 1024 * 1024;
const MAX_MEMBER: usize = 64 * 1024 * 1024;
const MAX_MEMBERS: usize = 4096;

fn bad() -> ErrorCode {
    ErrorCode::InvalidInput
}
fn read_u16(b: &[u8], at: usize) -> Result<u16, ErrorCode> {
    let x = b.get(at..at + 2).ok_or_else(bad)?;
    Ok(u16::from_le_bytes([x[0], x[1]]))
}
fn read_u32(b: &[u8], at: usize) -> Result<u32, ErrorCode> {
    let x = b.get(at..at + 4).ok_or_else(bad)?;
    Ok(u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn path_name(bytes: &[u8]) -> Result<String, ErrorCode> {
    if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
        return Err(bad());
    }
    let name = str::from_utf8(bytes).map_err(|_| bad())?;
    if name.starts_with('/') || name.contains('\\') || name.contains(':') || name.contains("//") {
        return Err(bad());
    }
    if name.split('/').any(|part| {
        part.is_empty() || part == "." || part == ".." || part.chars().any(|c| c.is_control())
    }) {
        return Err(bad());
    }
    Ok(name.to_owned())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bits: u64,
    count: u8,
}
impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bits: 0,
            count: 0,
        }
    }
    fn get(&mut self, n: u8) -> Result<u32, ErrorCode> {
        if n > 32 {
            return Err(bad());
        }
        if n == 0 {
            return Ok(0);
        }
        while self.count < n {
            let byte = *self.data.get(self.pos).ok_or_else(bad)?;
            self.pos += 1;
            self.bits |= (byte as u64) << self.count;
            self.count += 8;
        }
        let value = (self.bits & ((1u64 << n) - 1)) as u32;
        self.bits >>= n;
        self.count -= n;
        Ok(value)
    }
    fn align(&mut self) {
        self.bits = 0;
        self.count = 0;
    }
    fn byte_pos(&self) -> usize {
        // `pos` is the first byte after the bit buffer.  The last fetched
        // byte can still have unused bits, but it is nevertheless part of
        // the DEFLATE stream and the gzip trailer starts at `pos`.
        self.pos
    }
}

#[derive(Clone)]
struct HuffNode {
    child: [i16; 2],
    value: Option<u16>,
}
#[derive(Clone)]
struct Huff {
    nodes: Vec<HuffNode>,
}
fn reverse(mut value: u16, bits: u8) -> u16 {
    let mut out = 0;
    for _ in 0..bits {
        out = (out << 1) | (value & 1);
        value >>= 1;
    }
    out
}
fn huffman(lengths: &[u8]) -> Result<Huff, ErrorCode> {
    let mut count = [0u16; 16];
    for &len in lengths {
        if len > 15 {
            return Err(bad());
        }
        if len > 0 {
            count[len as usize] += 1;
        }
    }
    let mut next = [0u16; 16];
    let mut code = 0u16;
    let mut left = 1i32;
    for bits in 1..16 {
        left = (left << 1) - count[bits] as i32;
        if left < 0 {
            return Err(bad());
        }
        code = (code + count[bits - 1]) << 1;
        next[bits] = code;
    }
    let mut nodes = vec![HuffNode {
        child: [-1, -1],
        value: None,
    }];
    for (symbol, &len) in lengths.iter().enumerate() {
        if len != 0 {
            let c = reverse(next[len as usize], len);
            next[len as usize] += 1;
            let mut node = 0usize;
            for bit in 0..len {
                if nodes[node].value.is_some() {
                    return Err(bad());
                }
                let branch = ((c >> bit) & 1) as usize;
                let child = nodes[node].child[branch];
                node = if child < 0 {
                    let index = nodes.len();
                    nodes.push(HuffNode {
                        child: [-1, -1],
                        value: None,
                    });
                    nodes[node].child[branch] = index as i16;
                    index
                } else {
                    child as usize
                };
            }
            if nodes[node].value.is_some() || nodes[node].child != [-1, -1] {
                return Err(bad());
            }
            nodes[node].value = Some(symbol as u16);
        }
    }
    if nodes.len() == 1 {
        return Err(bad());
    }
    Ok(Huff { nodes })
}
fn symbol(bits: &mut Bits<'_>, tree: &Huff) -> Result<u16, ErrorCode> {
    let mut node = 0usize;
    for _ in 0..15 {
        let branch = bits.get(1)? as usize;
        let child = tree.nodes[node].child[branch];
        if child < 0 {
            return Err(bad());
        }
        node = child as usize;
        if let Some(value) = tree.nodes[node].value {
            return Ok(value);
        }
    }
    Err(bad())
}

const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn inflate_raw(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), ErrorCode> {
    let mut bits = Bits::new(data);
    let mut output = Vec::new();
    let fixed_lit = {
        let mut lengths = vec![0u8; 288];
        for i in 0..=143 {
            lengths[i] = 8;
        }
        for i in 144..=255 {
            lengths[i] = 9;
        }
        for i in 256..=279 {
            lengths[i] = 7;
        }
        for i in 280..288 {
            lengths[i] = 8;
        }
        huffman(&lengths)?
    };
    let fixed_dist = huffman(&[5u8; 32])?;
    let mut final_block = false;
    while !final_block {
        final_block = bits.get(1)? != 0;
        let kind = bits.get(2)?;
        if kind == 0 {
            bits.align();
            let len = bits.get(16)? as usize;
            let inv = bits.get(16)? as u16;
            if ((len as u16) ^ inv) != 0xffff || len > limit.saturating_sub(output.len()) {
                return Err(bad());
            }
            for _ in 0..len {
                output.push(bits.get(8)? as u8);
            }
            continue;
        }
        if kind == 3 {
            return Err(bad());
        }
        let (lit, dist) = if kind == 1 {
            (fixed_lit.clone(), fixed_dist.clone())
        } else {
            let hlit = bits.get(5)? as usize + 257;
            let hdist = bits.get(5)? as usize + 1;
            let hclen = bits.get(4)? as usize + 4;
            if hlit > 286 || hdist > 32 {
                return Err(bad());
            }
            let order = [
                16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
            ];
            let mut cl = [0u8; 19];
            for i in 0..hclen {
                cl[order[i]] = bits.get(3)? as u8;
            }
            let cl_tree = huffman(&cl)?;
            let mut all = Vec::with_capacity(hlit + hdist);
            while all.len() < hlit + hdist {
                let value = symbol(&mut bits, &cl_tree)?;
                match value {
                    0..=15 => all.push(value as u8),
                    16 => {
                        let repeat = bits.get(2)? as usize + 3;
                        let previous = *all.last().ok_or_else(bad)?;
                        if repeat > 138 || all.len() + repeat > hlit + hdist {
                            return Err(bad());
                        }
                        all.extend(std::iter::repeat_n(previous, repeat));
                    }
                    17 => {
                        let repeat = bits.get(3)? as usize + 3;
                        if repeat > 138 || all.len() + repeat > hlit + hdist {
                            return Err(bad());
                        }
                        all.extend(std::iter::repeat_n(0, repeat));
                    }
                    18 => {
                        let repeat = bits.get(7)? as usize + 11;
                        if repeat > 138 || all.len() + repeat > hlit + hdist {
                            return Err(bad());
                        }
                        all.extend(std::iter::repeat_n(0, repeat));
                    }
                    _ => return Err(bad()),
                }
            }
            (huffman(&all[..hlit])?, huffman(&all[hlit..])?)
        };
        loop {
            let value = symbol(&mut bits, &lit)?;
            match value {
                0..=255 => {
                    if output.len() == limit {
                        return Err(bad());
                    }
                    output.push(value as u8);
                }
                256 => break,
                257..=285 => {
                    let index = value as usize - 257;
                    let length = LENGTH_BASE[index] + bits.get(LENGTH_EXTRA[index])? as usize;
                    let dcode = symbol(&mut bits, &dist)? as usize;
                    if dcode >= DIST_BASE.len() {
                        return Err(bad());
                    }
                    let distance = DIST_BASE[dcode] + bits.get(DIST_EXTRA[dcode])? as usize;
                    if distance == 0 || distance > output.len() || length > limit - output.len() {
                        return Err(bad());
                    }
                    for _ in 0..length {
                        let at = output.len() - distance;
                        output.push(output[at]);
                    }
                }
                _ => return Err(bad()),
            }
        }
    }
    Ok((output, bits.byte_pos()))
}

fn octal(bytes: &[u8]) -> Result<usize, ErrorCode> {
    let mut value = 0usize;
    for &b in bytes {
        if b == 0 || b == b' ' {
            continue;
        }
        if !(b'0'..=b'7').contains(&b) {
            return Err(bad());
        }
        value = value
            .checked_mul(8)
            .and_then(|x| x.checked_add((b - b'0') as usize))
            .ok_or_else(bad)?;
    }
    Ok(value)
}
fn tar(bytes: &[u8], limit: usize) -> Result<BTreeMap<String, Vec<u8>>, ErrorCode> {
    if bytes.len() > limit || bytes.len() % 512 != 0 {
        return Err(bad());
    }
    let mut out = BTreeMap::new();
    let mut at = 0;
    let mut count = 0;
    loop {
        let header = bytes.get(at..at + 512).ok_or_else(bad)?;
        at += 512;
        if header.iter().all(|&x| x == 0) {
            let mut zero_blocks = 1usize;
            while at + 512 <= bytes.len() && bytes[at..at + 512].iter().all(|&value| value == 0) {
                zero_blocks += 1;
                at += 512;
            }
            if zero_blocks < 2 || bytes[at..].iter().any(|&value| value != 0) {
                return Err(bad());
            }
            return Ok(out);
        }
        let stored = octal(&header[148..156])?;
        let mut sum = 0usize;
        for i in 0..512 {
            sum += if (148..156).contains(&i) {
                32
            } else {
                header[i] as usize
            };
        }
        if stored != sum {
            return Err(bad());
        }
        if &header[257..263] != b"ustar\0" {
            return Err(bad());
        }
        if header[156] != 0 && header[156] != b'0' {
            return Err(bad());
        }
        let name_end = header[0..100].iter().position(|&x| x == 0).unwrap_or(100);
        let prefix_end = header[345..500].iter().position(|&x| x == 0).unwrap_or(155);
        let mut name_bytes = Vec::with_capacity(prefix_end + 1 + name_end);
        if prefix_end != 0 {
            name_bytes.extend_from_slice(&header[345..345 + prefix_end]);
            name_bytes.push(b'/');
        }
        name_bytes.extend_from_slice(&header[0..name_end]);
        let name = path_name(&name_bytes)?;
        let size = octal(&header[124..136])?;
        count += 1;
        if count > MAX_MEMBERS || size > MAX_MEMBER {
            return Err(bad());
        }
        let body = bytes.get(at..at + size).ok_or_else(bad)?;
        at += (size + 511) / 512 * 512;
        if out.insert(name, body.to_vec()).is_some() {
            return Err(bad());
        }
    }
}

pub(crate) fn gzip_tar(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, ErrorCode> {
    if bytes.len() > MAX_PACKAGE
        || bytes.len() < 18
        || bytes.get(..2) != Some(&[0x1f, 0x8b])
        || bytes[2] != 8
    {
        return Err(bad());
    }
    let flags = bytes[3];
    if flags & 0xe0 != 0 {
        return Err(bad());
    }
    let mut header_end = 10usize;
    if flags & 0x04 != 0 {
        let extra_len = read_u16(bytes, header_end)? as usize;
        header_end = header_end.checked_add(2 + extra_len).ok_or_else(bad)?;
        bytes.get(..header_end).ok_or_else(bad)?;
    }
    if flags & 0x08 != 0 {
        let end = bytes
            .get(header_end..)
            .ok_or_else(bad)?
            .iter()
            .position(|&value| value == 0)
            .ok_or_else(bad)?;
        header_end = header_end.checked_add(end + 1).ok_or_else(bad)?;
    }
    if flags & 0x10 != 0 {
        let end = bytes
            .get(header_end..)
            .ok_or_else(bad)?
            .iter()
            .position(|&value| value == 0)
            .ok_or_else(bad)?;
        header_end = header_end.checked_add(end + 1).ok_or_else(bad)?;
    }
    if flags & 0x02 != 0 {
        header_end = header_end.checked_add(2).ok_or_else(bad)?;
        bytes.get(..header_end).ok_or_else(bad)?;
    }
    let (expanded, consumed) = inflate_raw(&bytes[header_end..], MAX_PACKAGE)?;
    let trailer = header_end.checked_add(consumed).ok_or_else(bad)?;
    if trailer + 8 != bytes.len() {
        return Err(bad());
    }
    if read_u32(bytes, trailer)? != crc32(&expanded)
        || read_u32(bytes, trailer + 4)? != expanded.len() as u32
    {
        return Err(bad());
    }
    tar(&expanded, MAX_PACKAGE)
}

pub(crate) fn source_zip(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, ErrorCode> {
    if bytes.len() > MAX_SOURCE {
        return Err(bad());
    }
    let end = bytes.len().checked_sub(22).ok_or_else(bad)?;
    let mut eocd = None;
    for at in (0..=end).rev() {
        if bytes.get(at..at + 4) == Some(b"PK\x05\x06") {
            eocd = Some(at);
            break;
        }
    }
    let eocd = eocd.ok_or_else(bad)?;
    if eocd + 22 + read_u16(bytes, eocd + 20)? as usize != bytes.len() {
        return Err(bad());
    }
    if read_u16(bytes, eocd + 4)? != 0
        || read_u16(bytes, eocd + 6)? != 0
        || read_u16(bytes, eocd + 8)? != read_u16(bytes, eocd + 10)?
        || read_u16(bytes, eocd + 8)? > MAX_MEMBERS as u16
    {
        return Err(bad());
    }
    let count = read_u16(bytes, eocd + 8)? as usize;
    let central_size = read_u32(bytes, eocd + 12)? as usize;
    let central_at = read_u32(bytes, eocd + 16)? as usize;
    if central_at.checked_add(central_size) != Some(eocd) {
        return Err(bad());
    }
    let mut out = BTreeMap::new();
    let mut local_ranges: Vec<(usize, usize)> = Vec::with_capacity(count);
    let mut expanded_total = 0usize;
    let mut at = central_at;
    for _ in 0..count {
        if bytes.get(at..at + 4) != Some(&b"PK\x01\x02"[..]) {
            return Err(bad());
        }
        let version_made = read_u16(bytes, at + 4)?;
        let flags = read_u16(bytes, at + 8)?;
        let method = read_u16(bytes, at + 10)?;
        let crc = read_u32(bytes, at + 16)?;
        let csize = read_u32(bytes, at + 20)? as usize;
        let usize_ = read_u32(bytes, at + 24)? as usize;
        let nlen = read_u16(bytes, at + 28)? as usize;
        let xlen = read_u16(bytes, at + 30)? as usize;
        let clen = read_u16(bytes, at + 32)? as usize;
        let ext = read_u32(bytes, at + 38)?;
        let local = read_u32(bytes, at + 42)? as usize;
        if flags & !0x0800 != 0
            || (method != 0 && method != 8)
            || usize_ > MAX_MEMBER
            || csize > MAX_MEMBER
            || ext & 0x10 != 0
        {
            return Err(bad());
        }
        let nameb = bytes.get(at + 46..at + 46 + nlen).ok_or_else(bad)?;
        if flags & 0x0800 == 0 && !nameb.is_ascii() {
            return Err(bad());
        }
        let name = path_name(nameb)?;
        if out.contains_key(&name) {
            return Err(bad());
        }
        if bytes.get(local..local + 4) != Some(&b"PK\x03\x04"[..]) {
            return Err(bad());
        }
        let ln = read_u16(bytes, local + 26)? as usize;
        let lx = read_u16(bytes, local + 28)? as usize;
        if read_u16(bytes, local + 6)? != flags
            || read_u16(bytes, local + 8)? != method
            || read_u32(bytes, local + 14)? != crc
            || read_u32(bytes, local + 18)? as usize != csize
            || read_u32(bytes, local + 22)? as usize != usize_
            || bytes.get(local + 30..local + 30 + ln) != Some(nameb)
        {
            return Err(bad());
        }
        let start = local + 30 + ln + lx;
        let local_end = start.checked_add(csize).ok_or_else(bad)?;
        if local_end > central_at {
            return Err(bad());
        }
        if local_ranges
            .iter()
            .any(|(begin, end)| local < *end && *begin < local_end)
        {
            return Err(bad());
        }
        local_ranges.push((local, local_end));
        at += 46 + nlen + xlen + clen;
        if version_made >> 8 == 3 {
            let mode = ext >> 16;
            if mode & 0o170000 != 0o100000 {
                return Err(bad());
            }
        }
        let compressed = bytes.get(start..start + csize).ok_or_else(bad)?;
        let value = if method == 0 {
            if csize != usize_ {
                return Err(bad());
            }
            compressed.to_vec()
        } else {
            let (value, used) = inflate_raw(compressed, usize_)?;
            if used != csize {
                return Err(bad());
            }
            value
        };
        expanded_total = expanded_total
            .checked_add(value.len())
            .filter(|&total| total <= MAX_SOURCE)
            .ok_or_else(bad)?;
        if value.len() != usize_ || crc32(&value) != crc {
            return Err(bad());
        }
        out.insert(name, value);
    }
    if at != eocd {
        return Err(bad());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base64(text: &str) -> Vec<u8> {
        let mut value = 0u32;
        let mut bits = 0u8;
        let mut out = Vec::new();
        for byte in text.bytes().filter(|&byte| byte != b'=') {
            let digit = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => panic!("invalid fixture"),
            } as u32;
            value = (value << 6) | digit;
            bits += 6;
            while bits >= 8 {
                bits -= 8;
                out.push((value >> bits) as u8);
                value &= (1u32 << bits).saturating_sub(1);
            }
        }
        out
    }

    fn octal(value: usize, width: usize) -> Vec<u8> {
        let mut out = vec![b'0'; width];
        let text = format!("{value:o}");
        let start = width.saturating_sub(text.len() + 1);
        out[start..start + text.len()].copy_from_slice(text.as_bytes());
        out[width - 1] = 0;
        out
    }

    fn tar_fixture(name: &[u8], body: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name);
        header[100..108].copy_from_slice(&octal(0o644, 8));
        header[124..136].copy_from_slice(&octal(body.len(), 12));
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[148..156].fill(b' ');
        let checksum: usize = header.iter().map(|v| *v as usize).sum();
        header[148..156].copy_from_slice(&octal(checksum, 8));
        let mut out = header.to_vec();
        out.extend_from_slice(body);
        out.resize(out.len() + ((512 - body.len() % 512) % 512), 0);
        out.resize(out.len() + 1024, 0);
        out
    }

    fn gzip_stored(payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() <= u16::MAX as usize);
        let length = payload.len() as u16;
        let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 1];
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(payload);
        out.extend_from_slice(&crc32(payload).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out
    }

    fn zip_stored(name: &[u8], body: &[u8]) -> Vec<u8> {
        let crc = crc32(body);
        let size = body.len() as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name);
        out.extend_from_slice(body);
        let central = out.len();
        out.extend_from_slice(b"PK\x01\x02");
        out.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(name);
        let central_size = out.len() - central;
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&(central_size as u32).to_le_bytes());
        out.extend_from_slice(&(central as u32).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    fn zip_stored_two() -> Vec<u8> {
        let first = zip_stored(b"A", b"one");
        let second = zip_stored(b"B", b"two");
        let first_central = first.windows(4).rposition(|x| x == b"PK\x01\x02").unwrap();
        let first_eocd = first.windows(4).rposition(|x| x == b"PK\x05\x06").unwrap();
        let second_central = second.windows(4).rposition(|x| x == b"PK\x01\x02").unwrap();
        let second_eocd = second.windows(4).rposition(|x| x == b"PK\x05\x06").unwrap();
        let mut out = first[..first_central].to_vec();
        let second_local_at = out.len();
        out.extend_from_slice(&second[..second_central]);
        let central_at = out.len();
        out.extend_from_slice(&first[first_central..first_eocd]);
        let mut second_central_record = second[second_central..second_eocd].to_vec();
        second_central_record[42..46].copy_from_slice(&(second_local_at as u32).to_le_bytes());
        out.extend_from_slice(&second_central_record);
        let central_size = out.len() - central_at;
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&(central_size as u32).to_le_bytes());
        out.extend_from_slice(&(central_at as u32).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn sec03_gzip_tar_accepts_stored_member_and_rejects_crc_or_truncation() {
        let tar = tar_fixture(b"bin/tool", b"payload");
        let mut gzip = gzip_stored(&tar);
        let result = gzip_tar(&gzip).unwrap();
        assert_eq!(result.get("bin/tool").unwrap(), b"payload");
        let last = gzip.len() - 8;
        gzip[last] ^= 1;
        assert_eq!(gzip_tar(&gzip), Err(ErrorCode::InvalidInput));
        let mut truncated = gzip_stored(&tar);
        truncated.pop();
        assert_eq!(gzip_tar(&truncated), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn sec03_gzip_accepts_mtime_and_optional_header_fields() {
        let tar = tar_fixture(b"bin/tool", b"payload");
        let mut gzip = gzip_stored(&tar);
        gzip[3] = 0x04 | 0x08 | 0x10;
        gzip[4..8].copy_from_slice(&1u32.to_le_bytes());
        let mut framed = gzip[..10].to_vec();
        framed.extend_from_slice(&[2, 0, b'x', b'y']);
        framed.extend_from_slice(b"name\0comment\0");
        framed.extend_from_slice(&gzip[10..]);
        let result = gzip_tar(&framed).unwrap();
        assert_eq!(result.get("bin/tool").unwrap(), b"payload");
    }

    #[test]
    fn sec03_python_stdlib_gzip_fixed_and_dynamic_zip_deflate_fixtures() {
        let fixed = base64(
            "H4sIAAEAAAAA/0vKzNMvyc/PYaAhMAACc1NTMA0E6DRIFokNEjc3MTRlUDCgpaNgoLS4JLEIaCU97BqEoCCxMic/MWWgnTEKRsEoGAWjYBSMglEwCkbBKBgFo2AUjIJRMApGwSgYBaNgFIyCUTAKRsEoGAWjYBSMglEwCkbBKBgFo4BEAADCIwPGACgAAA==",
        );
        assert_eq!(
            gzip_tar(&fixed).unwrap().get("bin/tool").unwrap(),
            b"payload"
        );
        let dynamic = base64(
            "H4sIAAEAAAAE/+3QQQqDMBAF0BzFG2ihIeeJdFOQRtp00dsboXXRvRt9s5nwA8nMG++PvpYyhR1raJViDGtv9d9blH7Z9z5dLzF0w44zbU+/XzU/2/dbcK7DnD9TybdzLW1bAgQIECBAgAABAgQIECBAgAABAgQIECBAgAABAgQIECBAgAABAgQIHEBgAcIjA8YAKAAA",
        );
        assert_eq!(
            gzip_tar(&dynamic).unwrap().get("bin/tool").unwrap(),
            b"payload"
        );
        let zip = base64(
            "UEsDBBQAAAAIACaIZ12OMXqjBgAAACkAAAANAAAAU09VUkNFLUNPTU1JVEtMJBIAAFBLAQIUABQAAAAIACaIZ12OMXqjBgAAACkAAAANAAAAAAAAAAAAAACAAQAAAABTT1VSQ0UtQ09NTUlUUEsFBgAAAAABAAEAOwAAADEAAAAAAA==",
        );
        assert_eq!(
            source_zip(&zip)
                .unwrap()
                .get("SOURCE-COMMIT")
                .unwrap()
                .len(),
            41
        );
    }

    #[test]
    fn sec03_tar_accepts_ustar_prefix_and_python_padding() {
        let mut archive = tar_fixture(b"tool", b"payload");
        archive[0..100].fill(0);
        archive[..4].copy_from_slice(b"tool");
        archive[345..348].copy_from_slice(b"bin");
        archive[148..156].fill(b' ');
        let checksum: usize = archive[..512].iter().map(|v| *v as usize).sum();
        archive[148..156].copy_from_slice(&octal(checksum, 8));
        archive.extend_from_slice(&[0u8; 8192]);
        assert_eq!(
            gzip_tar(&gzip_stored(&archive))
                .unwrap()
                .get("bin/tool")
                .unwrap(),
            b"payload"
        );
    }

    #[test]
    fn sec03_tar_rejects_path_and_link_entries() {
        let mut unsafe_tar = tar_fixture(b"../escape", b"x");
        assert_eq!(
            gzip_tar(&gzip_stored(&unsafe_tar)),
            Err(ErrorCode::InvalidInput)
        );
        unsafe_tar = tar_fixture(b"bin/link", b"x");
        unsafe_tar[156] = b'2';
        assert_eq!(
            gzip_tar(&gzip_stored(&unsafe_tar)),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn sec03_source_zip_accepts_store_and_rejects_crc_or_path() {
        let zip = zip_stored(b"SOURCE-COMMIT", b"a".repeat(41).as_slice());
        assert_eq!(
            source_zip(&zip)
                .unwrap()
                .get("SOURCE-COMMIT")
                .unwrap()
                .len(),
            41
        );
        let mut bad = zip.clone();
        bad[14] ^= 1;
        assert_eq!(source_zip(&bad), Err(ErrorCode::InvalidInput));
        let path_zip = zip_stored(b"../escape", b"x");
        assert_eq!(source_zip(&path_zip), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn sec03_source_zip_rejects_local_mismatch_and_accepts_utf8_flag() {
        let mut zip = zip_stored("한통/SOURCE-COMMIT".as_bytes(), b"payload");
        // The helper intentionally omits the UTF-8 flag; a non-ASCII name
        // must not be accepted under an ambiguous legacy encoding.
        assert_eq!(source_zip(&zip), Err(ErrorCode::InvalidInput));
        zip[6] = 0x00;
        zip[7] = 0x08;
        let central = zip.windows(4).rposition(|x| x == b"PK\x01\x02").unwrap();
        zip[central + 8] = 0x00;
        zip[central + 9] = 0x08;
        assert!(source_zip(&zip).is_ok());

        let mut mismatch = zip_stored(b"SOURCE-COMMIT", b"payload");
        mismatch[18] ^= 1;
        assert_eq!(source_zip(&mismatch), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn sec03_source_zip_rejects_overlapping_local_records() {
        let mut zip = zip_stored_two();
        let central = zip.windows(4).rposition(|x| x == b"PK\x01\x02").unwrap();
        let first_central = zip[..central]
            .windows(4)
            .rposition(|x| x == b"PK\x01\x02")
            .unwrap();
        let first_offset = zip[first_central + 42..first_central + 46].to_vec();
        zip[central + 42..central + 46].copy_from_slice(&first_offset);
        assert_eq!(source_zip(&zip), Err(ErrorCode::InvalidInput));
    }
}
