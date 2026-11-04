# External MCP mail integration v3

Status: included in the beta.23 release scope after the user's 2026-11-04
explicit scope approval. Published beta.22 remains v2
and does not include these extensions. Human security review remains required.

## 검토할 변경 범위

원래 계획의 MCP-07/PUSH-08과 M7 이벤트 카드를 완성하기 위한 변경안이다.
이번 구현 승인을 요청하는 범위는 아래 두 가지다.

1. 외부 MCP에 단일 메일 휴지통 **요청** 도구를 추가한다. Gmail은 계정·메시지 ID·Modify
   권한을, IMAP은 계정·메일함 wire/encoding·UIDVALIDITY·UID·catalog generation을 결속한다.
   최종 확인은 기존 한통 터미널에서 수행한다. 영구·대량 삭제와 자동 확인은 제공하지 않는다.
2. Core의 백그라운드 수신 감지, 원자적 이벤트 원장, 계정별 커서·보존·ack,
   MCP 리소스 구독 및 CLI watch/notify/hook 조회를 연결한다. 기본 60초 간격으로
   공급자 변경을 확인하며, 수신 사실을 외부 에이전트에 먼저 알린다. 제목·본문·주소는
   알림에 포함하지 않는다. 지원하지 않는 호스트에는 로컬 조회/훅 연동을 제공한다.

기존 등록에 새 권한을 추가하지 않는다. 휴지통과 알림 사용은 표시된 계정·기능을
명시적으로 재검토한 새 등록/동의로 시작한다. 이전 v2 등록은 기존 범위로 계속
동작하고, v3 전환은 기존 보호 프로필 교체·복구 절차를 따른다.
발송/삭제 자동 실행, 웹훅 URL 호출, 임의 셸 실행, 외부 서비스 메시지 전송은 포함하지 않는다.
호스트별 별도 identity와 원격 HTTP는 이번 범위에서 제외하며 후속 비교만 기록한다.
MCP 알림을 모델 실행으로 연결하는 지원 호스트의 동작까지 합성 통합 시험으로 검증한다.

구현 승인과 사람 보안 검토 완료는 구분한다. 본 제안은 실제 계정·발송 시험을
새로 승인한 것으로 간주하지 않는다.

## Goal

Make a local MCP desktop integration easy to configure while preserving
HanTong's Core-owned account, consent, audit, and terminal-review boundaries.
The first release remains local `stdio`; remote HTTP is a separate future
contract.

## Existing local convenience interface

`hantong-mcp config` emits one reviewable JSON object:

```json
{"mcpServers":{"hantong":{"command":"<absolute UTF-8 path from current_exe>","args":[]}}}
```

The command reports `std::env::current_exe()` as an absolute UTF-8 path and
never edits a host configuration file. It does not itself prove that the
installation is trusted; the host and the normal executable-installation
boundary remain responsible for that. Its output contains no profile bytes,
account IDs, mail data, secrets, or tokens. After an update, the user reruns it
so the path follows the new version directory.

`hantong-mcp doctor` returns bounded JSON containing `schema_version`, the
stdio transport and protocol version, Core state/code, external registration
state/code, exposed granted tool names, and an action such as
`start_hantong`, `register_and_review`, `inspect_error`, or `null`. It is
read-only and must not create a registration, start account setup, or widen a
grant. There is no installed-identity verification field. Stable errors remain
the existing HanTong error codes.

## Current external scope

The v3 compatibility target is these nine externally callable tools:

`mail.accounts.list`, `mail.folders.list`, `mail.folders.refresh`,
`mail.sync.page`, `mail.messages.search`, `mail.messages.get`,
`mail.drafts.create`, `mail.send.request`, and `mail.messages.trash.request`.

`mail.send.execute` is coordinator-only. The external host may request a
message review and inspect its status, but cannot confirm or execute the send.
The Trash tool is a separately reviewed single-message request with exact
account, mailbox, UIDVALIDITY, UID, catalog generation, and terminal-confirm
binding. The host cannot confirm or execute the move. Permanent and batch
deletion remain out of scope.

## Implemented extensions: Trash and receive events

The v3 external profile exposes a single-message Trash request. Core binds the
request to the reviewed account and exact Gmail or IMAP message identity, then
requires the existing HanTong terminal review before the provider operation.
Permanent deletion, batch deletion, automatic confirmation, and automatic retry
remain unavailable.

The same v3 registration may opt in to metadata-only receive tracking through
`mail.events.configure`. Core checks provider changes about every 60 seconds and
commits observations with their durable event records. Events are retained for
seven days with bounded per-account and cursor retention; expiry is enforced
during delivery, observation, and read-time pruning, and contain only the
event ID, account, mailbox identity and encoding, change kind, opaque cursor,
and registration epoch. They never include bodies, subjects, sender or
recipient addresses, attachments, or provider tokens.

The MCP resource URI is `mail-hantong://events`. `resources/read` performs a
bounded read of the durable feed and `resources/subscribe` enables local stdio
update hints. The MCP process polls about every five seconds while subscribed;
a hint is coalesced and does not carry exact mail counts, wake a host LLM, or
authorize a follow-up tool call. `hantong-mcp watch` and `notify pending` are
explicit local fallbacks for hosts without resource subscriptions. `notify ack`
acknowledges a cursor. `notify off` changes the shared tracking configuration;
local MCP unsubscribe only stops that process's notifications.

Revocation, epoch changes, or a failed authorization check stop disclosure.
Expired or foreign opaque cursors return `CURSOR_EXPIRED`; `SCOPE_DENIED` is reserved for
registration scope or authorization failure.
No webhook, arbitrary shell callback, remote HTTP endpoint, or third-party
message is invoked.

## Identity and host separation

Current beta behavior uses one fixed external MCP profile and registration
identity. V3 expands only the explicitly reviewed scope; it does not silently
upgrade existing v2 registrations. Built-in AI provider registrations are separate and are never
expanded by external MCP setup. This is compatible with multiple local hosts,
but it does not provide per-host revocation or per-host account/tool scopes.

Before enabling a future per-host identity, compare at least:

1. one fixed shared profile (current behavior, simplest recovery, coarse
   revocation);
2. deterministic host identity (better isolation, requires stable host
   identity and migration/recovery rules); and
3. explicit host registration (strongest user control, more setup and state).

The implementation must not infer a host identity from a process name, command
line, working directory, or MCP client declaration alone.

## Compatibility and security boundaries

- MCP 2025-11-25 newline-delimited JSON-RPC over local stdio remains the
  compatibility target.
- A host must support launching a local executable. URL-only remote MCP hosts
  are unsupported by this proposal.
- Core remains the only authority for account scope, consent, rate limits,
  audit, draft version, send approval, and provider access.
- Mail content and headers remain untrusted data and are never interpreted as
  policy or tool instructions.
- External registration remains separate from built-in AI setup.
- `config` and `doctor` are read-only convenience commands; neither changes
  consent or grants.

## Affected requirements and decisions

The existing `config` and `doctor` convenience commands are source-level
additions to the external MCP application and are documented above; they are
not themselves an authority change. The implemented Trash and receive-event
extensions, plus future per-host identity work, affect AI-07 external data consent,
AI-09 approval boundaries, MCP-01 transport/concurrent hosts, MCP-03
client-specific profiles, MCP-04 tool annotations, MCP-05 approval pending
responses, MCP-07 resources/subscriptions, MCP-08 untrusted content, and
SEC-03 local IPC access control. They also update the external portion of
`docs/contracts/mcp-server.md` and the user-facing launch guide. This document
does not change the existing Windows authentication format or the Unix Core
relay design.

The beta.23 implementation covers the originally planned M7-01 event feed, M7-03
pending/ack, M7-07 MCP notifications and M7-11 retention, with review against
MCP-01/M6-01 and MCP-03/M6-02. No new card or requirement is invented.

Reviewers should still assess the fixed-profile limitation, exact `config` and
`doctor` output fields, and future per-host identity choices. This document is
not evidence of real-account provider operations, external desktop host
interoperability, or completed human security review.
