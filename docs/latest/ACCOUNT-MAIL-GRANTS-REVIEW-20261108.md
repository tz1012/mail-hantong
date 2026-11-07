# Account mail grants v2 — review handoff, 2026-11-08

Status: **[사람 검토 필요] — not approved for release or real-account execution.**

The review target is the complete candidate commit containing this document,
compared with `c22e1bf1507430fd88e3fa1625d96ed09b2e3c40`. Use the full commit ID
recorded in the handoff, not a moving branch or a partial diff. Include all newly
added files; earlier untracked files are part of this implementation.

## Authorized change

The user's full-mail requirement and “변경 진행해” authorize implementation of
`docs/contracts/account-mail-grants-v2.md`. They are not evidence that a person
reviewed the new security code, consented for a real account, or approved this
candidate's future binaries. The frozen c22 recovery package is unchanged.

The selected account gains read/draft/send/single-message Trash capability for
the fixed Default CLI/TUI identity only after its fresh provider/setup proof.
Existing account cells, GUI, MCP and provider-specific AI identities do not gain
a Cartesian union of authority. Message-specific send/Trash review, policy,
audit, claim and uncertain-result no-retry boundaries remain required.

## Review surfaces

| Boundary | Files / required checks |
| --- | --- |
| Contract and matrix | `account-mail-grants-v2.md`; contracts account scope/profile/auth/connect modules; exact account cells, global-only tools, canonical parsing and downgrade refusal |
| Authentication | IPC client/server Windows and Unix model-2 transcripts; exact consumer/key/pin/epoch/nonces/observed peer, fixed Default resolver, suspended/provisioning denial |
| Authority and recovery | Core `account_connect*.rs`; Store `account_grants.rs`, `full_mail.rs`, registration/Google/folder/UID changes; one-use proof, stale/expired/cancel no-grant, exact decision CAS, staged recovery, later supersession |
| Credentials | Secrets terminal input, staging, IMAP probe and crypto changes; masked input restoration, bounded deadlines, exact ticket deletion, no replacement of stored credentials by reconnect proof |
| Publication | Platform account publication/profile loader/Unix publisher and Windows publication; exact fixed leaf/ancestor object IDs, exclusive guards, target-before-activation crash recovery, committed cleanup |
| Migration | schema 28 and startup chains; frozen older schemas, row/key/profile preservation and foreign-key checks |

## Windows capability safety obligation

`VerifiedAccountProfilePublication::from_pinned_default_handles` is an unsafe
security-capability constructor, not a claim that Rust proves Windows sharing.
Its sole current call is the private native `Publication` wrapper. A reviewer
must confirm that its leaf denies write/delete sharing, ancestors deny namespace
replacement, and the original guard or exact handle clones survive through
Store activation. Exact volume/file-ID, type, ACL and fixed-path checks are
defense in depth; they do not establish caller exclusivity on their own.

`tests/Build/test_account_publication_boundary.py` rejects additional source
uses of that constructor. This search guard is not a semantic proof or a
substitute for reviewing future unsafe callers. Ordinary profile loading keeps
its existing anti-replacement sharing rules.

## Evidence and remaining gates

Fresh command results and counts are recorded in `PROGRESS.md`. Automated
code-review and architecture lanes are separate from the required actual human
review. Neither lane has authority to record a human review as completed.

Before release, attach the same final commit's native package/install/update and
Core restart results, Mac Intel/ARM results, and synthetic end-to-end account
connection/read/body/draft/send/Trash results. Unit and loopback tests do not
prove an interactive OS prompt or a deployed provider workflow. The Windows
pinned-handle tests cover native sharing, cursor and wrong-object refusal; they
are not a complete installed-product publication test.

If Core restarts, persisted staged decisions recover their exact publication;
in-memory app intents are not inherited by a fresh helper. Receipt durability
across that restart must not be advertised as implemented. A later legitimate
scope change must never cause recovery to regrant the old target.

The human record must name reviewer, full target commit, reviewed scope,
findings/disposition, unresolved items, date and explicit outcome. Do not
prepopulate a reviewer, outcome, future payload/digest approval or risk
acceptance. No real account, user vault, mail, ACL, credential, install or public
release was changed by this local validation.
