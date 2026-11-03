# Managed AI terminal clients

> 2026-11-03 approved contract extension: [built-in AI mail access and
> reviewed trash](builtin-ai-mail.md) defines a separate `ClientKind::Ai`
> identity. After the first provider/account feature setup, supported mail
> tools are discovered automatically; `hantong-mcp register`, copied request
> IDs, and `/agent on` are not prerequisites for the built-in flow. This is an
> approved contract change, not an implementation or beta-support claim.

Draft; human security review required. AI-07, CLI-16, SEC-08.

## Current Windows execution limitation

This contract describes the managed protocol and tool boundary. Codex 0.163.0
on this Windows host rejects root-deny profiles under the previously selected
legacy backend. The 2026-11-04 candidate selects MXC in request-local thread
configuration, with root deny, the official `:minimal` runtime read baseline,
and read access to the exact new empty temporary cwd only. Direct tool network
access stays disabled, the returned named permission profile must match, and
shell/MCP/apps/plugins/web remain disabled except the authorized dynamic mail
tools. It does not modify the global Codex configuration, run setup/UAC, or
fall back to root/workspace read when MXC is unavailable.

A command-only synthetic probe on this host read the allowed canary and was
denied the sibling canary; a metadata-only thread started with the exact
profile. Neither result alone proves a model turn or AI mail exchange. MXC
support remains host-dependent. Fail closed with CAPABILITY_UNSUPPORTED on an
unsupported host. Runtime-path exceptions are platform-defined, not a claim
that only the empty cwd can be opened. Human security review is pending.

Official references: https://developers.openai.com/codex/permissions and
https://developers.openai.com/codex/windows/windows-sandbox .

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

### User-owned interactive Claude Code session (2026-11-04)

An explicit interactive Claude request may hand the existing terminal to the
official unmodified Claude Code binary when its existing login uses OAuth.
This is a native Claude Code session, not embedded HanTong chat. Stdin/stdout/
stderr are inherited directly; HanTong supplies no prompt, parses no transcript,
and does not handle any provider credential. No print/stream-JSON/SDK mode,
automatic tool approval or bypass-permissions option is allowed in this path.
Noninteractive, JSON and --prompt requests keep the API-authentication boundary.

The native session gets an explicit provider-bound HanTong MCP configuration
after the same Core-owned account/transfer feature setup. A new empty cwd,
disabled built-in tools/customizations and strict MCP configuration limit
accidental context. Conversation history follows Claude Code's own settings;
HanTong does not capture it or claim that interactive history is disabled.
The print-only no-session-persistence flag is not used here. The native-session
notice discloses this before handoff. Existing provider/account consent,
tool policy and terminal confirmation of sends/Trash remain mandatory. The
official CLI owns its session and returns control on exit. API-authenticated
embedded Claude continues to use the existing protocol and validation.

2026-11-04 runtime correction: `--safe-mode` disables explicit MCP too, so it
is not used. The native process instead uses restricted mode, no built-in tools,
strict inline MCP, disabled skills/commands, explicit default permission mode,
and per-session settings disabling hooks, auto memory, claude.ai connectors and
Auto mode. Process-local environment disables CLAUDE.md discovery, auto memory
and automatic provider updates. Chrome integration is disabled. Global provider
settings and credentials are not rewritten; administrator policy still applies.
Ctrl+C twice returns to HanTong; slash-command availability is not promised.
Claude auth status is read as one bounded JSON document (1 MiB, 20-second
deadline), not as a line-framed session message. Normal streams retain framing.

Reference: https://code.claude.com/docs/en/legal-and-compliance and
https://code.claude.com/docs/en/mcp . Native model replies and real mail mutations
are not claimed verified by login status or argument tests. Human review pending.

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
