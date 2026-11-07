// Included in the coordinator so captured values never leave its private state.
struct ExistingSecretReview {
    id: [u8; 32],
    ticket: hantong_secrets::TerminalSecretTicket,
}
struct ExistingSecretCapture {
    ticket: hantong_secrets::TerminalSecretTicket,
    handle: hantong_secrets::TerminalSecretHandle,
}

impl State {
    fn cleanup_inactive_captures(&self, root: &Path) -> Result<(), ErrorCode> {
        let mut inner = self.inner.borrow_mut();
        let inactive = inner
            .sessions
            .iter()
            .filter(|(_, s)| terminal_state(&s.state))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let has_capture = inner.sessions.iter().any(|(id, s)| {
            inactive.contains(id) && (s.existing_review.is_some() || s.existing_capture.is_some())
        }) || inner.setup_reviews.values().any(|r| {
            r.ticket.is_some()
                && (inactive.contains(&r.intent_id) || !inner.sessions.contains_key(&r.intent_id))
        });
        if !has_capture {
            return Ok(());
        }
        let secrets =
            hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
        let discard = |ticket: &hantong_secrets::TerminalSecretTicket| match secrets
            .discard_terminal_secret(ticket, ticket.opaque_id())
        {
            Ok(()) | Err(hantong_secrets::SecretError::NotFound) => Ok(()),
            Err(_) => Err(ErrorCode::InternalError),
        };
        for id in &inactive {
            let s = inner.sessions.get_mut(id).ok_or(ErrorCode::InternalError)?;
            if let Some(r) = s.existing_review.as_ref() {
                discard(&r.ticket)?;
            }
            if let Some(c) = s.existing_capture.as_ref() {
                discard(&c.ticket)?;
            }
            s.existing_review.take();
            s.existing_capture.take();
        }
        let live_ids = inner.sessions.keys().copied().collect::<Vec<_>>();
        for r in inner.setup_reviews.values_mut() {
            if (inactive.contains(&r.intent_id) || !live_ids.contains(&r.intent_id))
                && let Some(ticket) = r.ticket.as_ref()
            {
                discard(ticket)?;
                r.ticket.take();
            }
        }
        Ok(())
    }
    pub(crate) fn check_setup_review_command(
        &self,
        store: &Store,
        peer: &ObservedPeer,
        command: &hantong_ipc_server::setup_dispatch::SetupCommand,
        capture: Option<(
            hantong_contracts::terminal_review::TerminalReviewKind,
            [u8; 32],
        )>,
    ) -> Result<(), ErrorCode> {
        use hantong_contracts::terminal_review::TerminalReviewCommand as C;
        let hantong_ipc_server::setup_dispatch::SetupCommand::TerminalReview(command) = command
        else {
            return Ok(());
        };
        let current = now()?;
        let mut inner = self.inner.borrow_mut();
        inner
            .setup_reviews
            .retain(|_, r| current < r.expires_at || r.ticket.is_some());
        match command {
            C::Get(get) => {
                if let Some(s) = inner
                    .sessions
                    .values()
                    .find(|s| s.matches_setup(get.kind, get.request_id))
                {
                    if inner.setup_reviews.len() >= 128 {
                        return Err(ErrorCode::QuotaExceeded);
                    }
                    if !matches!(s.state.as_str(), "awaiting_setup" | "provider_ready") {
                        return Err(ErrorCode::Conflict);
                    }
                    s.validate_setup_current(store, peer, current)?;
                }
            }
            C::SecretSubmit(submit) => {
                if let Some(bound) = inner.setup_reviews.get(&submit.ticket) {
                    let s = inner
                        .sessions
                        .get(&bound.intent_id)
                        .ok_or(ErrorCode::Conflict)?;
                    if !matches!(s.state.as_str(), "awaiting_setup" | "provider_ready") {
                        return Err(ErrorCode::Conflict);
                    }
                    s.validate_setup_current(store, peer, current)?;
                } else if capture.is_some_and(|(kind, id)| {
                    inner.sessions.values().any(|s| s.matches_setup(kind, id))
                }) {
                    // A legacy ticket issued before this app intent is not fresh proof.
                    return Err(ErrorCode::Conflict);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn bind_setup_review_receipt(
        &self,
        result: &hantong_ipc_server::setup_dispatch::SetupReceipt,
    ) -> Result<(), ErrorCode> {
        let hantong_ipc_server::setup_dispatch::SetupReceipt::TerminalReview(receipt) = result
        else {
            return Ok(());
        };
        if receipt.state != "pending" {
            return Ok(());
        }
        let Some(ticket) = receipt.ticket else {
            return Ok(());
        };
        let mut inner = self.inner.borrow_mut();
        if let Some((&id, _)) = inner
            .sessions
            .iter()
            .find(|(_, s)| s.matches_setup(receipt.kind, receipt.request_id))
        {
            inner.setup_reviews.insert(
                ticket,
                BoundSetupReview {
                    intent_id: id,
                    expires_at: receipt.expires_at_utc_ms,
                    ticket: receipt
                        .secret_ticket_opaque
                        .as_ref()
                        .map(|bytes| hantong_secrets::TerminalSecretTicket::from_opaque(bytes))
                        .transpose()
                        .map_err(|_| ErrorCode::InternalError)?,
                },
            );
        }
        Ok(())
    }

    fn cleanup_existing_capture(&self, root: &Path, id: [u8; 32]) -> Result<(), ErrorCode> {
        let mut inner = self.inner.borrow_mut();
        let s = inner.sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
        let ticket = s
            .existing_review
            .as_ref()
            .map(|r| &r.ticket)
            .or_else(|| s.existing_capture.as_ref().map(|r| &r.ticket));
        if let Some(ticket) = ticket {
            let secrets =
                hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
            match secrets.discard_terminal_secret(ticket, ticket.opaque_id()) {
                Ok(()) | Err(hantong_secrets::SecretError::NotFound) => {}
                Err(_) => return Err(ErrorCode::InternalError),
            }
        }
        s.existing_review.take();
        s.existing_capture.take();
        Ok(())
    }
    fn prepare_existing(
        &self,
        store: &Store,
        server: [u8; 72],
        caller: &RegistrationRecord,
        peer: ObservedPeer,
        account: String,
    ) -> Result<AccountConnectReceipt, ErrorCode> {
        let mut inner = self.inner.borrow_mut();
        if inner.sessions.len() >= MAX_SESSIONS {
            return Err(ErrorCode::QuotaExceeded);
        }
        if inner.sessions.values().any(|s| {
            s.provider == AccountConnectProvider::ImapSmtp
                && s.account_id.as_ref() == Some(&account)
                && !terminal_state(&s.state)
        }) {
            return Err(ErrorCode::Conflict);
        }
        let id = hantong_platform::random_nonce()?;
        let intent = ExistingConnectIntent::new(
            store,
            caller,
            id,
            hantong_platform::sha256(&server)?,
            peer.clone(),
            account.clone(),
            now()?,
        )?;
        let session = Session {
            owner: caller.registration_id,
            client_public_key: caller.public_key,
            profile_epoch: caller.profile_epoch,
            registration_version: caller.version,
            baseline_hash: store.full_mail_baseline_hash(caller.registration_id)?,
            peer,
            provider: AccountConnectProvider::ImapSmtp,
            intent: None,
            authorization: None,
            client_secret: None,
            pending: None,
            staged_decision: None,
            state: "awaiting_setup".into(),
            account_id: Some(account),
            login_url: None,
            error_code: None,
            expires_at_utc_ms: intent.expires_at_utc_ms(),
            selection_is_new: false,
            generic: None,
            generic_queued: false,
            existing: Some(intent),
            existing_delta: None,
            existing_review: None,
            existing_capture: None,
        };
        let result = receipt(id, &session);
        inner.sessions.insert(id, session);
        Ok(result)
    }

    /// Only the already image-verified Core foreground transport calls this.
    /// The actual Default registration is reloaded; no UI-supplied owner exists.
    pub(crate) fn existing_review(
        &self,
        store: &Store,
        root: &Path,
        server: [u8; 72],
        peer: &ObservedPeer,
        command: &hantong_ipc_server::setup_dispatch::SetupCommand,
    ) -> Option<Result<hantong_ipc_server::setup_dispatch::SetupReceipt, ErrorCode>> {
        use hantong_contracts::terminal_review::{
            TerminalReviewCommand as C, TerminalReviewKind as K,
        };
        let hantong_ipc_server::setup_dispatch::SetupCommand::TerminalReview(command) = command
        else {
            return None;
        };
        let intent_id = match command {
            C::Get(get) if get.kind == K::AccountConnectSetup => get.request_id,
            C::Cancel(cancel) if cancel.get.kind == K::AccountConnectSetup => cancel.get.request_id,
            C::Confirm(confirm) => self
                .inner
                .borrow()
                .sessions
                .iter()
                .find(|(_, s)| {
                    s.existing_review
                        .as_ref()
                        .is_some_and(|r| r.id == confirm.ticket)
                })
                .map(|(id, _)| *id)?,
            C::SecretSubmit(submit) => self
                .inner
                .borrow()
                .sessions
                .iter()
                .find(|(_, s)| {
                    s.existing_review
                        .as_ref()
                        .is_some_and(|r| r.id == submit.ticket)
                })
                .map(|(id, _)| *id)?,
            _ => return None,
        };
        Some(
            self.existing_review_command(store, root, server, peer, intent_id, command)
                .map(hantong_ipc_server::setup_dispatch::SetupReceipt::TerminalReview),
        )
    }

    fn existing_review_command(
        &self,
        store: &Store,
        root: &Path,
        server: [u8; 72],
        peer: &ObservedPeer,
        id: [u8; 32],
        command: &hantong_contracts::terminal_review::TerminalReviewCommand,
    ) -> Result<hantong_contracts::terminal_review::TerminalReviewReceipt, ErrorCode> {
        use hantong_contracts::terminal_review::{
            TerminalReviewCommand as C, TerminalReviewField, TerminalReviewKind as K,
            TerminalReviewReceipt,
        };
        let caller = store
            .load_default_cli_registration()?
            .ok_or(ErrorCode::ClientNotRegistered)?;
        validate_owner(&caller)?;
        self.session_checked(id, &caller, peer)?;
        let mut inner = self.inner.borrow_mut();
        let queue_full = inner.queue.len() >= MAX_QUEUE;
        let s = inner.sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
        if !matches!(s.state.as_str(), "awaiting_setup" | "provider_ready") {
            return Err(ErrorCode::Conflict);
        }
        let intent = s.existing.as_ref().ok_or(ErrorCode::Conflict)?;
        intent.validate_current(store, peer, now()?)?;
        if intent.server_pin() != hantong_platform::sha256(&server)? {
            return Err(ErrorCode::ScopeDenied);
        }
        let binding = intent.review_binding();
        let mut result = TerminalReviewReceipt {
            kind: K::AccountConnectSetup,
            request_id: id,
            ticket: None,
            snapshot_hash: binding.payload_hash,
            owner_registration_id: s.owner,
            profile_epoch: s.profile_epoch,
            expires_at_utc_ms: s.expires_at_utc_ms,
            state: "pending".into(),
            secret_ticket_opaque: None,
            fields: vec![
                TerminalReviewField::new("account".into(), binding.account_id.clone())?,
                TerminalReviewField::new(
                    "endpoint".into(),
                    serde_json::to_string(intent.endpoint())
                        .map_err(|_| ErrorCode::InternalError)?,
                )?,
            ],
        };
        match command {
            C::Get(get) => {
                if get
                    .account_id
                    .as_ref()
                    .is_some_and(|a| a != &binding.account_id)
                    || (get.payload_hash != [0; 32] && get.payload_hash != binding.payload_hash)
                {
                    return Err(ErrorCode::Conflict);
                }
                if s.existing_review.is_some() {
                    return Err(ErrorCode::Conflict);
                }
                let endpoint =
                    serde_json::to_vec(intent.endpoint()).map_err(|_| ErrorCode::InternalError)?;
                let mut canonical = Vec::new();
                for v in [
                    b"terminal-endpoint-v1".as_slice(),
                    b"terminal-imap-v1".as_slice(),
                    binding.account_id.as_bytes(),
                    endpoint.as_slice(),
                ] {
                    canonical.extend_from_slice(&(v.len() as u32).to_le_bytes());
                    canonical.extend_from_slice(v);
                }
                let (user, logon) = match peer {
                    ObservedPeer::Windows(p) => (
                        hantong_platform::sha256(p.user_sid().as_bytes())?,
                        hantong_platform::sha256(p.logon_sid().as_bytes())?,
                    ),
                    ObservedPeer::Unix(p) => (
                        hantong_platform::sha256(p.effective_uid.to_string().as_bytes())?,
                        hantong_platform::sha256(p.session_id.to_string().as_bytes())?,
                    ),
                };
                let secret_binding = hantong_secrets::TerminalSecretBinding::new(
                    *b"terminal-imap-v1",
                    id,
                    binding.payload_hash,
                    hantong_platform::sha256(binding.account_id.as_bytes())?,
                    hantong_platform::sha256(&canonical)?,
                    binding.endpoint_settings_version,
                    user,
                    logon,
                    intent.server_pin(),
                    binding.expires_at_utc_ms,
                )
                .map_err(|_| ErrorCode::InvalidInput)?;
                let ticket = hantong_secrets::SecretStore::open(root)
                    .map_err(|_| ErrorCode::InternalError)?
                    .issue_terminal_secret_ticket(secret_binding)
                    .map_err(|_| ErrorCode::InternalError)?;
                let review_id = hantong_platform::random_nonce()?;
                result.ticket = Some(review_id);
                result.secret_ticket_opaque = Some(ticket.to_opaque().to_vec());
                s.existing_review = Some(ExistingSecretReview {
                    id: review_id,
                    ticket,
                });
            }
            C::Confirm(_) => return Err(ErrorCode::ApprovalRequired),
            C::Cancel(cancel) => {
                if cancel
                    .get
                    .account_id
                    .as_ref()
                    .is_some_and(|a| a != &binding.account_id)
                    || (cancel.get.payload_hash != [0; 32]
                        && cancel.get.payload_hash != binding.payload_hash)
                    || s.existing_review
                        .as_ref()
                        .is_none_or(|r| r.id != cancel.ticket)
                {
                    return Err(ErrorCode::Conflict);
                }
                if let Some(review) = s.existing_review.as_ref() {
                    let secrets = hantong_secrets::SecretStore::open(root)
                        .map_err(|_| ErrorCode::InternalError)?;
                    // Cancellation removes only this ticket's optional encrypted capture.
                    match secrets.cancel_terminal_secret(&review.ticket, review.ticket.opaque_id())
                    {
                        Ok(()) | Err(hantong_secrets::SecretError::NotFound) => {}
                        Err(_) => return Err(ErrorCode::InternalError),
                    }
                }
                s.existing_review.take();
                s.existing.take();
                s.state = "cancelled".into();
                s.error_code = Some(ErrorCode::ApprovalDenied);
                result.state = "cancelled".into();
            }
            C::SecretSubmit(submit) => {
                if queue_full {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let review = s.existing_review.as_ref().ok_or(ErrorCode::Conflict)?;
                if review.id != submit.ticket || submit.snapshot_hash != binding.payload_hash {
                    return Err(ErrorCode::Conflict);
                }
                let decoded = hantong_secrets::TerminalSecretTicket::from_opaque(
                    &submit.secret_ticket_opaque,
                )
                .map_err(|_| ErrorCode::InvalidInput)?;
                if decoded.opaque_id() != review.ticket.opaque_id()
                    || !decoded.binding().matches(&review.ticket.binding())
                {
                    return Err(ErrorCode::ScopeDenied);
                }
                let secrets = hantong_secrets::SecretStore::open(root)
                    .map_err(|_| ErrorCode::InternalError)?;
                let handle = secrets
                    .terminal_secret_handle_from_opaque(&review.ticket, submit.staged_handle)
                    .map_err(|_| ErrorCode::ScopeDenied)?;
                let review = s.existing_review.take().ok_or(ErrorCode::Conflict)?;
                s.existing_capture = Some(ExistingSecretCapture {
                    ticket: review.ticket,
                    handle,
                });
                s.state = "queued".into();
                inner.queue.push_back(id);
                self.notify.notify_one();
                result.state = "provisioning".into();
            }
        }
        result.validate()?;
        Ok(result)
    }
}

impl Worker {
    async fn run_existing_capture(
        &self,
        store: &Store,
        root: &Path,
        smtp: &crate::setup_smtp::Sessions,
        id: [u8; 32],
    ) -> bool {
        let work = {
            let mut inner = self.inner.borrow_mut();
            let Some(s) = inner.sessions.get_mut(&id) else {
                return false;
            };
            let Some(capture) = s.existing_capture.take() else {
                return false;
            };
            s.existing.take().map(|intent| (intent, capture))
        };
        let Some((intent, capture)) = work else {
            self.fail_session(id, ErrorCode::Conflict);
            return true;
        };
        let result = async {
            intent.validate_current(store, intent.peer(), now()?)?;
            let secrets =
                hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
            let binding = capture.ticket.binding();
            let witness = secrets
                .consume_terminal_imap_probe(
                    capture.ticket,
                    capture.handle,
                    binding,
                    intent.account_id(),
                    intent.endpoint(),
                )
                .await
                .map_err(|e| match e {
                    hantong_secrets::MailProbeError::Authentication => ErrorCode::AuthRequired,
                    hantong_secrets::MailProbeError::Connection
                    | hantong_secrets::MailProbeError::Timeout
                    | hantong_secrets::MailProbeError::Tls => ErrorCode::Offline,
                    _ => ErrorCode::InternalError,
                })?;
            intent.verify(store, root, smtp, witness, now()?).await
        }
        .await;
        match result {
            Ok(delta) => {
                let mut inner = self.inner.borrow_mut();
                if let Some(s) = inner.sessions.get_mut(&id)
                    && s.state == "queued"
                    && now().is_ok_and(|n| n < s.expires_at_utc_ms)
                {
                    s.existing_delta = Some(delta);
                    s.state = "provider_ready".into();
                }
            }
            Err(e) => self.fail_session(id, e),
        }
        true
    }
}
