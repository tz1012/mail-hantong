# Read/send verification and conditional beta publication

Owner request, 2026-11-02: verify actual mail reading and sending, then publish
a beta only if they work. Preserve dirty source, real accounts, keys and backups.
Security code still requires human review. No automated provider traffic in tests.

- Verify one real account through normal authenticated CLI. Keep raw headers,
  message bodies, tokens and callback values out of logs and public artifacts.
  Record documented size-limit rejection separately from successful bounded read.
- Add the missing ordinary feature action: enable composing/sending for the
  selected existing personal read account. Fixed ReadSend selection precedes
  fresh same-account Google consent; no extra account/feature Hello prompt.
- Bind the existing OAuth permit to that exact reauthorized account, current
  Google credential version and peer. Preserve the same registration/key/pin.
  Support only exact single-account OAuth-origin B at epoch2 -> C at epoch3.
  Do not replace registrations, widen other accounts, or enable AI/MCP/modify.
- Store uses a distinct v9 scope hash with existing channel bytes and a new
  domain for B->C. Freeze all v1-v8 meanings. Schema v23 adds hash9 constraints
  through a complete v22->v23 encrypted migration, keeping old rows and sequence.
- Run integrated regression checks and a new configured local preview, preserving
  previous data/version. Request fresh ReadSend consent through Aside.
- Prepare one clearly named synthetic self-addressed draft. Actual sending
  requires the user to complete the existing native message-bound approval;
  never mint approval or perform Windows Hello for them. Verify sent result and
  receipt using exact IDs and marker. Never retry an uncertain send.
- Prepare beta package/source/release notes and secret-free source snapshot.
  Publish the GitHub prerelease only after actual read/send/receipt are verified.
  Keep unresolved security review and Google verification status explicit.

Ownership: Store executor owns hantong-store and schema23/migration files.
Parent owns contracts input, IPC/CLI/Core feature action, integration and release.
