# Gmail reviewed changes (proposed implementation; human security review required)

This extends google-write.md without changing existing send approvals. Two
operations are supported: save one immutable local draft to Gmail, and change
allowlisted labels on one cached Gmail message. UNREAD removal marks read;
UNREAD addition marks unread. Delete, trash, spam and batch routes are absent.

Every request binds one Google account, operation/idempotency IDs, expected
local object/cache version and payload digest. Draft requests bind the immutable
local draft including attachments. Label requests bind the exact message and
sorted add/remove lists; they never replace the whole label set. The cache
version is a local concurrency guard, not a Gmail server compare-and-swap.

The native review additionally binds owner registration/epoch, Google subject,
client/credential version, account version, captured content and a 300-second
lifetime. The native OS proof is single-use and unavailable through RPC. A
durable transaction must consume the pending review and claim execution before
any provider write. Accepted, rejected and uncertain results are distinct;
neither restart nor retry of an uncertain operation sends it again.

New grants are account-qualified capability IDs, separate from send authority:
`mail.google.change.<64 lowercase account hash>`. Only the exact Google account
may use `mail.google.changes.request` or native-only execute under that grant.
A generic method name, another account's grant, read/send-only Google mode or
old scope-refresh proof does not authorize these operations. Grant creation
requires the existing native scope-refresh flow and an explicit account list.
No existing registration is upgraded automatically.

Wire requests/status and audit events contain bounded metadata. No caller
approval flag, filesystem path, credential or arbitrary URL is accepted. Review
UI may show content to the human; Debug, errors and logs must not include it.
Implementation and synthetic tests do not authorize real migration, grants,
Google scope changes or messages.

## CLI and storage

`clients refresh-gmail-changes <comma-separated-google-account-ids>` adds 1..4
account-qualified grants to an already reviewed C11 profile. It preserves the
existing account/tool set; each selected account must already have Modify mode.
Scope proof v7 is distinct from v1..v6. Existing proofs never gain new authority.

`change draft-save --account <id> --draft-id <id> --expected-version 1
--payload-hash <hash> --operation-id <id> --idempotency-key <key>` requests native
review of the immutable local draft. `change labels --account <id> --message-id
<id> --expected-version <cache-version> --add-labels <comma-separated-labels>
--remove-labels <comma-separated-labels> --operation-id <id> --idempotency-key
<key>` computes the label payload digest locally. Omit an empty add/remove list.
`change status --account <id> --review-id <id>` returns metadata only.

The only public tool is `mail.google.changes.request` with request/status mode.
There is no public execute method. MCP cannot use the new change grant in this
implementation. Core's native coordinator performs one provider attempt.

Schema 21 adds the bounded mutation ledger and extends audit/scope-proof codes.
Migration rebuilds schema tables transactionally while preserving every existing
row, audit sequence high-water and grant; it creates no granted privilege or
pending action. A backup precedes each schema step. On success, label changes
invalidate only that cached message and advance local cache version; sync must
refetch it before another edit. This is local cache removal, not a Gmail delete.
An unknown operation remains blocked for the same target even with a new key.
Resolving an unknown outcome is intentionally a separate explicit process.
