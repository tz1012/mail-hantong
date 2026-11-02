# Built-in AI mail access and reviewed trash

2026-11-03 user-approved extension of
[the concrete proposal](../proposals/AI-AUTOMATIC-MAIL-TOOLS-20261103.md).
Implementation and human security review remain separate gates. This contract
supersedes default-off/manual-MCP requirements only for the built-in AI flow.

## Identity and activation

Built-in AI uses `ClientKind::Ai`, never the default CLI or external MCP key.
Provider-bound fixed registrations/profiles isolate Codex from Claude as well
as external MCP. Profile files are `ai-codex.json` and `ai-claude.json`; these
are the provider-qualified implementation of the proposal's AI profile. Their
kind, fixed label, non-exportable key, server pin, profile epoch and exact
account/tool scope are validated through existing protected publication and
authenticated IPC. A provider string or an OS process name alone grants nothing.

The first interactive AI chat automatically starts the Core-owned foreground
feature setup in the same console when no current consent exists. It explains
the selected provider, exact connected account list, metadata/body/attachment
disclosure and available tools. No copied request ID, separate MCP registration
command or `/agent on` is needed. EOF/cancel leaves existing authority unchanged.
Noninteractive/JSON/no-input use cannot create or enlarge consent and returns
HUMAN_STEP_REQUIRED before a model call when setup is needed. Existing configured
access is revalidated and connected automatically for each chat. `/agent off`
disconnects tools and clears provider history. Provider changes clear history
and revalidate the other provider's separately configured profile.

Only explicitly selected, ready accounts are included; empty does not mean all.
The initial terminal selection is explicit confirmation of the complete list
shown on screen, or Cancel. Candidates are ready Gmail/IMAP accounts, checked
again at staging; unsupported or unfinished accounts are excluded.
Adding an account or expanding tools requires another explicit feature setup.
Existing CLI/MCP profiles/consents are not broadened or converted. Migration,
startup, profile parsing and provider login never manufacture AI consent.

Existing AI registrations are not silently replaced: another registration
request returns Conflict. Missing/corrupt profiles and revoked registrations
are not automatically repaired or reauthorized. Account expansion/replacement
is a separate transition, not an implicit side effect of starting another chat.

### Registration IPC and storage

`registration.ai.request` accepts exactly protocol_version=1 and
provider=`codex|claude`. Core derives the fixed label and candidate scope; caller
tools, approval flags, credentials or paths are not accepted. It creates a
pending receipt only. `registration.ai.status` additionally requires the
nonzero 32-byte request_id as 64 lowercase hex and checks that the request is
for the same provider. Receipts use the existing bounded registration schema.
The Core foreground registration review stages `ai_feature` provenance, then
publishes the protected provider profile and activates it. Generic model RPC
cannot activate it.

Schema 25 freezes [v24](migrations/schema-v24.sql) and uses
[024-to-025.sql](migrations/024-to-025.sql). The full rebuild preserves existing
rows, foreign keys, historical scope hashes and audit sequence highwater.
`ai_consents` has primary key (provider, registration_id, account_id,
profile_epoch), plus the originating registration request and approval time.
Consent insertion and activation are atomic; publication/recovery never create
fresh consent for another account/provider. Current DDL has 49 product tables.

## Tool discovery and disclosure

AI discovers only the authenticated profile's supported mail tools. Account and
folder listing, scoped sync/search, message body retrieval, local draft creation,
send request/status and trash request/status are supported when the account's
provider capability and granted scope allow them. Unsupported actions fail
explicitly. No execute/confirm/approval endpoint is offered to the model.
Tool definitions are automatic; there is no bulk inbox upload on chat startup.
Every read, deferred sync and write rechecks the registration, epoch, account
and provider-bound AI consent. Mail content cannot change these checks.

External MCP retains its independent explicit registration and consent flow.
The external MCP default transport cannot authenticate with an AI profile.
AI transport cannot select the CLI/MCP profile. Same-user automation remains a
residual risk; terminal intent is not proof of human presence.

## Trash request and execution

`mail.messages.trash.request` provides request/status for one cached Gmail
message. The request binds account_id, message_id, expected_version,
operation_id, idempotency_key and canonical payload_hash. Core assigns review_id
and binds its immutable preview, owner registration/epoch, account/credential
versions and expiry. Only a Core foreground terminal review may confirm it.
Creation/status never invokes a provider or schedules an execution job.

Gmail trash requires selected Modify mode and a verified gmail.modify grant.
ReadOnly/ReadSend do not imply trash authority. Use the exact Gmail messages
trash endpoint after a durable single claim. Do not loosen TRASH restrictions
in ordinary label changes. Permanent delete, batch delete, empty-trash, spam,
arbitrary URL and IMAP EXPUNGE fallback remain unsupported.
The endpoint and scope follow Google's
[users.messages.trash reference](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/trash).

Confirm rechecks all bindings before provider effects. Cancel/EOF/expiry/tamper
have no provider effect. Accepted, rejected and uncertain results are durable;
uncertain actions never auto-retry, including after restart. Success invalidates
only the affected message cache. Audit/errors/debug contain no message content
or credential. Other providers advertise no trash support until their complete
capability and safe operation path exist.

## Verification and release

Synthetic integration must cover independent provider/AI/MCP identities,
account consent and revocation, auto discovery/call, redirected-input guards,
cancel/expiry/replay/tamper, migration data/FK/highwater preservation, exact
trash request and one-attempt/uncertain behavior. Never use real accounts in
automated tests. Actual deletion needs separate explicit real-operation scope.
Do not advertise implementation merely because this approved contract exists.
