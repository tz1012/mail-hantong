# Linux static pinned delivery - adopted R2

Status: **ADOPTED for local implementation; human review and final product gates pending.**

Adoption source: `572d3b93fc113f08ed1e672e23d40accc1b71cd2`; user-reported explicit approval and boundaries are recorded in
[the adoption record](../latest/LINUX-R2-ADOPTION-RECORD-20261107.md).
This adoption does not approve future payload/code/hash or authorize publication before gates.

## D1 — R2 exact host/path and input

Choose existing owner tz1012's Pages prefix exactly:
`https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/VERSION/FULL_COMMIT/`.
Accept only explicit tuple (release version, 40-lowercase-hex commit,
64-lowercase-hex expected descriptor SHA256). No arbitrary URL/base override,
latest alias, mutable branch, query, fragment, credential or redirect. Target is
Linux x86_64 only; missing host CPU/kernel prerequisites fail, not another asset.
This adoption would choose the namespace, not create/upload it before D6 passes.

## D2 — R2 pin, descriptor and provenance

Choose a manually, independently delivered **expected descriptor digest** for this
fixed pilot path, checked by the E1 trusted bootstrap/verifier. No new signing key
or dependency. Same-host SHA256SUMS/descriptor, an argv/environment claim or sealed
metadata does not by itself establish that trust. Actual human review and the
expected digest's trusted delivery must be recorded; no `reviewed=true` input is
accepted as proof. If independent trust/delivery is absent, installation is blocked.

Freeze descriptor schema_version integer 1 with exactly these keys:
`schema_version`, `version`, `package_version`, `target`, `commit`,
`source_filename`, `cargo_lock_sha256`, `source_sha256`, `source_archive_sha256`,
`package_filename`, `package_sha256`. Require current strict version/target/name/
hex bounds, reject duplicate/unknown fields, cap at256 KiB. Keep package <=256 MiB,
source <=128 MiB, member <=64 MiB, inventory <=4096; strict archive/path/link rules.
The pinned package digest binds exact embedded manifest, build metadata and files.
At build time prove ZIP names/sizes/Git blob identities equal the exact full-commit
tree. At install time verify pinned ZIP digest, full commit marker/canonical source
hash, Cargo.lock, package version/target/commit and exact manifest/inventory/file
hashes. The marker does not itself prove Git/publisher authenticity; build/provenance
evidence and independently trusted expected descriptor digest remain necessary.
Record descriptor/package/source/manifest/build metadata/file digests separately.

Installer may persist only public release evidence under the owned install root:
`release-pins/VERSION/FULL_COMMIT/{pin.json,descriptor.json}`. pin.json has exactly
format_version=1, target, version, commit, descriptor_sha256 from the reviewed tuple;
descriptor.json is the exact verified bytes. Apply existing owner/no-link/regular
file/size guards and retain validated snapshots for the running process. This is
not a secret-format change or protection against active same-UID pre-start edits.
Do not overwrite an existing different pin/descriptor or trust these files merely
because their own contents agree; the E1 bootstrap premise still applies.

## D3 — R2 native update state and fixed OS downloader

Use existing Core's fixed `linux-update-v2` mode for tuple validation, URL generation,
verification, staging, lifecycle and activation. Transport may directly exec only
`/usr/bin/curl` or its checked root-owned OS canonical target: final executable and
ancestors root-owned, no group/world write, no setuid/setgid, no user-selected path.
Missing/unsafe utility fails without auto-installation. This is the existing Unix
curl prerequisite, not a new crate, Python runtime or shell interpreter.

First curl argument is `--disable`; use fixed GET, standard TLS verification,
HTTPS-only, no redirect/location/insecure option, no netrc/cookie/client credential,
proxy or user config. Clear inherited environment, including config/proxy/CA
overrides; pass no identity role FDs. Construct URLs solely from D1's tuple and fixed
filenames. Enforce 15-second connect/300-second total deadline and Rust-side hard
body/header bounds, exact final HTTP200 and child exit success. Kill/reap only the
owned child on overflow/timeout. Curl diagnostics are not exposed verbatim.

Trust OS curl/loader/libraries/system CA as external transport prerequisites;
they are not measured/sealed HanTong assets and TLS is not publisher authentication.
No shell/PATH/update.sh fallback or automatic alternate version. User-facing
interactive/no-input restrictions remain those of cli-update. Exact argument,
environment, exit/status/header and timeout behavior requires regression tests.
The only proposed first-install exception is identity E9, requiring separate
explicit adoption of the existing OS-Python prerequisite; not an automatic fallback.

## D4 — R2 staging, preservation and lifecycle

Apply E6/E7: beta29 first migration offline with no legacy signal; later v2 updates
stop through the still-current v2 context before activation. Fresh private staging,
exact approved release inventory, no missing/extra/substituted files or overwrite
of another version. Preserve user data/old versions/unknown user files; unknown
release files cause refusal, not deletion or acceptance. Existing storage locks
do not authorize reading/modifying credentials or ACLs. After possible migration,
no downgrade/automatic data rollback; separately pinned roll-forward recovery.

## D5 — R2 shared-feed isolation and publication provenance

Do not create or edit a GitHub Release/tag, asset list, Atom/API response, existing
site HTML/config or latest alias for this route. Publication changes only the
chosen static version/commit prefix after D6. Existing Windows/Mac installers and
old bundled updates remain unchanged. No old Mac assets relabeled for a new Linux
version. Pinning Songpa's caller alone is not proof of isolation.

Before publication: run original Windows API/403/429 fallback and Linux/Mac Intel/
ARM selector fixtures with unchanged feeds and rejection cases. Verify updater
script hashes unchanged. Freeze source/full commit/manifest/build/file/checksum
evidence. Before/after static publication compare unchanged site paths and release
IDs/assets via read-only snapshots; flag concurrent changes instead of fabricating
equality. Re-download each new artifact and compare all pinned hashes. Never use
metadata relabeling or earlier-commit CI as final candidate evidence.

## D6 — R2 gates and exact authority

Adoption permits local integration, not publication or human-review completion.
Keep readiness-blocked candidates blocked; only a new correctly built/frozen and
fully validated candidate may qualify. E8, native same-final-commit package/install/
update/restart/scope recovery, selector regressions, independent findings closure
and actual human review of new boundaries are mandatory. Failed Mac compatibility
remains failed; Linux-only authorization does not claim Mac success. No account/
vault/key/mail reset or automatic grant. After gates, use existing conditional
publication/Songpa authorization without re-requesting general permission. Actual
Gmail2+Daum1 list/body reads, with normal scope confirmation, remain the finish line.
