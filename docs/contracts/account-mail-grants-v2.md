# Account-qualified mail grants, model 2

Status: adopted for local implementation by the user's 2026-11-07 instruction
“변경 진행해”, following the full-mail bounded-connect proposal. This records
implementation authority, not actual human security review, provider consent,
permission to operate real accounts, or release qualification.

## Scope and compatibility

Every supported connected account must support read, compose/send and single-mail
Trash. Default CLI/TUI, GUI, MCP and provider-qualified AI retain separate identities.
Connecting one account cannot grant another identity or broaden another account.
First implementation targets the existing default CLI/TUI consumer; no GUI or
external-agent route may impersonate that consumer. FullMail capability is not
unattended message approval. All existing per-message review/claim/policy/audit
and uncertain-outcome nonretry controls remain mandatory.

Scope model 1 and all historical profile/hash/transcript/schema definitions remain
frozen. Model 2 is a different account-qualified representation, not Unix auth v2.
Its profile schema is 2; Windows authentication revision is 2 and Unix revision is 3.
Signatures bind the exact model/representation and cannot be replayed across revisions.
A model-2 registration rejects legacy authentication even with a flattened scope or
old profile. No compatibility path flattens a model-2 permission matrix.

## Canonical account scope

`AccountScopeV2` owns a sorted, unique bounded account list with each account's
sorted unique tool set and read-only restriction. It exposes account-qualified
permission checks and pointwise intersection; no account/tool union is an authority
API. `mail.accounts.list` is the sole principal-level mail discovery tool, lists
only authorized accounts, and cannot authorize body or mutation access. The two
existing accountless `AppReadTool` capabilities (`app.settings.display.get` and
`app.diagnostics.snapshot`) also remain explicit principal tools; they are never
account grant cells. This preserves their frozen zero-account semantics, not a
new grant. Migration copies only the capabilities actually present in the legacy
registration. No principal-level body/send/Trash/event authority is introduced.

Wire JSON has exactly `scope_model_version` (integer 2), `principal_tools`,
`toolsets` and `account_grants`. Each `toolsets` element is a sorted unique tool
array; arrays are unique and canonically sorted. Each account grant has exactly
`account_id`, `toolset` (zero-based dictionary index), `read_only` (boolean).
Grant rows are sorted by canonical account ID. Unknown/duplicate JSON keys, bad
references, unused dictionary entries, unknown tool semantics and noncanonical
order are rejected, not repaired. Builders may canonicalize trusted typed data.
Maximum 128 accounts/toolsets, 128 tools per set, 128-byte account IDs, 96-byte
tool IDs, and 24 KiB canonical encoded scope; existing enclosing frame limits remain.
The signed scope bytes are the compact UTF-8 canonical JSON of this exact shape.
No runtime/tool/profile parser accepts a union fallback.

The frozen ordinary tool registry consists of existing body/send/manage tools,
GUI read tools, event tools and the existing account-qualified Google-change
capabilities, without inventing extra operations. Unknown legacy semantics fail
closed rather than migrating authority incorrectly. For ordinary tools, translate
legacy accounts × tools into equal effective cells; account-qualified change
capabilities remain confined to their account. Preserve legacy read-only restrictions,
state/expiry/rate/policy vetoes. Prove old effective permissions equal translated
permissions for every recognized tool/account. Schema migration grants no FullMail,
creates no account/key/consent, and changes no historical row/source/hash.

## Bound app connection decision

Core creates a random one-use immutable intent for one originating authenticated
registration, exact ID/kind/key/epoch/version/server pin and old scope hash, one
selected account intent and finite FullMail feature mapping. App UI displays the
account/local consumer/features before provider consent; UI booleans, actor labels,
helper identity or same-user process observation are not authorization evidence.

Only live PKCE/state/scope/identity verification can produce a private Core Google
permit bound to intent/request, actual account/subject/client/credential version,
observed peer/session, mode, expiry and the registration baseline. An entered address
is a selector only. For an existing account the exact stored subject must match;
for a new account fix the subject from the verified provider identity and reject
any mismatch with the selected account intent. Existing tokens/ready state/status
receipts cannot mint the permit. Model 2 adds exactly that account's FullMail cells;
old account cells and all other principals remain unchanged.

Consume the permit and persist the exact target staged decision in one CAS
transaction. Reject concurrent baseline changes, unrelated pending/staged scope
work or mismatched idempotency; never rebase, auto-cancel, merge ready accounts or
reauthorize old accounts. Original registration metadata/key/source remain intact.
The new account decision uses a distinct versioned history/hash domain, not a
reinterpretation of terminal_cli or initial google_oauth evidence.

## State machine and recovery

### Authenticated app service and foreground consumer

Account connection uses exactly `app.accounts.connect.prepare`,
`app.accounts.connect.status`, `app.accounts.connect.complete` and
`app.accounts.connect.cancel`, plus `app.accounts.connect.bind_smtp` for the
generic protected-setup continuation. These are bounded Default-principal app actions,
not mail tools, unauthenticated setup methods, or an arbitrary method-prefix
allowlist. Each request is at most 4 KiB and uses protocol version 1. Prepare
selects Google or IMAP/SMTP and either a new account or one exact existing
account. Generic prepare selects an exact existing pending IMAP setup request
using its nonzero 32-byte opaque request ID (`selection.kind=imap_setup`).
BindSmtp contains exactly protocol_version, intent_id and the nonzero 32-byte
SMTP request_id; both references are selectors revalidated by Core, not permits.
FullMail is the fixed feature set; no supplied tools, scope, profile
path, subject, key, epoch, or approval boolean can select authority. Later
status/complete/cancel commands contain only the intent ID. Responses bind that ID, provider, finite
state and expiry; they contain no credential or provider response text.

On Unix a dedicated foreground Core consumer loads only the fixed Default
profile and its registration-bound opaque signer inside Core/Secrets. It
authenticates directly to the daemon using the profile's actual model/revision,
and remains the same process through provider completion. CLI/TUI never load a
private key. Ordinary `unix-relay`/`core.relay` rejects this app-service family.
The helper image is process admission, not permission evidence: the daemon must
verify the Default signature, registration state, exact scope/epoch/server pin
and kernel-observed peer on every command. A restarted helper cannot inherit an
old process-bound intent. This is no OS human-presence claim.

The interactive CLI requires terminal stdin/stdout before starting the verified
foreground consumer. Its child inherits terminal stdin/stderr; child stderr is
the screen/progress channel and stdout is a private, at-most-4-KiB result pipe.
The child verifies terminal stdin/stderr, never reads a credential into CLI
memory, and emits one strictly validated ready receipt to that pipe. The parent
validates provider, selected account, finite ready state and frame bounds; this
metadata is not an approval or authorization capability. Missing/partial result
is uncertainty, not permission to repeat completion or undo staged work.

The app displays the local consumer and FullMail features before starting the
provider flow. Completing the original Connect action needs no second local
scope prompt: a fresh verified provider result and the bound authenticated
continuation authorize only that intent's exact account delta. Status alone
does not consume or activate it. A lost/uncertain completion response is resolved
by status and exact staged recovery, never by repeating OAuth or a new grant.

Before DecisionConsumed + GrantStaged, cancellation/EOF/expiry, provider failure,
wrong subject/client/account/peer/mode/credential/baseline yields no local grant,
epoch/profile change or mail operation. Provider OAuth and local authorization are
not a distributed transaction: an external consent/credential may already exist,
but it cannot authorize a later implicit local grant and is not automatically revoked.

Persist staged target separately; effective stored rights remain the old model
while provisioning denies new privileged access. Publish the exact protected schema-2
profile, then atomically activate target matrix/model/epoch. Recover only the exact
recorded target. Never publish a flat fallback, repeat OAuth/provider operations or
reinterpret committed staged intent as pending. Expiry/cancel after a valid consumed
decision cannot silently undo that decision. Exact completed duplicates return status;
divergent duplicates/replay cause no second grant/epoch transition.

## Generic mail readiness

Use existing Secrets ticket/capture/stage/probe protections bound to account,
IMAP/SMTP endpoints/settings and sender; no Google proof is fabricated for IMAP.
FullMail readiness requires verified IMAP read, SMTP readiness, exactly one selectable
Trash mailbox and supported MOVE. Persist endpoint/settings/catalog-generation-bound
readiness and invalidate it when these change. It is capability evidence, never a
grant. Per-operation binding checks remain mandatory. Missing functionality is
incomplete setup, not success or permanent-delete fallback. Existing working data
and credentials are preserved. A separately bound Core app setup intent, not a
password or UI bool alone, authorizes this principal's exact account cells.
The persisted record is valid only while its exact versions, catalog generation
and Trash binding match current observations. A stored `full` label alone is
never readiness evidence; consumers recompute current bindings rather than
accepting a stale cached label.

The generic continuation must bind the pending IMAP setup request before its
protected capture. A setup reference is only a selector: Core loads the request,
requires its exact review-pending leaf, snapshots its account/endpoint/payload and
version, and binds it to the authenticated Default principal and observed process.
After that request's successful credential consumption and IMAP probe, bind one
exact pending SMTP session (including sender/endpoint/account versions) before
its protected capture. A previously completed setup or a prior ready observation
cannot substitute for either fresh continuation. The same foreground process
performs both captures; neither CLI memory nor app-service JSON carries plaintext.
For an already configured, exactly unchanged SMTP endpoint, the bound app
continuation can reuse its stored credential instead of replacing it. A new
SMTP authentication probe after binding is still required, together with the
fresh bound IMAP capture/probe; the old SMTP ready observation is insufficient.
The daemon reports whether protected SMTP input remains necessary through the
finite awaiting_setup/provider_ready state, never a supplied approval boolean.

An already ready IMAP account uses a distinct `account_connect_setup` protected
review, addressed by the bound app intent ID. It does not reopen, reinterpret or
overwrite the completed historical IMAP setup request. The new review's owner,
epoch, endpoint/settings, consumer baseline, observed peer and expiry come from
the authenticated app intent; Confirm without protected input is rejected.
Its one-use Secrets ticket accepts fresh hidden input and performs a live IMAP
authentication using an in-memory protected credential, without publishing or
replacing a stored credential. Only Secrets can create the move-only verification
witness, bound to the exact request, payload, account, endpoint and expiry.
Core separately verifies that the unchanged stored IMAP/SMTP credentials still
authenticate and that fresh folder discovery supports the required Trash/MOVE.
Existing ready flags, a captured value alone, or an unrelated successful probe
cannot mint the account delta. A mismatch or failed probe leaves working stored
credentials intact; cancellation, expiry and the existing staged rules still apply.
If the existing IMAP account has no SMTP configuration, bind its exact new SMTP
setup session before SMTP capture. Only that previously absent endpoint may be
configured; existing endpoints and credentials cannot be replaced by this path.
The same intent then requires its fresh IMAP witness and fresh SMTP authentication.

Only the live bound continuations, current successful SMTP authentication (without
sending mail), and a fresh complete mailbox catalog satisfying unique Trash/MOVE
can produce the generic private delta. Record the exact setup request identities,
payload hashes, endpoint/observation/catalog versions and consumer baseline in its
authority digest. Cancellation/expiry, resume to a different leaf, changed sender
or endpoint, missing capture, stale observation or wrong peer cannot produce it.
After a restart, pending generic setup may be resumed through normal protected
setup, but cannot automatically recreate an app grant; only a consumed staged
grant follows the existing exact publication recovery. Existing credentials and
working account metadata are preserved on local authorization failure.

## Required evidence

Test A/read-only + B/FullMail isolation, explicit A-only upgrade, all principal
separations, exact legacy permission translation, strict profile/wire downgrade
rejection, source/hash tampering, single-use/expiry/cancel/mismatch, concurrent
decisions, each stage/publication/activation crash boundary, DB preservation and
unchanged per-message approval. Test all read/send/Trash features through fake
providers and native product integration, not only DTOs. Real provider trials,
human security review and same-final-source release gates remain separate.

c22e1bf recovery/release and its exact E9 payload remain frozen in the previous
worktree. This new UX is not required for its pending-scope repair; do not fold
unreviewed new code into that candidate. Songpa's final goal remains Gmail2+Daum1
actual list/body reads, without repeated denied grants, resets or actual mail writes.
