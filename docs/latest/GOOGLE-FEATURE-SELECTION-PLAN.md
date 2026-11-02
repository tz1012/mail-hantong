# Google feature selection Implementation Plan

> For agentic workers: follow repository AGENTS.md. Independent implementation
> slices share this workspace; preserve all existing dirty/untracked changes.

**Goal:** Ordinary personal CLI uses finite feature selection and Google consent
to reach ready, without a second HanTong/Windows Hello account/CLI approval.

**Architecture:** Live OAuth verification creates private, one-use Core authority
for the exact selected account and fixed local feature set. Keep CNG mutual
authentication and durable source-bound registration/profile/scope transitions.
Provider feature selection precedes Google for least privilege. Per-send human
approval, AI/MCP isolation and legacy advanced native setup remain unchanged.

**Tech Stack:** Existing Rust workspace, Windows CNG/DPAPI, SQLCipher, native IPC.
No additional dependencies. Contract: ../contracts/google-personal-cli.md.

- [x] Contracts/input: add GoogleLocalProfile and strict optional wire field;
  legacy absent maps None, profile/mode mismatch and reauthorization rejected.
  Parent owns contracts crate + IPC client/server context/wire + documentation.
- [x] Persistence: Store worker owns Store, DB v22 DDL and migration. Preserve
  v21 schema snapshot, every legacy row/hash, source checks, encrypted rollback,
  request-derived key/profile recovery and no grant effects during migration.
- [x] Core: Core worker owns apps/mail-hantong-core. Bind actual setup peer,
  publish personal verified OAuth without native account review, mint/consume
  typed authority, configure L then B/C through OAuth-specific Store methods.
  Existing advanced paths and per-send/native mutation proof remain unchanged.
- [x] CLI: parent integrates read/read-write feature selection before Google, finite
  local profile mapping, no native local approval for guided ready. English and
  Korean copy calls this feature setup; expose no internal IDs on normal path.
- [x] Regression: preserve send proof denial, invalid scope/peer/source rejection,
  actual authenticated fake-provider registration/profile/policy integration,
  migration/crash recovery, cancellation/no automatic inbox and legacy behavior.
- [x] Integrate once slices land: regenerate traceability, run check_all offline,
  fix failures, build next configured preview, verify actual install/cold start.
  Actual provider checks remain separate and require user-authorized interaction.
- [x] Record exact files, commands/results, limitations and human security review
  status in PROGRESS.md; do not infer completed human review from user UX decision.

- [ ] Human security code review and public release readiness remain separate
  gates. Actual provider verification covered read-only connection, persistence
  and CLI authentication; write/send provider trials were not performed.

Implementation batches follow the owner's instruction to implement first then
perform combined checks; no repeated RED/GREEN loop or automatic commit/push.
