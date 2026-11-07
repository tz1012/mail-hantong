> 2026-11-07 adopted implementation update: [account-qualified mail grants, model 2](account-mail-grants-v2.md) supersedes the read-only default and second terminal scope review **only for its exact bounded app connection decision**. The originating default CLI/TUI displays the selected account, local consumer and read/compose/send/single-message Trash features before fresh verified provider consent. It adds only that account's cells to that principal. Existing grants, MCP/AI identities and per-message send/Trash review remain unchanged. Unqualified/legacy refresh and recovery still use their existing explicit review. This is an implementation contract, not evidence that the new runtime or human security review is complete.

> Latest user correction (2026-11-02): [all-terminal workflows](terminal-workflows.md) supersedes native-window and Windows Hello requirements for every CLI/TUI path. Confirmation happens in the same console; provider OAuth retains its browser. Legacy descriptions below remain historical context. Security code requires human review.

# Guided CLI first run

Latest 2026-11-02 correction: compose/review/send/cancel use the terminal-only
personal flow in terminal-personal-send.md. The previous native personal sending
window is superseded. Provider OAuth still opens the browser.


> 2026-11-02: [Google personal CLI extension](google-personal-cli.md) supersedes native-only account/CLI setup clauses only for the bounded guided personal flow. Live verified OAuth authorizes its fixed feature configuration; feature selection alone does not. Per-send/mutation approval and advanced native setup remain. Human security review required.

Implementation extension, 2026-11-02 user instruction to implement the single
install → launch → connect → approve → use flow. Human security review required.
Related: CLI-01/04, ACC-02/03, SETUP-01, INST-03/04, APR-01.

2026-11-03 owner correction: choosing Connect Google account (including add and
Connect another account) proceeds directly to the existing Google browser flow
with PersonalRead. Remove the pre-connection feature menu. Only after connection
is ready show mail actions and explicit option 4 to add composing/sending. There
is no automatic inbox read, send permission request or message send on connection.
Earlier descriptions of the feature-before-connection sequence are superseded.

`hantong init [--lang en|ko]` opens the guided flow. Bare `hantong` does so only
when stdin and stdout are terminals. Help/version remain side-effect free.
Explicit init rejects JSON, no-input and redirected input/output with
HUMAN_STEP_REQUIRED before starting Core, loading a profile or opening OAuth.
Unknown options/client selectors on init are INVALID_INPUT. Headless bare CLI
retains its existing noninteractive error behavior.

The first-run controller first probes Core readiness. A ready Core is reused.
Only CORE_UNAVAILABLE allows one start attempt. Authentication, access, version
and other errors never cause replacement, registration, credential capture or
permission expansion. An installed Windows CLI may launch only its sibling
bin/mail-hantong-core.exe from the same release directory after bounded manifest
validation and matching CLI/Core SHA-256 records, rejecting links/reparse paths,
duplicate keys and unsafe paths. It never searches PATH or accepts an executable
argument. The process runs `run` without a console, caller input or copied mail
arguments. Readiness waiting is bounded; no repeated spawning or killing of an
existing Core. A Core started for interactive use remains available after the
menu exits. No Windows startup/registry registration is added. This extends the
invocation of the existing foreground Core command, not its IPC authority.

Authenticated allowed Gmail accounts are shown with escaped display labels and
numbered menu choices. CLIENT_NOT_REGISTERED yields the connect menu; other
profile/auth errors fail closed. Existing accounts use the existing read-access
probe and native review if needed. New Google connections reuse the existing
protected same-client configuration and readonly OAuth mode. Native verification,
300-second stage deadlines, exact account scope and separate local access review
remain unchanged. Missing publisher/client configuration is reported explicitly;
the flow never invents or embeds a real client ID, secret, token or consent.

The normal menu omits internal request/account/message IDs and operation hashes.
Advanced login/resume commands remain available. Connection alone performs no
mail read. Choosing View inbox explicitly requests one INBOX page (maximum20),
polls its account-bound execution for a bounded duration, and lists that cache.
The same common policy/audit/version/idempotency path is used. No automatic
retry follows an interrupted/failed sync, and no write/send grant is requested.
Cancel/EOF exits without new effects; invalid choices reprompt. Input is bounded.

Synthetic tests cover the state transitions, denial/cancellation, no implicit
inbox, invalid/headless entry, package/hash/path validation, one-start gating,
and a bounded sync/list seam. No automated test uses a real account or OAuth.

2026-11-02 installation correction: all components resolve LocalAppData with
KF_FLAG_NO_PACKAGE_REDIRECTION so a packaged terminal host cannot redirect one
component to a private data store while sharing the same OS-user pipe endpoint.
Core prepares the fixed product/profile directories with the existing protected
creation ACL before storage creates children. Existing unsafe ACLs are rejected,
never silently repaired. Preparation creates no registration or approval. Old
package-private user data remains untouched; its migration is a separate human
step. Optional publisher provisioning follows publisher-google-configuration.md;
its actual credential publication still requires the owner's policy decision.

Google publishing and verification are separate external states. The homepage
being published is not evidence of either Console state. This contract does not
authorize actual account login, credentials/grant changes or provider testing.

## 2026-11-02 beta guided compose/send

To complete the owner's simple install/start/connect/consent/use flow, retain
3=Exit and 4=explicit same-account send feature activation; add 5=Compose mail.
The selected account must already have exact personal ReadSend access before
composition. Scope denial does not implicitly request OAuth or widen access.
The menu collects bounded recipients, subject and multiline plain text, with
clear cancellation/EOF handling and no command construction. Optional attachment
paths use the existing bounded attachment snapshot loader; no new provider API.
After a complete valid form, create one local immutable draft using the existing
policy/audit/idempotency APIs. An explicit choice may request the native review
of that exact draft/version/hash. The CLI never issues approval or treats its
confirmation as authority. The native message review and human verification
remain mandatory. Display bounded stable status, never raw internal identifiers
or provider errors. Cancellation does not send; pending/unknown outcomes are
never retried automatically. Automated tests use synthetic Effects only.

## Personal sending confirmation, 2026-11-02

After composing in the personal Google flow, the CLI requests a native content
review once and polls its status. The native window displays the complete
message and its Send button issues a distinct personal confirmation without
Windows Hello. The CLI prompt only opens that window. Eligibility and atomic
claim guards follow native-send-ledger; other registrations keep OS verification.
No human/direct/approved wire option is introduced. Unknown never resubmits.

## Startup diagnostics, 2026-11-03

An unavailable Core does not authorize account reset, database deletion, ACL
repair or new OAuth consent. Core startup failures may report only a fixed
stage name and stable error code: directories, profiles, publisher, storage,
identity, instance, runtime or transport. The CLI drains a bounded 16 KiB
stderr capture while waiting and relays only exact allowlisted failure lines
after startup fails. It never relays arbitrary child stderr, underlying error
text, user paths, account information, database content or credentials. Capture
does not create a persistent log. Existing readiness deadlines and child
ownership/cleanup remain unchanged. A diagnostic stage locates a failing
boundary; it does not establish the root cause or authorize bypassing it.
