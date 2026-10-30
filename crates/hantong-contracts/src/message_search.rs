use crate::{ErrorCode, uid_page::UidMailboxIdentity};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapMetadataFilter {
    All,
    Unread,
    Favorite,
}
impl ImapMetadataFilter {
    pub fn parse(v: &str) -> Result<Self, ErrorCode> {
        match v {
            "all" => Ok(Self::All),
            "unread" => Ok(Self::Unread),
            "favorite" => Ok(Self::Favorite),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Unread => "unread",
            Self::Favorite => "favorite",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMetadataSearchCursor {
    digest: [u8; 32],
    source: u64,
    resume: u32,
    expires: i64,
}
impl ImapMetadataSearchCursor {
    pub fn parse(v: &str) -> Result<Self, ErrorCode> {
        if v.len() > 256 {
            return Err(ErrorCode::InvalidInput);
        };
        let p: Vec<_> = v.split(':').collect();
        if p.len() != 5
            || p[0] != "m1"
            || p[1].len() != 64
            || !p[1]
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let parse_u = |s: &str, max: u64| {
            if s.is_empty()
                || (s.len() > 1 && s.starts_with('0'))
                || !s.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(ErrorCode::InvalidInput);
            };
            let n = s.parse::<u64>().map_err(|_| ErrorCode::InvalidInput)?;
            if n > max {
                Err(ErrorCode::InvalidInput)
            } else {
                Ok(n)
            }
        };
        let source = parse_u(p[2], i64::MAX as u64)?;
        if source == 0 {
            return Err(ErrorCode::InvalidInput);
        };
        let resume = parse_u(p[3], u32::MAX as u64)? as u32;
        let expires = parse_u(p[4], i64::MAX as u64)? as i64;
        if expires <= 0 {
            return Err(ErrorCode::InvalidInput);
        };
        let mut digest = [0; 32];
        for (i, b) in p[1].as_bytes().chunks(2).enumerate() {
            digest[i] = u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap();
        }
        Ok(Self {
            digest,
            source,
            resume,
            expires,
        })
    }
    pub fn encode(&self) -> String {
        format!(
            "m1:{}:{}:{}:{}",
            crate::wire_hex::encode(&self.digest),
            self.source,
            self.resume,
            self.expires
        )
    }
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    pub fn source_cursor_version(&self) -> u64 {
        self.source
    }
    pub fn resume_uid(&self) -> u32 {
        self.resume
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires
    }
}
impl fmt::Debug for ImapMetadataSearchCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapMetadataSearchCursor(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMetadataSearchRequest {
    account: String,
    mailbox: UidMailboxIdentity,
    filter: ImapMetadataFilter,
    limit: u32,
    cursor: Option<ImapMetadataSearchCursor>,
}
impl ImapMetadataSearchRequest {
    pub fn new(
        a: &str,
        m: UidMailboxIdentity,
        f: Option<ImapMetadataFilter>,
        l: Option<u32>,
        c: Option<ImapMetadataSearchCursor>,
    ) -> Result<Self, ErrorCode> {
        if a.is_empty()
            || a.len() > 128
            || !a
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ErrorCode::InvalidInput);
        };
        let n = l.unwrap_or(16);
        if !(1..=32).contains(&n) {
            return Err(ErrorCode::InvalidInput);
        };
        Ok(Self {
            account: a.into(),
            mailbox: m,
            filter: f.unwrap_or(ImapMetadataFilter::All),
            limit: n,
            cursor: c,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account)
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn filter(&self) -> ImapMetadataFilter {
        self.filter
    }
    pub fn limit(&self) -> u32 {
        self.limit
    }
    pub fn cursor(&self) -> Option<&ImapMetadataSearchCursor> {
        self.cursor.as_ref()
    }
}
impl fmt::Debug for ImapMetadataSearchRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapMetadataSearchRequest(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMetadataSearchItem {
    id: String,
    account: String,
    uid: u32,
    date: i64,
    size: u32,
    read: bool,
    favorite: bool,
    index: u32,
}
impl ImapMetadataSearchItem {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        a: &str,
        uid: u32,
        date: i64,
        size: u32,
        read: bool,
        favorite: bool,
        index: u32,
    ) -> Result<Self, ErrorCode> {
        if id.len() != 68
            || !id.starts_with("msg_")
            || !id[4..]
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
            || uid == 0
            || date % 1000 != 0
            || !(-62_135_596_800_000..=253_402_300_799_000).contains(&date)
            || a.is_empty()
            || a.len() > 128
            || !a
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || index >= 32
        {
            return Err(ErrorCode::InvalidInput);
        };
        Ok(Self {
            id: id.into(),
            account: a.into(),
            uid,
            date,
            size,
            read,
            favorite,
            index,
        })
    }
    pub fn internal_message_id(&self) -> &str {
        &self.id
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn uid(&self) -> u32 {
        self.uid
    }
    pub fn internal_date_utc_ms(&self) -> i64 {
        self.date
    }
    pub fn size_bytes(&self) -> u32 {
        self.size
    }
    pub fn is_read(&self) -> bool {
        self.read
    }
    pub fn is_favorite(&self) -> bool {
        self.favorite
    }
    pub fn header_content_index(&self) -> u32 {
        self.index
    }
}
impl fmt::Debug for ImapMetadataSearchItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapMetadataSearchItem(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct HeaderAddressProjection {
    host: Option<Vec<u8>>,
    mailbox: Option<Vec<u8>>,
    name: Option<Vec<u8>>,
    route: Option<Vec<u8>>,
}
impl HeaderAddressProjection {
    pub fn new(
        h: Option<Vec<u8>>,
        m: Option<Vec<u8>>,
        n: Option<Vec<u8>>,
        r: Option<Vec<u8>>,
    ) -> Result<Self, ErrorCode> {
        for x in [&h, &m, &n, &r] {
            if x.as_ref().is_some_and(|v| v.len() > 8192) {
                return Err(ErrorCode::InvalidInput);
            }
        }
        Ok(Self {
            host: h,
            mailbox: m,
            name: n,
            route: r,
        })
    }
    pub fn host(&self) -> Option<&[u8]> {
        self.host.as_deref()
    }
    pub fn mailbox(&self) -> Option<&[u8]> {
        self.mailbox.as_deref()
    }
    pub fn name(&self) -> Option<&[u8]> {
        self.name.as_deref()
    }
    pub fn route(&self) -> Option<&[u8]> {
        self.route.as_deref()
    }
}
impl fmt::Debug for HeaderAddressProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HeaderAddressProjection(<redacted>)")
    }
}
fn hx(v: Option<&Vec<u8>>) -> String {
    v.map(|x| format!("\"{}\"", crate::wire_hex::encode(x)))
        .unwrap_or_else(|| "null".into())
}
#[derive(Clone, Eq, PartialEq)]
pub struct HeaderProjection {
    subject: Option<Vec<u8>>,
    from: Option<Vec<HeaderAddressProjection>>,
    json: String,
}
impl HeaderProjection {
    pub fn new(
        s: Option<Vec<u8>>,
        f: Option<Vec<HeaderAddressProjection>>,
    ) -> Result<Self, ErrorCode> {
        if s.as_ref().is_some_and(|v| v.len() > 8192)
            || f.as_ref().is_some_and(|v| v.is_empty() || v.len() > 128)
        {
            return Err(ErrorCode::InvalidInput);
        };
        let mut total = s.as_ref().map_or(0, Vec::len);
        if let Some(v) = &f {
            for a in v {
                total += a.host.as_ref().map_or(0, Vec::len)
                    + a.mailbox.as_ref().map_or(0, Vec::len)
                    + a.name.as_ref().map_or(0, Vec::len)
                    + a.route.as_ref().map_or(0, Vec::len)
            }
        }
        if total > 65536 {
            return Err(ErrorCode::InvalidInput);
        };
        let from = f.as_ref().map(|v| {
            format!(
                "[{}]",
                v.iter()
                    .map(|a| format!(
                        "{{\"host_hex\":{},\"mailbox_hex\":{},\"name_hex\":{},\"route_hex\":{}}}",
                        hx(a.host.as_ref()),
                        hx(a.mailbox.as_ref()),
                        hx(a.name.as_ref()),
                        hx(a.route.as_ref())
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });
        let j = format!(
            "{{\"from\":{},\"subject_hex\":{}}}",
            from.unwrap_or_else(|| "null".into()),
            hx(s.as_ref())
        );
        Ok(Self {
            subject: s,
            from: f,
            json: j,
        })
    }
    pub fn projection_json(&self) -> &str {
        &self.json
    }
    pub fn subject(&self) -> Option<&[u8]> {
        self.subject.as_deref()
    }
    pub fn from(&self) -> Option<&[HeaderAddressProjection]> {
        self.from.as_deref()
    }
}
impl fmt::Debug for HeaderProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HeaderProjection(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMetadataSearchPage {
    account: String,
    mailbox: UidMailboxIdentity,
    catalog: u64,
    binding: bool,
    observed: Option<i64>,
    items: Vec<ImapMetadataSearchItem>,
    headers: Vec<HeaderProjection>,
    next: Option<ImapMetadataSearchCursor>,
}
impl ImapMetadataSearchPage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        a: &str,
        m: UidMailboxIdentity,
        c: u64,
        b: bool,
        o: Option<i64>,
        i: Vec<ImapMetadataSearchItem>,
        h: Vec<HeaderProjection>,
        n: Option<ImapMetadataSearchCursor>,
    ) -> Result<Self, ErrorCode> {
        if a.is_empty()
            || a.len() > 128
            || !a
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || c == 0
            || c > i64::MAX as u64
            || i.len() != h.len()
            || i.len() > 32
        {
            return Err(ErrorCode::InvalidInput);
        };
        let mut ids = std::collections::BTreeSet::new();
        let mut uids = std::collections::BTreeSet::new();
        for (x, p) in i.iter().zip(0..) {
            if x.account_id() != a
                || x.header_content_index() != p as u32
                || !ids.insert(x.internal_message_id())
                || !uids.insert(x.uid())
            {
                return Err(ErrorCode::InvalidInput);
            }
        }
        if o.is_none() && (!i.is_empty() || n.is_some()) {
            return Err(ErrorCode::InvalidInput);
        };
        if o.is_some_and(|x| x <= 0) {
            return Err(ErrorCode::InvalidInput);
        };
        Ok(Self {
            account: a.into(),
            mailbox: m,
            catalog: c,
            binding: b,
            observed: o,
            items: i,
            headers: h,
            next: n,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn catalog_generation(&self) -> u64 {
        self.catalog
    }
    pub fn binding_current(&self) -> bool {
        self.binding
    }
    pub fn observed_at_utc_ms(&self) -> Option<i64> {
        self.observed
    }
    pub fn items(&self) -> &[ImapMetadataSearchItem] {
        &self.items
    }
    pub fn headers(&self) -> &[HeaderProjection] {
        &self.headers
    }
    pub fn next_cursor(&self) -> Option<&ImapMetadataSearchCursor> {
        self.next.as_ref()
    }
}
impl fmt::Debug for ImapMetadataSearchPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapMetadataSearchPage(<redacted>)")
    }
}
