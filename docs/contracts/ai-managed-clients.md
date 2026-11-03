# Managed AI terminal clients

> 2026-11-03 approved contract extension: [built-in AI mail access and
> reviewed trash](builtin-ai-mail.md) defines a separate `ClientKind::Ai`
> identity. After the first provider/account feature setup, supported mail
> tools are discovered automatically; `hantong-mcp register`, copied request
> IDs, and `/agent on` are not prerequisites for the built-in flow. This is an
> approved contract change, not an implementation or beta-support claim.

Draft; human security review required. AI-07, CLI-16, SEC-08.

## Current Windows execution limitation

This contract describes the managed protocol and tool boundary; it does not
claim that a provider can execute on every host. In the beta17 candidate,
Codex 0.163.0 named profiles on Windows can reject the required effective
root-read/temp-read capability even when an elevated backend is already
configured. HanTong fails closed with localized `CAPABILITY_UNSUPPORTED`,
preserves the existing registration, checks the exact active permission
profile, and does not edit UAC/configuration or fall back to a broader legacy
profile. The on-request enum and namespace tool protocol are implemented, but
no real Codex model turn or AI mail exchange was performed. A read-only status
check is not model-turn evidence. Provider authentication and inference remain
separate environment gates.

AI is initiated only by explicit freeform chat/login commands. Built-in mail
tools are not attached until the Core-owned provider/account feature setup has
completed; after that setup, supported tool definitions are discovered and
connected automatically. No mail/cache/account context is uploaded at chat
startup. Provider subprocesses own credentials: this application never opens
token files, requests raw auth tokens, logs raw responses, or handles
subscription tokens. Official executables are resolved from absolute PATH
entries or known official npm vendor executable locations, with no shell
interpolation or executable path supplied by mail. Missing tools fail with a
stable actionable provider-missing result.

Codex uses the official app-server stdio initialize, managed account/login/start type chatgpt, trusted-HTTPS browser authUrl and managed completion notification. Auth status returns only an authenticated boolean/type. Chat creates a separate empty working directory and a dedicated thread, disables shell, MCP, apps, plugins and web tools, restricts filesystem reads, and refuses all server-initiated tool/approval requests. Only bounded sanitized agentMessage deltas are displayed. Turn IDs/thread IDs must match the active request. No approvalPolicy=never autoapproval or unrestricted filesystem/network tool flags are used.

Claude uses its official native CLI. Browser sign-in uses auth login --console for API usage billing, not claude.ai subscriber OAuth. Headless chat accepts only api_key/api_key_helper/third_party auth modes, refuses subscription or ambiguous auth modes, disables built-in and MCP tools/customizations, and streams sanitized text. The provider owns credential setup; mail-hantong does not extract CLI tokens. Anthropic requires prior approval for third-party products offering claude.ai login or subscriber limits, so that is not advertised as supported.

Every child has bounded frames/output, deadlines, kill and reap on error/drop; raw stderr is discarded with a bound. Tests use fake subprocess streams and never launch actual provider auth/inference. External MCP mail use remains a separate explicit registered Core permission flow; it is not silently broadened by built-in AI consent.

Sources: https://learn.chatgpt.com/docs/app-server ; https://learn.chatgpt.com/docs/config-file/config-reference ; https://code.claude.com/docs/en/cli-reference ; https://code.claude.com/docs/en/agent-sdk/overview
# Session commands extension (implementation draft)

Session commands are local preference controls. `/provider` and `/provide`
select `openai` (an alias for the managed Codex provider), `codex`, or `claude`.
Changing provider starts a clean session and connects only that provider's
separately configured AI mail profile. `/model`
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

`/agent` reports the built-in tool state. Built-in AI connects only its
provider-qualified `ClientKind::Ai` profile (`ai-codex.json` or
`ai-claude.json`) after Core-owned feature setup, and exposes only its current
advertised mail tools. `/agent off` disconnects tools and clears provider
history. Generic shell/filesystem/web/permission requests remain disabled.
External MCP still uses its own explicit registration and consent. Every send
or trash request remains a reviewed, durable single-attempt operation; the
model receives no confirm/approval endpoint.

Provider model fields: [Codex Model schema](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/Model.ts),
[Codex turn parameters](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/schema/typescript/v2/TurnStartParams.ts),
[Claude runtime model capabilities](https://code.claude.com/docs/en/agent-sdk/typescript#modelinfo),
[Claude session fast mode](https://code.claude.com/docs/en/fast-mode).
