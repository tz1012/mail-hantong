//! Foreground Core helper for same-console protected secret capture.
//!
//! This module intentionally exposes only an opaque staged handle.  The
//! Secrets crate owns masked input, sensitive buffers, DPAPI protection, and
//! one-use consumption.

use hantong_secrets::{SecretError, SecretStore, TerminalSecretHandle, TerminalSecretTicket};

/// Capture one secret from the inherited foreground console.  The actual
/// console mode and echo restoration live in the Secrets boundary so a caller
/// cannot accidentally read or transport plaintext.
pub fn capture(
    secrets: &SecretStore,
    ticket: &TerminalSecretTicket,
) -> Result<TerminalSecretHandle, SecretError> {
    secrets.capture_terminal_secret(ticket)
}

/// Decodes the opaque ticket returned by the Core review offer.  No secret
/// value is present in this representation.
pub fn ticket_from_opaque(bytes: &[u8]) -> Result<TerminalSecretTicket, SecretError> {
    TerminalSecretTicket::from_opaque(bytes)
}

/// Returns the staged handle identifier for the submit message.  The staged
/// path and protected bytes never leave the Secrets boundary.
pub fn opaque_handle(handle: &TerminalSecretHandle) -> [u8; 32] {
    handle.opaque_id()
}
