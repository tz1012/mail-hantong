# mail-hantong CLI — Windows developer preview

The command is `hantong`. English is the default; use `--lang ko` for Korean. The companion `mail-hantong-core` process handles encrypted local storage, mail operations and policy. The graphical mail application is not included in the CLI package.

**Release status: Windows developer preview.** Package label `0.1.0-preview.4` identifies a preview of binaries reporting `0.1.0`. [Release downloads](https://github.com/tz1012/mail-hantong/releases) include checksums. Security review, clean Windows installation testing and real-provider interoperability remain open. macOS mail operations are not available yet.

## What works in this preview

Preview.4 adds Google multi-account browser login and Gmail read-only mail. Preview.3 does not include these features. See [Google setup and commands](GOOGLE-MAIL.md) and [current verification status](https://github.com/tz1012/mail-hantong/blob/main/docs/latest/PROGRESS.md). First setup needs public Google Desktop OAuth client configuration; publisher verification remains a separate prerequisite.

| Available | Not yet available |
| --- | --- |
| Windows 11 x64; English/Korean text and JSON output | macOS/Linux product mail operations |
| Protected IMAP and multi-account Google browser setup; explicit client registration | Graph OAuth setup and Google publisher verification |
| Account and cached folder listing; explicit folder refresh | Automatic background synchronization of every mailbox |
| Bounded UID metadata collection, reconciliation and recovery | Attachments and rich HTML rendering |
| Local metadata lookup, bounded text body reading and immutable drafts | Scheduled sending, contacts, tag editing and rules |
| SMTP setup and individually human-approved sending | Unattended sending or automatic resend |
| Managed Codex / Claude Console AI sessions and MCP mail tools | Claude subscription OAuth in third-party products |
| Core status, diagnostics and stable error/exit codes | Complete background mailbox synchronization |

IMAP setup and registration use a small protected Windows window for password entry and human verification. Passwords and approval are not accepted through command arguments. A terminal with an interactive Windows desktop is required for these steps. `--no-input` does not bypass them.

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

In terminal 1, run the core in the foreground. Keep this terminal open; Ctrl+C stops it.

```powershell
mail-hantong-core run
```

In terminal 2:

```powershell
hantong core status --json
hantong setup imap request --host imap.example.com --port 993 --tls implicit_tls --login user@example.com --name Personal
hantong setup imap status REQUEST_ID --json
```

Replace the example host/login with your server information. Enter the password only in the protected setup window and complete human verification there. A successful setup is not client authorization. After setup reports ready:

```powershell
hantong clients register
hantong clients status REQUEST_ID --json
hantong clients verify
hantong accounts list --json
hantong clients refresh
hantong clients refresh-status REFRESH_ID --json
```

Use the IDs returned by the preceding commands. Review the account and tool scopes in the protected confirmation window. Registering when no account exists grants no implicit access to future accounts. `clients refresh` requests a new explicit scope review; it does not automatically approve it.

In preview.3, a fresh refresh explicitly requests body/draft/send scopes as well as display language and diagnostics. An older interrupted refresh keeps its original permissions; finish that request before starting a fresh refresh if these permissions are missing. Installing an update does not grant permissions. Every send still requires its own protected human confirmation.

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

Set up SMTP for an existing account. Enter its app password only in the protected window:

```powershell
hantong setup smtp request --account ACCOUNT --host smtp.example.com --port 587 --tls starttls_required --login user@example.com
hantong setup smtp status REQUEST_ID --json
hantong draft create --account ACCOUNT --to recipient@example.com --subject "Hello" --body-file .\body.txt --operation-id draft_1 --idempotency-key draft_1 --json
hantong send --account ACCOUNT --draft-id DRAFT_HEX --expected-version 1 --payload-hash HASH_HEX --operation-id send_1 --idempotency-key send_1 --json
hantong send status --account ACCOUNT --review-id REVIEW_HEX --json
```

The body file must be UTF-8 and at most 1MiB. Use IDs and hash returned by the preceding response. SMTP setup currently initializes an absent SMTP endpoint; it does not silently replace an existing one or send a probe email. `send` requests a protected native review of the complete message. Only successful human verification may start one SMTP attempt. `unknown` / `SEND_UNKNOWN` means delivery cannot be determined: it is never automatically retried. Repeating an operation/idempotency key returns its existing result.

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
hantong-mcp status REQUEST_ID
```

Review the exact accounts, tools and external-AI disclosure in the protected window. Then `/agent on` may connect the packaged server. Other MCP hosts may launch `hantong-mcp` with no arguments over stdio. Tool discovery returns current grants; accounts are discovered through the authorized account-list tool. Every send still requires its own native human review. Generic shell, filesystem and web tools are disabled in the integrated AI session. Revoked or missing account consent blocks mail disclosure.

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

현재 개발 미리보기는 Windows x64의 IMAP·SMTP 설정, 등록, 폴더·메타데이터·본문 조회, 초안 작성, 보호된 사람 확인을 거친 발송, AI 대화와 MCP 연동을 제공합니다. 새 `clients refresh`에서 필요한 권한을 검토합니다. 업데이트만으로 권한을 부여하지 않으며 과거 중단 요청의 범위도 바꾸지 않습니다. 첨부·예약·주소록·규칙 자동화와 Mac 제품은 후속입니다.

위 PowerShell 한 줄로 설치·업데이트할 수 있습니다. 한 터미널에서 `mail-hantong-core run`, 다른 터미널에서 `hantong` 명령을 실행합니다. 비밀번호 입력과 권한 승인은 보호된 Windows 확인 창에서 사람이 수행합니다. 메일 GUI 실행 파일은 필요하지 않습니다.

서명·새 Windows 설치·실제 제공자 검증과 사람 보안 검토가 남아 있습니다. 진행 상황은 저장소의 `docs/latest/PROGRESS.md`에서 관리합니다.
