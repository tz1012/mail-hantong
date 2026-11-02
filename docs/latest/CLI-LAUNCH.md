# mail-hantong CLI — Windows beta.3

The command is `hantong`. English is the default; use `--lang ko` for Korean. The companion `mail-hantong-core` process handles encrypted local storage, mail operations and policy. The graphical mail application is not included in the CLI package.

**Published release: [0.1.0-beta.3](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.3).** The [Korean website guide](https://tz1012.github.io/mail-hantong/) covers installation, mail and AI feature switching. Release downloads include checksums and build metadata. Full checks passed 1,833 tests; manual Gmail verification covered account retention, terminal cancellation, one synthetic send/receive and matching body/attachment. Human security review, clean Windows VM testing, other real mail providers and Google verification remain separate open gates. Real AI login and paid inference were not tested. macOS mail operations are not available yet.

## What works in this preview

The current beta includes guided installation/startup, personal feature selection followed by Google consent, bounded Gmail text previews, and an explicit same-account read-to-send feature action. Publisher configuration is included, so ordinary users do not configure Google Cloud. Compose, complete message review, Send/Cancel, client registration and advanced settings use the same terminal. Google OAuth opens its standard provider browser. See [Google setup and commands](GOOGLE-MAIL.md) and [verification status](PROGRESS.md).

| Available | Not yet available |
| --- | --- |
| Windows 11 x64; English/Korean text and JSON output | macOS/Linux product mail operations |
| Protected IMAP and multi-account Google browser setup; explicit client registration | Graph OAuth setup and Google publisher verification |
| Account and cached folder listing; explicit folder refresh | Automatic background synchronization of every mailbox |
| Bounded UID metadata collection, reconciliation and recovery | General attachment download and rich HTML display |
| Local metadata lookup, bounded text body reading and immutable drafts | Scheduled sending, contacts, tag editing and rules |
| SMTP setup and individually human-approved sending | Unattended sending or automatic resend |
| Managed Codex / Claude Console AI sessions and MCP mail tools | Claude subscription OAuth in third-party products |
| Core status, diagnostics and stable error/exit codes | Complete background mailbox synchronization |

IMAP/SMTP and advanced Google setup use hidden credential input in the same console. Passwords are never accepted through command arguments or JSON. Core validates the complete pending request before applying the selected operation. Interactive steps require terminal input and output; `--no-input`, redirected output and JSON do not confirm an operation. Terminal confirmation expresses intent and does not provide Windows Hello human-presence verification.

To review an agent-created pending request, use `hantong review --kind <kind> --request-id <id>`. Kinds include `registration`, `scope_refresh`, `google_setup`, `imap_setup`, `smtp_setup`, `google_mutation`, `personal_send` and `protected_send`. The terminal shows Core's complete stored request before offering Confirm/Cancel.

## Install with one command

Run in PowerShell on Windows x64:

```powershell
irm https://raw.githubusercontent.com/tz1012/mail-hantong/main/install.ps1 | iex
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

Inside an interactive AI session:

| Command | Behavior |
| --- | --- |
| `/provider` or `/provide` | Choose Codex/OpenAI or Claude with ↑↓, Enter; Esc cancels |
| `/model` | Choose from the provider's current model catalog |
| `/mode` | Choose supported light, med, high or ultra-high effort |
| `/fast on`, `/fast off` | Request the provider's actual supported fast service setting |
| `/agent on`, `/agent off` | Enable or disconnect the separately authorized mail tools |
| `/agent`, `/fast`, `/status` | Show current state |
| `/cli`, `/quit` | Return to the command line |

Typed arguments such as `/provider claude`, `/model MODEL_ID`, `/mode high` also work. Unsupported model settings return an error and preserve the previous state. Redirected input, JSON and `--no-input` do not open selection menus. Fast mode is not a promise of latency or lower cost.

Freeform chat initially has no mail access or automatic mail context. To let an agent discover and use authorized mail tools, register the independent stdio server:

```powershell
hantong-mcp register
hantong review --kind registration --request-id REQUEST_ID
hantong-mcp status REQUEST_ID
```

Replace REQUEST_ID with the ID printed by `register`. Registration prepares a pending request; the explicit `hantong review` step completes it in the same terminal. Review the accounts and tools before connecting them to external AI: authorized tool results may disclose mail data to the selected provider. Then `/agent on` may connect the packaged server. Other MCP hosts may launch `hantong-mcp` without arguments over stdio. Tool discovery returns current grants; accounts are discovered through the authorized account-list tool. Send requests require separate full-message terminal review; an AI session cannot itself confirm them. Generic shell, filesystem and web tools are disabled in the integrated AI session. Revoked or missing account consent blocks mail disclosure.

## Update and remove

Close both CLI and core processes before installing another preview. Run the new extracted package's installer; failed validation must leave the previous version usable. Do not downgrade an encrypted database after a newer core has migrated it.

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

현재 beta.3는 Windows x64의 IMAP·SMTP 설정, 등록, 폴더·메타데이터·본문 조회, 초안·첨부 작성, 터미널 전체 검토 후 발송, AI 대화와 MCP 연동을 제공합니다. 새 `clients refresh`에서 필요한 권한을 검토합니다. 업데이트만으로 권한을 부여하지 않으며 과거 중단 요청의 범위도 바꾸지 않습니다. 일반 첨부 다운로드·예약·주소록·규칙 자동화와 Mac 제품은 후속입니다.

위 PowerShell 한 줄로 설치·업데이트한 뒤 `hantong --lang ko init`을 실행하세요. 코어는 필요할 때 자동으로 시작합니다. 계정 연결·기능 선택·숨김 입력·메일 전체 검토·보내기/취소는 같은 터미널에서 진행하며, Google 로그인·동의만 브라우저를 사용합니다. 계정 작업 메뉴 4번은 작성·보내기 활성화, 5번은 메일 작성입니다. AI는 별도 `hantong ai login codex`, `hantong ai chat codex` 명령으로 시작하고 `/provider`, `/model`, `/mode`, `/fast`, `/agent`로 지원 기능을 전환합니다. 자세한 순서는 [홈페이지](https://tz1012.github.io/mail-hantong/)에 있습니다.

실제 Gmail 합성 송수신·본문/첨부 일치와 설치·업데이트는 확인했습니다. 서명·새 Windows 설치 환경·다른 실제 메일 제공자·실제 AI 로그인/유료 추론 검증과 사람 보안 검토는 남아 있습니다. 진행 상황은 `docs/latest/PROGRESS.md`에서 관리합니다.

## Simple personal Gmail sending

In the personal Gmail work menu, choose Compose and enter the recipients, subject,
body and optional attachment. Review the complete message in the same terminal,
then choose Send or Cancel. This personal path opens no native HanTong window
and requests no Windows Hello. Headless/JSON sending cannot confirm a message.
Other registered profiles and protected settings retain their existing identity
checks. Cancelling does not send; unknown is never automatically retried.
