# Songpa: complete three actual account reads

## Completion and known state

Completion is **two Gmail accounts plus one Daum account each successfully listing
and opening actual mail in Songpa Linux HanTong**. A deployment, scope grant or
synthetic provider test is not completion. User evidence: Gmail 1 actual read
succeeded; Gmail 2 and Daum receive CLI SCOPE_DENIED; Daum IMAP setup is ready.
Do not recreate its app password or repeat setup based on a scope denial alone.
Current installed version and current pending/staged state remain unknown until
updated non-secret user evidence arrives. Request IDs stay local.

No send, draft creation, Trash, account reset, vault/key/permission change or
automatic scope grant is part of the read-verification objective. A normal scope
preview may offer a broader toolset; approval of a read test is not automatic
acceptance of every displayed tool. Preserve explicit user review.

## Is a separate Linux origin mandatory?

**No.** It was a proposed way to isolate a partial Linux release, not a mail read
prerequisite. An already-installed working recovery path or a valid full official
release needs no new origin. An explicitly pinned download also avoids selection
of the wrong version for that caller, but creating its release may still affect
other clients' feeds. Pinning one caller does not isolate publication effects.

Read-only GitHub release enumeration on 2026-11-07 found beta.29 is the highest
published beta, with Linux, Windows and both Mac assets. This does not prove the
Songpa installed version. Local tag beta.29 is `5d1e4734dfe1475c951f8d052079212e1437dc50`.
The scope recovery fix `add6cbbf7d7a9073b290a42aef51d8353f74db95`
is a descendant and is documented as unreleased. No publication was performed.

## Existing beta.29 recovery: exact source limits

Evidence from the actual beta.29 tag, not current candidate documentation:

- `apps/mail-hantong-core/src/unix_scope_refresh.rs::prepare_exact` compares
  accounts/tools/base/server/channel **before** calling pending expiry. A changed
  target can keep returning Conflict even after the old deadline. Waiting five
  minutes alone is therefore not a general repair for this version.
- `unix_setup.rs`'s scope Status maps the stored phase without expiring it.
  It provides safe observation, not the candidate's normal expiration repair.
- Its action Cancel match has no ScopeRefresh arm; it returns
  CapabilityUnsupported for that kind. Do not instruct beta.29 users to depend
  on the candidate's bound single-use scope cancellation.
- `unix_registration.rs::activate_scope_refresh_for_channel` requires the base
  approved channel to equal terminal_cli for terminal confirmation. A Google OAuth
  base can therefore fail despite valid terminal review. The candidate separates
  original registration provenance from the new decision channel.
- The fixed `hantong_scope_refresh stage=request_conflict field=...` records are
  candidate behavior, not promised beta.29 diagnostics.

These are supported failure mechanisms, not proof of Songpa's exact row/channel.
No production DB inspection or edit is justified by this analysis. If Songpa
already has the repaired version, its normal status/expiry/cancel/staged recovery
path should be tried before considering another release. On unrepaired beta.29,
repeated refresh/review/timeout attempts cannot be promised to complete the goal.

## Ranked safe routes and minimum gates

| Route | Minimum change/test/decision | Assessment |
| --- | --- | --- |
| Existing installed normal recovery | Confirm version and state only. On a repaired version: normal expiry or explicit bound Cancel for live pending; preserve staged recovery; fresh exact preview and human confirmation. Then read each account. No new package/origin. | Shortest if already supported; applicability unknown. Beta.29 has the concrete limits above. |
| Existing beta.29 pinned official asset | Exact existing Linux URL/checksum/manifest, normal preservation/restart and supported recovery. No new publication or feed change. | Assets exist, but this source lacks the scope fix. Reinstalling the same version is not an evidenced repair; do not reset data or recommend it as the solution. |
| New full release in existing official repository | Fix and validate all published platforms at one final commit, including unresolved Mac compatibility; normal artifact/source/manifest/checksum and security review gates. | Reuses current origin/contract, but Mac failures currently block a truthful full release. Do not reuse old Mac binaries under new version metadata. |
| New Linux-only release in existing shared feed, even if Songpa pins it | Must prove every affected old API/Atom selector remains safe, not just the Songpa client. | Unsafe today: a v-prefixed partial tag breaks Mac selection/Windows fallback. A non-v Linux tag also makes current Windows Atom parsing fail. Explicit pin or GitHub latest flag does not repair old clients. |
| Feed-independent, exact Linux candidate on an existing official host | Define an explicit static artifact path or other mechanism proven not to enter shared release feeds, exact version/redirect/trust rules; normal final package/installation/Core/scope/security gates. No address is selected here. | Possible without a new domain/repository, but still a distribution-contract/path decision, not an already functioning route. Do not publish or silently invent a namespace. |
| Separate repository/origin | Same runtime/package/security gates plus new exact owner/address/trust/retention decisions. | An option, not a prerequisite and not automatically the fastest route. User explicitly says undecided; do not create it. |

A draft release is not public distribution, and private CI artifacts are not an
already-supported official update mechanism. Do not backdate/relabel a release,
replace beta.29 assets, manipulate feed metadata or disable validation to make a
partial candidate look compatible. Merely appending different-version bytes under
an old release also needs a reviewed provenance/selection contract; do not do it.

## Shortest conditional path

1. Obtain current installed version, most recent refresh code and known request
   state only. No request ID, account address, mail body or secret needs to leave
   Songpa. This distinguishes existing recovery from a required code update.
2. If repaired code is installed, use its normal recovery and explicit preview;
   never cancel a staged decision or automatically expand grants. If it is
   beta.29 with the demonstrated blockers, prioritize the narrow scope repair
   rather than new account setup or a new origin. Freeze the minimum reviewed
   candidate only after assessing its connected runtime/security dependencies;
   do not blindly cherry-pick a reported fix or drop mandatory security gates.
3. If an update is necessary, prefer a supported existing distribution path when
   its gates pass. Otherwise present the precise feed-independent official-host
   path decision; a new repository is not inherently required. Same-inode execution
   risk and required review are not waived to accelerate this route.
4. Validate that exact candidate with synthetic old registration/profile/DB
   preservation, pending expiry/cancel/replay denial, OAuth vs terminal origin,
   staged restart, unchanged grants before confirmation, package/install/update
   and Core restart. Test shared selectors on Windows plus both Mac targets.
   Actual Mac compatibility failures remain separate, accurately blocked.
5. After an eligible deployment/update, obtain three independent actual list/open
   outcomes and stable error codes if unsuccessful. Recheck Gmail 1 after update.
   Keep mail content local; account labels and IDs need not be reported. Only
   three actual successes satisfy completion; setup ready or scope ready does not.

No supported date estimate is available until installed state is known and the
needed recovery/update route is selected and validated. A source defect diagnosis
is not a time estimate. Report state transitions and evidence, not a guessed date.

## Validation of the feed alternative

Added `test_SEC03_linux_namespaced_tag_still_breaks_legacy_atom_fallback` exercises
the unchanged real PowerShell parser with fake transports and a Linux-prefixed tag
followed by a valid old Windows release. Both 403 and 429 fail with `feed is invalid`
and no installation marker. This is a reproduced compatibility blocker, not a
repaired update path. Existing normal API, Unix selection, explicit pin and rate
fallback tests are retained. No network, real installation or account in this test.

References: `install.sh` first Atom tag selection; `install.ps1::Get-AtomReleases`
strict v-tag parsing; `tests/Build/test_linux_feed_isolation.py`; actual beta.29
tagged Core code; current `unix_scope_refresh.rs` and `unix_registration.rs`;
[published beta.29](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.29).
