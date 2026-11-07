# Proposed fixed Linux delivery on the existing official host

Status: **proposal only; no hosting, upload, channel or trust rule adopted**.
Related: INST-01/03/04, SEC-02, CLI-01/04, cli-update, ADR-033,
linux-execution-identity-v2 and linux-pinned-release-20261107 proposals.

## Purpose and verified host facts

Songpa's two Gmail accounts and one Daum account must each support actual reads.
The confirmed installed version is beta.29; its last refresh returned CONFLICT.
No request ID is available. Do not repeat that known failure, reset the installation,
change permissions or recreate credentials to discover its state.

This proposal makes an alternative to a new repository concrete; it is not a new
requirement for the mail-read objective. Read-only GitHub Pages metadata on
2026-11-07 reports the current official site is
`https://tz1012.github.io/mail-hantong/`, legacy Pages from `gh-pages:/`.
The root includes `.nojekyll`; no `artifacts` path was observed in that root listing.
No branch, setting, file, release, tag or DNS record was changed.

## Candidate exact namespace, not selected or created

Candidate base:

```text
https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/VERSION/FULL_COMMIT/
```

`VERSION` must be an explicitly selected validated release version. `FULL_COMMIT`
is the exact 40-lowercase-hex source commit, not a branch or short SHA. Do not assign
an official version by editing its label or use a different code commit under an
old version path. No `latest`, mutable alias or shared-feed discovery in this route.
The existing website and GitHub release/API/Atom paths remain unchanged.

Candidate files reuse locally prepared artifacts:

- `linux-pinned-candidate.json`: bounded descriptor, exact source/package binding;
- `mail-hantong-cli-VERSION-linux-x86_64.tar.gz`;
- `mail-hantong-source-VERSION.zip`;
- `BUILD-METADATA.json`;
- `SHA256SUMS`.

The current verifier/schema is an offline prototype, not adopted production wire.
The production schema must be explicitly fixed and reviewed before consumption.
Full package digest binds its manifest/inventory and exact file bytes; evidence
must additionally record manifest, descriptor, source archive, canonical source,
Cargo.lock and build metadata digests. Existing `blocked_identity_and_origin`
metadata must not be relabeled as ready or stripped to qualify a candidate.

## Transport and independent trust

Proposed exact request input is a fixed tuple: version, full commit, expected
descriptor SHA256 and a contract-selected base, not an arbitrary download URL.
Validate running OS/CPU first. Permit only HTTPS with standard certificate
validation; no HTTP, credentials, query/fragment, arbitrary redirect host or
alternate version on failure. The proposed first implementation rejects redirects
rather than silently expanding trust; actual host behavior must be verified before
adoption. Reuse current bounded descriptor/archive/member limits and strict parsers.

The expected descriptor digest must come through an independently trusted review
and delivery procedure and be checked by a trusted verifier/launcher. A URL path,
pin copied beside the payload or same-origin SHA256SUMS is not independent
authentication. Immutable-looking paths do not make the host or its Git branch
write-once. A signed descriptor is an alternative only after key custody/bootstrap/
rotation/revocation policy and any dependencies are approved. No new key is created.

Origin HTTPS identity, publisher provenance and same-UID execution integrity are
separate properties. This path proposal does not resolve ADR-033's execution-image,
peer/stop, bootstrap or loader boundaries. Do not bypass those outstanding gates
to call this an official installer.

## Local installer/update behavior to review

No download-and-pipe-to-shell command is provided before an eligible package exists.
An adopted entrypoint would receive the exact tuple, validate its bytes and all
artifact bindings, stage under the existing owned installation root, and retain
old binaries plus all account/profile/key/vault/mail data. Preserve user-data
roots and never automatically delete unknown files. The staged and active release
directory must equal the exact approved inventory; an extra, missing or substituted
file aborts activation and preserves the old pointer. Do not silently delete or
carry unexpected release files. This exact-inventory rule needs implementation
and tests; the existing installer's listed-file checks are not proof of it.
Never introduce a plaintext vault or invoke account/permission APIs.

Before activation, the adopted lifecycle contract must authenticate the exact
predecessor, stop it and prove exit/socket disappearance. The existing updater's
activate-before-stop order is not sufficient proof. Unknown predecessor, open
storage lock, failed inspection, seal failure or policy denial leaves the old
pointer intact; no process-name/PID kill or writable-image fallback. A genuinely
staged scope continues through existing exact-key/profile/epoch recovery; pending
scope is never auto-confirmed on startup.

Do not downgrade after a possible migration. Unless synthetic old-reader/vault/
profile compatibility is proved, recovery is a separately pinned higher version
that repairs forward. A retained old binary is not a data rollback guarantee.

## Required tests and publication gates

1. Local mock HTTP: exact host/prefix/tuple, HTTPS/redirect rejection, wrong OS/CPU,
   missing asset, duplicate/oversized/invalid descriptor, wrong checksum, substituted
   source/manifest/package, extra release files, changed bytes and interrupted
   transfer all fail closed.
2. Package/source consistency at one final commit; source/file/manifest/lock/build
   digests and reproduced provenance. No metadata rewriting to borrow old CI success.
3. Actual Linux native install/update/storage preservation/Core stop/start and
   scope recovery at that final commit, with no automatic grant. No synthetic
   ELF or mocked Core success substitutes for these tests.
4. Original Windows normal API and 403/429 Atom fallback, and original Mac Intel/
   ARM selection remain unaffected. Existing selector tests demonstrate that both
   ordinary partial release tags and Linux-prefixed tags are unsafe alternatives.
   Validate the proposed static path without creating a shared release entry.
5. ADR-033's coordinated execution/IPC/stop and same-inode race tests pass under
   the adopted policy; independent findings closed and actual human review of
   the exact final SHA/range/files recorded. Mac compatibility failures remain
   failed, not reused or reported passed by a Linux-only decision.
6. After explicit path/trust/policy adoption and all gates: publish only frozen
   bytes, re-download and compare every expected digest, then apply the authorized
   Songpa update and perform normal terminal scope confirmation and three actual
   account reads. Do not infer those reads from package or permission readiness.

## Minimum outstanding decisions

- Accept this exact existing-host namespace, choose a different reviewed path,
  or use a fully eligible normal release. Do not create any path/channel now.
- Adopt exact descriptor authentication and trusted bootstrap, redirect policy,
  retention/no-overwrite procedure and version-pinned update contract.
- Record the user's existing conditional Linux-only publication authorization in
  the adopted policy; do not request the same general approval again. It does not
  waive failed gates or adopt this new path/trust contract. Shared selector
  regressions and required human review remain mandatory; Mac failures stay failed.

An address decision is not needed for offline scope/kernel tests or local package
preparation. It is required before real transport/publication of this proposed
route. This proposal does not authorize site publication or implement a product
installer; contracts must be approved before production integration (AGENTS §7).
