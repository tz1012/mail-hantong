//! A single private authenticated metadata operation. No session escapes.
use super::*;
use crate::imap_probe::uid_page_gate::{Mode, Observation, Reply, UidPageGate};
use io_imap::{
    codec::{ResponseCodec, decode::Decoder, encode::Fragment},
    types::{core::LiteralMode, fetch::MessageDataItemName, response::Response},
};

#[derive(Default)]
struct Selection {
    validity: Option<u32>,
    next: Option<u32>,
    read_only: bool,
}
impl Selection {
    fn observe(&mut self, observation: Observation) -> Result<(), MailProbeError> {
        match observation {
            Observation::None => {}
            Observation::UidValidity(n) if self.validity.is_none() => self.validity = Some(n),
            Observation::UidNext(n) if self.next.is_none() => self.next = Some(n),
            Observation::ReadOnly if !self.read_only => self.read_only = true,
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
}

impl<S: AsyncRead + AsyncWrite + Unpin> Protocol<S> {
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
        request: &ImapUidPageRequest,
    ) -> Result<(u32, u32), MailProbeError> {
        let command = Command {
            tag: tag.try_into().map_err(|_| MailProbeError::Protocol)?,
            body: CommandBody::examine(request.mailbox().wire_name())
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
    pub(super) async fn uid_page(
        &mut self,
        request: &ImapUidPageRequest,
        encoding: MailboxEncoding,
    ) -> Result<ImapUidPage, ImapUidPageError> {
        if request.mailbox().encoding() != encoding {
            return Err(MailProbeError::Protocol.into());
        }
        let (validity, next) = self.examine("h6", request).await?;
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
        let mut messages = Vec::new();
        if through > request.after_uid() {
            self.uid_fetch(request.after_uid() + 1, through).await?;
            let mut statuses = 0;
            loop {
                match self.uid_frame(Mode::Fetch, "h7").await? {
                    Reply::Tagged(Observation::None) => break,
                    Reply::Status(Observation::None) if statuses < 128 => statuses += 1,
                    Reply::Message(message) => {
                        if messages.len() == 64
                            || message.uid() <= request.after_uid()
                            || message.uid() > through
                            || messages.iter().any(
                                |existing: &hantong_domain::imap_sync::ImapMessageMetadata| {
                                    existing.uid() == message.uid()
                                },
                            )
                        {
                            return Err(MailProbeError::Protocol.into());
                        }
                        messages.push(*message);
                    }
                    _ => return Err(MailProbeError::Protocol.into()),
                }
            }
        }
        let (final_validity, final_next) = self.examine("h8", request).await?;
        if final_validity != validity {
            return Err(ImapUidPageError::UidValidityChanged);
        }
        if final_next < next {
            return Err(ImapUidPageError::CursorInvalid);
        }
        ImapUidPage::new(request.clone(), validity, next, messages)
            .map_err(|_| MailProbeError::Protocol.into())
    }
}
