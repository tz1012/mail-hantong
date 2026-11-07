# Proposed Linux execution identity v2

R1 historical draft. The concrete adoption candidate is
[revision R2](linux-execution-identity-v2-r2.md), clauses E1–E8. Neither is adopted.
R2 chooses a recommended per-process model/FD map/native state machine/offline
legacy transition instead of leaving those implementation choices open.

Status: review draft, NOT adopted and NOT implemented. ADR-033 describes the
alternative and unresolved trust/support decisions. Deployment authorization does
not approve this changed peer/stop identity protocol. No existing inode check is
removed by this draft.

## Candidate contract for the memfd option

Separate two objects: original installed release context and actual executed
sealed image. A parent constructs context only after verified_asset validates a
fixed installed caller/asset and independently authenticated release pin. Direct
Core/CLI/TUI/MCP startup needs an equivalent reviewed bootstrap; an environment
path, argv[0], memfd label or caller-supplied JSON is never its substitute.

An inherited descriptor table (draft roles, final numbers/version to be reviewed)
contains an open installed-root directory, sealed manifest bytes, independently
authenticated descriptor/pin bytes, sealed context bytes and the sealed executable
object. Reject missing, duplicate-role, wrong-type or unsealed descriptors. These
contain only public release metadata, never profile keys or credentials.

Proposed context fields, exact and bounded: protocol_version=2, target,
release_version, source_commit, canonical_source_sha256, trusted_pin_sha256,
manifest_sha256, asset_relative_path, asset_size, asset_sha256. Kernel fstat of the
retained root/asset descriptors supplies dev/inode identity; claimed numeric values
in JSON do not prove those identities. Fixed asset paths and original release
root membership remain mandatory. A context is valid only against the recipient's
trusted release evidence, not because its own bytes happen to be sealed.

For executable images: fresh executable memfd -> bounded copy -> WRITE/GROW/
SHRINK and finalized seal-set verification -> read back sealed length/SHA256
against fixed manifest -> execute that same FD. Explicit execute-bit/noexec-seal
handling and loader/environment policy are required. Original hash before copy
is not authorization of the new object. Reject syscall, seal, digest or size
failure without pathname or unsealed fallback. vm.memfd_noexec=2 remains enforced;
do not change namespace/sysctl/mount/ACL policy or attempt chmod around it.

## Peer and stop comparison

Always retain kernel UID, boot/start process instance, session semantics and fresh
pidfd checks. Open actual /proc/PID/exe and measure that object with seals present.
The peer's executed-image inode must match its retained image FD; do not accept
its memfd name or a claimed digest. If either object cannot be inspected, deny.

Across executions, candidate identity-v2 would compare the same trusted release
pin/manifest/root identity and exact expected Core asset size/digest, while each
process remains bound to its own actual sealed object and kernel instance. This
deliberately differs from v1's cross-process same-inode equality. It requires
explicit approval and tests before replacing peer_matches_current_image. A
different release, different root, wrong asset, forged context or missing pin is
denied even if some bytes/hash appear equal. No arbitrary process/file selector
is accepted over IPC.

stop_verified_core would use the same reviewed comparison before/after pidfd
acquisition and retain bounded stop/exit/socket verification. Old unsealed daemons
are not implicitly equivalent. Specify exact supported predecessor evidence and
transition behavior separately; never weaken general peer comparison to stop an
unknown image or accept an arbitrary PID.

## Installer/update and trust decisions still required

- Name the exact separate Linux HTTPS origin/repository and its owner, bounded
  redirects, version/asset rules and independently trusted descriptor pin source.
  A digest fetched only beside its payload is not independent authentication.
- Select shared-sealed-object lifetime ownership or the identity-v2 comparison
  above; approve the exact FD framing/lifecycle and bootstrap trust source.
- Decide the trusted update interpreter. A sealed update.sh still needs a fixed
  verified shell; shebang/PATH or an unmeasured /bin/sh is insufficient. A native
  Core-owned updater could avoid that interpreter boundary, but is a separate
  contract/implementation decision, not an already approved fallback.
- Define loader/LD_PRELOAD/ptrace and pre-start same-UID manifest-tree tampering
  limits. Sealing protects selected bytes after sealing, not the entire process
  environment or authenticity of untrusted pre-install evidence.

## Required product tests and human review

Mutation during copy; same-size write/truncate/mapping after final sealed check;
mapping preventing seals; forged image/context/root; cross-release equal-looking
claims; missing FD; PID reuse; noexec rejection; direct Core/CLI/relay/TUI/MCP;
terminal review; exact old/new Core stop; update and interrupted activation. Run
real package/install/update/storage preservation/restart/scope recovery at the
same final commit; no mock or non-executable memfd experiment substitutes.

Human review records exact final SHA/range/files, actual reviewer confirmation,
findings and closure. Independent AI advice and the user's deployment approval
are not that record. Preserve existing accounts/vault/mail/keys and normal scope
approval; Windows/Mac identity and existing shared release feeds stay unchanged.
