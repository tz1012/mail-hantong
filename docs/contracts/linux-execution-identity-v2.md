# Linux execution identity v2 - adopted R2

Status: **ADOPTED for local implementation; human review and final product gates pending.**

Adoption source: `572d3b93fc113f08ed1e672e23d40accc1b71cd2`; user-reported explicit approval and boundaries are recorded in
[the adoption record](../latest/LINUX-R2-ADOPTION-RECORD-20261107.md).
This adoption does not approve future payload/code/hash or authorize publication before gates.

## E1 — Model, initial trust and guarantee

Recommend per-process sealed images, not a shared-image broker or fs-verity.
The starting installed caller and independently supplied expected descriptor digest
must be trusted through initial context construction; ownership checks or sealed
public context alone do not establish that premise. Missing trusted bootstrap/pin
means refusal. Persisted public pin metadata does not resist active same-UID
pre-start replacement.

Guarantee: after successful sealed-copy measurement, selected main executable bytes
cannot change before execution or through same-size writes/truncate/writable maps.
This does not retroactively authenticate mutable bootstrap's first instructions,
pre-start tree/manifest/pin tampering, OS loader/libraries/LD_PRELOAD, ptrace or
root/kernel. It does not independently prove publisher provenance. These separate
boundaries are not claimed solved and are not acceptance of the selected-byte
TOCTOU window. If first-instruction resistance is required too, a separately
protected launcher/OS mechanism is a real additional decision, not an implicit one.

## E2 — Fixed retained FD roles

Require RLIMIT_NOFILE > 201. Roles: **197=root, 198=image, 199=context,
200=manifest, 201=descriptor**. Root is a retained checked O_PATH/O_DIRECTORY
release directory. Other roles are regular memfds requiring WRITE/GROW/SHRINK/SEAL;
metadata also requires explicit non-executable seal. Bounds: image 1..64 MiB,
context 1..4096 bytes, manifest 1..4 MiB, descriptor 1..256 KiB.

Reject missing/wrong/duplicate objects or roles and unowned occupied-role remapping.
Only the intended verified exec inherits roles; child validates them before any
mail/IPC/Secrets/dispatch and restores CLOEXEC on all retained roles. Retain them
for process lifetime. A verified sibling gets its own complete map; unrelated
children inherit none. Parent roles are not overwritten by child setup. No role or
authority is discovered from a memfd name, argv[0] or environment path.

## E3 — Strict context and private trust object

Context is strict UTF-8 JSON with exactly: `protocol_version` integer 2, `target`,
`release_version`, `source_commit`, `canonical_source_sha256`, `descriptor_sha256`,
`manifest_sha256`, `asset_relative_path`, `asset_size`, `asset_sha256`.
Reject unknown/duplicate fields, control/NUL bytes, non-integer sizes and malformed
versions/digests. Sender emits compact lexicographic field order. Target is exactly
linux-x86_64; commit is 40 lowercase hex; SHA256 fields are 64 lowercase hex.
Asset is one of `bin/hantong`, `bin/hantong-t`, `bin/hantong-mcp`,
`bin/mail-hantong-core`, with exact pinned manifest size/hash.

Retain a process-local private `ExecutionIdentity`/`ReleasePinV2` derived from an
independently expected descriptor SHA plus verified descriptor/package/source/
manifest. No production constructor trusts network/IPC context claims. FD201 or
context.descriptor_sha256 is not itself the trust anchor. Kernel fstat, not JSON,
supplies object identities. The recipient compares against its own established
trust, never the peer's asserted authority.

## E4 — Copy/seal/measure/exec and entrypoints

Fresh explicit MFD_EXEC image -> bounded copy/short-write handling -> require and
finalize four seals -> read sealed length/SHA256 against fixed manifest -> execute
that same held object by FD. A duplicate must fstat as that same object; reopening
a mutable pathname is forbidden. Metadata uses MFD_NOEXEC_SEAL. Any failure aborts.
Original-file hashing before copy is insufficient.

Require actual kernel primitives and /proc inspection; glibc alone proves neither.
Unsupported operations, vm.memfd_noexec=2 and inspection denial fail closed. No
sysctl/namespace/mount/ACL change, chmod workaround or mutable fallback. Direct
Core/CLI/relay/TUI/MCP/foreground entries cross this barrier before protected work.
After exec, release lookup and relay caller selection use retained context, not
stripped memfd current_exe strings or argv basename.

## E5 — Peer admission

Keep UID/session/boot/start-instance checks. Between instance rechecks open actual
`/proc/PID/exe` and `/proc/PID/fd/197..201`; actual exe and FD198 must match dev/inode,
required seals and exact expected size/digest. Recheck image/object identity after
metadata inspection. Missing, denied, replaced or changed objects mean denial.
Peer root dev/inode must match own retained root; descriptor/manifest/context must
match own independently established release trust and the expected fixed asset.

Cross-process image inodes may differ. The replacement comparison is exact trusted
release/root/manifest/code identity PLUS each actual image-to-retained-FD equality
and process instance, not digest-only admission. core.relay stays Core-to-Core;
UnixPeerContextV2 gains no caller-claimed fields. Existing profile/account/epoch/
policy and terminal ticket checks still follow admission.

## E6 — v2 stop and updater

Use the still-current v2 release's sealed Core stop helper and retained context,
not a newer helper pretending to be the predecessor. Validate actual socket peer
under E5, acquire fresh pidfd and repeat image/instance checks before bounded signal,
exit and socket-disappearance verification. No arbitrary PID/path/digest input,
cross-release relaxed comparison or unknown-image signaling. Stop before activation.

Choose a fixed Rust mode of existing Core, `linux-update-v2`, for update state,
validation, staging and activation. Its fixed OS downloader is specified by D3 of
static-delivery R2; this is not a self-contained native TLS implementation or a
sealed-product guarantee for OS code. No new executable, shell/Python interpreter,
crate dependency or old update.sh fallback is authorized.
E9 alone proposes a one-time first-transition OS-Python exception; adoption of E6
without E9 supplies no first bootstrap path.

## E7 — First beta.29 migration and activation

Choose one-time **offline migration**, not a new-helper legacy signal allowlist.
Require core-v2.sock absent and exclusive existing core-v2.lock,
data/database.lock and data/database-key.lock with current owner/type/no-link
guards. Acquire locks without reading DB/key contents. A live/stale socket or busy
lock refuses activation; no stale-file deletion, process-name kill, auto logout or
auto reboot. Operator separately chooses an idle session/normal OS shutdown if
needed; adoption does not authorize that action now.

Hold lifecycle/storage locks through activation; release to start verified new
Core. A competing process can make startup fail: report Conflict/CoreUnavailable,
never kill it or claim success. Success requires bounded readiness and E5 for the
expected new release. Preserve old pointer before activation failures and retain
old versions/data. After possible migration, no automatic downgrade; separately
pinned roll-forward recovery. Staged scope retains exact-key/profile/epoch recovery;
pending terminal intent is not auto-confirmed. No account creation/grant, credential
regeneration, send or Trash action is part of installation.

## E8 — Adoption and release gates

Adoption permits coordinated local Linux implementation, not partial removal of
inode checks, invented trust anchors or publication. Release lookup, every entry,
peer, stop and update must agree. Test copy/final-check mutation, mappings, forged
roles/context/root, cross-release replay, FD leaks, PID/exec races, noexec denial,
offline busy locks and interrupted activation. Run same-final-SHA native package/
install/update/restart/scope recovery plus Windows/Mac selector regressions.

Record exact new SHA/range/files, independent findings/closure and actual human
review. Prior 6322b9a confirmation stays recorded for unchanged scope code; it does
not review future changes. Mac compatibility failures stay failed. Publication also
requires D1–D6 and all mandatory gates. Completion is Songpa Gmail2+Daum1 actual
list/body reads, not these tests or deployment.

## E9 — Explicit one-time beta29 bootstrap exception (new adoption decision)

E6's no-Python rule remains for steady-state updates. For the first offline beta29
transition only, propose existing `/usr/bin/python3 -I -S -c REVIEWED_PAYLOAD`.
The exact UTF-8 payload SHA256 and independent D1 version/commit/descriptor digest
must be frozen in the actual human-reviewed release record before execution.
The command is not supplied or executed by this proposal. Contract adoption selects
this algorithm/prerequisite; it does not approve an unwritten payload or its hash.

Trust is the operator's exact reviewed command input under E1, never argv as peer
authority. Do not fetch/pipe an unverified mutable script or execute a mutable
candidate first. Operator terminal/shell and pre-seal input integrity are explicit
initial prerequisites, not protected by the later selected-ELF guarantee.

Require the fixed Python canonical executable, root-owned safe ancestors and
absolute root-owned non-group/world-writable OS stdlib paths. Preflight isolated
and no_site flags, reject unexpected import paths; no local/user modules, site,
PYTHONPATH, user-selected interpreter or auto-install. OS Python/stdlib/loader/libs
and fixed curl/CA remain external OS trust, not sealed HanTong product assets.
Missing/unsafe prerequisites refuse; no environment disguise or fallback.

The payload clears inherited environment, invokes only D3's fixed curl without a
shell or identity FDs, verifies independent descriptor pin, exact package/source/
manifest/inventory and builds fresh private staging under D2/D4. It copies candidate
Core to fresh MFD_EXEC, finalizes required seals, re-reads sealed size/SHA256, builds
E2's complete FD197–201 map from the verified release and FD-executes the same held
Core with fixed `linux-update-v2` bootstrap mode. No pathname execution gap.

Candidate entry revalidates roles/identity before any dispatch. Bootstrap mode
exposes no IPC/mail/Secrets service; it acquires E7 lifecycle/DB/key locks and
checks socket absence before activation. Competing old Core means refusal, not
signal/lock bypass. Only after activation may the verified normal Core be started.
Later updates use E6 and cannot select this bootstrap as a fallback. Required tests
include modified payload/tuple, unsafe OS prerequisites/import paths, malformed or
changed package/context, noexec, lock races, interrupted staging/activation and
zero mail/Secrets dispatch before activation. Final payload/code review and native
product gates remain mandatory; no actual OS/user operation is authorized now.
