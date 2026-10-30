# Managed AI terminal clients

Draft; human security review required. AI-07, CLI-16, SEC-08.

AI is initiated only by explicit freeform chat/login commands. No mail/cache/account context is loaded or sent automatically. Provider subprocesses own credentials: this application never opens token files, requests raw auth tokens, logs raw responses, or handles subscription tokens. Official executables are resolved from absolute PATH entries or known official npm vendor executable locations, with no shell interpolation or executable path supplied by mail. Missing tools fail with a stable actionable provider-missing result.

Codex uses the official app-server stdio initialize, managed account/login/start type chatgpt, trusted-HTTPS browser authUrl and managed completion notification. Auth status returns only an authenticated boolean/type. Chat creates a separate empty working directory and a dedicated thread, disables shell, MCP, apps, plugins and web tools, restricts filesystem reads, and refuses all server-initiated tool/approval requests. Only bounded sanitized agentMessage deltas are displayed. Turn IDs/thread IDs must match the active request. No approvalPolicy=never autoapproval or unrestricted filesystem/network tool flags are used.

Claude uses its official native CLI. Browser sign-in uses auth login --console for API usage billing, not claude.ai subscriber OAuth. Headless chat accepts only api_key/api_key_helper/third_party auth modes, refuses subscription or ambiguous auth modes, disables built-in and MCP tools/customizations, and streams sanitized text. The provider owns credential setup; mail-hantong does not extract CLI tokens. Anthropic requires prior approval for third-party products offering claude.ai login or subscriber limits, so that is not advertised as supported.

Every child has bounded frames/output, deadlines, kill and reap on error/drop; raw stderr is discarded with a bound. Tests use fake subprocess streams and never launch actual provider auth/inference. MCP agent mail use remains a separate explicit registered Core permission flow.

Sources: https://learn.chatgpt.com/docs/app-server ; https://learn.chatgpt.com/docs/config-file/config-reference ; https://code.claude.com/docs/en/cli-reference ; https://code.claude.com/docs/en/agent-sdk/overview
# Session commands extension (implementation draft)

Session commands are local preference controls. `/provider` and `/provide`
select `openai` (an alias for the managed Codex provider), `codex`, or `claude`.
Changing provider starts a clean session and resets agent access. `/model`
queries the provider's live catalog, never a hard-coded model-name list.
`/mode light|med|high|ultra-high` maps to low/medium/high/xhigh only when that
model advertises the requested effort. For models advertising max instead of
xhigh, ultra-high sends max. `/fast on|off` changes a real provider
service setting: Codex turn serviceTier priority/default, or Claude's explicit
session fastMode setting, gated by the current catalog and provider confirmation.
Unsupported settings fail without changing state. `/status` shows configured
preferences; it is not a delivery-speed guarantee.

Bare model/provider/mode commands use an arrow-key chooser in an interactive
Windows console; Escape cancels without changing preferences. Redirected input,
JSON, and no-input paths return lists without waiting for keys. `/cli` and a
hidden runtime handoff command switch local frontends without conveying mail or
approval authority. They are not permission controls.

`/agent` reports state and defaults off. Explicit on connects only the packaged
hantong MCP server under its own protected registration, checks current account
AI consent through Core, and exposes only its current advertised mail tools.
Codex dynamic callbacks are bridged to this real local MCP server; Claude uses
one explicit strict MCP configuration. Generic shell/filesystem/web/permission
requests remain disabled. Turning agent off starts a clean provider thread so
prior mail context is not silently reused. Every outgoing submission still
passes the native human review and durable single-attempt ledger.

Provider model fields: [Codex Model schema](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/Model.ts),
[Codex turn parameters](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/TurnStartParams.ts),
[Claude runtime model capabilities](https://code.claude.com/docs/en/agent-sdk/typescript#modelinfo),
[Claude session fast mode](https://code.claude.com/docs/en/fast-mode).
