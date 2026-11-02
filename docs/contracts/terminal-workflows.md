# All-terminal application workflows

2026-11-02 latest user instruction: ALL HanTong CLI/TUI operations finish in the
same terminal; no HanTong native window or Windows Hello dialog. Provider browser
OAuth remains external. Personal Gmail sending follows terminal-personal-send.
This document extends the target to protected send, Google mutations, client/MCP
registration, scope changes and advanced Google/IMAP/SMTP secret setup. Human
security review required. Same-user terminal automation is a documented residual
risk; terminal choice is intent, not OS-verified human presence.

The approved 2026-11-03 built-in AI extension uses the same foreground terminal
review boundary for send and single-message Gmail trash requests, as specified
in [builtin-ai-mail.md](builtin-ai-mail.md). It does not make the model an
approval issuer, and it does not remove the separate explicit external MCP
registration flow. Implementation and human security review remain pending.

## Core-owned typed review

Each existing immutable request keeps its typed DTO, owner/account/epoch,
operation/idempotency, exact version/hash, expiry and existing stage/claim CAS.
Request prepares pending state only; no native UI job is scheduled. A foreground
mode of the installed Core is launched with inherited console by the CLI. It
shows the full typed preview and asks Confirm/Cancel; EOF cancels. No general
approved bool or caller-selected grant endpoint is introduced.

The helper communicates with the existing Core through a separate, strictly
typed terminal-review bootstrap surface. Core verifies the OS-observed same
SID/logon session and exact installed Core helper image/path/hash; a caller
declared PID/name is never evidence. Reviews carry a random single-use ticket
bound to kind/request ID/snapshot hash/owner/account/epoch/expiry and are only
confirmed from that observed helper context. These checks separate protocol
roles; they do not prove that a human launched or drove the helper. An existing
MCP credential cannot itself confirm a pending request. User-visible CLI/TUI
drives the foreground helper in the same console, without a second window.

The initial selector may omit a known payload hash (encoded as all-zero lookup
hash). Core derives the authoritative nonzero snapshot hash from its pending
record. A supplied nonzero selector hash must match. Confirmation always binds
the returned nonzero snapshot hash and single-use ticket; a lookup selector
never changes a request, account, tool scope or decision.

Core repeats immutable snapshot/current permissions/version/expiry checks and
consumes the ticket before existing typed Store staging/claim. Store records
`terminal_cli` provenance; never record terminal intent as OS verification or
as legacy approval_helper. Existing history stays frozen. New registration
constructors and scope hash domain/version bind terminal_cli. A new schema
migration preserves legacy rows, FKs, sequences and previous hashes. No old
profile is silently granted broader accounts/tools.

## Protected console secret capture

CLI and IPC client must not depend on Secrets or receive plaintext credentials.
The Core foreground helper uses Secrets' masked console input. Core first
issues a short-lived, one-use capture ticket bound to request kind/ID, canonical
payload/account/endpoint/version, SID/logon and server identity. The helper
validates it, reads with echo disabled into the existing sensitive-buffer
discipline, restores console mode on all exits and stages only DPAPI-protected
material. CLI receives only an opaque ticket/status; no password in argv, env,
pipe, JSON, logs or plaintext file. Core consumes the staged value once through
Secrets against the exact ticket, then uses the existing probe/commit flow.
The long-running background Core must not AttachConsole or read caller stdin.

## Completion evidence

All eight existing native call sites must be absent from CLI/TUI execution
graphs: registration, scope refresh, advanced Google client capture, legacy
Google account setup, IMAP, SMTP, Google mutation, protected send. Native code
may remain for future GUI, but cannot be a CLI fallback. Verify synthetic
review/secret cancel/EOF/expiry/tamper/cross-owner/replay paths, old-schema
migration/rollback/FKs/highwater, no plaintext credential logs, unchanged
outbox/unknown recovery, then full integration checks. No actual account or
network is used in automated tests. Do not claim all-terminal while any route
above still opens a window.
