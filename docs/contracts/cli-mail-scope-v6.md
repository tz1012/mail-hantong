# CLI mail capability upgrade v6 (implementation draft)

Status: human security review required. Requirements CLI-15, SEC-03, APR-01,
AI-09, SYNC-07. The user's continuing implementation mandate authorizes this
draft and local fake-provider implementation, not actual account approval.

Existing L/F/U/M/A profiles and hash versions 1 through 5 keep their original
bytes, membership, and transitions. Installing an update grants nothing.
Historical pending/staged requests resume their stored exact target.

B is the sorted exact A set plus `mail.messages.get`. C is sorted exact B plus
`mail.drafts.create`, `mail.send.request`, and `mail.send.execute`. Partial,
reordered, duplicate, or arbitrary tool sets remain invalid. New v6 fresh
requests explicitly present full old and target account/tool sets to native
human verification. B is read-only; C is writable. This flag is derived from
the exact canonical set and must agree in registration, protected profile, and
policy. A profile file is public metadata and never grants authority itself.

v6 hash domain is `mail-hantong.cli-scope-refresh.v6\0`; field ordering stays
v5. Version 6 alone permits B/C and monotone L<=F<=U<=M<=A<=B<=C transitions.
No-op remains conflict; changed accounts require explicit review. Historical
validators never call a future expanded tool-set validator.

Every send additionally requires native approval bound to immutable account,
draft/version, full recipients, subject, canonical compose payload hash, operation and
idempotency key. CLI/MCP may request or inspect a review but never issue or
consume a native authority token. Policy and current registration are checked
again after verification and before committing the outbox send claim.

The encrypted Store commits a Sending claim before network submission. A crash
or uncertain SMTP DATA acknowledgement is SEND_UNKNOWN; no startup loop,
automatic retry, or repeated idempotent request submits it again. Explicit
rejection is distinct from uncertain delivery. Returned errors/logs contain
stable codes, never SMTP text, credentials, recipients, subject, or body.

Body cache and draft/outbox records use the existing SQLCipher database. Body
keys include account, wire mailbox, encoding, UIDVALIDITY and UID. Current
account/endpoint/namespace binding is checked before cache use and commit.
