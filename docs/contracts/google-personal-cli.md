# Google consent and personal CLI feature configuration

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

The guided CLI first offers read mail or read/write/send. Selection happens
before opening Google so only that feature's provider scopes are requested.
Google consent then connects the account and applies the selected fixed local
feature set without an additional HanTong/Hello approval. Cancel/EOF has no
OAuth/registration/grant effects. Selecting compose does not approve a message.
AI/MCP configuration remains a separate opt-in action and is never preselected.
After ready, show available actions; never automatically read an inbox or send.

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
