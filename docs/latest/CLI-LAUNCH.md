# mail-hantong CLI — Windows developer preview

The command is `hantong`. This is a terminal command-line client, not a full-screen terminal mail interface (TUI). English is the default; use `--lang ko` for Korean. The companion `mail-hantong-core` process handles encrypted local storage, mail operations and policy. The graphical mail application is not included in the CLI package.

**Release status: local developer preview; no public release yet.** Package label `0.1.0-preview.2` identifies a preview of binaries reporting `0.1.0`. This preview is not the completed mail product. Security review, clean Windows installation testing and real-provider interoperability remain open. macOS mail operations are not available yet.

## What works in this preview

| Available | Not yet available |
| --- | --- |
| Windows 11 x64; English/Korean text and JSON output | macOS/Linux product mail operations |
| Protected IMAP setup and explicit client registration | Gmail/Graph OAuth setup and provider certification |
| Account and cached folder listing; explicit folder refresh | Automatic background synchronization of every mailbox |
| Bounded UID metadata collection, reconciliation and recovery | Message bodies, attachments, composing and sending |
| Local subject/sender metadata lookup; unread/favorite filters | Drafts, scheduled sending, contacts, tag editing and rules |
| Core status, bounded diagnostic counts and stable error/exit codes | Full-screen keyboard mail interface (TUI), unattended approval |

IMAP setup and registration use a small protected Windows window for password entry and human verification. Passwords and approval are not accepted through command arguments. A terminal with an interactive Windows desktop is required for these steps. `--no-input` does not bypass them.

## Install from a preview ZIP

Download only the release ZIP and its `SHA256SUMS` from the project's published release when one exists. Compare the ZIP's `Get-FileHash -Algorithm SHA256` result with the published checksum before extracting. A matching checksum checks bytes; it is not a publisher signature. This local preview is unsigned. Do not change machine-wide execution policy or disable SmartScreen to install it.

After extracting, open PowerShell in the extracted directory:

```powershell
.\install.ps1
```

Follow the installer output for the current terminal's PATH command. This preview changes no persistent user or system PATH settings. Each new terminal needs that command, or you can invoke the installed launchers by their full path.

The preview installs for the current user. It does not start the core, connect accounts, create an auto-start entry or change the mail application's data directory. A developer SDK, Rust, Git, Python, Docker and WSL are not required to run the packaged executables. Runtime DLL requirements must be verified for the final artifact before public release.

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

In preview.2, a fresh refresh also requests read access to the saved CLI display language and diagnostic counts. An older interrupted refresh keeps its original permissions; finish that request before starting a fresh refresh if these permissions are missing. Installing an update does not grant permissions.

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

## Update and remove

Close both CLI and core processes before installing another preview. Run the new extracted package's installer; failed validation must leave the previous version usable. Do not downgrade an encrypted database after a newer core has migrated it.

Use the installed uninstall script as described by the installer output. Removal preserves mail data, encryption keys and registration state; it does not promise to erase accounts or revoke credentials. The data is under the Windows user's local application-data `mail-hantong` directory and is separate from the CLI program directory.

## Build the package from source

Packaging requires Python 3.11 or newer, the pinned Rust toolchain, MSVC build tools and the Windows SDK. These are developer requirements only.

```powershell
cargo fetch --locked
python scripts/prepare_native_tools.py
python scripts/check_all.py
python scripts/package_cli.py --version 0.1.0-preview.2
```

The package tool builds only the CLI and core, records versions and source metadata, includes license notices and produces a ZIP plus checksums under `artifacts/cli/`. It does not publish, sign or install the package. Dependency inventory is build evidence, not a completed legal review or a claimed standards-compliant SBOM.

## 한국어 안내

`hantong`은 터미널에서 명령어로 사용하는 CLI입니다. 메일 목록을 전체 화면으로 조작하는 TUI는 아직 없습니다. `--lang ko`로 한국어를 선택하고 자동화에는 `--json`과 종료 코드를 사용합니다.

현재 개발 미리보기는 Windows 11 x64의 IMAP 설정·등록·폴더 조회·명시적 메타데이터 수집·제목/발신자 조회·비민감 진단 집계를 제공합니다. preview.2의 새 `clients refresh` 요청에서 표시 언어·진단 읽기 권한을 보호된 창으로 확인합니다. 업데이트만으로 권한을 부여하지 않으며 과거 중단 요청의 범위도 바꾸지 않습니다. 본문·첨부·작성·발송·예약·주소록·자동화는 아직 지원하지 않습니다. Mac에서 메일을 사용하는 제품도 아직 준비되지 않았습니다.

압축을 푼 뒤 `install.ps1`로 설치하고 안내된 PATH 설정을 적용합니다. 한 터미널에서 `mail-hantong-core run`, 다른 터미널에서 `hantong` 명령을 실행합니다. 비밀번호 입력과 권한 승인은 보호된 Windows 확인 창에서 사람이 수행합니다. 메일 GUI 실행 파일은 필요하지 않습니다.

공개 릴리스·서명·새 Windows 설치·실제 제공자 검증과 사람 보안 검토가 남아 있습니다. 진행 상황은 저장소의 `docs/latest/PROGRESS.md`에서 관리합니다.
