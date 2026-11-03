# Installed CLI update

Status: implementation contract for the owner's 2026-11-03 request.
Related: INST-01/03/04, CLI-01/04, SEC-02.

An installed Windows CLI accepts `hantong update`. Its owned launcher directory
also provides `update hantong`: the `update.cmd` shim accepts exactly the single
argument `hantong` and delegates to the same command. It does not redefine a
PowerShell function, alias or profile. Unrelated/unmanaged launchers are never
overwritten, and uninstall removes only the exact owned shim.

Update requires interactive stdin/stdout; JSON, no-input and redirected mode
return HUMAN_STEP_REQUIRED before any installer/network effect. Unknown options
and extra command arguments fail before effects. Help remains read-only.

Resolve the current executable's version directory, installation owner marker,
current version pointer and bounded release manifest. Reject reparse/linked paths,
unsafe paths, changed manifests/owned executables and mismatched active versions.
Invoke only that version's packaged `update.ps1` whose manifest hash matches.
Never search PATH for an updater or fall back to a default installation root
when running a development/unowned binary. The script is the public bootstrap
installer bundled by the packager, not an AI-generated command or downloaded
arbitrary script. Pass the verified installation root as a typed argument to the
Windows system PowerShell, with inherited terminal input/output.

Reuse latest-release selection, HTTPS restrictions, SHA256SUMS and package-file
verification, staged installation and final activation. Preserve user data,
profiles, credentials and previous installed versions. Update does not perform
account login, OAuth consent or mail operations. First use of a new core may
run its existing schema migration; older cores must not open migrated data.
Do not promise a downgrade or rollback of a database migration.

Installation shows honest phase progress and real item counts for extraction,
file verification and copying. Downloads retain PowerShell byte progress; phase
percentages are progress through installation steps, not remaining-time estimates.
Progress uses the host progress stream, with separate bootstrap/package IDs,
respects disabled progress, adds no JSON/success-stream fields, and clears on
success or failure. Display 100 percent only after successful activation and any
requested PATH update. Tests use isolated synthetic packages and mock transports;
do not update the user's real installation or connect a real mail account.

Changes to package verification and launcher ownership remain subject to human
security review; automated test success does not close that review.

## Public release lookup under API limits (2026-11-03)

The installer must not require a personal GitHub token to download a public
package. On a GitHub REST 403/429 rate restriction, do not repeatedly call that
API. A bounded public releases Atom feed from the exact repository may supply
validated release tags; construct asset URLs only beneath that repository's
HTTPS release-download prefix. Explicit requested versions remain exact.
Disable XML DTD and external entity resolution; bound feed size and candidate
count. Obtain and validate bounded asset lengths before using the existing
download, SHA256SUMS, archive path and manifest checks. Unexpected status,
malformed feed/tag, host changes or missing/oversized assets fail closed. This
lookup fallback grants no mail, account, credential or arbitrary script access.

## Compatible release ordering

REST and Atom response order is not version precedence. Select the highest valid
compatible published package with both the exact Windows ZIP and checksums.
Compare numeric version and prerelease components without integer overflow;
stable releases outrank prereleases of the same core version. For the product's
legacy same-core channel transition, `beta` outranks the earlier `preview`
channel. Other prerelease identifiers retain SemVer precedence. An explicit
requested tag stays exact.

The latest channel must not move an owned installation's current pointer to a
lower version. Inspect only the exact bounded owner marker and bounded current
version file, rejecting reparse points before reading. Version selection grants
no installation ownership and does not replace the package installer's existing
ownership, manifest, hash, lock and activation checks.
