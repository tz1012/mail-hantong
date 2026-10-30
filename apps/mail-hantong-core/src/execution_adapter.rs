//! Trusted core adapter for the common execution coordinator.
//!
//! This module is deliberately private to the core process.  It accepts only
//! an OS-authenticated, connection-owned session and the already opened Store;
//! it does not create identity or approval evidence.

use hantong_application::render_uid_reconcile_receipt;
use hantong_application::render_uid_recovery_receipt;
use hantong_application::{
    AccountsListRequest, AuditHandoff, ExecutionContext, ReadResult, ToolRequest,
    render_account_list, render_folder_page, render_folder_refresh_receipt, render_uid_cursor,
    render_uid_receipt, validated_uid_mailbox,
};
use hantong_contracts::uid_reconcile::UidReconcileRequest;
use hantong_contracts::uid_recovery::UidRecoveryRequest;
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditTerminal},
    folders::{FolderListRequest, FolderRefreshRequest, FolderRefreshStatusRequest},
    uid_page::{UidCursorRequest, UidCursorSnapshot, UidPageRequest, UidPageStatusRequest},
};
use hantong_ipc_server::{
    RpcRequest,
    auth::{AuthenticatedConnection, RegisteredClient},
};
use hantong_platform::random_nonce;
use hantong_policy::ReadPolicy;
use hantong_store::{RegistrationRecord, RegistrationState, Store};
use serde_json::Value;

pub(crate) struct CoreExecutionAdapter<'a, 'server> {
    connection: &'a mut AuthenticatedConnection<'server>,
    store: &'a Store,
    policy: &'a mut ReadPolicy,
    folder_worker: &'a crate::folder_worker::FolderWorkerHandle<'a>,
    uid_worker: Option<&'a crate::uid_worker::UidWorkerHandle<'a>>,
}

pub(crate) fn load_registration(
    store: &Store,
    registration_id: &[u8; 16],
) -> Result<Option<RegisteredClient>, ErrorCode> {
    store
        .load_registration(*registration_id)?
        .map(registered_client)
        .transpose()
}

#[cfg(test)]
pub(crate) fn execute<'server>(
    store: &Store,
    policy: &mut ReadPolicy,
    worker: &crate::folder_worker::FolderWorkerHandle<'_>,
    connection: &mut AuthenticatedConnection<'server>,
    request: &RpcRequest,
) -> Result<Value, ErrorCode> {
    execute_with_uid(store, policy, worker, None, connection, request)
}
pub(crate) fn execute_with_uid<'server>(
    store: &Store,
    policy: &mut ReadPolicy,
    worker: &crate::folder_worker::FolderWorkerHandle<'_>,
    uid_worker: Option<&crate::uid_worker::UidWorkerHandle<'_>>,
    connection: &mut AuthenticatedConnection<'server>,
    request: &RpcRequest,
) -> Result<Value, ErrorCode> {
    let mut adapter = CoreExecutionAdapter::new(connection, store, policy, worker, uid_worker);
    hantong_application::execute_tool(&mut adapter, &request.method, &request.params)
}

impl<'a, 'server> CoreExecutionAdapter<'a, 'server> {
    pub(crate) fn new(
        connection: &'a mut AuthenticatedConnection<'server>,
        store: &'a Store,
        policy: &'a mut ReadPolicy,
        folder_worker: &'a crate::folder_worker::FolderWorkerHandle<'a>,
        uid_worker: Option<&'a crate::uid_worker::UidWorkerHandle<'a>>,
    ) -> Self {
        Self {
            connection,
            store,
            policy,
            folder_worker,
            uid_worker,
        }
    }

    fn current_registration(
        &mut self,
    ) -> Result<(RegistrationRecord, RegisteredClient), ErrorCode> {
        let current = self
            .store
            .load_registration(*self.connection.registration_id())
            .and_then(|record| record.ok_or(ErrorCode::ClientNotRegistered))
            .and_then(|record| registered_client(record.clone()).map(|client| (record, client)));
        if current.is_err() {
            let _ = self.connection.revalidate_registration(None);
        }
        current
    }
}

fn registered_client(record: RegistrationRecord) -> Result<RegisteredClient, ErrorCode> {
    RegisteredClient::from_registration_store(
        record.registration_id,
        record.profile_epoch,
        record.public_key,
        record.scope,
        record.state == RegistrationState::Active,
    )
    .map_err(|_| ErrorCode::InternalError)
}

// A returned admission error may follow a committed transaction. Store keeps
// audit ownership even when the caller cannot determine whether commit won.
fn durable_admission<T>(outcome: Result<T, ErrorCode>) -> Result<T, AuditHandoff<Value>> {
    outcome.map_err(AuditHandoff::error)
}

impl<'a, 'server> ExecutionContext for CoreExecutionAdapter<'a, 'server> {
    type Output = Value;

    fn revalidate(&mut self) -> Result<(), ErrorCode> {
        let (_, current) = self.current_registration()?;
        self.connection.revalidate_registration(Some(&current))
    }

    fn begin_audit(&mut self, request: &ToolRequest) -> Result<[u8; 32], ErrorCode> {
        let execution_id = random_nonce()?;
        let begin = AuditBegin::new(
            execution_id,
            *self.connection.registration_id(),
            self.connection.profile_epoch(),
            request.tool(),
            request.account_ids().to_vec(),
        )?;
        self.store.begin_audit(&begin)?;
        Ok(execution_id)
    }

    fn check_scope(&mut self, request: &ToolRequest) -> Result<(), ErrorCode> {
        let (_, current) = self.current_registration()?;
        self.connection.check_accounts_scope(
            Some(&current),
            request.account_ids(),
            request.tool().as_str(),
        )
    }

    fn check_policy(&mut self, request: &ToolRequest) -> Result<(), ErrorCode> {
        // Keep the current Store policy fields alongside the authentication
        // view: RegisteredClient deliberately contains no rate/policy grant.
        let (record, current) = self.current_registration()?;
        self.connection.revalidate_registration(Some(&current))?;
        self.policy.check(
            record.registration_id,
            record.read_only,
            record.requests_per_minute,
            request.tool(),
            std::time::Instant::now(),
        )
    }

    fn read_accounts(
        &mut self,
        request: &AccountsListRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        let snapshot = self
            .store
            .list_account_summaries(request.account_ids(), request.include_capabilities())?;
        render_account_list(snapshot)
    }

    fn read_folders(
        &mut self,
        request: &FolderListRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
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
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        let receipt = self
            .store
            .load_folder_refresh_receipt(*self.connection.registration_id(), request)?;
        let value = render_folder_refresh_receipt(&receipt);
        ReadResult::new(value, None)
    }

    fn request_folder_refresh(
        &mut self,
        audit_id: [u8; 32],
        request: &FolderRefreshRequest,
    ) -> Result<AuditHandoff<Self::Output>, ErrorCode> {
        let admission = match durable_admission(self.folder_worker.admit(audit_id, request)) {
            Ok(value) => value,
            Err(handoff) => return Ok(handoff),
        };
        match admission {
            hantong_store::folder_refresh::FolderRefreshAdmission::Admitted(receipt)
            | hantong_store::folder_refresh::FolderRefreshAdmission::Existing(receipt)
            | hantong_store::folder_refresh::FolderRefreshAdmission::Failed(receipt) => Ok(
                AuditHandoff::response(render_folder_refresh_receipt(&receipt)),
            ),
            hantong_store::folder_refresh::FolderRefreshAdmission::Rejected(error) => {
                Ok(AuditHandoff::error(error))
            }
        }
    }

    fn read_uid_cursor(
        &mut self,
        request: &UidCursorRequest,
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
        let value = UidCursorSnapshot::new(
            request.account_id(),
            request.mailbox().clone(),
            catalog.generation(),
            version,
            covered,
            current && catalog.binding_current(),
        )
        .map_err(|_| ErrorCode::InternalError)?;
        ReadResult::new(render_uid_cursor(&value), None)
    }
    fn read_uid_page_status(
        &mut self,
        request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Value>, ErrorCode> {
        let receipt = self
            .store
            .load_uid_page_receipt(*self.connection.registration_id(), request)?;
        ReadResult::new(render_uid_receipt(&receipt), None)
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
    fn finish_audit(&mut self, id: [u8; 32], terminal: AuditTerminal) -> Result<(), ErrorCode> {
        self.store.finish_audit(id, terminal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sec03_uncertain_durable_admission_retains_audit_ownership() {
        let outcome: Result<(), ErrorCode> = Err(ErrorCode::InternalError);
        assert!(matches!(
            durable_admission(outcome),
            Err(AuditHandoff::Error(ErrorCode::InternalError))
        ));
        assert!(matches!(durable_admission(Ok(7)), Ok(7)));
    }
    use hantong_contracts::auth::RequestedScope;
    #[test]
    fn sec03_stored_invalid_curve_point_is_internal_error() {
        let mut public_key = [0; 72];
        public_key[..4].copy_from_slice(b"ECS1");
        public_key[4..8].copy_from_slice(&32u32.to_le_bytes());
        // Store validates the BLOB header; CNG validates its curve point at
        // the core boundary. Invalid persisted metadata is not wire input.
        let record = RegistrationRecord {
            registration_id: [1; 16],
            request_id: [2; 32],
            client_kind: hantong_store::ClientKind::Cli,
            display_label: "fixture".into(),
            public_key,
            profile_epoch: 1,
            state: RegistrationState::Active,
            read_only: true,
            requests_per_minute: 60,
            approved_channel: "gui".into(),
            version: 1,
            scope: RequestedScope::new(vec!["account_a".into()], vec!["mail.accounts.list".into()])
                .unwrap(),
        };
        assert!(matches!(
            registered_client(record),
            Err(ErrorCode::InternalError)
        ));
    }
}
