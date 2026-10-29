//! Trusted core adapter for the common execution coordinator.
//!
//! This module is deliberately private to the core process.  It accepts only
//! an OS-authenticated, connection-owned session and the already opened Store;
//! it does not create identity or approval evidence.

use hantong_application::{
    AccountsListRequest, ExecutionContext, ReadResult, ToolRequest, render_account_list,
};
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditTerminal},
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

pub(crate) fn execute<'server>(
    store: &Store,
    policy: &mut ReadPolicy,
    connection: &mut AuthenticatedConnection<'server>,
    request: &RpcRequest,
) -> Result<Value, ErrorCode> {
    let mut adapter = CoreExecutionAdapter::new(connection, store, policy);
    hantong_application::execute_tool(&mut adapter, &request.method, &request.params)
}

impl<'a, 'server> CoreExecutionAdapter<'a, 'server> {
    pub(crate) fn new(
        connection: &'a mut AuthenticatedConnection<'server>,
        store: &'a Store,
        policy: &'a mut ReadPolicy,
    ) -> Self {
        Self {
            connection,
            store,
            policy,
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

    fn finish_audit(&mut self, id: [u8; 32], terminal: AuditTerminal) -> Result<(), ErrorCode> {
        self.store.finish_audit(id, terminal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
