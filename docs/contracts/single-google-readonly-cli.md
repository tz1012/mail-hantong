# Single-account Gmail read-only CLI request (implementation draft)

Status: human security review required. CLI-15, SEC-03, ACC-03. Local source/test
authorization does not authorize an actual registration, key issue, or grant.

The existing Gmail list/body calls read the encrypted local cache. A real first
read also needs mail.sync.page to fetch Gmail and exercise token refresh. Three
tools (accounts.list/messages.search/messages.get) alone cannot complete that
flow. Reuse existing canonical B, with no new grant set, DB migration or hash.

New explicit commands: `clients register-readonly <google_account_id>` and
`clients refresh-readonly <google_account_id>`. IDs use the existing canonical
google_ plus 64 lowercase hex shape. They are selectors, not authority.
Wire methods are registration.google_readonly.request and
registration.scope.google_readonly.request, with exactly protocol_version=1
and account_id. Unknown/duplicate fields and arbitrary tools/approval flags
are rejected. Existing request/status bytes and behavior stay unchanged.

Initial registration uses existing L (mail.accounts.list) and exactly the
selected registrable Gmail account. Existing registration causes CONFLICT;
no key/profile is replaced or repaired automatically. Existing pending/running
requests are reused only for the exact same account and L scope.

Read-only refresh requires a current CLI registration already limited to that
same account, and L or B tools. Its target is exact B: app.diagnostics.snapshot,
app.settings.display.get, mail.accounts.list, mail.folders.list,
mail.folders.refresh, mail.messages.get, mail.messages.search, mail.sync.page.
It never requests drafts/send, arbitrary subsets, other accounts, or a downgrade.
Pending/staged targets must match exact B and selected account; conflicts are
not converted, cancelled or broadened. Same-account B no-op remains CONFLICT.

Reuse the existing native reviews, two separate Windows Hello confirmations,
300-second review TTL, 60 requests/minute, immutable snapshot/key/pin binding,
single-use proof, stage old grant / protected profile publish / activate order.
Grant persistence has no new automatic expiry: existing revocation applies.
Profiles are public metadata under the fixed protected LocalAppData path;
signing keys remain non-exportable CNG, registration/grants in encrypted Store.
No OAuth secret, Google scope or existing credential is changed by these calls.

Development uses synthetic Store/key/account fixtures only. Test selected
account existence, different-account and writable pending conflicts, unchanged
legacy flow, native-proof boundaries, wire strictness and explicit CLI routing.
No actual request is issued before separate user approval of both steps and
the full eight-tool B set, local caching and existing credential refresh use.
