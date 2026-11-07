# Linux pinned release proposal — NOT an active release contract

Status: proposal; production integration, origin/channel creation and public
release require approval. The user authorized disconnected local prototypes and
synthetic isolation tests on 2026-11-07, not a new trusted download origin.
Requirements: INST-01/03/04, SEC-02, CLI-01/04; scope/approval contracts remain intact.

## Authorization and policy delta

The existing cli-update contract and shared release eligibility are unchanged.
Proposed delta: Linux x86_64 may become independently eligible only after its
complete native gates and human security review pass. Failed Intel/ARM Mac
compatibility gates remain failed and block Mac publication. No continue-on-error,
renamed old Mac assets, shared-feed partial release, or assumed risk acceptance.

Before production implementation, approve the precise Linux HTTPS origin and its
owner, URL/redirect allowlist and retention policy. It must not create a release in
the existing repository's Atom feed. Existing Windows API and 403/429 Atom fallback
and old bundled Mac update.sh must remain byte-identical until separately reviewed.
Fixing only a new bootstrap does not migrate those old clients.

## Proposed binding

A reviewed, out-of-band pinned descriptor binds a full Git commit (40 hex), release
version, linux-x86_64 target, canonical source hash, source archive digest, package
digest, manifest digest, build metadata digest, file inventory and Cargo.lock hash.
The actual format is not yet adopted; the offline prototype's schema is a test
fixture, not an authorized wire/publication format. Hashes establish consistency,
not publisher authenticity; a descriptor obtained alongside an archive from an
untrusted location provides no independent trust.

Production validation must derive the running OS/CPU, check native ELF headers,
validate complete source/manifest/build relationships, and fail before extraction
or installation for missing assets, target mismatch, duplicate fields/members,
unsafe paths, links, oversized data or any digest mismatch. Exact pins never fall
back to another version, public Atom/API feed, arbitrary shell or PATH executable.
The archive is immutable through validation and activation; local offline verifier
success does not supply that execution guarantee or resolve same-inode writes.

## Preservation and activation

Use owned installation roots, package inventory and staged activation; retain prior
versions and all account/key/profile/vault/mail data. Never copy a user's secrets
into release evidence. No account creation, login, grant, send or Trash action is
part of installation. Scope recovery preserves expired/pending/staged distinctions,
channel provenance, bound single-use cancellation and explicit normal review.

A failure before activation preserves the old pointer. A crash after activation
must be diagnosed against both the active binary and schema, not reported as an
automatic rollback. Until old-reader compatibility is proved with synthetic DB,
profile and vault fixtures, refuse downgrade and repair through a pinned higher
version (roll-forward). Retained binaries are not proof of safe data rollback.

## Required gates and review

Freeze a final full commit only after integration. At that same commit run Linux
native package/install/update/Core restart/scope recovery, all shared-code checks,
and Windows plus both Mac selection regressions. Test missing target assets under
normal API and 403/429 fallback without changing the shared feed. Publish exact
source archive and canonical hash, package manifest, BUILD-METADATA, file hashes
and SHA256SUMS; record all final digests and downloaded-asset verification.

The Linux same-UID/same-inode modification window remains an unresolved design
blocker, not a risk accepted by this proposal. Sealed images or other mitigations
need explicit process-image/IPC identity analysis, deterministic race tests and
human review before integration. Do not silently relax peer identity or ACLs.

AGENTS.md sections 6–7 require separate review of the final range after
6322b9a1a5f27f7e0ef32a65554ef20a79db0be6: executable trust/IPC, install activation,
scope transitions and any Secrets changes. Record exact reviewed SHA, files,
reviewer's actual confirmation, findings and resolutions. Dependency approval and
the earlier user confirmation are not review of future changes.

The disconnected verifier and synthetic shell tests do not authorize a real
origin, production wiring, CI dispatch, Songpa installation or public release.

## Subsequent authorization and remaining decisions (2026-11-07)

The user subsequently reports the owner's words “배포도 승인하고 보안도 오케이해”
and directs implementation, independent review, actual human review and final
same-commit verification before Linux-only publication and Songpa validation.
This authorizes continuing the work and conditional publication; it is not a
review outcome or a waiver of failed gates. The local Linux package provenance
stage now emits source_commit, a SOURCE-COMMIT source bundle, descriptor and
checksums and runs the consistency verifier before producing a candidate.

No exact HTTPS origin/repository, descriptor authentication authority, retention
or redirect policy has been chosen. No origin, feed or public release is created.
The old Windows/Mac shared Atom/API paths remain unchanged. Origin-dependent
transport/update activation stays blocked until those explicit trust decisions.

The sealed-image/context proposal is ADR-033. The new execution identity,
interpreter pin and predecessor stop transition are not adopted by a broad
deployment authorization. Memfd names, caller-provided paths, equal digest alone
and removal of existing inode checks are not valid replacements for that contract.
The latest user steering requires sealed-copy verification and coordinated
manifest/IPC/stop design and forbids noexec-policy or mutable-image fallbacks.
