//! Connection-owned authentication primitives. The bootstrap dispatcher does
//! not activate this path until trusted registration storage/UI are connected.

use hantong_contracts::{
    ErrorCode,
    auth::{AuthTranscript, RequestedScope, SignerRole},
};
use hantong_platform::{
    LocalPipeConnection, ServerSigningKey, current_peer_context, random_nonce, sha256,
    validate_public_key, verify_signature,
};
use std::time::{Duration, Instant};

/// A snapshot supplied only by the trusted registration repository, never by
/// request deserialization. It does not itself prove a connected client's key.
#[derive(Clone)]
pub struct RegisteredClient {
    id: [u8; 16],
    epoch: u64,
    public_key: [u8; 72],
    scope: RequestedScope,
    active: bool,
}
impl RegisteredClient {
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.id
    }

    pub fn profile_epoch(&self) -> u64 {
        self.epoch
    }

    pub fn from_registration_store(
        id: [u8; 16],
        epoch: u64,
        public_key: [u8; 72],
        scope: RequestedScope,
        active: bool,
    ) -> Result<Self, ErrorCode> {
        if epoch == 0 {
            return Err(ErrorCode::InvalidInput);
        }
        validate_public_key(&public_key)?;
        Ok(Self {
            id,
            epoch,
            public_key,
            scope,
            active,
        })
    }
}

pub struct ServerChallenge {
    pub transcript: AuthTranscript,
    pub signature: [u8; 64],
}

/// This non-cloneable value owns the OS-validated connection. Finishing an
/// attempt consumes it even when the signature or registration check fails.
pub struct PendingAuthentication<'server> {
    connection: LocalPipeConnection<'server>,
    transcript: AuthTranscript,
    registration: RegisteredClient,
    issued: Instant,
}

impl<'server> PendingAuthentication<'server> {
    pub fn begin(
        connection: LocalPipeConnection<'server>,
        server_key: &ServerSigningKey,
        server_instance: [u8; 32],
        registration: &RegisteredClient,
        client_nonce: [u8; 32],
        requested: RequestedScope,
        now_utc_ms: i64,
    ) -> Result<(Self, ServerChallenge), ErrorCode> {
        if !registration.active {
            return Err(ErrorCode::ClientSuspended);
        }
        if now_utc_ms < 0 {
            return Err(ErrorCode::InvalidInput);
        }
        let issued = Instant::now();
        let transcript = AuthTranscript {
            registration_id: registration.id,
            profile_epoch: registration.epoch,
            issued_at_utc_ms: now_utc_ms,
            expires_at_utc_ms: now_utc_ms
                .checked_add(30_000)
                .ok_or(ErrorCode::InvalidInput)?,
            server_instance_id: server_instance,
            server_nonce: random_nonce()?,
            client_nonce,
            server_key_hash: sha256(&server_key.public_key()?)?,
            client_key_hash: sha256(&registration.public_key)?,
            server: current_peer_context()?,
            client: connection.peer_context()?,
            scope: requested,
        };
        let signature = server_key.sign(&transcript.encode(SignerRole::Server)?)?;
        let challenge = ServerChallenge {
            transcript: transcript.clone(),
            signature,
        };
        Ok((
            Self {
                connection,
                transcript,
                registration: registration.clone(),
                issued,
            },
            challenge,
        ))
    }

    pub fn transport(&mut self) -> &mut LocalPipeConnection<'server> {
        &mut self.connection
    }

    pub fn finish(
        self,
        signature: &[u8],
        current: Option<&RegisteredClient>,
        now_utc_ms: i64,
    ) -> Result<AuthenticatedConnection<'server>, ErrorCode> {
        if now_utc_ms < self.transcript.issued_at_utc_ms
            || now_utc_ms >= self.transcript.expires_at_utc_ms
            || self.issued.elapsed() >= Duration::from_secs(30)
        {
            return Err(ErrorCode::AuthRequired);
        }
        let current = current.ok_or(ErrorCode::ClientNotRegistered)?;
        check_registration(&self.registration, current)?;
        let valid = verify_signature(
            &self.registration.public_key,
            &self.transcript.encode(SignerRole::Client)?,
            signature,
        )
        .map_err(|_| ErrorCode::AuthRequired)?;
        if !valid {
            return Err(ErrorCode::AuthRequired);
        }
        let granted = self
            .transcript
            .scope
            .intersection(&self.registration.scope)
            .intersection(&current.scope);
        Ok(AuthenticatedConnection {
            connection: Some(self.connection),
            registration: self.registration,
            granted,
        })
    }
}

pub struct AuthenticatedConnection<'server> {
    connection: Option<LocalPipeConnection<'server>>,
    registration: RegisteredClient,
    granted: RequestedScope,
}
impl<'server> AuthenticatedConnection<'server> {
    /// Returns the registration selected by the authenticated handshake. These
    /// values identify the session but are not authorization evidence by
    /// themselves; callers must still revalidate before every operation.
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration.id
    }

    pub fn profile_epoch(&self) -> u64 {
        self.registration.epoch
    }

    pub fn transport(&mut self) -> Result<&mut LocalPipeConnection<'server>, ErrorCode> {
        self.connection.as_mut().ok_or(ErrorCode::AuthRequired)
    }

    /// Rechecks the connection-owned registration against the current trusted
    /// registration snapshot. Any identity/session change consumes the
    /// transport so it cannot be used after revocation or key rotation.
    pub fn revalidate_registration(
        &mut self,
        current: Option<&RegisteredClient>,
    ) -> Result<(), ErrorCode> {
        if self.connection.is_none() {
            return Err(ErrorCode::AuthRequired);
        }
        let result = current
            .ok_or(ErrorCode::ClientNotRegistered)
            .and_then(|value| check_registration(&self.registration, value));
        if let Err(code) = result {
            self.connection.take();
            return Err(code);
        }
        let current = current.expect("validated registration");
        if self.granted.intersection(&current.scope) != self.granted {
            self.connection.take();
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(())
    }

    /// Checks access scope only. Mutation policy and human approval must still
    /// be applied by the common tool executor before any effect is performed.
    pub fn check_account_scope(
        &mut self,
        current: Option<&RegisteredClient>,
        account: &str,
        tool: &str,
    ) -> Result<(), ErrorCode> {
        self.revalidate_registration(current)?;
        let current = current.expect("validated registration");
        if !self.granted.permits_account(account, tool)
            || !current.scope.permits_account(account, tool)
        {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(())
    }

    /// Checks an entire request scope before a service call. A mixed request
    /// is rejected atomically when any account is outside the session scope.
    pub fn check_accounts_scope(
        &mut self,
        current: Option<&RegisteredClient>,
        accounts: &[String],
        tool: &str,
    ) -> Result<(), ErrorCode> {
        if accounts.is_empty() {
            return Err(ErrorCode::InvalidInput);
        }
        self.revalidate_registration(current)?;
        let current = current.expect("validated registration");
        if accounts.iter().any(|account| {
            !self.granted.permits_account(account, tool)
                || !current.scope.permits_account(account, tool)
        }) {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(())
    }

    /// Checks one closed global read tool without interpreting empty accounts as all accounts.
    pub fn check_app_read_scope(
        &mut self,
        current: Option<&RegisteredClient>,
        tool: hantong_contracts::auth::AppReadTool,
    ) -> Result<(), ErrorCode> {
        self.revalidate_registration(current)?;
        let current = current.expect("validated registration");
        if !self.granted.tools().iter().any(|v| v == tool.as_str())
            || !current.scope.tools().iter().any(|v| v == tool.as_str())
        {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(())
    }
}

fn check_registration(
    original: &RegisteredClient,
    current: &RegisteredClient,
) -> Result<(), ErrorCode> {
    if current.id != original.id {
        return Err(ErrorCode::ClientNotRegistered);
    }
    if !current.active {
        return Err(ErrorCode::ClientSuspended);
    }
    if current.epoch != original.epoch || current.public_key != original.public_key {
        return Err(ErrorCode::AuthRequired);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
