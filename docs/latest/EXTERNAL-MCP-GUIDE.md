# External MCP connection guide

This document describes the beta.23 release scope.
The published beta.22 binaries expose the existing v2 external MCP flow only;
they do not include `config`, `doctor`, v3 registration, external Trash, or
receive events. Use these instructions only with the matching beta.23 Core and
MCP executable from the matching release. The release page is the authority for assets.

This guide is for Claude Desktop, Claude Code, Codex-compatible hosts, and
other desktop applications that can start a local MCP server. HanTong uses the
standard MCP `stdio` transport: the host starts `hantong-mcp`, sends JSON-RPC
over stdin, and reads JSON-RPC over stdout. HanTong does not open a remote HTTP
listener in this flow.

이 문서는 Claude Desktop, Claude Code, Codex 호환 호스트처럼 로컬 MCP 서버를
실행할 수 있는 데스크톱 앱을 위한 안내입니다. 한통은 표준 MCP `stdio` 전송을
사용합니다. 호스트가 `hantong-mcp`를 실행하고 표준 입출력으로 JSON-RPC를
주고받습니다. 이 연결 방식에서는 원격 HTTP 서버를 열지 않습니다.

## 1. Check the installation

Start HanTong once so its Core is available:

```text
hantong
```

The connection helper is read-only:

```text
hantong-mcp doctor
```

It prints bounded JSON with the stdio transport, MCP protocol version, Core
state/error, external registration state/error, the exposed granted tool names,
and a suggested action. It never prints account IDs, mail content, tokens,
passwords, or profile keys. A failed check does not change the profile or start
a new account connection.

먼저 `hantong`을 실행해 Core를 준비합니다. `hantong-mcp doctor`는 설치된
클라이언트, Core 연결 상태, 외부 MCP 프로필 상태와 허용된 도구 수만 JSON으로
확인합니다. 계정 ID, 메일 내용, 토큰, 비밀번호, 프로필 키는 출력하지 않으며
프로필이나 계정을 변경하지 않습니다.

## 2. Generate host configuration

Ask HanTong for a reviewable local configuration:

```text
hantong-mcp config
```

The output is JSON in this shape (the exact absolute path depends on the
installation):

```json
{
  "mcpServers": {
    "hantong": {
  "command": "C:\\Users\\you\\AppData\\Local\\Programs\\mail-hantong-cli\\versions\\VERSION\\bin\\hantong-mcp.exe",
      "args": []
    }
  }
}
```

On macOS or Linux, the command is the current absolute `hantong-mcp` path for
that platform. Copy this object into the desktop host's MCP configuration and
restart that host. HanTong does not edit another application's configuration,
environment, credentials, or permission files. The command reports the current
executable path; the host must still be configured to trust and launch it.

`command` is absolute and normally includes the current installed version
directory. Run `hantong-mcp config` again after updating HanTong. Do not replace
it with a downloaded script, shell command, or a path inside a temporary
directory.

`hantong-mcp config`은 데스크톱 앱의 설정을 직접 수정하지 않고 현재 실행
중인 `hantong-mcp`의 절대 경로를 JSON으로 출력합니다. 해당 JSON을 호스트
설정에 검토 후 붙여 넣고, 한통을 업데이트한 뒤에는 명령을 다시
실행하십시오.

## 3. Register external mail access

The external host has its own v3 registration and consent. Built-in Codex or
Claude sessions do not expand this grant. The review must explicitly include
the accounts and tools to be used; receive events are an additional reviewed
`mail.events.read` capability.

```text
hantong-mcp register
hantong review --kind registration --request-id REQUEST_ID
hantong-mcp status REQUEST_ID
```

Replace `REQUEST_ID` with the ID returned by `register`. The first command only
creates a pending request. Review shows the exact accounts and tools before the
request can become active. `status` is read-only. If the request expires or is
cancelled, create a new request; do not reuse a stale ID.

After updating from beta.22, run the normal `hantong update` and then run
`hantong` again before creating a v3 request. The updater handles the owned Core
transition; do not submit the request while an older Core remains active.

외부 호스트 권한은 내장 Codex·Claude 권한과 별도로 관리됩니다. `register`는
대기 요청만 만들고, 같은 터미널의 `hantong review`에서 계정과 도구 범위를
확인한 뒤 활성화합니다. `status`는 조회만 수행합니다.

## 4. Tools available to an external host

After activation, `tools/list` exposes only the current grant. The external
mail set is intentionally bounded:

- `mail.accounts.list`
- `mail.folders.list`
- `mail.folders.refresh`
- `mail.sync.page`
- `mail.messages.search`
- `mail.messages.get`
- `mail.drafts.create`
- `mail.send.request`
- `mail.messages.trash.request` (single-message review request)

The host can read authorized metadata and bodies, create local drafts, and
request a send review. `mail.send.execute` is never exposed to the host. A send
request remains pending until the HanTong terminal review completes it.

The external MCP flow exposes a single-message Trash request as a review-only
operation. Permanent deletion, batch deletion, automatic sending, and automatic
retries remain unavailable. The host cannot confirm or execute a Trash move.

When the reviewed v3 grant includes `mail.events.read`, `resources/list` exposes
`mail-hantong://events`. A host may use `resources/read` for a bounded poll or
`resources/subscribe` to opt in to metadata-only update notifications. Core
checks provider changes about every 60 seconds; the stdio MCP process checks
the durable feed about every 5 seconds while subscribed. Events contain an
opaque cursor and mailbox change metadata; they never contain bodies, subjects,
or addresses. Notifications are coalesced hints rather than exact mail counts.
Revoking the external grant stops the feed. A notification does not wake the
host LLM or authorize a follow-up tool call.

For scripts and desktop hosts without MCP resource support, the same external
profile has an explicit local fallback:

```text
hantong-mcp notify on
hantong-mcp notify pending
hantong-mcp notify ack CURSOR
hantong-mcp notify off
hantong-mcp watch
```

`watch` emits one bounded event object per line and polls every five seconds.
It does not invoke a shell callback, wake an LLM, or send a message to another
service.

`tools/list`는 허용된 범위만 반환합니다. 목록에 없는 도구를 직접 호출할 수
없으며, 계정 목록이 비어 있다고 해서 모든 계정을 의미하지 않습니다. 메일
원문은 신뢰할 수 없는 데이터로 취급됩니다. 외부 AI로 전달될 수 있는 계정과
본문 범위는 등록 검토 전에 확인하십시오.

## 5. Troubleshooting

`CORE_UNAVAILABLE` means that the HanTong Core is not running or cannot be
reached. Start `hantong` and run `hantong-mcp doctor` again.

`SCOPE_DENIED`, `POLICY_BLOCKED`, or an empty tool list means that the external
registration has no matching active consent. Run the explicit registration
flow again and review the displayed account/tool scope.

If the host cannot start a local command, it probably supports only remote MCP
URLs. This beta does not provide a remote HTTP MCP endpoint. Use a host with
local stdio support or wait for a separately reviewed remote-host contract.

`config` and `doctor` never install software, edit host files, add accounts, or
grant permissions. They are safe to run again after an update.
