# Terminal personal sending — 2026-11-02

Latest user instruction supersedes ADR-028's personal native window. Human
security review required. Normal personal Gmail composition, review, confirm and
cancel occur inside CLI/TUI. Browser OAuth remains the provider login surface.
This is terminal interaction intent, not OS human-presence verification; a
same-user program with the default CLI key can request these actions. No claim
that TTY or client identity distinguishes a human from AI is permitted.

Use existing authenticated `mail.send.request` tool/grant with strict new modes:

- `prepare`: exact existing request fields (`account_id,draft_id,expected_version,
  operation_id,idempotency_key,payload_hash`) plus `mode`. Create/reuse the bound
  pending review, verify exact personal eligibility, return preview; schedule no
  job, native window or provider call.
- `confirm`: exact `mode,account_id,review_id,payload_hash`. Verify authenticated
  owner/epoch/account, expiry and hash and exact personal eligibility, then queue
  one terminal send job. Repeats only inspect/reuse the same claim/result.
- `cancel`: exact `mode,account_id,review_id`. Validate owner/epoch/account and
  personal eligibility; terminate pending with APPROVAL_REQUIRED/interrupted.
  Never cancel or resubmit a committed sending claim.
- Existing `status` is unchanged. Existing native `request` remains a protected
  compatibility route; it must not open a personal window for the exact personal
  registration. Such callers receive HUMAN_STEP_REQUIRED and use prepare/confirm.

Prepare response is the existing receipt plus `preview`, null for terminal
receipts. Pending preview has exact fields `from,to,subject,body,attachments,
payload_hash,draft_id,version`. Attachments are metadata only with exact fields
`filename,media_type,size,sha256`. The preview comes from the immutable Core
snapshot, is never truncated, and must fit the existing 1MiB response frame
(otherwise QUOTA_EXCEEDED, no sending). No credential or raw provider text is
returned. The client checks account/review/hash/draft/version and field bounds.

CLI displays complete escaped terminal-safe sender/recipients/subject/body and
attachment metadata, then offers Send/Cancel. Only explicit Send calls confirm,
once; EOF/cancel never confirms. Headless/JSON/no-input sending is
HUMAN_STEP_REQUIRED before effect; no yes/approved shortcut is introduced.
Status/draft commands remain usable for automation as already contracted.

Core source guard remains exact Active/default Cli/google_oauth/writable/single
matching account/default_cli_send_tools/Gmail ReadSend/current version+endpoint.
It repeats in the same immediate Store claim transaction. Confirm queues a
separate terminal job that never calls native approval APIs. Existing policy,
durable SendExecute audit, immutable CAS/outbox, expiry, no duplicate provider
poll, startup recovery and SEND_UNKNOWN no-resend rules remain. No DB migration
or broader MCP grant is introduced. Tests use synthetic data only.
