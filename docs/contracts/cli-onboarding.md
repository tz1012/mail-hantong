# Guided CLI first run


> 2026-11-02: [Google personal CLI extension](google-personal-cli.md) supersedes native-only account/CLI setup clauses only for the bounded guided personal flow. Live verified OAuth authorizes its fixed feature configuration; feature selection alone does not. Per-send/mutation approval and advanced native setup remain. Human security review required.

Implementation extension, 2026-11-02 user instruction to implement the single
install → launch → connect → approve → use flow. Human security review required.
Related: CLI-01/04, ACC-02/03, SETUP-01, INST-03/04, APR-01.

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
