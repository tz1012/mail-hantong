# Linux R2 implementation review packet

Status: implementation and native validation in progress; NOT human-reviewed or release-qualified.
The adoption subject remains 572d3b93fc113f08ed1e672e23d40accc1b71cd2.
The implementation review target must be the new full commit reported with this packet after freezing.
Do not substitute the adoption commit, previous 6322b9a review, or an advisor's findings for that review.

## Review scope

- `crates/hantong-platform/src/linux_sealed.rs`: copy/seal/readback/ELF validation, fixed roles, CLOEXEC, helper-only seccomp.
- `linux_identity.rs`, `unix_release.rs`: independent public-pin premise, strict context/manifest/build identity, retained original release root, sibling/candidate launches.
- `unix_peer.rs`, `unix_lifecycle.rs`: UID/session/start/boot/process/image/seal/digest observations, pidfd stop, image rechecks.
- `linux_archive.rs`, `linux_pinned_update.rs`: bounded native parsers, fixed OS curl/HTTPS, exact tuple/source/inventory/hash validation, private staging/public evidence and no overwrite.
- `linux_install.rs`, Core `linux_update_v2.rs`: offline locks, retained old pointer, atomic pointer rename, explicit post-rename durability uncertainty, stop/activate/start/readiness.
- four product entrypoints, Core broker dispatch, IPC relay and terminal review: no cached-admission or mutable-image fallback.
- Core public bootstrap ordering: only fixed unauthenticated system.readiness/version responses precede image-admission error propagation; broker, stop and authenticated dispatch retain admission/recheck requirements. Review the unsealed socket probe and protected-command rejection assertions.
- `scripts/linux_bootstrap_v2.py`: one-time isolated OS Python bundled payload, no mutable project imports, no Secrets/DB before sealed Core activation, fixed OS curl.
- `scripts/package_unix.py`: newly built candidate metadata and four real sealed-image smoke checks; no requalification of earlier blocked artifacts.
- synthetic kernel/attack/installation/bootstrap/package tests and migrated real-binary integration fixtures.

## Guarantees and limits that must remain visible

Sealing protects the adopted execution bytes after copy/seal/readback, including equal-size writes,
truncate and writable mappings. The actual process image is checked against its role image, seals,
digest and process instance. Fixed names, argv and environment paths do not establish trust.
Helper-only seccomp denies subsequent execve/execveat; it does not make /proc observations atomic.
E1 still excludes pre-start same-UID installation/pin tampering, ptrace, loader/LD_PRELOAD,
root/kernel compromise and untrusted provenance. An independently obtained descriptor digest and
trusted first operator command remain premises; a locally matching public pin is not a publisher signature.
Linux R2 is x86_64 only. No sysctl, ACL, Keychain, or OS approval bypass is permitted.

## Activation failure handling

Before pointer activation, retain and fsync `activation-before-VERSION` with the previous version.
A pre-rename failure preserves the current pointer. A post-rename directory-sync failure is explicitly
`Activation::DurabilityUncertain`; report fixed `activation_durability / uncertain / CORE_UNAVAILABLE`,
do not start the candidate or automatically downgrade, and retain both versions and public recovery evidence.
The pointer may already name the new release; do not claim otherwise or blindly repeat bootstrap.
Read-only comparison of current.txt and the retained record determines the next operator action.
The explicit `hantong update resume VERSION FULL_COMMIT DESCRIPTOR_SHA256` path re-fetches the exact pinned
bundle, revalidates every installed byte/public pin and seals the candidate before locked activation.
It accepts only matching recovery/current/temporary pointer state. Unknown state remains a refusal.
The verified current release's ordinary bounded startup or separately pinned roll-forward may follow. Never reset DB/vault/profiles or use an old installer to roll back potentially migrated data.

## Release gates still required

Record the exact final full commit, source archive/canonical hash, Cargo.lock hash, all image hashes,
manifest, descriptor, package checksum, reviewed E9 payload and payload hash. Run native Linux package,
first transition, update, preservation, Core restart/readiness and scope recovery on that same commit.
Run Linux/Mac selectors and Windows API/403/429 fallback fixtures with existing feeds unchanged.
Close independent findings and record the actual human reviewer identity/confirmation, exact target,
reviewed scope and findings disposition. Conditional publication authorization is not review evidence.
Do not publish any Release/tag/latest alias or alter shared site content for this Linux-only path.
The static prefix is the existing Pages /artifacts/linux-x86_64/VERSION/FULL_COMMIT/ only after gates.

Mac compatibility failures remain unresolved evidence; Linux-only work does not turn them green.
Final product completion is Songpa Gmail2 + Daum1 each actually listing and reading mail, with ordinary
explicit scope review. Synthetic tests and successful publication do not prove those account reads.
