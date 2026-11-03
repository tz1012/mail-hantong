# Gmail write extension — staged implementation


> 2026-11-02: [Google personal CLI extension](google-personal-cli.md) supersedes native-only account/CLI setup clauses only for the bounded guided personal flow. Live verified OAuth authorizes its fixed feature configuration; feature selection alone does not. Per-send/mutation approval and advanced native setup remain. Human security review required.

Status: **human security review required**. ACC-02, APR-01, AI-09,
SYNC-07, SEC-02. No real grant or mail operation is authorized by this contract.

The approved 2026-11-03 built-in AI extension is specified separately in
[`builtin-ai-mail.md`](builtin-ai-mail.md). It adds provider-qualified AI tool
discovery and a single-message Gmail trash request under `Modify`; it does not
make permanent, batch, empty-trash, spam, or arbitrary-delete operations
available, and it does not change the send/mutation review rules below.

The existing readonly runtime remains unchanged until all integration checks pass.
This extends google-mail.md and native-send-ledger.md; it does not remove their
policy, native verification, audit, account isolation, TLS or single-attempt rules.

## Access modes

ReadOnly is the default. ReadSend requires openid/email/gmail.readonly/gmail.send.
Modify requires openid/email/gmail.modify and supports drafts, send and message
label changes. These are requested modes, not capabilities inferred from a broader
provider response. Required scopes must actually be granted. Additional provider
scopes never elevate the selected mode. Legacy credentials remain ReadOnly.
Full-mail scope is never requested. The native review must explain that Google's
modify grant itself also permits trash and permanent draft deletion, while the
HanTong interface excludes those operations. A profile grant does not replace a
Google grant, and neither replaces one-time native mail-send review.

## Immutable attachments and change inputs

An attachment is an immutable in-memory snapshot, not a pathname to reopen after
approval. The first bounded implementation permits at most 8 attachments, 1 MiB
total binary content, and ASCII basename-only filenames (1..120 bytes). MIME
types are selected from a small allowlist; untrusted headers are not accepted.
Names, media types, length and every byte join the canonical compose payload,
account, ordered recipients, subject and body. Debug and errors redact content.
An empty attachment list preserves the v1 compose bytes for compatibility.

Single-message label changes carry explicit account/message ID, operation,
idempotency key, expected version and payload hash. Allow UNREAD, STARRED,
IMPORTANT, INBOX, CATEGORY_* known categories, and provider user label IDs.
Reject TRASH, SPAM, SENT, DRAFT, unknown system labels, duplicate changes, overlap,
empty changes and multi-message/batch deletion paths. There is no delete route.
Label data cannot issue approval, change account or bypass policy/audit.

## Integration gates

Before exposing writes: bind access mode through OAuth, protected bundle and
Store publication; connect exact-account grant review; extend immutable encrypted
draft persistence; bind attachment metadata/content digest into native review;
route Gmail send through the same single-use sending claim and audit lifecycle;
persist uncertain outcomes without automatic resend; add exact-method/path JSON
transport allowlists; implement scoped attachment receive validation. Do not
expose a direct-send/debug bypass while these gates are incomplete.

The separately approved real trial is two messages only between the two named
accounts, with synthetic text and a 23-byte UTF-8 txt attachment. That permission
does not authorize broader OAuth access, arbitrary messages, deletion or uploads.

## Validation

Use synthetic scopes, identities, MIME and mail transport responses. Check partial
grants, extra scopes without elevation, cross-account/version mismatch, attachment
and header injection, byte/metadata changes to the canonical payload, forbidden
labels, denied/expired approval, accepted-but-response-lost and restart/replay.
Only a completed native end-to-end path can be called Gmail send support; typed
inputs and encoders alone are preparatory implementation.

## Compose wire and local draft

mail.drafts.create remains an encrypted local immutable draft, not a remote Gmail
draft. It accepts optional attachments [{filename,media_type,content_hex}]; no
path or approval field crosses IPC. Existing requests without attachments retain
their payload hash. CLI draft create snapshots --attachment-file once, using
--attachment-type (default application/octet-stream). The existing 1 MiB IPC
frame ceiling remains enforced, including JSON/hex expansion and body; oversized
combined input is rejected, never split or silently truncated. Native approval
shows attachment names/types/sizes, text preview where valid, and full digest.

## Explicit send registration selection

clients refresh-gmail-send <comma-separated-google-account-ids> requests C11 only
for 1..128 exact selected sending accounts (the existing RequestedScope account
bound). Change-label grants retain their separate 1..4 account bound until the
account-qualified tool representation is extended. Empty, duplicate and
non-Google IDs are rejected.
The same native scope review/Windows Hello and durable activation protocol apply;
no CLI approval is added. Every selected account must already have Google Send
mode. Existing account grants outside that selection cause Conflict rather than
silent removal or extension. No label-change grant is implicitly included.

## Opt-in receive verification

gmail sync --verify-attachments true requires --limit 1. It downloads at most
8 attachments / 1 MiB aggregate, binds attachment paths to the selected message
and account, checks provider size against decoded bytes, and caches only bounded
filename/type/size/SHA-256 results with the encrypted message. Raw attachment
bytes remain in Secrets memory and are zeroed on drop. Normal sync does not
download attachments. Missing verification is distinct from an empty verified
list; metadata alone is never represented as content verification. Existing
pending sync request hashes remain unchanged when opt-in is absent/false.
