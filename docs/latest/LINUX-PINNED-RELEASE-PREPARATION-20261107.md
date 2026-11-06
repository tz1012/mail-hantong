# Linux pinned release preparation

Status: disconnected local preparation; production contract approval and security
design/review blocked. No release, origin, channel, Songpa installation or account
operation is authorized by these preparation results.

## Fixed baselines and scope

- Last four-platform general-CI baseline:
  `c2e1aa4f601e8b2094e2911c02bfca14121143f9`, run 42226733662.
- This local preparation starts at
  `0420e378ed2c4f79042805c7abebb889f34a318e`; it includes the later Mac test tools
  as existing source, not as successful Mac compatibility evidence.
- No deployable commit/version/assets are frozen. New local prototype files are
  not packaged or invoked by install.sh, install.ps1, package_unix.py or CI.
- Linux package/native restart evidence at
  `9c7f91eea0933c8d957be33d3e3b98267918ee22` / run 42206703892 is historical,
  not proof of this preparation or a future final release commit.
- Mac run 42223324904 at
  `95349fed75eb3b0faa2a547d0bf79b531878f67d` failed. Both architectures'
  old-credential/candidate-start compatibility remains blocked, not waived.

## Approval split

The user authorized local code and disposable synthetic tests. The proposed
[contract and platform policy delta](../contracts/proposals/linux-pinned-release-20261107.md)
is NOT adopted. AGENTS sections 6–7 continue to gate new trust/activation contracts.
Before integration, approve an exact separately owned Linux origin and its trust
rules, independent Linux eligibility while Mac remains blocked, and the pinned
descriptor/provenance/activation contract. No actual origin is configured here.

No new libraries, release jobs, runtime authority or signing/Keychain behavior are
added. No environment variables emulate production approval or CI authorization.
The proposal prototype has no network or process execution capability.

## Local implementation and verification claims

`scripts/linux_pinned_candidate.py` is a disconnected verifier, not an installer,
updater, published schema or authenticity authority. Its local descriptor is a
test fixture. Only its exact tested binding checks may be reported as verified;
native product execution, trusted publication and immutable execution are distinct.

`test_linux_local_update.py` exercises the unchanged shell package installer with
disposable HOME/XDG roots and synthetic executable scripts. It tests upgrade,
corrupt checksum, failed staging, interruption before current.txt activation,
recovery via higher-version retry and downgrade refusal. Account/key/profile/vault/
mail sentinels are synthetic byte-preservation checks, not database, real Core,
Secret Service or actual account compatibility tests.

`test_linux_feed_isolation.py` uses fake curl/uname and PowerShell mock transports.
It checks Linux/Mac selection with the unchanged feed and demonstrates that a
Linux-only first tag would fail both Mac targets and Windows 403/429 fallback.
The expected rejection is evidence of a shared-feed hazard, not its repair.
Public scripts/packaging/workflow files remain unchanged. OS-specific fixture
skips are reported and covered on the applicable Windows/WSL host, never hidden.

## Implementation after contract approval

1. Freeze an exact production source commit after the approved delta. Include
   scope recovery and execution-boundary fixes; enumerate Mac test-only source
   separately. Do not combine assets built from different commits under one hash.
2. Implement a fixed Linux-only HTTPS origin with bounded asset transport and
   exact version pins. No latest lookup or fallback to the existing Atom/API feed.
   A trusted descriptor must be independently pinned/reviewed, not merely fetched
   beside untrusted payloads. Reuse audited package logic after OS/CPU validation.
3. Bind source commit/archive/canonical source hash, Cargo.lock, package filename,
   release label/target, manifest, metadata and every installed file. Cargo package
   version and release label are distinct existing fields; do not conflate them.
4. Review activation locking, hash-to-execution immutability and Core lifecycle.
   Resolve same-UID same-inode writes before claiming safe execution. Offline hash
   checks and retained file descriptors alone do not resolve this risk.
5. Run the same-commit Linux native package/install/update/Core restart gates and
   scope recovery tests. Include cancellation binding/replay, expiry ordering,
   origin provenance, staged recovery and unchanged pre-approval authority.
6. Run Windows normal API plus 403/429 fallback and both Mac target selection
   regressions against unchanged legacy feed fixtures. Actual Mac compatibility
   stays blocked until its independent Intel/ARM tests pass.
7. No downgrade or manual current pointer repair. Prove compatibility using
   synthetic old DB/profile/vault fixtures before any rollback implementation;
   otherwise preserve data and deliver a higher-version roll-forward.
8. Human review records the exact final commit/range/files, actual reviewer
   confirmation, findings and closure. Only then consider the separately approved
   publication action; fresh downloaded assets must match final checksums.

## Required evidence not produced by this preparation

No final same-commit native Linux package, installed Core restart, real DB/vault
upgrade, native Mac execution, final four-platform CI, independently trusted
source-to-Git proof, same-inode mitigation or human security review is claimed.
Those are explicit open gates, not successful tests omitted from a report.
Songpa's three existing accounts remain untouched. Future validation must use
normal account/tool previews and explicit approval; no automatic grant, fresh
credential generation, mail send or Trash operation is part of this preparation.

## Local verification record

The fixed preparation commit is recorded with its canonical source digest in
`target/linux-pinned-preparation-evidence.json`. Fresh Windows and WSL logs are
`target/linux-pinned-preparation-windows.log`,
`target/linux-pinned-preparation-wsl.log` and
`target/linux-pinned-preparation-docs.log`. These are local synthetic evidence,
not release assets or native four-platform CI attestations.

The new 20 tests cover different host capabilities: WSL executes 18 and explicitly
skips the 2 Windows-only tests; Windows executes 12 and explicitly skips 8 Unix-only
tests. Existing Windows selection/rate-limit (9+5) and Unix package-shape (5) tests
are included in the Windows regression run. Docs has 29 tests. The two hosts'
counts are execution counts, not a count of unique security requirements.

Independent AI review found the ZIP preallocation and tar extension-parser gaps;
both were repaired and re-reviewed with hostile archive fixtures. It found no
additional CRITICAL/HIGH parser blocker in the disconnected prototype. This does
not replace AGENTS section 6 human review. Human review must record the exact local
preparation commit's seven changed files and later the full production integration
range; no earlier user confirmation is extended to these new files.
