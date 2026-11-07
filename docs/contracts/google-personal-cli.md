> 2026-11-07 adopted implementation update: [account-qualified mail grants, model 2](account-mail-grants-v2.md) supersedes the read-only default and second terminal scope review **only for its exact bounded app connection decision**. The originating default CLI/TUI displays the selected account, local consumer and read/compose/send/single-message Trash features before fresh verified provider consent. It adds only that account's cells to that principal. Existing grants, MCP/AI identities and per-message send/Trash review remain unchanged. Unqualified/legacy refresh and recovery still use their existing explicit review. This is an implementation contract, not evidence that the new runtime or human security review is complete.

> Latest user correction (2026-11-02): [all-terminal workflows](terminal-workflows.md) supersedes native-window and Windows Hello requirements for every CLI/TUI path. Confirmation happens in the same console; provider OAuth retains its browser. Legacy descriptions below remain historical context. Security code requires human review.

# Google consent and personal CLI feature configuration

> 2026-11-03 approved extension: built-in AI mail access is a separate
> provider-qualified `ClientKind::Ai` flow defined in
> [`builtin-ai-mail.md`](builtin-ai-mail.md). Google account feature setup may
> be followed by automatic discovery for that built-in identity; it does not
> convert the Google CLI or MCP registration, and does not make `/agent on` or
> manual MCP registration a prerequisite. This extension remains an approved
> contract change pending implementation and human security review.

Latest personal sending UX: [terminal-personal-send](terminal-personal-send.md)
supersedes the native personal send window below. Preview, confirm, cancel and
result remain in CLI. Terminal choice is intent, not OS human-presence proof.

2026-11-02 owner decision: Google consent is the approval for connecting the
ordinary personal CLI; HanTong presents feature selection, not an additional
account approval or Windows Hello step. **Human security review required.**
This extension supersedes native-only account/CLI setup clauses in the older
Google onboarding, CLI onboarding, registration, profile and scope contracts
only for the bounded personal flow below. Actual send/mutation approval,
MCP/GUI identities, external-AI policy and advanced native configuration remain.

## Public feature selection and provider consent

`GoogleLocalProfile` is the finite enum `None | PersonalRead | PersonalReadSend`,
with wire strings `none | personal_read | personal_read_send`. Add `local_profile`
to GoogleSetupInput; an omitted legacy wire field means None. Unknown values,
duplicates, caller-selected tools/accounts/keys/approval results remain invalid.
PersonalRead requires ReadOnly provider mode; PersonalReadSend requires ReadSend.
PersonalReadSend may combine with reauthorize_account only for the fixed
same-account personal B->C feature action below. PersonalRead cannot. Advanced setup
with None retains its existing native behavior and does not configure CLI grants.

The guided CLI Connect action opens Google account selection/sign-in directly
with the fixed PersonalRead scope. It does not ask the user to choose a mail
operation before the account is connected. After Google consent and local access
are ready, show the work menu with explicit Enable composing and sending (4).
Only that later action requests the same-account send upgrade; connecting alone
never requests write/send access. Advanced commands may still explicitly select
PersonalReadSend. There is no additional HanTong/Hello approval. Cancel/EOF before
Connect has no OAuth/registration/grant effects. Selecting compose does not
approve a message.
External MCP configuration remains a separate opt-in action and is never
preselected by Google setup. Built-in AI feature setup is a separate
provider/account flow; when completed, its supported tools are discovered
automatically under `ClientKind::Ai`.
After ready, show available actions; never automatically read an inbox or send.

2026-11-03 reconnect correction: after a fresh verified OAuth permit, an existing
exact active same-account default CLI Google OAuth grant is an idempotent success.
PersonalRead matches body tools at epoch 2; PersonalReadSend matches send tools
at epoch 2 (initial L-to-C) or 3 (B-to-C upgrade). Do not create another scope row,
change keys/epochs, republish profiles or replace registrations for that case.
Other accounts, sources, states, mixed scopes and unexpected epochs retain their
existing conflict behavior for automatic OAuth grant configuration. Credential version CAS remains unchanged. A missing
or corrupted profile is a separate recovery issue, never an implicit repair.

2026-11-03 guided additional-account correction: when PersonalRead connects a
different account and the active default CLI already has the exact body-read
toolset with Google OAuth registration provenance (single-account epoch 2, or
multi-account epoch 3 or later with Store-validated scope history), account
publication completes without changing that registration or
issuing an OAuth scope refresh. The CLI then uses the existing read-only
account-add workflow from [Google onboarding](google-onboarding.md), with the
exact old/new account and tool snapshot selected in the same terminal. The
existing registration/key/epoch remain unchanged until that terminal selection
commits. Existing multi-account body-read grants follow this path too; probing
an already included account has no new scope effect. Authentication errors never
start a scope request. PersonalReadSend and existing writable/mixed/legacy scopes
are not broadened by this exception. Ready OAuth account publication is not
proof that additional local CLI access was selected; the CLI verifies that
separately before presenting the work menu.

2026-11-03 user correction: guided new accounts must support composing and
sending too. Guided connection requests PersonalReadSend and Google's ReadSend
consent. A fresh verified permit for a different account may hand off from an
active exact OAuth-origin C grant (singleton epoch 2 or 3, or multi-account
epoch >=3 with validated history) without changing any local grant. The CLI
authenticates the current C profile, forms the explicit union of its existing
accounts and only the selected account, then runs RequestGoogleSend in the same
terminal. Core checks every selected Google account's Send capability and
preserves all old accounts/tools before staging the existing terminal_cli v10
refresh. Authentication failures never start this selection. Up to 128 selected
accounts share RequestedScope's existing resource bound; no four-account limit.
After confirmation both old and new accounts retain C. Each message still
targets exactly one account and needs its own immutable preview confirmation.
This is not authority to send mail during setup or grant external AI/MCP access.
ReadOnly/Modify or mixed non-C scopes are not silently reinterpreted as C.

Setup receipts now carry bounded `local_profile` routing metadata copied from
the Core session input. A resumed personal send setup must follow its original
send-scope terminal handoff, not the legacy read-only continuation. Missing
metadata from old servers means `none`; unknown values and an explicit flow
mismatch are rejected. This receipt field is never OAuth or grant authority.

## Authority and identity

Only successful live PKCE/state/scope/identity verification inside Secrets may
lead to personal account publication. No receipt, account existence, TTY/name,
caller boolean, client-supplied OAuth result, Google URL or capability selection
alone is authority. A private move-only Core GoogleBootstrapAuthority binds the
setup request, observed OS pipe peer user/logon, exact Google account/subject,
client ID, credential version, requested mode/profile and current expiry.
Consume it once and revalidate the current binding, peer, expiry and requested
feature mapping. Never construct or fake VerifiedRegistration/VerifiedScopeRefresh.

IPC adds SetupRequestContext carrying the actual connection.peer_context().
The constructor/fields are private to IPC; Core gets the verified peer through
a getter. New dispatch_connection_with_setup_context/serve_until_with_setup_context
callbacks receive &SetupRequestContext and SetupCommand. Existing setup APIs
remain compatibility wrappers. Personal request/status deduplication binds the
same observed peer. There is no feature-configuration approval tool or endpoint.

## Durable configuration

Core alone may configure CLI/default for the exact freshly connected Google
account: original L registration, then existing fixed B (read) or C (read/write/
send tools). Non-exportable CNG keys, request-derived ID, server pin, profile
epoch, current policy, rate limits and account scoping remain unchanged.
An existing other registration/profile is never replaced. Never widen tools
for an old account as a side effect of adding a different account. Conflicting
or unsupported mixed scopes fail closed rather than invoking hidden approval.

Store adds stage_google_oauth_cli_registration(&CliRegistrationStage) and
create_google_oauth_cli_scope_refresh for this narrow transition. Existing
native entry points retain native provenance and their previous restrictions.
Default personal setup starts with exact L/single canonical Google account;
its initial refresh is exact same-account L->B or L->C, epoch1/no prior history.
Any supported later addition must preserve old accounts/tools exactly and bind
the new account to fresh OAuth authority; otherwise it is rejected.

DB v22 permits `google_oauth` as registration request/record channel and adds
client_scope_refreshes.decision_channel (approval_helper|google_oauth). Existing
rows retain their channel; old refresh rows backfill approval_helper. V1-v7
hashes remain frozen. V8 hashes bind the length-prefixed channel in a new domain.
Readers validate request/record source equality, fixed CLI/default shape and
source-bound scope history. GUI/MCP may never claim google_oauth. A migration
creates no account, key, profile, registration or grant and preserves every old
row, sequence high-water and encrypted backup/rollback guarantees.

Crash order: activate verified Google account; consume OAuth bootstrap authority;
stage/publish/activate L; create/stage/publish/activate B/C. Existing provisioning
recovery validates the recorded source without manufacturing a new authority.
An account alone after an interrupted setup cannot authorize fresh CLI access.
Fresh Google connection or the existing advanced native path is needed.

## 2026-11-02 beta prerequisite: explicit existing-account send feature

The owner requested actual send verification and conditional beta publication.
The guided work menu adds a finite enable-compose/send action for its selected
account. It requests PersonalReadSend/ReadSend with reauthorize_account set to
that exact account. Google identity, bound client and credential version must
match; normal OAuth verification and the one-use permit remain mandatory.
No client-selected account set/tools/source/approval is accepted.

The only new durable transition is an active OAuth-origin CLI/default registration
at epoch2, exact single canonical Google account, exact B scope -> same account C.
Require exactly the initial active v8 L->B history and no other history/open job.
Keep registration identity, key, server pin and all account boundaries unchanged.
Already configured C is verified without silently broadening scope. Other or
mixed registrations and downgrade requests fail closed.

Store adds create_google_oauth_cli_send_upgrade(refresh_id,target_scope,pin).
Hash v9 uses domain mail-hantong.cli-scope-refresh.v9 and the same length-prefixed
channel encoding as v8. V1-v8 bytes and allowed transitions remain frozen. V9
requires google_oauth source, base epoch2/B, exact same-account target C. Schema
v23 adds only this history shape, with encrypted backup/full-row/FK/sequence
preservation. Source-bound pending/staged recovery also accepts v9. This is feature
configuration, never per-message send approval. Human security review required.

## Verification

Synthetic tests cover invalid/expired OAuth, identity/mode/peer mismatch,
single-use authority, unknown local profiles and strict legacy compatibility;
no post-Google native review for personal flow; no effects on cancelled feature
choice; source/channel/hash tamper, wrong kind/label/account and profile conflict;
L->B/C policy/audit/protected profile integration; migration preservation and
rollback; interrupted key/profile stages; no AI/MCP grant; per-send human proof
still required. Automated tests never use actual accounts or provider traffic.

## Personal per-message confirmation, 2026-11-02

The native-send-ledger personal exception now applies after this exact writable
single-account personal Gmail setup. Review contents and press Send in the
native window; no Hello is requested for that bounded send. Account connection
and per-message confirmation remain distinct. Core derives eligibility from
persisted registration and rechecks it in the claim transaction. Other profiles
retain OS verification. CLI identity is not evidence of human origin. See
proposed ADR-028; human security review remains pending.
