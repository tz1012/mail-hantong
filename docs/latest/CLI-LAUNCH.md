# mail-hantong CLI — beta.22

beta.22 keeps the Windows PowerShell update path, newest-compatible release
selection and automatic downgrade protection. It also ships native macOS 15
(Intel/ARM) and Linux x86_64 (glibc >= 2.39) packages. New accounts use the same read,
body, compose and individually confirmed send flow as existing accounts. Google
consent is followed by exact scope selection in the same terminal. Generic
IMAP/SMTP accounts use the same CLI/TUI flow. The sending-account selection is
bounded at 128 accounts.

The beta also creates absent storage directories with explicit user permissions and
uses minimal ancestor traversal access. Existing storage ACLs, accounts and keys
are preserved. GitHub API rate limiting falls back to the public release feed.
An affected PC recovered access through a separately approved targeted repair;
this update does not automatically repair existing folder permissions.
Beta.21 also includes the Windows package-path fix for external publisher configuration under 8.3 aliases and long paths. Existing Windows v1 accounts, encrypted databases and keys remain intact.
If startup still fails after updating, report the fixed `hantong_startup stage`
and stable error code. Do not send credentials, database files or mail content.

The command is `hantong`. English is the default; use `--lang ko` for Korean. The companion `mail-hantong-core` process handles encrypted local storage, mail operations and policy. The graphical mail application is not included in the CLI package.

**Release: 0.1.0-beta.22.** Release assets are Windows x64, Linux x64, macOS Intel and macOS ARM; there is no Linux ARM asset. The source is pinned to `c50c01dd88fe75e2b643e580fb25ba85430639d6a4e9e55650cf7595c86332fd` from `feature/linux-product-20261104` at `3eabc14`. See [Unix installation and use](UNIX-INSTALL.md) for the pinned installer and headless flow. Human security review remains pending; macOS signing/notarization and real Unix provider login/send/Trash or AI conversation are not claimed.

## What works in this preview

The current beta includes guided installation/startup, Google account selection and read/write/send consent followed by explicit terminal account-scope confirmation, bounded Gmail text previews, and an explicit same-account read-to-send feature action. Publisher configuration is included, so ordinary users do not configure Google Cloud. Compose, complete message review, Send/Cancel, client registration and advanced settings use the same terminal. Google OAuth opens its standard provider browser. See [Google setup and commands](GOOGLE-MAIL.md) and [verification status](PROGRESS.md).

| Available | Not yet available |
| --- | --- |
| Windows 11 x64, macOS 15 Intel/ARM, Linux x86_64 glibc >= 2.39; English/Korean text and JSON output | Linux ARM; macOS SSH vault; advanced custom Google client on Unix |
| Protected IMAP and multi-account Google browser setup; explicit client registration | Graph OAuth setup and Google publisher verification |
| Account and cached folder listing; explicit folder refresh | Automatic background synchronization of every mailbox |
| Bounded UID metadata collection, reconciliation and recovery | General attachment download and rich HTML display |
| Local metadata lookup, bounded text body reading and immutable drafts | Scheduled sending, contacts, tag editing and rules |
| SMTP setup and individually confirmed sending | Unattended sending or automatic resend |
| Managed AI protocol/tool integration and the published v2 MCP mail tools (provider execution environment required) | v3 external Trash/events/config/doctor source candidate; Codex on hosts without MXC; Claude subscription OAuth in embedded/programmatic integrations |
| Core status, diagnostics and stable error/exit codes | Complete background mailbox synchronization |

IMAP/SMTP and advanced Google setup use hidden credential input in the same console. Passwords are never accepted through command arguments or JSON. Core validates the complete pending request before applying the selected operation. Interactive steps require terminal input and output; `--no-input`, redirected output and JSON do not confirm an operation. Terminal confirmation expresses intent and does not provide Windows Hello human-presence verification.

### Generic IMAP/SMTP and Trash

Beta.22 carries ordinary IMAP/SMTP onboarding and a provider-neutral one-message
Trash flow. The real Daum authentication, synthetic Gmail send, and 17-mailbox
catalog/body-read results are historical beta.20 evidence; beta.22 adds no new
real-account or real-mail test claim. Real Trash movement was not performed.

Use the normal onboarding menus rather than assembling request IDs. At the first
menu, enter `3` to connect an IMAP/SMTP account. When accounts already exist,
enter `imap` at the account-selection prompt. After the connection, work-menu
option `6` starts generic IMAP/SMTP setup. The flow collects the IMAP server,
port, TLS mode and login, then the SMTP server, port, TLS mode, login and an
optional distinct sender address. Credentials are collected only through the
Core-owned protected input in the same terminal.

The `hantong-t` view keeps mailbox actions in the terminal:

| Key | Action |
| --- | --- |
| `j` / `k` | Move the selection |
| Enter | Open the selected account, folder or message |
| `c` | Compose a message |
| `t` | Request a one-message move to Trash |
| `h` | Go back |
| `r` | Refresh |
| `q` | Quit |

`hantong` and `hantong-t` are separate terminal clients over the same Core,
account store, policy checks and provider operations. `hantong` remains the
guided command-style CLI for setup and explicit workflows. `hantong-t` is the
full-screen communications-style terminal client: it keeps accounts, folders,
messages and the body in fixed panes and supports keyboard-first navigation
with mouse selection. Press `u` to combine cached Inbox pages from every ready
account into one list with account tags. Press `v` to expand the focused body.
The package shares the build and runtime safety boundary; the presentation and
interaction loop are separate.

Start the full-screen client with:

```powershell
hantong-t [--lang en|ko] [--account ACCOUNT_ID] [--theme blue|green|amber|mono|bbs8|bbs16] [--demo] [--plain]
```

Blue is the default communications-style theme. `F` cycles blue, green, amber,
mono, bbs8 and bbs16 for the current run; the choice is not persisted yet.
The bbs8 and bbs16 palettes provide the ANSI eight-color and sixteen-color
communications-terminal styles. Arrow keys and
`j`/`k` move, Enter opens, Tab changes pane, Esc goes back, PageUp/PageDown
scroll the body, and `?` opens help. A mouse click selects, a double-click
opens, and the wheel scrolls. `c` enters compose and `t` starts the one-message
Trash review. `:` restores the cooked console, accepts an AI or login command,
and returns to the full-screen client afterward. `--demo` loads only synthetic
data and disables account effects. `--plain` selects the compatibility text
view. Account aliases can be changed from the terminal view and persist as
display-only labels; they do not alter From/authentication, account scope or
provider data. Setup, compose, AI/login and Trash review continue through the
same Core and terminal confirmation path.

On wide windows, the body receives about 60% of the remaining width after the
navigation area. On a narrow window, the list and body stack. Startup shows a
short preparation animation while the existing startup work runs, then returns
to the normal accounts/version/update notice prompt. The update notice is
informational; startup never installs an update automatically.

Setup, compose and Trash requests use the Core-bound preview and terminal
review before the final choice. IMAP Trash is available only when the server
advertises `MOVE` and the selected catalog contains exactly one selectable
mailbox with the `\Trash` special-use role. The client never turns a failed
move into permanent deletion and never falls back to `COPY`, `STORE`, or
`EXPUNGE`. SMTP compose currently does not accept attachments. The Google
sending selection remains bounded at 128 selected accounts; it is not unlimited.

For Daum, the official FAQ lists `imap.daum.net` with SSL port `993` and
`smtp.daum.net` with SSL port `465`, with `imap.hanmail.net` and
`smtp.hanmail.net` also available. Daum Smart Work uses the registered Smart
Work address as the mail address/From value while the login name remains the
Daum ID. Follow Daum's current password or app-password instructions and enter
the credential only in protected input. See [Daum IMAP/SMTP settings](https://cs.daum.net/faq/service/43/category/9234/detail/17988)
and [Daum Smart Work Outlook settings](https://cs.daum.net/faq/service/43/category/13114/detail/24727).

To review an agent-created pending request, use `hantong review --kind <kind> --request-id <id>`. Kinds include `registration`, `scope_refresh`, `google_setup`, `imap_setup`, `smtp_setup`, `google_mutation`, `personal_send` and `protected_send`. The terminal shows Core's complete stored request before offering Confirm/Cancel.

## Install with one command

For macOS and Linux, download the pinned installer to a file and run it from a
protected TTY. See [UNIX-INSTALL.md](UNIX-INSTALL.md); it covers the encrypted
headless vault, Keychain requirements and SSH/browser boundaries.

Run in PowerShell on Windows x64:

```powershell
irm https://raw.githubusercontent.com/tz1012/mail-hantong/3eabc14b9ed8463aa58db0304dbf86d6da00bb94/install.ps1 | iex
```

The installer selects the latest compatible published preview, verifies its ZIP against `SHA256SUMS`, and installs under the current user's local Programs directory. It adds the owned launcher directory to the user's PATH and the current terminal. Repeating the command updates the installation. It does not start Core, register accounts or log in to an AI provider. No Rust, Git, Python, Docker or WSL is required for the mail executables. The optional AI connections use separately installed official Codex or Claude Code executables.

The installation does not change execution policy. Enterprise application restrictions remain in force. The preview is unsigned; published SHA-256 checksums verify file bytes and are not a publisher signature.

## Install from a preview ZIP

Download only the release ZIP and its `SHA256SUMS` from the project's published release when one exists. Compare the ZIP's `Get-FileHash -Algorithm SHA256` result with the published checksum before extracting. A matching checksum checks bytes; it is not a publisher signature. This local preview is unsigned. Do not change machine-wide execution policy or disable SmartScreen to install it.

After extracting, open PowerShell in the extracted directory:

```powershell
.\install.ps1
```

The local ZIP installer is portable by default: follow its output to set the current terminal's PATH, or pass `-AddToPath` to save the owned directory in your user PATH. The one-command installer above enables this automatically. No machine PATH is changed.

The preview installs for the current user. It does not start the core, connect accounts, create an auto-start entry or change the mail application's data directory. Windows executables use the static MSVC runtime; system Windows DLLs remain required.

```powershell
hantong --version
hantong --help
hantong --lang ko --help
mail-hantong-core --help
```

## Start and connect

### Current beta: guided start

Install the published beta, then run:

```powershell
hantong init
# Korean:
hantong --lang ko init
```

An interactive bare `hantong` opens the same flow. It connects to an existing Core or starts the verified sibling Core in the background, offers an existing Gmail account or Google connection, and keeps HanTong interaction in the same terminal. Only provider OAuth uses its browser. Opening the inbox is a separate menu choice; connecting an account does not automatically read mail. Select a numbered message to read its text body. Menu option 4 enables composing/sending with any necessary new Google consent; option 5 composes a message. Enabling the feature does not send mail. Exit with option 3; Core continues running. This is a guided CLI, not a full-screen TUI.

The Google flow reuses protected app configuration already stored on this PC or the publisher Desktop configuration included in beta.3. Users do not create a Google Cloud project or enter an app credential. Human security review and Google verification remain separate. A custom developer package without configuration needs advanced setup. `init --json`, `init --no-input` and redirected input/output stop before starting Core or opening account setup.

Current builds keep the same Windows data location when launched from a packaged terminal host or a normal terminal. Existing data created under a host application's private AppData is preserved and needs a separate recovery step; do not delete it or repeatedly reconnect accounts to address an authentication error.

### Advanced commands and optional foreground Core

Ordinary users can use the guided start above. For manual diagnostics, Core can instead run in a separate foreground terminal; keep it open, and use Ctrl+C to stop it.

```powershell
mail-hantong-core run
```

In terminal 2:

```powershell
hantong core status --json
hantong setup imap request --host imap.example.com --port 993 --tls implicit_tls --login user@example.com --name Personal
hantong setup imap status REQUEST_ID --json
```

Replace the example host/login with your server information. Review the endpoint in the same terminal and enter the password only through hidden console input. A successful setup is not client authorization. After setup reports ready:

```powershell
hantong clients register
hantong clients status REQUEST_ID --json
hantong clients verify
hantong accounts list --json
hantong clients refresh
hantong clients refresh-status REFRESH_ID --json
```

Use the IDs returned by the preceding commands. Review the account and tool scopes in the same terminal. Registering when no account exists grants no implicit access to future accounts. `clients refresh` requires explicit terminal review; requesting it does not itself grant the scope.

A fresh general refresh requests body/draft/send scopes as well as display language and diagnostics. Older interrupted requests retain their recorded permissions and decision channel; they are not silently converted into new terminal grants. A conflicting legacy request must be resolved explicitly. Installing an update does not grant permissions. Every send requires a separate complete-message terminal review and Send/Cancel choice.

## Browse stored metadata

Use the returned account ID. If `folders list` returns `NOT_FOUND` because no local catalog exists, the first refresh uses `--expected-generation 0`. Otherwise, use the current catalog generation from the list response. A refresh returns its execution ID; query its status before listing folders again:

```powershell
hantong folders list --account ACCOUNT --json
hantong folders refresh --account ACCOUNT --expected-generation GENERATION --operation-id folder_1 --idempotency-key folder_1
hantong folders refresh-status --account ACCOUNT --execution-id EXECUTION_HEX --json
hantong folders list --account ACCOUNT --json
hantong sync cursor --account ACCOUNT --wire-hex HEX --encoding ENCODING --json
```

Replace `HEX` and `ENCODING` with the selected folder's exact `wire_hex` and `encoding` values. Encoding can be `modified_utf7` or `utf8`; do not assume UTF-8. To collect metadata for a new mailbox cursor, use the returned catalog/cursor versions:

```powershell
hantong sync page --account ACCOUNT --wire-hex HEX --encoding ENCODING --expected-catalog-generation GENERATION --expected-cursor-version VERSION --operation-id page_1 --idempotency-key page_1
hantong sync status --account ACCOUNT --execution-id EXECUTION_HEX --json
hantong search --account ACCOUNT --wire-hex HEX --encoding ENCODING --limit 16
hantong search --account ACCOUNT --wire-hex HEX --encoding ENCODING --filter unread --json
hantong search --account ACCOUNT --wire-hex HEX --encoding ENCODING --filter favorite --lang ko
```

These are bounded, explicit operations. A successful page is not a complete mailbox sync. Results come from the local partial cache and can be stale. A returned next cursor must retain the same account, mailbox, filter and limit; it may expire. The recent-mail `sync initial-*` commands are documented in the repository README. Raw encoded subjects are escaped safely in CLI output; full MIME decoding is not implemented here.

After the protected refresh grants app-read permissions, request a bounded diagnostic snapshot:

```powershell
hantong diagnostics snapshot --sections sync,recent_work,errors --hours 24 --limit 100 --json
```

This summarizes stored work for your registered client within its current account scope. It does not query mail servers or expose mail content, account identifiers or raw logs. Stored job states do not prove that a worker is currently running or that a mailbox is fully synchronized.

For scripting, use `--json` and inspect the process exit code (`$LASTEXITCODE` in PowerShell). Common codes: `2` invalid input, `11` scope denied, `22` client not registered, `24` human step required, `25` core unavailable, `27` cursor expired, `28` unsupported capability. Do not retry mutations merely because a command failed.

## Read, compose and send

After metadata search returns an internal `message_id`, request its body:

```powershell
hantong read --account ACCOUNT --message-id MESSAGE_ID --json
```

The first request may report pending while Core fetches a bounded body with IMAP `EXAMINE` and `BODY.PEEK[]`. Repeat the same read to inspect the encrypted cache. It does not mark mail read. The output is sanitized plain text, limited to 64KiB; supported MIME text parts are decoded, HTML is reduced to text, remote images/scripts and attachments are not opened. Truncation and unsupported encodings remain explicit.

Set up SMTP for an existing account. Enter its app password only in the same console's hidden credential input:

```powershell
hantong setup smtp request --account ACCOUNT --host smtp.example.com --port 587 --tls starttls_required --login user@example.com
hantong setup smtp status REQUEST_ID --json
hantong draft create --account ACCOUNT --to recipient@example.com --subject "Hello" --body-file .\body.txt --operation-id draft_1 --idempotency-key draft_1 --json
hantong send --account ACCOUNT --draft-id DRAFT_HEX --expected-version 1 --payload-hash HASH_HEX --operation-id send_1 --idempotency-key send_1
hantong send status --account ACCOUNT --review-id REVIEW_HEX --json
```

The body file must be UTF-8 and at most 1MiB. Use IDs and hash returned by the preceding response. SMTP setup currently initializes an absent endpoint; it does not silently replace one or send a probe email. Interactive `send` shows the complete stored message in the same terminal and asks Send/Cancel. No separate HanTong window or Windows Hello is required. The selection expresses intent, not OS human-presence proof. Sending requires terminal input/output; JSON and `--no-input` cannot confirm a message. `unknown` / `SEND_UNKNOWN` means delivery cannot be determined and is never automatically retried. Repeating an operation/idempotency key returns its existing result.

## AI sessions and agent tools

Install the official [Codex CLI](https://developers.openai.com/codex/cli) or [Claude Code](https://code.claude.com/docs/en/setup), then use:

```powershell
hantong ai providers --json
hantong ai login codex
hantong ai status codex
hantong ai chat codex
# Claude Console / API billing connection:
hantong ai login claude
hantong ai chat claude
```

Provider tools manage browser login and credentials. mail-hantong does not read their token files. Claude connects through Console/API billing; third-party use of a Claude subscription login is not supported. Provider authentication and paid inference were not exercised in development tests.

The beta21 candidate does not claim that `hantong ai chat codex` is usable on
the current Windows provider environment. Codex 0.163.0 named profiles can
reject the required effective root-read/temp-read capability even when an
elevated backend is already configured. HanTong returns localized
`CAPABILITY_UNSUPPORTED`, preserves the existing registration, checks the
exact active permission profile, and does not edit UAC/configuration or fall
back to a broader legacy profile. The on-request enum and namespace tool
protocol are implemented, but no real model turn or AI mail exchange was
performed. A read-only status check was not treated as model-turn evidence.

Inside an interactive AI session:

| Command | Behavior |
| --- | --- |
| `/provider` or `/provide` | Choose Codex/OpenAI or Claude with ↑↓, Enter; Esc cancels |
| `/model` | Choose from the provider's current model catalog |
| `/mode` | Choose supported light, med, high or ultra-high effort |
| `/fast on`, `/fast off` | Request the provider's actual supported fast service setting |
| `/help` | List available session commands with descriptions |
| `/agent on`, `/agent off` | Reconnect or disconnect the current provider's configured mail tools |
| `/agent`, `/fast`, `/status` | Show current state |
| `/cli`, `/quit` | Return to the command line |

Typed arguments such as `/provider claude`, `/model MODEL_ID`, `/mode high` also work. Unsupported model settings return an error and preserve the previous state. Redirected input, JSON and `--no-input` do not open selection menus. Fast mode is not a promise of latency or lower cost.

Built-in AI chat automatically connects mail tools after its first provider-specific setup when the provider execution environment is available. The same terminal shows all ready accounts, permitted tools and external mail-data transfer before you choose Connect or Cancel. Codex and Claude use separate registrations and consent. No manual registration command, request-ID copying or required `/agent on` is needed for this built-in flow. Startup does not upload the inbox; scoped tool results can be sent to the selected AI provider when used. `/agent off` disconnects tools and clears session context. Existing registrations are not silently expanded when new accounts are added.

For an independent external MCP host, the published beta.22 registration remains
the v2 workflow:

The v3 source candidate adds `config`, `doctor`, a reviewed Trash request and
metadata-only receive events, but these are not included in the published
beta.22 binaries. See the [external MCP connection guide](EXTERNAL-MCP-GUIDE.md)
only when using that matching source candidate.

```powershell
hantong-mcp register
hantong review --kind registration --request-id REQUEST_ID
hantong-mcp status REQUEST_ID
```

Replace REQUEST_ID with the ID printed by `register`. Registration prepares a pending request; the explicit `hantong review` step completes it in the same terminal. External MCP hosts launch `hantong-mcp` without arguments over stdio and use the fixed external MCP profile; per-host identities are not yet available. The built-in AI setup does not expand these grants. Tool discovery returns current v2 grants; accounts are discovered through the authorized account-list tool. Send requests require separate terminal review; an AI cannot confirm them itself. Generic shell, filesystem and web tools are disabled in the integrated AI session. Revoked or missing account consent blocks mail disclosure. The v3 source candidate adds the separate resource and Trash flow described above, but beta.22 users do not have it.

Built-in AI also exposes `mail.messages.trash.request` for moving one Gmail message to Trash. It requires an existing Google Modify grant and a final same-terminal preview and confirmation. Missing permissions are not granted automatically. Permanent deletion, batch deletion and deletion through other mail providers are not supported. An uncertain send or trash outcome is never automatically retried. JSON, redirected input/output and `--no-input` cannot perform the interactive confirmation.

## Update and remove

If an older `update hantong` fails with `Join-Path` / null `drive`, run the
public installation command directly once to replace the old updater. This
preserves account data and keys. See [Windows recovery and program-only clean
reinstallation](WINDOWS-RECOVERY.md) for both procedures. For macOS and Linux
installation, update and headless secret handling, see [UNIX-INSTALL.md](UNIX-INSTALL.md).

From beta.5 onward, update in an interactive terminal with either command:

```powershell
update hantong
# Equivalent:
hantong update
```

Install beta.5 once using the installation command above if your older version
does not yet provide these commands. The updater checks the current installation,
downloads the latest release, verifies its checksums, stages its files and then
activates it. The terminal shows phase progress and percentages during download,
verification, extraction and installation. Percentages describe progress through
the installation steps rather than a remaining-time estimate. Existing account
data and keys are preserved. A failed validation leaves the previous version
usable. The installer stops only a verified owned Core when activation requires
it; there is no need to manually stop the service first. Do not downgrade an
encrypted database after a newer core has migrated it. JSON, redirected mode and
`--no-input` cannot start an update.

Use the installed uninstall script as described by the installer output. Removal preserves mail data, encryption keys and registration state; it does not promise to erase accounts or revoke credentials. The data is under the Windows user's local application-data `mail-hantong` directory and is separate from the CLI program directory.

## Build the package from source

Packaging requires Python 3.11 or newer, the pinned Rust toolchain, MSVC build tools and the Windows SDK. These are developer requirements only.

```powershell
cargo fetch --locked
python scripts/prepare_native_tools.py
python scripts/check_all.py
python scripts/package_cli.py --version 0.1.0-preview.3
```

The package tool builds the terminal distribution and core, records versions and source metadata, includes license notices and produces a ZIP plus checksums under `artifacts/cli/`. It does not publish, sign or install the package. Dependency inventory is build evidence, not a completed legal review or a claimed standards-compliant SBOM.

## 한국어 안내

`hantong`은 터미널에서 사용하는 CLI입니다. `--lang ko`로 한국어를 선택하고 자동화에는 `--json`과 종료 코드를 사용합니다.

현재 beta.22 후보는 `/help`, 내장 AI 메일 도구 자동 연결, Gmail과 일반 IMAP 계정의 메일 조회·본문 읽기·작성·개별 발송 확인, 한 통씩 휴지통 이동을 제공합니다. Unix 실제 제공자 로그인·발송·휴지통과 AI 대화는 아직 시험하지 않았습니다. 일반 첨부 다운로드·예약·주소록·규칙 자동화는 후속입니다.

위 PowerShell 한 줄로 설치·업데이트한 뒤 `hantong --lang ko init`을 실행하세요. 코어는 필요할 때 자동으로 시작합니다. 계정 연결·기능 선택·숨김 입력·메일 전체 검토·보내기/취소는 같은 터미널에서 진행하며, Google 로그인·동의만 브라우저를 사용합니다. 계정 작업 메뉴 4번은 작성·보내기 활성화, 5번은 메일 작성입니다. AI는 별도 `hantong ai login codex`, `hantong ai chat codex` 명령으로 시작하고 `/provider`, `/model`, `/mode`, `/fast`, `/agent`로 지원 기능을 전환합니다. 현재 beta17 후보는 Windows Codex 0.163.0 named profile의 유효 root-read/temp-read 기능 거부를 감지하면 기존 등록을 보존한 채 현지화된 `CAPABILITY_UNSUPPORTED`로 종료합니다. UAC·설정 변경이나 넓은 레거시 프로필 대체는 하지 않으며, 실제 모델 turn·AI 메일 교환은 수행하지 않았습니다. 자세한 순서는 [홈페이지](https://tz1012.github.io/mail-hantong/)에 있습니다.

실제 Gmail 합성 송수신·본문/첨부 일치와 설치·업데이트를 확인했고, 실제 Daum IMAP·SMTP 인증과 합성 메일 발송 상태도 확인했습니다. Daum IMAP 카탈로그에서 사서함 17개를 조회했고, 최근 2페이지와 반복 새로고침을 두 번 모두 완료했으며 본문 읽기도 두 실행에서 검증했습니다. 새 Windows 설치 환경·실제 AI 로그인/유료 추론 검증과 사람 보안 검토는 남아 있습니다. 실제 휴지통 이동은 수행하지 않았습니다. 진행 상황은 `docs/latest/PROGRESS.md`에서 관리합니다.

일반 IMAP/SMTP 계정은 첫 메뉴에서 `3`, 기존 계정 메뉴에서는 `imap`, 연결
후 작업 메뉴에서는 `6`을 선택합니다. SMTP 로그인 주소와 별도의 발신 주소를
입력할 수 있으며, 비밀번호는 같은 터미널의 보호 입력에서 받습니다.
`hantong`과 `hantong-t`는 같은 Core·저장소·정책·공급자 작업을 공유하는
별도 터미널 클라이언트입니다. `hantong`은 설정과 명시적 작업을 안내하는
명령형 CLI이고, `hantong-t`는 계정·폴더·목록·본문을 고정 영역에 표시하는
모뎀 통신풍 전체 화면 터미널입니다. `hantong-t`에서는 방향키와 `j`/`k`로
이동하고 Enter로 열며 Tab으로 영역을 바꾸고 Esc로 돌아갑니다. 마우스
클릭은 선택, 더블클릭은 열기, 휠은 스크롤입니다. `c`로 작성하고 `t`로
한 통을 휴지통으로 이동합니다. `--theme blue|green|amber|mono|bbs8|bbs16`로
시작 테마를 고르고 `F`로 실행 중 전환할 수 있으며, 아직 저장하지 않습니다.
`bbs8`과 `bbs16`은 ANSI 8색·16색 통신 터미널 팔레트입니다. `u`는 준비된
모든 계정의 캐시 받은편지함을 계정 태그와 함께 합쳐 표시하고, `v`는
현재 본문을 넓혀 표시합니다. 계정 별칭은 표시용으로만 저장되며 발신자·
인증·계정 범위·공급자 데이터는 바꾸지 않습니다.
`--demo`는 합성 데이터만 사용하고 계정 작업을 실행하지 않습니다.
`--plain`은 호환용 텍스트 화면입니다. 서버가 `MOVE`와 정확히 하나의
선택 가능한 `\Trash` 사서함을 제공해야 하며, 영구 삭제로 대체하지 않습니다.
넓은 창에서는 남은 공간의 약 60%를 본문에 배정하고, 좁은 창에서는 목록과
본문을 쌓습니다. 시작 준비 중에는 짧은 캐릭터 애니메이션을 표시한 뒤
계정·버전·업데이트 알림 안내로 돌아옵니다. 업데이트 알림은 안내일 뿐이며
시작 시 자동 설치하지 않습니다.
SMTP 작성의 첨부는 현재 지원하지 않습니다. Google 발송 계정 선택은 최대
128개로 제한됩니다. Daum의 IMAP/SMTP 주소와 Smart Work 로그인 규칙은 [공식 IMAP/SMTP
안내](https://cs.daum.net/faq/service/43/category/9234/detail/17988)와
[공식 Smart Work 안내](https://cs.daum.net/faq/service/43/category/13114/detail/24727)를
확인하세요.

## Simple personal Gmail sending

In the personal Gmail work menu, choose Compose and enter the recipients, subject,
body and optional attachment. Review the complete message in the same terminal,
then choose Send or Cancel. This personal path opens no native HanTong window
and requests no Windows Hello. Headless/JSON sending cannot confirm a message.
Other registered profiles and protected settings retain their existing identity
checks. Cancelling does not send; unknown is never automatically retried.


### Windows provider connection behavior (beta.21; beta.20 evidence)

Codex selects MXC for the current thread only, with root-deny/minimal-runtime-read
and exact temporary cwd read; no global configuration or UAC change is made.
The host must support MXC. A synthetic answer and a denied sibling canary were
verified on this host. No actual mail content was used in that inference.

Claude subscription/OAuth login uses the official interactive Claude Code CLI
with explicit HanTong MCP settings in the same terminal. The native CLI owns
input/output and authentication; Ctrl+C twice returns to HanTong. Custom commands
are disabled in this restricted native session. API billing modes
retain embedded chat. Headless prompt/JSON requests never fall back to OAuth
interactive mode. Beta.20 evidence recorded two synthetic turns with follow-up context for both providers; beta.21 adds no new provider-turn claim. No mail content was requested; real AI-driven send/Trash remains
unverified.
