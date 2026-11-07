# ADR-033: Linux sealed executable and installed release identity

Status: proposal; no runtime contract approval or implementation claimed.
Related: ADR-030, SEC-02, INST-01/04, Unix relay/terminal review contracts.

## Authorization record

The 2026-11-07 user message reports the user's words as
“배포도 승인하고 보안도 오케이해” and instructs continued integration, independent
review, actual human review, final same-commit testing and publication only after
all required gates pass. This is execution/publication authorization, not a human
code review finding, a waiver, or approval of an unspecified new identity protocol.
The precise Linux origin is not yet selected. No origin is created here.

## Why this is a coordinated contract change

`unix_release::verified_asset` derives the installed release from `current_exe`.
`unix_peer::peer_matches_current_image` compares kernel executable device/inode
and digest. `unix_lifecycle::stop_verified_core` and Core `unix_stop` additionally
bind the installed path and process instance before pidfd signaling. Current
descriptor execution prevents pathname replacement but not later same-inode writes.

Executing a memfd changes the kernel image path and inode. Removing inode checks,
trusting argv/environment, or stripping a memfd/deleted suffix from current_exe is
not an acceptable migration. The following design must be reviewed as one change.

## Proposed separate identities

1. **Installed release context:** an independently authenticated release pin
   identifies full source commit, source hash, exact target/version, manifest hash
   and expected asset digests. Retain checked root/directory handles and installed
   manifest context before copying. Context delivery must have an explicit bounded
   inherited descriptor format and lifecycle; no path/env/argv string is authority.
   Validate its contents against pinned release evidence, not another untrusted
   context claim. Ownership and existing account/profile/scope checks still apply.
2. **Executed image:** copy to a fresh memfd; require WRITE/GROW/SHRINK seals (and
   seal-set finalization); read back the sealed object; compare length and SHA-256
   against the fixed manifest; execute that same retained FD. Hashing only the
   source before copying is insufficient. Failed copy/seal/measurement aborts.
3. **Peer identity:** retain UID, kernel process start/boot instance and pidfd
   checks. Open the actual `/proc/PID/exe` object, verify seals and measured digest.
   Fresh memfds differ in inode, so either share the same sealed executable object
   through a reviewed lifetime owner, or introduce a reviewed release+code identity
   v2 comparison. A matching digest alone is not the entire release context.
4. **Stop identity:** compare the exact trusted release/code context, actual sealed
   executable and fresh process instance before and after acquiring pidfd. Do not
   terminate by caller PID/name or silently fall back when inspection is denied.

The mechanism/context format and shared-object vs identity-v2 choice are **not
decided** by this proposal. Existing IPC v2 or stop semantics are not silently
extended. A new version must define old daemon/client interoperability: unsupported
mixes fail with existing stable errors, preserve storage and never force-stop an
unverified old process. First transition cannot depend on a trusted new image
having already replaced a running old daemon.

## Entry points and failure behavior

Cover direct Core, CLI, relay, TUI, MCP, foreground terminal helper, stop, initial
startup and self-update. Root release discovery and relay caller asset binding
must survive sealed execution without trusting the memfd name. `update.sh` needs
a separately measured sealed script and a fixed verified interpreter contract;
shebang/PATH resolution or an unverified system shell is not automatically safe.
Direct invocation bootstrap/re-exec and the inherited FD ownership/close-on-exec
rules must be specified for every entry point, not only Core child spawning.

On `vm.memfd_noexec=2`, absent seals, unsupported kernels, inaccessible `/proc`,
denied image/context inspection or failed interpreter verification, fail closed.
Do not lower sysctl/security settings, relax ACLs, accept an unsealed inode or add
a pathname fallback. No root/admin setup is implicitly authorized.

Alternative: filesystem-enforced immutable executable contents (e.g. fs-verity)
could preserve the installed inode and the three current comparisons. It requires
an explicit filesystem/kernel support policy and installation semantics. Neither
Songpa support nor WSL/hosted support has been established, so this is not an
assumed replacement and no verity flag is enabled by this proposal.

## Required adversarial and product tests

The old-daemon transition needs its own reviewed rule: either execute the exact
verified predecessor's stop path while it still matches the old image, or define
an independently authenticated exact predecessor-digest allowlist and fresh peer
instance/pidfd procedure. A new helper is not automatically authorized to signal
another image. No arbitrary PID/digest input or unconditional SIGTERM is allowed.
The proposed allowlist, if selected, must come from trusted release evidence and
be tested against unknown/tampered predecessors without modifying current.txt.

- Mutate original bytes during copy; only the sealed-copy digest may authorize.
- Same-size write, truncate, writable mapping and seal removal after the last
  measurement fail, and no mutated code is executed.
- Reject forged/missing/unsealed memfds, fake release contexts, cross-release or
  cross-asset identical-looking claims and PID reuse; retain same-UID rejection.
- Exercise all entry points above and stop before/after upgrade, including an old
  daemon, timeout, partial install and interrupted activation without data loss.
- Kernel noexec policy fails without global policy changes or mutable fallback.
- Prove real Linux install/update/Core restart/scope recovery at the final same
  commit; synthetic parser tests are not these product tests.

## Guarantees and review gates

Independent architecture review notes that fs-verity would retain current inode
and path semantics and potentially reduce the identity-protocol change. However,
requiring a verity-capable installation filesystem is a new product support
restriction; Songpa and hosted-test support is unproved. This review is advice,
not user acceptance of a filesystem requirement or permission to enable verity.
The mechanism remains undecided until the explicit support/identity contract is
selected. Memfd-only anonymous synthetic experiments do not select that contract.

Sealing protects the selected executable bytes against subsequent writes. It does
not authenticate an attacker-controlled install tree/manifest, trusted digest
origin, ptrace, dynamic loader/shared objects or LD_PRELOAD. Define and review
those boundaries and any child environment policy separately; do not claim general
same-UID isolation or a human-presence guarantee.

AGENTS sections 6–7 require the exact selected identity/context/interpreter
contract before implementation, independent review and actual human review of the
final commit/range/files, findings and closure. Existing 6322b9a confirmation is not
new code review. Mac compatibility remains failed/blocked; Windows/Mac feed and
existing authentication/storage behavior remain unchanged.

Primary references:
- https://man7.org/linux/man-pages/man3/fexecve.3.html
- https://man7.org/linux/man-pages/man2/memfd_create.2.html
- https://docs.kernel.org/userspace-api/mfd_noexec.html
- https://docs.kernel.org/filesystems/fsverity.html
