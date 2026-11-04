# Proposed Unix Core relay for CLI/MCP authentication

Status: **proposed and unapproved** (2026-11-04)

This proposal extends the approved ADR-030 Unix runtime only at the client
authentication boundary. It does not approve a new mail capability, a new
registration path, or a human approval mechanism. Windows protocol v1 and its
CNG client path remain unchanged.

## Problem

ADR-030 keeps Unix private signing keys inside `hantong-secrets` and exposes
only `OpaqueSigner` to the authentication implementation. The architecture
also forbids `hantong-ipc-client`, CLI, and MCP from depending on Secrets.
Therefore a Unix CLI cannot directly call
`hantong_ipc_client::unix_authenticated_wire::authenticate_connected`, because
that function requires a signer while the CLI must not load the private key.

The public client profile contains the registration ID, client key ID, client
public key, server public key, epoch, and scope. It intentionally contains no
private key.

## Proposed boundary

The CLI/MCP starts a short-lived Core-owned **stdio relay subprocess** using a
fixed, installed helper image. The relay is a local transport adapter, not a
second authority:

1. The CLI passes only a bounded profile selector and one bounded RPC request
   to the relay over its inherited stdin/stdout pipes.
2. The relay loads the fixed profile and the matching private signing key from
   Secrets. It constructs the Unix `OpaqueSigner` inside Secrets-owned code.
3. The relay establishes the Unix auth-v2 connection to Core and keeps the
   authenticated connection private to the relay.
4. The relay forwards only the approved bounded RPC response to the CLI. It
   never returns private key bytes, raw credential bytes, or an auth signing
   key. A challenge signature is not exposed as a standalone CLI API.
5. EOF, malformed framing, method mismatch, timeout, epoch change, scope
   reduction, or Core disconnect closes the relay and the authenticated
   connection.

The relay must use the existing fixed profile selector (`default`, `mcp`, or
the provider-qualified built-in AI profile). It must not accept arbitrary
profile paths, registration IDs, server pins, account lists, tool lists, or
secret selectors from the CLI.

## Relay framing and method limits

The relay protocol is a private implementation contract until approved. Its
initial frame is a four-byte little-endian length followed by UTF-8 JSON
containing exactly `protocol_version`, `profile`, `request_id`, `method`, and
`params`. Version is integer `1`; profile is exactly `default`, `mcp`,
`ai-codex`, or `ai-claude`; params is an object. Request ID and method use the
existing authenticated-wire ASCII grammar and bounds (64/128 bytes).
Unknown, duplicate, null, trailing, or oversized input is rejected.

The relay input frame is at most 1,049,600 bytes including its selector
envelope. After removing that envelope, the serialized Core RPC must fit
the existing method limit: display settings request/response 4,096 bytes;
diagnostics request 4,096 and response 36,864 bytes; supported `mail.*`
request/response 1,048,576 bytes. The output is the unchanged bounded Core
response in the same length framing. Raw setup credentials are never valid
params; their existing Secrets-owned protected input remains separate.

One child handles one request. Startup/input admission has a five-second
deadline, auth-v2 retains its existing begin/overall deadlines, and the
protected exchange has one five-second deadline including its receipt.
The parent imposes a 40-second total child lifetime, closes pipes and reaps
only the child it launched on expiry. It never automatically repeats a
request after a partial response or child failure; existing unknown-operation
recovery remains authoritative.

The relay may forward only methods already authorized by the loaded profile.
It cannot add accounts or tools, change profile scope, approve registration,
issue terminal-review tickets, claim a send/trash operation, or bypass the
existing Core policy and audit paths. Mail mutation and send confirmation
remain separate Core workflows. A relay request is never human presence proof.

This does not remove composing, sending, or Trash from Unix clients. The
relay may forward their existing request/status/execute methods when the
fixed profile permits them. Core alone validates the message-specific
confirmation, consumes the ledger claim and invokes the provider. The relay
cannot synthesize that confirmation or perform a provider operation itself.

## Trust and revalidation

Core must authenticate the relay's Unix peer using the existing ADR-030 peer
context and process-instance evidence. The relay is accepted only when all of
the following match the installed helper record:

- effective UID and the signed, observed Unix peer context, including its
  session field (the two processes need not belong to the same session);
- fixed helper executable identity and release hash;
- the expected process instance/start proof;
- the selected profile's registration ID, epoch, server pin, and scope.

Core revalidates the registration, epoch, account scope, tool scope, expiry,
and revocation before every forwarded request. The relay does not create or
activate a registration and cannot turn a profile file into authority.

The relay must not claim that same-image or same-UID evidence proves a human
launched it. Human confirmation remains in the existing explicit terminal or
native review contracts.

## Why direct client-to-Secrets access is rejected

Adding `hantong-secrets` as a dependency of `hantong-ipc-client` or the CLI
would expose the private signing boundary to every client entry point and
would contradict ADR-013's dependency direction and
`terminal-workflows.md`'s explicit rule. Moving keyring access into
`hantong-platform` would only hide the same violation and would make the
platform crate a secret consumer. Exporting a signature endpoint without the
relay's fixed-image and per-request Core checks would create a new authority
surface.

## Implementation files after approval

The smallest implementation would be:

- `crates/hantong-ipc-client/src/unix_relay.rs`: bounded relay client facade;
- `crates/hantong-ipc-server/src/unix_authenticated_wire.rs`: Core-side
  relay admission and per-request revalidation;
- `apps/mail-hantong-core/src/unix_status.rs` or the existing Unix Core
  startup module: fixed helper admission and status forwarding;
- `crates/hantong-secrets/src/unix_signing.rs`: retain signer ownership;
- `apps/hantong-cli`: invoke the installed relay without loading Secrets.

The existing Windows `trusted_profile.rs`, CNG signer, and Windows pipe
protocol are not modified.

## Required tests before implementation is accepted

- CLI/MCP dependency graph proves no direct Secrets edge.
- Relay rejects arbitrary profile paths, selectors, registration IDs, and
  scope/tool overrides.
- Helper image, UID, process-instance, and release-hash mismatch fails closed.
- Missing, expired, revoked, or scope-reduced registrations are rejected on
  each request.
- EOF, malformed/duplicate/oversized frames, deadline expiry, and Core
  disconnect wipe/close relay state without retry.
- Private key bytes and standalone challenge signatures never enter CLI
  stdout, argv, environment, JSON logs, or ordinary IPC responses.
- The relay cannot approve registration, issue confirmation, send, trash, or
  widen scope without the existing Core workflow.
- Windows v1 authentication and all existing terminal-review boundaries remain
  unchanged.

## Remaining Unix entry points

This proposal only addresses signer ownership for CLI/MCP-to-Core
authentication. It does not complete Unix Core startup, registration
publication, Google/IMAP/SMTP setup, mail synchronization, send, trash, AI
provider sessions, or Linux headless Secret Service behavior. Those remain
separate implementation and native verification work after this proposal is
approved.
