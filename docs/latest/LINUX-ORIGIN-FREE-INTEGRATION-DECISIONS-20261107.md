# Linux origin-free integration preparation

Status: proposed integration plan, not an adopted runtime contract or release.
Base implementation: `075b0ca5f7ca144ff31fb4a86b907eda9d397ab9`.

Latest priority: the user clarified that completion means actual reads from
Songpa's two Gmail accounts and one Daum account, not this design, a security fix
or a deployment. The [goal-first route comparison](SONGPA-THREE-ACCOUNT-READ-PATH-20261107.md)
supersedes any implication that a new origin is a prerequisite for that goal.
Stages below describe the proposed new distribution/execution track only.

## Current authority and evidence

The user answered **“미정입니다. 원점·채널을 임의 생성하거나 공개하지 마세요.”**
The Linux origin is therefore explicitly undecided. Do not create a repository,
feed, channel, signing authority or public release. Continue local proposals and
non-destructive preparation. Earlier conditional deployment authorization remains
distinct from contract adoption, actual human review and successful product tests.

The base commit binds candidate packages to a clean full source commit and exact
source archive. Its metadata intentionally says `blocked_identity_and_origin`.
At that commit Windows targeted tests ran 57 (42 passed, 15 platform skips), WSL
ran 38 (36 passed, two Windows-only skips), and Docs ran 29, all passed. These are
targeted executions, not a new four-platform full test result. Four WSL kernel
experiments use non-executable synthetic memfds; they do not prove sealed product
execution. Mac compatibility failures remain unresolved.

## Decisions to review, not permissions already inferred

| Decision | Proposed narrow choice | Alternative and unresolved cost |
| --- | --- | --- |
| Linux distribution location | Separate owner-controlled HTTPS namespace or separate GitHub repository, exact address later | Never add a Linux-only tag to the current shared Atom feed. Neither candidate location has been created or selected. |
| Initial publisher trust | Explicit, independently delivered SHA256 pin plus an independently trusted verifier/launcher; exact version only | A separately anchored signing key supports recurring verification but still requires trusted bootstrap, key ownership, rotation and revocation policy. No key or signing dependency is selected here. |
| Execution identity | Review identity-v2 per-process sealed image plus exact trusted release/root/manifest/asset context | Shared memfd objects preserve inode equality but need a trusted lifetime owner before any client starts, restart rules and recovery. Do not remove inode checks without adopting either complete model. |
| Update execution | Review a native fixed-operation updater so no new shell code is interpreted | A sealed script needs independently verified interpreter bytes, its descriptor lifetime and loader policy. The current `update.sh` is not that solution. |
| First old-Core transition | Remains undecided: authenticate the exact predecessor and verify termination before pointer activation | An old stop path is usable only if its own execution satisfies the adopted protection model. Current stop still uses mutable images, and current updater activates before stop; neither is proof of the proposed order. New-helper signaling needs a reviewed predecessor rule. |

A manually delivered pin is only a trust anchor if its delivery is independently
trusted; fetching `SHA256SUMS` beside the package is not independent. A fixed local
pin also does not resist a same-UID adversary replacing that pin before startup.
Do not convert a proposed delivery model into an authenticity claim.

The minimum publication decision must name the owner, exact origin/base path,
permitted download and redirect hosts, immutable version/asset naming and retention,
descriptor authentication/bootstrap and update freshness/downgrade policy. None is
needed to run an offline synthetic test; all are needed before real transport.

## Concrete contract review packet

These are candidate rules for ADR-033/identity-v2, not active wire fields or code:

1. Use a versioned, bounded inherited-FD map with one role each for checked release
   root, authenticated pin, manifest snapshot, release context and executed image.
   Specify the exact FD slots, maximum byte counts, canonical encoding and unknown
   field/role rejection before implementation. Never discover authority via a memfd
   name, argv or an environment path. Public metadata contains no credentials.
2. Open the release root with current ownership/no-link guards and retain its FD.
   Bind the fixed caller asset and sibling asset to that root and authenticated
   manifest. An inherited sealed JSON claim alone cannot authenticate its sender.
3. Copy the exact selected asset to a new executable memfd with a bounded loop.
   Require WRITE/GROW/SHRINK and final seal-set verification, then read back its
   size and SHA256 against the fixed manifest; only that same held FD may execute.
   Existing writable mappings, missing seals, short reads and mismatches abort.
4. Keep context/image descriptors across the intended exec only. Audit each fixed
   child entry point for role inheritance, duplicates, leaks and close-on-exec;
   unrelated children inherit none. Receiver checks its actual kernel executable
   object against the retained image FD, not just supplied context fields.
5. Peer admission still checks UID, kernel process instance and actual executable
   seals/digest. The proposed cross-process v2 equality additionally requires the
   same authenticated release, retained root identity, manifest and expected Core
   asset. Different memfd inodes are not silently treated as the old equality.
   Recheck process instance around inspection; denial to inspect is a denial.
6. Verified stop retains fresh pidfd acquisition and before/after image/instance
   checks, bounded termination and socket disappearance. No arbitrary PID, digest,
   predecessor path, process-name kill or fallback signal is accepted.
7. `vm.memfd_noexec=2`, unavailable kernel primitives or image inspection denial
   fail closed. Do not change sysctls, namespaces, mounts, permissions or executable
   flags to evade the host policy. Do not fall back to a mutable installed image.

FD framing, trust delivery, environment/loader handling and predecessor semantics
must be approved together; the list is not a license to choose magic FD numbers or
weaken the existing protocol while the other pieces remain absent.

## First-execution and same-UID boundary

A mutable CLI that has already started cannot retroactively authenticate its own
first instructions by re-executing a sealed copy. A user shell invoking an installed
binary and the trusted verifier/launcher used before that invocation need an explicit
bootstrap trust assumption or an independently protected launch mechanism. This is
distinct from preserving the selected bytes after the final checksum. Do not claim
complete direct-entry protection from an in-process re-exec alone.

For a guarantee covering the first launch under an active same-UID writer, review
an OS-enforced protected/immutable bootstrap or an independently trusted launcher.
Filesystem verity would introduce an unproved filesystem support requirement;
do not enable it or relax its requirements here. The user's same-inode protection
requirement is not waived. Precisely define the trusted first-launch assumptions
separately from post-selection protection; do not claim first-instruction protection
without a mechanism or silently expand that claim into general same-UID isolation.

Similarly, sealed main executable bytes do not authenticate dynamic loaders,
libraries or `LD_PRELOAD`, prevent ptrace, or establish publisher authenticity of
pre-start mutable manifest/pin files. Before product implementation, document and
review what trusted bootstrap/environment is required and how unsupported contexts
are rejected. Existing account/scope/Secrets checks remain independent and mandatory.

## Dependency order and evidence gates

Read-only source mapping identified these coordinated integration points:

| Boundary | Existing symbols/files | Required evidence before replacing it |
| --- | --- | --- |
| Release lookup / launch | `unix_release::{verified_asset, verified_sibling_command, VerifiedCoreCommand}` in `crates/hantong-platform/src/unix_release.rs` | Retained original root/manifest plus measured sealed copy; fixed caller membership still checked. Current held file is not sealed. |
| Peer admission | `unix_peer::{observe_peer, peer_matches_current_image, same_executable}` | Actual image FD, process instance and selected cross-process identity; no name or claimed digest authority. |
| Stop | `unix_lifecycle::stop_verified_core`, Core `unix_stop::run_async` | Same identity before/after pidfd and supported predecessor transition; no unknown process signals. |
| Relay | IPC-client `unix_relay::{call, broker_exchange}`, Core `unix_core_runtime` admission | Both directions admit the same release/image model before dispatch; existing profile/account/scope checks retained. |
| Direct and foreground entries | Core `main.rs`, CLI `unix_core_start`, TUI `main.rs`, MCP relay and review entrypoints | First-launch trust and inherited-FD validation for every entry; Unix MCP review needs explicit coverage rather than inference from Windows console code. |
| Update / activation | CLI `update::run_unix_updater`, root `install.sh`, `packaging/unix/install.sh`, `package_unix.py` | New pin-consuming Linux route, reviewed updater execution and predecessor stop before activation; existing Mac/Windows feed unchanged. |

Before executable integration, keep the following test inventory as a review
checklist, not unimplemented green tests: (1) mutation during copy; (2) post-check
write/truncate/map; (3) writable map prevents sealing; (4) missing/forged/duplicate
FD roles and context; (5) actual image FD mismatch; (6) root/manifest/release/asset
substitution; (7) PID reuse/exit during pidfd acquisition; (8) supported predecessor
stop versus unknown image denial; (9) noexec/inspection denial without fallback;
(10) descriptor leaks and unrelated child inheritance; (11) interrupted activation
and downgrade denial; (12) all direct/relay/TUI/MCP/review entrypoints. Cases 1–3
have limited kernel mechanism evidence only, not product execution evidence.

| Stage | Completion evidence | Current state / dependency |
| --- | --- | --- |
| A. Origin-free preparation | Source binding, attack matrix, exact code map, isolated kernel tests and explicit contract choices | Source binding and limited kernel tests passed at the base commit; contract choices remain proposals. |
| B. Adopt identity/bootstrap rules | Exact reviewed ADR/contract revision, supported host policy, FD lifecycle, stop/update decisions; actual decision recorded | Unmet. No origin URL is required to review these choices. |
| C. Local product integration | Core/CLI/relay/TUI/MCP/foreground/stop/update all use the same selected context and sealed-image rules; race/forgery/PID-reuse tests pass | Blocked on B, not on the address alone. No partial identity replacement. |
| D. Frozen candidate validation | Same final full SHA: native Linux package, isolated install/update, old-version/data preservation, Core restart and scope recovery; shared OS selection regression | Unmet. Mocked ELF assembly is not native compilation or live Core evidence. |
| E. Publication readiness | Exact origin/authentication decision, final package/manifest/source/file digests, independent findings closed and actual human review of final range | Unmet; origin, new human review and runtime implementation are outstanding. |
| F. Linux-only publication | Downloaded public bytes match frozen evidence; existing Mac/Windows feed remains intact | No publication before D/E. Mac failure is neither ignored nor marked passed. |
| G. Songpa application | Authorized pinned update, preserved existing storage, Core restart, three accounts accessible under normal account/tool approval | Only after F; no automatic grant, credential regeneration, send or Trash test implied. |

A scope conflict test must prove pending expiry/cancellation/source transitions,
staged recovery and unchanged grants before confirmation. Actual three-account
access is a separate Songpa result, not inferred from synthetic fixtures.

On 2026-11-07 a read-only check found the local WSL host is x86_64/glibc 2.39 with
`vm.memfd_noexec=0` and readable process-image interfaces. `cargo` and `rustc` were
not found on that shell's PATH. This is not a proof that no toolchain exists or a
permission refusal; native build prerequisites still need checking/preparing before
stage D. No host policy, user account, key, vault or mail was changed by this check.

## Estimate and next decisions

There is no evidence-backed deployment date yet. The execution model is not
adopted, product implementation is not complete, final native tests have not run,
the origin has no address, and human review/host support are unresolved. No elapsed
duration for those steps can be inferred from unit test runtime. Re-estimate only
after B, then after the first complete native D run; keep execution time distinct
from waiting for decisions, review and Songpa availability.

The user does not need to invent an address to continue reviewing B. The immediate
decision packet is: identity-v2 versus shared-object ownership; bootstrap/threat
boundary; native updater versus measured interpreter; exact predecessor handling.
The later publication decision is the exact address and descriptor trust policy.

Human review must identify the final full SHA/range/files, actual confirmation,
findings and closures for platform release/peer/lifecycle, relay/runtime/entrypoints,
installer/updater and scope changes. Add Secrets review only if that boundary
changes. Prior approval and an independent AI review do not substitute. No new
runtime changes in this preparation document are approved or implemented.

References: [ADR-033](../adr/proposed/ADR-033-linux-sealed-release-identity.md),
[identity-v2 draft](../contracts/proposals/linux-execution-identity-v2.md),
[Linux release preparation](LINUX-PINNED-RELEASE-PREPARATION-20261107.md).
Primary API references: [fexecve](https://man7.org/linux/man-pages/man3/fexecve.3.html)
and [kernel memfd policy](https://docs.kernel.org/userspace-api/mfd_noexec.html).
