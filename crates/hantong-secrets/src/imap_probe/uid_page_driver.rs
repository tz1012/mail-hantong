//! A single private authenticated metadata operation. No session escapes.
use super::*;
use crate::imap_probe::uid_page_gate::{Mode, Observation, Reply, UidPageGate};
use hantong_domain::imap_sync::ImapMessageMetadata;
use io_imap::{
    codec::{CommandCodec, ResponseCodec, decode::Decoder, encode::Fragment},
    types::{core::LiteralMode, fetch::MessageDataItemName, response::Response},
};

#[derive(Default)]
struct Selection {
    validity: Option<u32>,
    next: Option<u32>,
    read_only: bool,
    read_write: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UidMoveOutcome {
    Accepted,
    Unknown,
}
impl Selection {
    fn observe(&mut self, observation: Observation) -> Result<(), MailProbeError> {
        match observation {
            Observation::None => {}
            Observation::UidValidity(n) if self.validity.is_none() => self.validity = Some(n),
            Observation::UidNext(n) if self.next.is_none() => self.next = Some(n),
            Observation::ReadOnly if !self.read_only => self.read_only = true,
            Observation::ReadWrite if !self.read_write => self.read_write = true,
            _ => return Err(MailProbeError::Protocol),
        }
        Ok(())
    }
    fn finish(self) -> Result<(u32, u32), MailProbeError> {
        if !self.read_only {
            return Err(MailProbeError::Protocol);
        }
        Ok((
            self.validity.ok_or(MailProbeError::Protocol)?,
            self.next.ok_or(MailProbeError::Protocol)?,
        ))
    }

    fn finish_read_write(self) -> Result<u32, MailProbeError> {
        if !self.read_write {
            return Err(MailProbeError::Protocol);
        }
        self.validity.ok_or(MailProbeError::Protocol)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Protocol<S> {
    pub(super) async fn body(
        &mut self,
        request: &hantong_contracts::message_body::MessageBodyRequest,
        encoding: MailboxEncoding,
    ) -> Result<hantong_contracts::message_body::MessageBody, crate::imap_body::ImapBodyError> {
        use hantong_contracts::uid_page::UidMailboxEncoding;
        let expected_encoding = match request.mailbox().encoding() {
            UidMailboxEncoding::Utf8 => MailboxEncoding::Utf8,
            UidMailboxEncoding::ModifiedUtf7 => MailboxEncoding::ModifiedUtf7,
        };
        if encoding != expected_encoding {
            return Err(MailProbeError::Protocol.into());
        }
        let wire = hantong_contracts::wire_hex::decode_bounded(request.mailbox().wire_hex(), 1024)
            .map_err(|_| MailProbeError::Protocol)?;
        let mailbox =
            ImapMailboxName::from_wire(&wire, encoding).map_err(|_| MailProbeError::Protocol)?;
        let (validity, next) = self.examine("h6", &mailbox).await?;
        if validity != request.uidvalidity() {
            return Err(crate::imap_body::ImapBodyError::UidValidityChanged);
        }
        if request.uid() >= next {
            return Err(crate::imap_body::ImapBodyError::NotFound);
        }
        // All interpolated bytes are validated numeric identity; PEEK and EXAMINE
        // together avoid changing the message's Seen flag.
        self.write(SensitiveBuffer(
            format!("h7 UID FETCH {} (UID BODY.PEEK[])\r\n", request.uid()).into_bytes(),
        ))
        .await?;
        let mut frame = Vec::new();
        while !frame.ends_with(b"\r\n") {
            if frame.len() >= 512 {
                return Err(MailProbeError::Protocol.into());
            }
            frame.push(self.byte().await?);
        }
        if frame.starts_with(b"h7 OK ") || frame == b"h7 OK\r\n" {
            return Err(crate::imap_body::ImapBodyError::NotFound);
        }
        let open = frame
            .iter()
            .rposition(|b| *b == b'{')
            .ok_or(MailProbeError::Protocol)?;
        let digits = frame
            .get(open + 1..frame.len().saturating_sub(3))
            .ok_or(MailProbeError::Protocol)?;
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(MailProbeError::Protocol.into());
        }
        let length: usize = std::str::from_utf8(digits)
            .map_err(|_| MailProbeError::Protocol)?
            .parse()
            .map_err(|_| MailProbeError::Protocol)?;
        if length > request.max_bytes() {
            return Err(crate::imap_body::ImapBodyError::TooLarge);
        }
        for _ in 0..length {
            frame.push(self.byte().await?);
        }
        let literal_end = frame.len();
        loop {
            if frame.len() - literal_end >= 512 {
                return Err(MailProbeError::Protocol.into());
            }
            frame.push(self.byte().await?);
            if frame[literal_end..].ends_with(b"\r\n") {
                break;
            }
        }
        let body = crate::imap_body::parse_body_peek(request.clone(), validity, &frame)
            .map_err(|_| MailProbeError::Protocol)?;
        if !matches!(
            self.uid_frame(Mode::Fetch, "h7").await?,
            Reply::Tagged(Observation::None)
        ) {
            return Err(MailProbeError::Protocol.into());
        }
        Ok(body)
    }
    async fn uid_fetch(&mut self, first: u32, last: u32) -> Result<(), MailProbeError> {
        if first == 0 || last < first || u64::from(last) - u64::from(first) >= 64 {
            return Err(MailProbeError::Protocol);
        }
        let range = format!("{first}:{last}");
        let command = Command {
            tag: "h7".try_into().map_err(|_| MailProbeError::Protocol)?,
            body: CommandBody::fetch(
                range.as_str(),
                vec![
                    MessageDataItemName::Uid,
                    MessageDataItemName::Flags,
                    MessageDataItemName::InternalDate,
                    MessageDataItemName::Rfc822Size,
                    MessageDataItemName::Envelope,
                ],
                true,
            )
            .map_err(|_| MailProbeError::Protocol)?,
        };
        let mut encoded = CommandCodec::new().encode(&command).dump();
        const PREFIX: &[u8] = b"h7 UID FETCH ";
        const SUFFIX: &[u8] = b" (UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)\r\n";
        let normalized = encoded
            .strip_prefix(PREFIX)
            .and_then(|b| b.strip_suffix(SUFFIX))
            .ok_or(MailProbeError::Protocol)?;
        if normalized != range.as_bytes() {
            // The pinned codec normalizes 1:1 to 1 and a range ending at
            // u32::MAX-1 to '*'. Restore ONLY this validated numeric token;
            // all other bytes come from the codec's fixed command. Decode
            // and compare the complete command before sending anything.
            encoded.splice(PREFIX.len()..PREFIX.len() + normalized.len(), range.bytes());
        }
        let (rest, decoded) = CommandCodec::new()
            .decode(&encoded)
            .map_err(|_| MailProbeError::Protocol)?;
        if !rest.is_empty() || decoded != command || encoded.contains(&b'*') {
            return Err(MailProbeError::Protocol);
        }
        self.write(SensitiveBuffer(encoded)).await
    }
    async fn literal_continuation(&mut self) -> Result<(), MailProbeError> {
        let mut line = Vec::with_capacity(128);
        // Unlike SASL challenges, literal continuation text is not base64.
        // Restrict the shape before invoking the codec and never retain it.
        while line.len() < 512 {
            line.push(self.byte().await?);
            if line.last() == Some(&b'\n') {
                if !line.starts_with(b"+ ")
                    || !line.ends_with(b"\r\n")
                    || !line[2..line.len() - 2]
                        .iter()
                        .all(|b| (32..=126).contains(b))
                    || line.get(2) == Some(&b'[')
                {
                    return Err(MailProbeError::Protocol);
                }
                let (rest, response) = ResponseCodec::new()
                    .decode(&line)
                    .map_err(|_| MailProbeError::Protocol)?;
                return if rest.is_empty()
                    && matches!(response, Response::CommandContinuationRequest(_))
                {
                    Ok(())
                } else {
                    Err(MailProbeError::Protocol)
                };
            }
        }
        Err(MailProbeError::Protocol)
    }
    async fn uid_frame(&mut self, mode: Mode, tag: &str) -> Result<Reply, MailProbeError> {
        let mut gate = UidPageGate::new(mode, tag).map_err(|_| MailProbeError::Protocol)?;
        loop {
            if let Some(reply) = gate
                .push(self.byte().await?)
                .map_err(|_| MailProbeError::Protocol)?
            {
                return Ok(reply);
            }
        }
    }
    async fn examine(
        &mut self,
        tag: &str,
        mailbox: &ImapMailboxName,
    ) -> Result<(u32, u32), MailProbeError> {
        let command = Command {
            tag: tag.try_into().map_err(|_| MailProbeError::Protocol)?,
            body: CommandBody::examine(mailbox.wire_name())
                .map_err(|_| MailProbeError::Protocol)?,
        };
        // UTF-8 mailbox names can require a synchronizing literal. Send the
        // codec's fragments and wait for one bounded continuation before data.
        // This is deliberately restricted to the fixed EXAMINE constructor.
        for fragment in CommandCodec::new().encode(&command) {
            match fragment {
                Fragment::Line { data } => self.write(SensitiveBuffer(data)).await?,
                Fragment::Literal { data, mode } => {
                    if mode == LiteralMode::Sync {
                        self.literal_continuation().await?;
                    }
                    self.write(SensitiveBuffer(data)).await?;
                }
            }
        }
        let mut selection = Selection::default();
        // At most 128 untagged status/data frames, followed by one tagged OK.
        for count in 0..=128 {
            match self.uid_frame(Mode::Examine, tag).await? {
                Reply::Tagged(observation) => {
                    selection.observe(observation)?;
                    return selection.finish();
                }
                Reply::Status(observation) if count < 128 => selection.observe(observation)?,
                Reply::Data if count < 128 => {}
                _ => return Err(MailProbeError::Protocol),
            }
        }
        Err(MailProbeError::Protocol)
    }
    pub(super) async fn uid_move(
        &mut self,
        uid: u32,
        destination: &ImapMailboxName,
    ) -> Result<UidMoveOutcome, MailProbeError> {
        use io_imap::types::{mailbox::Mailbox, sequence::SequenceSet};
        // The identity's wire bytes are already validated for the selected
        // encoding. Passing the display string here would re-encode modified
        // UTF-7 names as UTF-8 and can move to a different mailbox.
        let mailbox: Mailbox<'static> = destination
            .wire_name()
            .to_vec()
            .try_into()
            .map_err(|_| MailProbeError::Protocol)?;
        let command = Command {
            tag: "h7".try_into().map_err(|_| MailProbeError::Protocol)?,
            body: CommandBody::Move {
                sequence_set: SequenceSet::from(
                    std::num::NonZeroU32::new(uid).ok_or(MailProbeError::Protocol)?,
                ),
                mailbox,
                uid: true,
            },
        };
        for fragment in CommandCodec::new().encode(&command) {
            match fragment {
                Fragment::Line { data } => {
                    if self.write(SensitiveBuffer(data)).await.is_err() {
                        return Ok(UidMoveOutcome::Unknown);
                    }
                }
                Fragment::Literal { data, mode } => {
                    if mode == LiteralMode::Sync && self.literal_continuation().await.is_err() {
                        return Ok(UidMoveOutcome::Unknown);
                    }
                    if self.write(SensitiveBuffer(data)).await.is_err() {
                        return Ok(UidMoveOutcome::Unknown);
                    }
                }
            }
        }
        let mut remaining = 32;
        match self
            .frame(GateMode::Mutation, Some("h7"), &mut remaining)
            .await
        {
            Ok(ReplyKind::Tagged(StatusKind::Ok)) => Ok(UidMoveOutcome::Accepted),
            // RFC 6851 permits a server to report NO/BAD after it has
            // partially applied a MOVE. Once the command was written, the
            // result is therefore ambiguous and must never be retried.
            Ok(ReplyKind::Tagged(StatusKind::No | StatusKind::Bad)) | Ok(_) | Err(_) => {
                Ok(UidMoveOutcome::Unknown)
            }
        }
    }
    pub(super) async fn select(
        &mut self,
        tag: &str,
        mailbox: &ImapMailboxName,
    ) -> Result<u32, MailProbeError> {
        let command = Command {
            tag: tag.try_into().map_err(|_| MailProbeError::Protocol)?,
            body: CommandBody::select(mailbox.wire_name()).map_err(|_| MailProbeError::Protocol)?,
        };
        for fragment in CommandCodec::new().encode(&command) {
            match fragment {
                Fragment::Line { data } => self.write(SensitiveBuffer(data)).await?,
                Fragment::Literal { data, mode } => {
                    if mode == LiteralMode::Sync {
                        self.literal_continuation().await?;
                    }
                    self.write(SensitiveBuffer(data)).await?;
                }
            }
        }
        let mut selection = Selection::default();
        for count in 0..=128 {
            match self.uid_frame(Mode::Select, tag).await? {
                Reply::Tagged(observation) => {
                    selection.observe(observation)?;
                    return selection.finish_read_write();
                }
                Reply::Status(observation) if count < 128 => selection.observe(observation)?,
                Reply::Data if count < 128 => {}
                _ => return Err(MailProbeError::Protocol),
            }
        }
        Err(MailProbeError::Protocol)
    }
    async fn uid_messages(
        &mut self,
        after: u32,
        through: u32,
    ) -> Result<Vec<ImapMessageMetadata>, MailProbeError> {
        if after > through {
            return Err(MailProbeError::Protocol);
        }
        let mut messages = Vec::new();
        if after == through {
            return Ok(messages);
        }
        self.uid_fetch(after + 1, through).await?;
        let mut statuses = 0;
        loop {
            match self.uid_frame(Mode::Fetch, "h7").await? {
                Reply::Tagged(Observation::None) => return Ok(messages),
                Reply::Status(Observation::None) if statuses < 128 => statuses += 1,
                Reply::Message(message) => {
                    if messages.len() == 64
                        || message.uid() <= after
                        || message.uid() > through
                        || messages
                            .iter()
                            .any(|existing: &ImapMessageMetadata| existing.uid() == message.uid())
                    {
                        return Err(MailProbeError::Protocol);
                    }
                    messages.push(*message);
                }
                _ => return Err(MailProbeError::Protocol),
            }
        }
    }
    pub(super) async fn uid_observation(
        &mut self,
        request: &ImapUidObservationRequest,
        encoding: MailboxEncoding,
    ) -> Result<ImapUidObservation, ImapUidPageError> {
        if request.mailbox().encoding() != encoding {
            return Err(MailProbeError::Protocol.into());
        }
        let (validity, next) = self.examine("h6", request.mailbox()).await?;
        ImapUidObservation::new(request.clone(), validity, next)
            .map_err(|_| MailProbeError::Protocol.into())
    }

    pub(super) async fn uid_page(
        &mut self,
        request: &ImapUidPageRequest,
        encoding: MailboxEncoding,
    ) -> Result<ImapUidPage, ImapUidPageError> {
        if request.mailbox().encoding() != encoding {
            return Err(MailProbeError::Protocol.into());
        }
        let (validity, next) = self.examine("h6", request.mailbox()).await?;
        if request
            .expected_uid_validity()
            .is_some_and(|expected| expected != validity)
        {
            return Err(ImapUidPageError::UidValidityChanged);
        }
        let ceiling = next - 1;
        if request.after_uid() > ceiling {
            return Err(ImapUidPageError::CursorInvalid);
        }
        let through = (u64::from(request.after_uid()) + 64).min(u64::from(ceiling)) as u32;
        let messages = self.uid_messages(request.after_uid(), through).await?;
        let (final_validity, final_next) = self.examine("h8", request.mailbox()).await?;
        if final_validity != validity {
            return Err(ImapUidPageError::UidValidityChanged);
        }
        if final_next < next {
            return Err(ImapUidPageError::CursorInvalid);
        }
        ImapUidPage::new(request.clone(), validity, next, messages)
            .map_err(|_| MailProbeError::Protocol.into())
    }

    pub(super) async fn uid_reconcile_page(
        &mut self,
        request: &ImapUidReconcileRequest,
        encoding: MailboxEncoding,
    ) -> Result<ImapUidReconcilePage, ImapUidPageError> {
        if request.mailbox().encoding() != encoding {
            return Err(MailProbeError::Protocol.into());
        }
        let (validity, next) = self.examine("h6", request.mailbox()).await?;
        if validity != request.uid_validity() {
            return Err(ImapUidPageError::UidValidityChanged);
        }
        if next < request.minimum_uid_next() {
            return Err(ImapUidPageError::CursorInvalid);
        }
        // The source range is fixed before connecting; UIDNEXT never widens it.
        let messages = self
            .uid_messages(request.after_uid(), request.through_uid())
            .await?;
        let (final_validity, final_next) = self.examine("h8", request.mailbox()).await?;
        if final_validity != validity {
            return Err(ImapUidPageError::UidValidityChanged);
        }
        if final_next < next {
            return Err(ImapUidPageError::CursorInvalid);
        }
        ImapUidReconcilePage::new(request.clone(), next, messages)
            .map_err(|_| MailProbeError::Protocol.into())
    }

    pub(super) async fn uid_recovery_page(
        &mut self,
        request: &ImapUidRecoveryRequest,
        encoding: MailboxEncoding,
    ) -> Result<ImapUidRecoveryPage, ImapUidPageError> {
        if request.mailbox().encoding() != encoding {
            return Err(MailProbeError::Protocol.into());
        }
        let (validity, next) = self.examine("h6", request.mailbox()).await?;
        if validity == request.previous_uid_validity() {
            return Err(ImapUidPageError::CursorInvalid);
        }
        // The old namespace's UIDNEXT is irrelevant after an epoch change.
        // Bind the first page to the newly observed validity, never to None.
        let first = ImapUidPageRequest::new(
            request.account_id(),
            request.endpoint_version(),
            request.mailbox().clone(),
            Some(validity),
            0,
        )
        .map_err(|_| MailProbeError::Protocol)?;
        let messages = self.uid_messages(0, (next - 1).min(64)).await?;
        let (final_validity, final_next) = self.examine("h8", request.mailbox()).await?;
        if final_validity != validity {
            return Err(ImapUidPageError::UidValidityChanged);
        }
        if final_next < next {
            return Err(ImapUidPageError::CursorInvalid);
        }
        let page = ImapUidPage::new(first, validity, next, messages)
            .map_err(|_| MailProbeError::Protocol)?;
        ImapUidRecoveryPage::new(request.clone(), page).map_err(|_| MailProbeError::Protocol.into())
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;

    #[test]
    fn sec03_move_preserves_modified_utf7_wire_mailbox() {
        use io_imap::types::{mailbox::Mailbox, sequence::SequenceSet};

        let wire = b"Archive/&U,BTFw-";
        let mailbox: ImapMailboxName =
            ImapMailboxName::from_wire(wire, MailboxEncoding::ModifiedUtf7).unwrap();
        let target: Mailbox<'static> = mailbox.wire_name().to_vec().try_into().unwrap();
        let command = Command {
            tag: "h7".try_into().unwrap(),
            body: CommandBody::Move {
                sequence_set: SequenceSet::from(std::num::NonZeroU32::new(17).unwrap()),
                mailbox: target,
                uid: true,
            },
        };
        let encoded = CommandCodec::new().encode(&command).dump();
        assert_eq!(encoded, b"h7 UID MOVE 17 Archive/&U,BTFw-\r\n");
    }

    #[tokio::test]
    async fn sec03_move_maps_postwrite_no_and_disconnect_to_unknown_without_retry() {
        for (reply, expected) in [
            (
                Some(b"h7 NO destination unavailable\r\n".as_slice()),
                UidMoveOutcome::Unknown,
            ),
            (None, UidMoveOutcome::Unknown),
        ] {
            let (client, mut server) = tokio::io::duplex(1024);
            let task = tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                let mut server = BufReader::new(&mut server);
                let mut command = Vec::new();
                server.read_until(b'\n', &mut command).await.unwrap();
                assert_eq!(command, b"h7 UID MOVE 17 Archive/&U,BTFw-\r\n");
                if let Some(reply) = reply {
                    server.get_mut().write_all(reply).await.unwrap();
                }
            });
            let mut protocol = Protocol {
                io: BufReader::new(client),
                remaining: 4096,
                written: 0,
            };
            let mailbox =
                ImapMailboxName::from_wire(b"Archive/&U,BTFw-", MailboxEncoding::ModifiedUtf7)
                    .unwrap();
            assert_eq!(protocol.uid_move(17, &mailbox).await.unwrap(), expected);
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn sec03_select_requires_read_write_and_binds_uidvalidity_before_move() {
        for (status, expected) in [
            (
                b"* OK [UIDVALIDITY 42] selected\r\n* OK [UIDNEXT 9] next\r\nh6 OK [READ-WRITE] selected\r\n".as_slice(),
                Ok::<u32, MailProbeError>(42),
            ),
            (
                b"* OK [UIDVALIDITY 42] selected\r\n* OK [UIDNEXT 9] next\r\nh6 OK [READ-ONLY] selected\r\n".as_slice(),
                Err(MailProbeError::Protocol),
            ),
        ] {
            let (client, mut server) = tokio::io::duplex(2048);
            let task = tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                let mut server = BufReader::new(&mut server);
                let mut command = Vec::new();
                server.read_until(b'\n', &mut command).await.unwrap();
                assert_eq!(command, b"h6 SELECT INBOX\r\n");
                server.get_mut().write_all(status).await.unwrap();
            });
            let mut protocol = Protocol {
                io: BufReader::new(client),
                remaining: 4096,
                written: 0,
            };
            let mailbox = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap();
            assert_eq!(protocol.select("h6", &mailbox).await, expected);
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn mail03_body_driver_uses_readonly_examine_and_peek_then_requires_ok() {
        let (client, server) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut server = BufReader::new(server);
            let mut line = String::new();
            server.read_line(&mut line).await.unwrap();
            assert_eq!(line, "h6 EXAMINE INBOX\r\n");
            server.get_mut().write_all(b"* 1 EXISTS\r\n* OK [UIDVALIDITY 42] stable\r\n* OK [UIDNEXT 10] next\r\nh6 OK [READ-ONLY] selected\r\n").await.unwrap();
            line.clear();
            server.read_line(&mut line).await.unwrap();
            assert_eq!(line, "h7 UID FETCH 9 (UID BODY.PEEK[])\r\n");
            server
                .get_mut()
                .write_all(b"* 77 FETCH (UID 9 BODY[] {5}\r\nhello)\r\nh7 OK fetched\r\n")
                .await
                .unwrap();
        });
        let request = hantong_contracts::message_body::MessageBodyRequest::new(
            "acct",
            hantong_contracts::uid_page::UidMailboxIdentity::new(
                "494e424f58",
                hantong_contracts::uid_page::UidMailboxEncoding::ModifiedUtf7,
            )
            .unwrap(),
            42,
            9,
            64,
        )
        .unwrap();
        let mut client = Protocol {
            io: BufReader::new(client),
            remaining: 4096,
            written: 0,
        };
        let body = client
            .body(&request, MailboxEncoding::ModifiedUtf7)
            .await
            .unwrap();
        assert_eq!(body.bytes(), b"hello");
        server.await.unwrap();
    }
}
