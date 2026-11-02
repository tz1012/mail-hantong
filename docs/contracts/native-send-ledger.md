> Latest user correction (2026-11-02): [all-terminal workflows](terminal-workflows.md) supersedes native-window and Windows Hello requirements for every CLI/TUI path. Confirmation happens in the same console; provider OAuth retains its browser. Legacy descriptions below remain historical context. Security code requires human review.

# Native send ledger (implementation draft)

2026-11-02 최신 개인 CLI 변경: [terminal-personal-send](terminal-personal-send.md)가 아래 개인 네이티브 창 예외를 대체한다. 개인 prepare/confirm/cancel은 창이나 Hello를 열지 않는다. 아래 불변 원장·정책·단일 claim·복구 계약은 그대로 적용하며 보호 등록의 기존 경로는 유지한다.

Status: human security review required. APR-01, AI-09, SYNC-07, SEC-03.

## 2026-11-02 personal CLI confirmation exception

The latest user UX decision and proposed ADR-028 narrowly replace OS verification
with native content confirmation for the active default Cli registration whose
approved channel is google_oauth, writable scope is exactly the single-account
personal send tool set, and endpoint is that account's Gmail ReadSend binding.
Core derives this classification from Store; no RPC flag selects it. Pending
owner/epoch/account/version/endpoint must match current state. The same guard is
repeated inside the immediate send-claim transaction before any outbox effect.
All other registrations retain the OS Verified path described below.

The native helper alone issues a distinct opaque ConfirmedPersonalSend after
the complete immutable snapshot is displayed and the user presses Send. It is
single-use, exact-bound and expiring; it is not a VerifiedSend or a synthetic OS
verification result. CLI/MCP confirmation cannot issue either proof. Policy,
audit, CAS, single attempt and SEND_UNKNOWN behavior remain mandatory. No schema
or wire change is required. A Cli identity does not prove human origin; same-user
UI automation resistance is weaker. Human security review remains required.

`mail.drafts.create` stores one immutable version-1 draft in SQLCipher. Its
canonical compose hash binds account, ordered recipients, subject, and complete
UTF-8 body. Exact account/operation/idempotency replay returns the same draft;
changed payload, owner, or split operation/key matches return CONFLICT. The
client supplies no native verification data.

`mail.send.request` either requests a review or inspects an existing review.
Creation binds the draft ID/version/hash, owner registration/profile epoch,
account settings version, SMTP endpoint version, operation, and idempotency key.
The native queue is serialized with registration/setup windows. A pending
review lasts at most five minutes and never resumes automatically at startup.
An explicit repeated request may display a still-pending review. Status is
restricted to the original account, registration, and profile epoch.

Only the trusted Core native coordinator calls the protected approval helper.
The helper displays all recipients, account, SMTP sender/server, subject, and
full body, then requires native personal confirmation for the bounded exception
above, or existing OS human verification for protected registrations. Its opaque
proof is consumed against that exact immutable review. Review DTOs and Store
constructors are data, not approval. CLI/MCP host confirmation cannot substitute
for this proof. Normal RPC `mail.send.execute` remains policy blocked.

After consuming native confirmation or verification, Core repeats registration, writable
scope, account, endpoint, review expiry, and shared rate-policy checks. It
creates an immutable SendExecute audit start. One immediate Store transaction
then verifies that audit's owner/epoch/tool/account and missing terminal,
CAS-consumes the pending review, and inserts a unique Sending outbox claim.
SMTP is first polled after this transaction commits. No duplicate claim or
replayed review may submit again. The outbox retains its audit ID.

During SMTP, Core checks the registration, epoch, writable execute grant,
account readiness/settings, and endpoint binding every 100 ms and again before
recording the response. A changed binding cancels the in-flight future and
records SEND_UNKNOWN. Explicit SMTP acceptance is sent, explicit rejection is
failed, and uncertain results/errors are unknown. There is no automatic retry.
Terminal outbox state and immutable audit terminal commit atomically. Failure to
insert exactly one audit terminal rolls back the whole terminal transaction.

Startup recovery changes existing Sending rows to unknown and records their
SEND_UNKNOWN audit terminals, without opening SMTP or displaying approval UI.
Status/replay remains available after endpoint changes without substituting a
new endpoint into an old approval. Old epochs cannot inspect or reactivate a
review. All returned diagnostics use stable codes and omit provider text,
credentials, recipients, subject, and body.

Draft response: `{schema_version:1,ok:true,account_id,draft_id,version:1,payload_hash}`.
Review response: `{schema_version:1,ok:true,account_id,review_id,state,error_code}`.
IDs/hashes are lower-case fixed 64-digit hex. Review states are pending, expired,
denied, interrupted, sending, sent, failed, or unknown. A cancelled verification
requiring a fresh human step is interrupted/APPROVAL_REQUIRED. Other admission
failures are failed with their stable error; denied is APPROVAL_DENIED only.
State is authoritative; a pending
review after an interrupted UI process has no consumed proof and no send claim.
New MCP mail registration requires its own distinct full-mail native review.
Legacy metadata grants stay read-only until explicit native replacement. Neither
path inherits identity, credentials, or approval authority from the CLI.
