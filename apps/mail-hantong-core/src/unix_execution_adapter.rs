//! Unix authenticated execution adapter.
//!
//! The Unix transport deliberately uses the same application coordinator as
//! Windows.  Only the connection identity is platform-specific; Store,
//! policy, audit, folder refresh, UID and message-body handling stay shared.

use hantong_application::{
    AccountsListRequest, AuditHandoff, ExecutionContext, ReadResult, ToolRequest,
    render_account_list, render_folder_page, render_folder_refresh_receipt,
    render_initial_sync_current, render_initial_sync_plan, render_initial_sync_receipt,
    render_uid_cursor, render_uid_inspection, render_uid_receipt, render_uid_reconcile_receipt,
    render_uid_recovery_receipt, validated_uid_mailbox,
};
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditTerminal},
    auth::AppReadTool,
    folders::{FolderListRequest, FolderRefreshRequest, FolderRefreshStatusRequest},
    initial_sync::{InitialSyncPlanRequest, InitialSyncRequest},
    uid_page::{UidCursorSnapshot, UidPageRequest, UidPageStatusRequest},
    uid_reconcile::UidReconcileRequest,
    uid_recovery::UidRecoveryRequest,
};
use hantong_ipc_server::unix_auth::{AuthenticatedUnixConnection, RegisteredUnixClient};
use hantong_platform::random_nonce;
use hantong_policy::ReadPolicy;
use hantong_store::{RegistrationRecord, RegistrationState, Store};
use serde_json::Value;

pub(crate) struct UnixExecutionAdapter<'a> {
    connection: &'a mut AuthenticatedUnixConnection,
    store: &'a Store,
    policy: &'a mut ReadPolicy,
    folder_worker: &'a crate::folder_worker::FolderWorkerHandle<'a>,
    uid_worker: Option<&'a crate::uid_worker::UidWorkerHandle<'a>>,
    google_worker: Option<&'a crate::google_worker::GoogleHandle<'a>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_with_google(
    store: &Store,
    policy: &mut ReadPolicy,
    folder_worker: &crate::folder_worker::FolderWorkerHandle<'_>,
    uid_worker: Option<&crate::uid_worker::UidWorkerHandle<'_>>,
    google_worker: &crate::google_worker::GoogleHandle<'_>,
    connection: &mut AuthenticatedUnixConnection,
    request: &hantong_ipc_server::RpcRequest,
) -> Result<Value, ErrorCode> {
    let mut adapter = UnixExecutionAdapter {
        connection,
        store,
        policy,
        folder_worker,
        uid_worker,
        google_worker: Some(google_worker),
    };
    hantong_application::execute_tool(&mut adapter, &request.method, &request.params)
}

fn registered_client(record: RegistrationRecord) -> Result<RegisteredUnixClient, ErrorCode> {
    RegisteredUnixClient::from_registration_store(
        record.registration_id,
        record.profile_epoch,
        record.public_key,
        record.scope,
        record.state == RegistrationState::Active,
    )
    .map_err(|_| ErrorCode::InternalError)
}

fn durable_admission<T>(outcome: Result<T, ErrorCode>) -> Result<T, AuditHandoff<Value>> {
    outcome.map_err(AuditHandoff::error)
}

impl UnixExecutionAdapter<'_> {
    fn current_registration(
        &mut self,
    ) -> Result<(RegistrationRecord, RegisteredUnixClient), ErrorCode> {
        let current = self
            .store
            .load_registration(*self.connection.registration_id())?
            .ok_or(ErrorCode::ClientNotRegistered)
            .and_then(|record| registered_client(record.clone()).map(|client| (record, client)));
        if current.is_err() {
            let _ = self.connection.revalidate_registration(None);
        }
        current
    }
}

impl ExecutionContext for UnixExecutionAdapter<'_> {
    type Output = Value;

    fn mail_events(
        &mut self,
        accounts: &[String],
        action: &hantong_application::MailEventsAction,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        crate::mail_events::execute(
            self.store,
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            accounts,
            action,
        )
    }

    fn google_mutation(
        &mut self,
        account: &str,
        request: Option<&hantong_contracts::google_mutation::MutationPayload>,
        id: Option<[u8; 32]>,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        let receipt = match (request, id) {
            (Some(request), None) => self.store.request_google_mutation(
                *self.connection.registration_id(),
                self.connection.profile_epoch(),
                request,
            )?,
            (None, Some(id)) => self.store.google_mutation_receipt(
                *self.connection.registration_id(),
                self.connection.profile_epoch(),
                account,
                id,
            )?,
            _ => return Err(ErrorCode::InvalidInput),
        };
        ReadResult::new(crate::google_mutation::render(&receipt), None)
    }

    fn read_google(
        &mut self,
        request: &hantong_contracts::google_mail::GoogleMailRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let (value, count) = crate::google_worker::read(
            self.store,
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            request,
        )?;
        ReadResult::new(value, Some(count))
    }

    fn request_google_sync(
        &mut self,
        audit: [u8; 32],
        request: &hantong_contracts::google::GoogleSyncRequest,
    ) -> Result<AuditHandoff<Value>, ErrorCode> {
        let worker = self.google_worker.ok_or(ErrorCode::CoreUnavailable)?;
        Ok(
            match worker.request(
                audit,
                *self.connection.registration_id(),
                self.connection.profile_epoch(),
                request,
            )? {
                AuditHandoff::Response(receipt) => AuditHandoff::response(serde_json::json!({
                    "schema_version": 1,
                    "ok": true,
                    "provider": "gmail",
                    "read_only": true,
                    "account_id": request.account_id,
                    "sync": crate::google_worker::receipt(&receipt),
                })),
                AuditHandoff::Error(error) => AuditHandoff::error(error),
            },
        )
    }

    fn revalidate(&mut self) -> Result<(), ErrorCode> {
        let (_, current) = self.current_registration()?;
        self.connection.revalidate_registration(Some(&current))
    }

    fn begin_audit(&mut self, request: &ToolRequest) -> Result<[u8; 32], ErrorCode> {
        let id = random_nonce()?;
        let begin = AuditBegin::new(
            id,
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            request.tool(),
            request.account_ids().to_vec(),
        )?;
        self.store.begin_audit(&begin)?;
        Ok(id)
    }

    fn check_scope(&mut self, request: &ToolRequest) -> Result<(), ErrorCode> {
        let (_, current) = self.current_registration()?;
        let app_read = match request {
            ToolRequest::DisplaySettingsGet { .. } => Some(AppReadTool::DisplaySettingsGet),
            ToolRequest::DiagnosticsSnapshot { .. } => Some(AppReadTool::DiagnosticsSnapshot),
            _ => None,
        };
        if let Some(tool) = app_read {
            self.connection.check_app_read_scope(Some(&current), tool)?;
            return Ok(());
        }
        if request.account_ids().is_empty() {
            return self.connection.revalidate_registration(Some(&current));
        }
        for account in request.account_ids() {
            self.connection
                .check_account_scope(Some(&current), account, request.tool_name())?;
        }
        Ok(())
    }

    fn check_policy(&mut self, request: &ToolRequest) -> Result<(), ErrorCode> {
        let (record, current) = self.current_registration()?;
        self.connection.revalidate_registration(Some(&current))?;
        if record.client_kind == hantong_contracts::client::ClientKind::Mcp {
            self.store.check_mcp_ai_consent(
                record.registration_id,
                record.profile_epoch,
                request.account_ids(),
            )?;
        } else if record.client_kind == hantong_contracts::client::ClientKind::Ai {
            let provider = if record.display_label == hantong_store::DEFAULT_AI_CODEX_LABEL {
                hantong_contracts::registration::AiProvider::Codex
            } else if record.display_label == hantong_store::DEFAULT_AI_CLAUDE_LABEL {
                hantong_contracts::registration::AiProvider::Claude
            } else {
                return Err(ErrorCode::PolicyBlocked);
            };
            self.store.check_ai_consent(
                provider,
                record.registration_id,
                record.profile_epoch,
                request.account_ids(),
            )?;
        }
        self.policy.check(
            record.registration_id,
            record.read_only,
            record.requests_per_minute,
            request.tool(),
            std::time::Instant::now(),
        )?;
        if request.tool().is_global_read()
            && record.client_kind != hantong_contracts::client::ClientKind::Cli
        {
            return Err(ErrorCode::PolicyBlocked);
        }
        Ok(())
    }

    fn read_display_settings(&mut self) -> Result<ReadResult<Self::Output>, ErrorCode> {
        let language = self
            .store
            .load_config()?
            .map(|config| match config.cli.language {
                hantong_domain::config::Language::En => "en",
                hantong_domain::config::Language::Ko => "ko",
            });
        ReadResult::new(
            serde_json::json!({"schema_version":1,"ok":true,"cli_language":language}),
            None,
        )
    }

    fn read_diagnostic_snapshot(
        &mut self,
        audit_id: [u8; 32],
        request: &hantong_contracts::diagnostic_snapshot::DiagnosticSnapshotRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        use hantong_contracts::diagnostic_aggregate::{
            DiagnosticAggregateOwner, DiagnosticAggregateQuery,
        };
        let owner = DiagnosticAggregateOwner::new(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            Some(audit_id),
        )?;
        let query = DiagnosticAggregateQuery::new(
            owner,
            request.from_utc_ms(),
            request.to_utc_ms(),
            request.max_executions(),
            request.sections(),
        )?;
        let aggregate = self.store.collect_diagnostic_aggregate(&query)?;
        let snapshot = hantong_application::DiagnosticSnapshot::from_aggregate(aggregate)
            .map_err(|_| ErrorCode::InternalError)?;
        let snapshot_json = snapshot
            .preview_json()
            .map_err(|_| ErrorCode::InternalError)?;
        ReadResult::new(
            serde_json::json!({"schema_version":1,"ok":true,"snapshot_json":snapshot_json}),
            None,
        )
    }

    fn read_accounts(
        &mut self,
        request: &AccountsListRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        render_account_list(
            self.store
                .list_account_summaries(request.account_ids(), request.include_capabilities())?,
        )
    }

    fn read_folders(
        &mut self,
        request: &FolderListRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let catalog = self
            .store
            .load_imap_mailbox_snapshot(request.account_id())?
            .ok_or(ErrorCode::NotFound)?;
        render_folder_page(
            request,
            catalog.generation(),
            catalog.account_version(),
            catalog.endpoint_version(),
            catalog.stored_at_utc_ms(),
            catalog.snapshot(),
            catalog.binding_current(),
        )
    }

    fn read_folder_refresh_status(
        &mut self,
        request: &FolderRefreshStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self
            .store
            .load_folder_refresh_receipt(*self.connection.registration_id(), request)?;
        ReadResult::new(render_folder_refresh_receipt(&receipt), None)
    }

    fn request_folder_refresh(
        &mut self,
        audit: [u8; 32],
        request: &FolderRefreshRequest,
    ) -> Result<AuditHandoff<Value>, ErrorCode> {
        let admission = match durable_admission(self.folder_worker.admit(audit, request)) {
            Ok(value) => value,
            Err(handoff) => return Ok(handoff),
        };
        use hantong_store::folder_refresh::FolderRefreshAdmission;
        match admission {
            FolderRefreshAdmission::Admitted(r)
            | FolderRefreshAdmission::Existing(r)
            | FolderRefreshAdmission::Failed(r) => {
                Ok(AuditHandoff::response(render_folder_refresh_receipt(&r)))
            }
            FolderRefreshAdmission::Rejected(e) => Ok(AuditHandoff::error(e)),
        }
    }

    fn request_imap_trash(
        &mut self,
        request: &hantong_contracts::imap_trash::ImapTrashRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self.store.request_imap_trash(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            request,
        )?;
        ReadResult::new(crate::imap_trash::render(&receipt), None)
    }

    fn read_imap_trash_status(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self.store.status_imap_trash(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            review_id,
        )?;
        if receipt.account_id != account {
            return Err(ErrorCode::ScopeDenied);
        }
        ReadResult::new(crate::imap_trash::render(&receipt), None)
    }

    fn request_draft_create(
        &mut self,
        request: &hantong_contracts::google_write::ComposeSnapshot,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let id = self.store.create_mail_compose(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            request,
        )?;
        ReadResult::new(
            serde_json::json!({
                "schema_version": 1,
                "ok": true,
                "account_id": request.account_id(),
                "draft_id": crate::send_mail::hex(&id),
                "version": 1,
                "payload_hash": crate::send_mail::hex(request.payload_hash()),
            }),
            Some(1),
        )
    }

    fn request_send_review(
        &mut self,
        request: &hantong_contracts::mail_send::SendReviewRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self.store.request_mail_send_review(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            request,
        )?;
        ReadResult::new(crate::send_mail::render(&receipt), None)
    }

    fn prepare_send_review(
        &mut self,
        request: &hantong_contracts::mail_send::SendReviewRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self.store.request_mail_send_review(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            request,
        )?;
        let value = if receipt.state == "pending" {
            let pending = self.store.pending_mail_send(receipt.review_id)?;
            crate::send_mail::render_with_preview(&receipt, Some(&pending))?
        } else {
            crate::send_mail::render_with_preview(&receipt, None)?
        };
        ReadResult::new(value, None)
    }

    fn cancel_personal_send(
        &mut self,
        request: &hantong_contracts::mail_send::SendReviewStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        self.store
            .deny_mail_send(request.review_id, ErrorCode::ApprovalDenied)?;
        let receipt = self.store.mail_send_receipt(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            &request.account_id,
            request.review_id,
        )?;
        ReadResult::new(crate::send_mail::render(&receipt), None)
    }

    fn read_send_review(
        &mut self,
        request: &hantong_contracts::mail_send::SendReviewStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self.store.mail_send_receipt(
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            &request.account_id,
            request.review_id,
        )?;
        ReadResult::new(crate::send_mail::render(&receipt), None)
    }

    fn read_message_locator(
        &mut self,
        account: &str,
        id: &str,
        max: usize,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let located = self.store.locate_message_body(account, id, max)?;
        ReadResult::new(
            serde_json::json!({
                "schema_version":1,"ok":true,"account_id":account,"internal_message_id":id,
                "mailbox_wire_hex":located.mailbox().wire_hex(),"mailbox_encoding":located.mailbox().encoding().as_str(),
                "uidvalidity":located.uidvalidity(),"uid":located.uid()
            }),
            Some(1),
        )
    }

    fn read_message_body(
        &mut self,
        request: &hantong_contracts::message_body::MessageBodyRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let cached = self.store.cached_message_body(request)?;
        let (state, text, truncated, headers) = if let Some(body) = cached {
            let mut text = hantong_contracts::message_body::render_message_text_with_charset(
                body.bytes(),
                hantong_platform::mail_charset::decode_mail_charset,
            )?;
            let truncated = text.len() > 65_536;
            if truncated {
                let mut end = 65_536;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
            }
            let headers = hantong_contracts::message_body::message_envelope(body.bytes())
                .ok()
                .flatten()
                .map(|value| value.to_json());
            ("ready", Some(text), truncated, headers)
        } else {
            self.uid_worker
                .ok_or(ErrorCode::CoreUnavailable)?
                .request_body(
                    *self.connection.registration_id(),
                    self.connection.profile_epoch(),
                    request,
                )?;
            ("pending", None, false, None)
        };
        ReadResult::new(
            serde_json::json!({
                "schema_version":1,"ok":true,"state":state,"account_id":request.account_id(),
                "mailbox_wire_hex":request.mailbox().wire_hex(),"mailbox_encoding":request.mailbox().encoding().as_str(),
                "uidvalidity":request.uidvalidity(),"uid":request.uid(),"text":text,"truncated":truncated,"headers":headers
            }),
            if state == "ready" { Some(1) } else { None },
        )
    }

    fn read_message_search(
        &mut self,
        request: &hantong_contracts::message_search::ImapMetadataSearchRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        hantong_application::render_imap_metadata_search(&self.store.search_imap_metadata(request)?)
    }

    fn read_uid_cursor(
        &mut self,
        request: &hantong_contracts::uid_page::UidCursorRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let mailbox = validated_uid_mailbox(request.mailbox())?;
        let catalog = self
            .store
            .load_imap_mailbox_snapshot(request.account_id())?
            .ok_or(ErrorCode::NotFound)?;
        if !catalog.snapshot().mailboxes().iter().any(|m| {
            m.selectable()
                && m.name().encoding() == mailbox.encoding()
                && m.name().wire_name() == mailbox.wire_name()
        }) {
            return Err(ErrorCode::NotFound);
        }
        let stored = self
            .store
            .load_imap_uid_cursor(request.account_id(), &mailbox)?;
        let (version, covered, current) = stored
            .map(|c| (c.version(), c.covered_through_uid(), c.binding_current()))
            .unwrap_or((0, 0, true));
        let snapshot = UidCursorSnapshot::new(
            request.account_id(),
            request.mailbox().clone(),
            catalog.generation(),
            version,
            covered,
            current && catalog.binding_current(),
        )
        .map_err(|_| ErrorCode::InternalError)?;
        ReadResult::new(render_uid_cursor(&snapshot), None)
    }

    fn read_uid_inspection(
        &mut self,
        request: &hantong_contracts::uid_page::UidCursorRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        ReadResult::new(
            render_uid_inspection(&self.store.inspect_imap_uid(request)?),
            None,
        )
    }

    fn read_uid_page_status(
        &mut self,
        request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        ReadResult::new(
            render_uid_receipt(
                &self
                    .store
                    .load_uid_page_receipt(*self.connection.registration_id(), request)?,
            ),
            None,
        )
    }

    fn request_uid_page(
        &mut self,
        id: [u8; 32],
        request: &UidPageRequest,
    ) -> Result<AuditHandoff<Value>, ErrorCode> {
        use hantong_store::uid_page_jobs::UidPageAdmission;
        let worker = self.uid_worker.ok_or(ErrorCode::CoreUnavailable)?;
        let admission = match durable_admission(worker.admit(id, request)) {
            Ok(value) => value,
            Err(handoff) => return Ok(handoff),
        };
        match admission {
            UidPageAdmission::Admitted(receipt)
            | UidPageAdmission::Existing(receipt)
            | UidPageAdmission::Failed(receipt) => {
                Ok(AuditHandoff::response(render_uid_receipt(&receipt)))
            }
            UidPageAdmission::Rejected(error) => Ok(AuditHandoff::error(error)),
        }
    }

    fn read_uid_reconcile_status(
        &mut self,
        request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self
            .store
            .load_uid_reconcile_receipt(*self.connection.registration_id(), request)?;
        ReadResult::new(render_uid_reconcile_receipt(&receipt), None)
    }

    fn request_uid_reconcile(
        &mut self,
        id: [u8; 32],
        request: &UidReconcileRequest,
    ) -> Result<AuditHandoff<Value>, ErrorCode> {
        use hantong_store::uid_reconcile_jobs::UidReconcileAdmission;
        let worker = self.uid_worker.ok_or(ErrorCode::CoreUnavailable)?;
        let admission = match durable_admission(worker.admit_reconcile(id, request)) {
            Ok(value) => value,
            Err(handoff) => return Ok(handoff),
        };
        match admission {
            UidReconcileAdmission::Admitted(receipt)
            | UidReconcileAdmission::Existing(receipt)
            | UidReconcileAdmission::Failed(receipt) => Ok(AuditHandoff::response(
                render_uid_reconcile_receipt(&receipt),
            )),
            UidReconcileAdmission::Rejected(error) => Ok(AuditHandoff::error(error)),
        }
    }

    fn read_uid_recovery_status(
        &mut self,
        request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self
            .store
            .load_uid_recovery_receipt(*self.connection.registration_id(), request)?;
        ReadResult::new(render_uid_recovery_receipt(&receipt), None)
    }

    fn request_uid_recovery(
        &mut self,
        id: [u8; 32],
        request: &UidRecoveryRequest,
    ) -> Result<AuditHandoff<Value>, ErrorCode> {
        use hantong_store::uid_recovery_jobs::UidRecoveryAdmission;
        let worker = self.uid_worker.ok_or(ErrorCode::CoreUnavailable)?;
        let admission = match durable_admission(worker.admit_recovery(id, request)) {
            Ok(value) => value,
            Err(handoff) => return Ok(handoff),
        };
        match admission {
            UidRecoveryAdmission::Admitted(receipt)
            | UidRecoveryAdmission::Existing(receipt)
            | UidRecoveryAdmission::Failed(receipt) => Ok(AuditHandoff::response(
                render_uid_recovery_receipt(&receipt),
            )),
            UidRecoveryAdmission::Rejected(error) => Ok(AuditHandoff::error(error)),
        }
    }

    fn read_initial_sync_status(
        &mut self,
        request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self
            .store
            .load_initial_sync_receipt(*self.connection.registration_id(), request)?;
        ReadResult::new(render_initial_sync_receipt(&receipt), None)
    }

    fn read_initial_sync_plan(
        &mut self,
        request: &InitialSyncPlanRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let plan = self
            .store
            .load_initial_sync_plan(*self.connection.registration_id(), request)?;
        ReadResult::new(render_initial_sync_plan(&plan), None)
    }

    fn read_current_initial_sync_plan(
        &mut self,
        request: &hantong_contracts::uid_page::UidCursorRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let plan = self
            .store
            .load_current_initial_sync_plan(*self.connection.registration_id(), request)?;
        ReadResult::new(
            render_initial_sync_current(request.account_id(), request.mailbox(), plan.as_ref()),
            None,
        )
    }

    fn request_initial_sync(
        &mut self,
        id: [u8; 32],
        request: &InitialSyncRequest,
    ) -> Result<AuditHandoff<Value>, ErrorCode> {
        use hantong_store::initial_sync::InitialSyncAdmission;
        let worker = self.uid_worker.ok_or(ErrorCode::CoreUnavailable)?;
        let admission = match durable_admission(worker.admit_initial(id, request)) {
            Ok(value) => value,
            Err(handoff) => return Ok(handoff),
        };
        match admission {
            InitialSyncAdmission::Admitted(receipt)
            | InitialSyncAdmission::Existing(receipt)
            | InitialSyncAdmission::Failed(receipt) => Ok(AuditHandoff::response(
                render_initial_sync_receipt(&receipt),
            )),
            InitialSyncAdmission::Rejected(error) => Ok(AuditHandoff::error(error)),
        }
    }

    fn finish_audit(&mut self, id: [u8; 32], terminal: AuditTerminal) -> Result<(), ErrorCode> {
        self.store.finish_audit(id, terminal)
    }
}
