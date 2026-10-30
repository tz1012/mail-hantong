# mail-hantong

A multi-account mail hub for Windows, with a shared core service, the `hantong` CLI, MCP access, and an optional GUI. GUI languages are Korean and English; themes are light, dark, and system. The CLI defaults to English and supports Korean.

**Status: Windows CLI developer preview.** [Download releases](https://github.com/tz1012/mail-hantong/releases). The CLI runs in a terminal; account setup and outgoing-mail authorization use a protected Windows confirmation window. See the [CLI installation and usage guide](docs/latest/CLI-LAUNCH.md) for body reading, composing, sending, AI sessions, MCP tools and remaining limitations.

Install or update from PowerShell on Windows x64:

```powershell
irm https://raw.githubusercontent.com/tz1012/mail-hantong/main/install.ps1 | iex
```

The installer verifies release checksums and adds the current user's launcher directory to PATH. This unsigned developer preview still needs human security review and real-provider validation.

Preview.4 adds Google multi-account browser login and Gmail read-only synchronization. Preview.3 does not include them. See [Google setup and mail commands](docs/latest/GOOGLE-MAIL.md); initial setup requires public Google Desktop OAuth client configuration. Implementation and verification status are recorded in [PROGRESS.md](docs/latest/PROGRESS.md).

Windows GUI는 `cargo run -p mail-hantong-gui -- --lang ko --theme system`으로 실행합니다. 영어는 `--lang en`, 테마는 `light`, `dark`, `system`입니다. GUI는 CLI와 별도 등록을 사용하며, 사용자가 연결을 요청하고 보호된 확인 창에서 검토한 계정의 저장된 폴더·메일 메타데이터만 읽습니다. 선택한 폴더에서 전체·읽지 않음·즐겨찾기로 목록을 좁히고 다시 조회할 수 있습니다. 현대적인 메일 화면의 구성과 참조 기준은 [디자인 가이드](docs/latest/DESIGN.md)에 기록합니다. 표시 설정 저장·본문·첨부·작성·발송은 아직 개발 중입니다. 최신 검증 여부는 [진행 기록](docs/latest/PROGRESS.md)을 확인하세요.

Windows ships first. Planned macOS support starts with the local core, CLI, and a minimal approval helper; the full GUI follows later.

개발용 화면 확인은 `cargo run -p hantong-desktop --example fixture_preview`로 실행할 수 있습니다. 새로고침 후 `[Sample / 예시] Preview` 계정과 받은편지함을 선택하면 명시적으로 표시된 합성 메일 32개로 목록·테마·스크롤을 확인합니다. 이 예제는 Core·IPC·실제 계정·네트워크를 사용하지 않으며 제품 실행 파일에는 포함되지 않습니다.

The core service and CLI use **Rust 1.95.0**, selected to balance startup speed and memory use. The Windows GUI uses native Win32 controls. See [ADR-001](docs/adr/ADR-001-core-runtime.md), [ADR-002](docs/adr/ADR-002-desktop-ui.md), and the [workspace structure](docs/latest/RUST-STRUCTURE.md).

여러 메일 계정을 함께 사용하는 메일 허브를 개발하고 있습니다. 원문 170개 요건을 보존한 [Markdown 분할본](docs/requirements/README.md)을 사용자 승인으로 개발 기준 문서에 채택했습니다. GitHub 공개와 명령어 설치·개발 환경 구성을 목표로 합니다.

- [개발 진행 기록](docs/latest/PROGRESS.md): 현재 상태, 변경 내용, 검증 결과, 다음 작업
- [최신 작업 문서](docs/latest/README.md): 요건·디자인·작업 카드
- [GitHub 공개와 설치 계획](docs/latest/DISTRIBUTION.md): 사용자 설치와 개발자 작업 환경의 목표
- [작업 규칙](AGENTS.md): 이 폴더에서 개발할 때 적용할 규칙

Rust/rustup과 Python 3.11 이상이 있는 개발 환경에서 다음 명령을 실행할 수 있습니다. Windows는 MSVC C++ build tools와 Windows SDK가 필요합니다. 네이티브 도구 준비 명령은 검증된 NASM·Perl을 `.tools/`에만 설치하며 시스템 PATH를 바꾸지 않습니다. 아래 명령은 소스 개발용이며, CLI 미리보기 ZIP의 설치 방법은 위 CLI 안내를 따릅니다.

```powershell
git clone https://github.com/tz1012/mail-hantong.git
cd mail-hantong
cargo fetch --locked
python scripts/prepare_native_tools.py
python scripts/cargo_native.py build --workspace --all-targets --locked --offline
cargo run -p hantong-cli -- --help
cargo run -p hantong-cli -- --lang ko --help
./scripts/check-all.ps1
```

Python 실행 파일을 지정하려면 `./scripts/check-all.ps1 -Python <python-path>`를 사용합니다. Windows의 첫 전체 빌드는 SQLCipher·OpenSSL을 소스에서 컴파일하므로 수 분 이상 걸릴 수 있습니다. 다음 빌드는 Cargo 캐시를 사용합니다. Linux 개발 환경에서는 Windows 도구 준비를 건너뛰고 `cargo fetch --locked` 후 `sh scripts/check-all` 또는 `python3 scripts/check_all.py`로 검사합니다. OS 비밀 저장소의 Linux/macOS 제품 구현은 아직 제공하지 않습니다. Windows의 보호된 계정 조회에는 기존 등록 프로필이 필요합니다.

Windows 코어의 연결 상태를 확인하려면 첫 터미널에서 `python scripts/cargo_native.py run -p mail-hantong-core --locked --offline -- run`을 실행하고 다른 터미널에서 `cargo run -p hantong-cli -- core status --json`을 실행합니다. `--lang ko core status`로 한국어 상태를 볼 수 있습니다. 코어는 Windows Known Folder의 사용자 로컬 앱 데이터 아래 `mail-hantong/data`와 `mail-hantong/secrets`에 암호화 DB·키를 초기화하며, 환경 변수나 명령 인자로 경로를 바꾸지 않습니다. 기존 설정·키·스키마가 올바르지 않으면 시작을 거부합니다. 지원하는 이전 DB는 검증과 단계별 암호화 백업 후 현재 스키마로 이전하고, 서버 서명 키는 Windows 현재 사용자의 CNG에 보관해 다음 실행에도 유지합니다. 활성 서버 키 누락·불일치 시 자동으로 다른 키를 만들지 않습니다. 현재 응답은 전송 준비 상태만 뜻하며 `mail_ready`와 `authenticated`는 false입니다. 코어는 Ctrl+C로 종료합니다. CLI의 본문 읽기·작성·발송·AI·MCP 사용 방법은 위 설치·사용 안내를 따릅니다. 기본 클라이언트 등록은 아래 개발 명령으로도 제공합니다.

코드나 시험이 바뀌면 `python scripts/traceability.py`로 추적표를 갱신한 뒤 전체 검사를 실행합니다. 태그나 문서 연결만으로 요건 통과를 표시하지 않습니다. [CI 구성과 미검증 외부 상태](docs/latest/CI.md)도 확인하십시오. 공개 소스와 개발 미리보기는 위 GitHub 저장소에서 관리하며 WinGet 패키지 등록은 아직 제공하지 않습니다.

Licensed under the custom [mail-hantong Source-Available License, Version 1.0](LICENSE). Personal and internal business use, private modifications, and free redistribution are permitted. Selling, renting, charging for access or use, and paid hosted or managed provision of the software are prohibited. Preserve license/copyright notices and identify modified versions when redistributing. Third-party licenses apply separately. [한국어 라이선스 안내](docs/latest/LICENSING.md).

현재 실행할 수 있는 문서 도구 검사는 Python 3.10 이상에서 외부 패키지 없이 동작합니다. 아래 명령은 프로젝트 루트에서 실행하며, 제품 빌드·설치 시험과는 별개입니다.

```powershell
python -X utf8 -m unittest discover -s tests/Docs -v
python -X utf8 scripts/prepare_requirements.py --check
python -X utf8 scripts/check_requirements.py
```

[채택된 기준 요건](docs/requirements/README.md)의 무결성은 `check_requirements.py`로 검사합니다. [과거 요건 분할 검토본](docs/proposals/M0-00/README.md)은 원문 대응 증거로 보관하며 `prepare_requirements.py --check`로 별도 검증합니다. 후자는 기준 요건을 덮어쓰지 않습니다.

등록 연결 개발 명령은 Windows 11에서 `cargo run -p hantong-cli -- clients register`, `cargo run -p hantong-cli -- clients status <request_id>`, `cargo run -p hantong-cli -- clients verify`입니다. 먼저 코어를 실행해야 합니다. 등록 요청은 코어 소유 창을 열며 사용자의 Windows Hello 확인 후 기본 읽기 전용 프로필을 발행합니다. CLI는 승인하지 않습니다. `verify`는 기존 프로필과 서버 신원만 확인합니다. 현재 등록 권한은 요청 당시 존재하는 계정 snapshot의 accounts.list로 제한되고, 계정이 없으면 권한도 0개입니다. 계정 설정·전체 GUI와 실제 Hello·사람 보안 검토가 남아 있습니다. 개발 자동 시험은 실제 사용자 등록이나 계정 로그인을 실행하지 않습니다.

등록된 계정 조회는 `cargo run -p hantong-cli -- accounts list --json` 또는 `cargo run -p hantong-cli -- --lang ko accounts list`로 실행합니다. `--include-capabilities`를 추가하면 저장된 기능 식별자도 반환합니다. 상호 인증·현재 계정 범위·등록별 호출 한도·감사를 거친 로컬 계정 표시 자료이며, 지금 외부 메일 서버에 접속했다는 뜻은 아닙니다. 빈 권한은 SCOPE_DENIED이고 계정을 자동 추가하지 않습니다. 기본 CLI 범위 갱신의 bounded progress 요청/조회도 다음 명령으로 제공됩니다.

```powershell
cargo run -p hantong-cli -- clients refresh
cargo run -p hantong-cli -- clients refresh-status <refresh_id>
```

범위 갱신은 현재 등록의 최초 요청·키·서버 pin을 보존하고, 현재 계정 snapshot과 계정 목록·폴더 목록/갱신·IMAP UID 메타데이터 한 페이지 수집 권한을 native 검토 대상으로 삼습니다. 새 권한은 새 명시 요청에서만 제안하며, 기존 대기/복구 요청의 범위를 바꾸지 않습니다. revoked 등록을 되살리지 않으며, native 확인·Core stage/activate·보호 프로필 배포의 실제 효과는 사람 검토가 필요한 Windows 경계입니다. 발송 명령은 계속 차단됩니다.


신규 IMAP 연결 개발 명령은 다음과 같습니다. 코어를 먼저 실행하고 실제 서버 정보로 바꿉니다. 요청은 세션 ID를 반환하고 코어의 영/한 설정 확인 창을 엽니다. 비밀번호는 그 보호 입력 창에서만 입력하며 Windows Hello 확인 뒤 저장과 1회 TLS/AUTH 진단을 실행합니다. 이 기능은 메일 수신·발송을 수행하지 않습니다.

```powershell
cargo run -p hantong-cli -- setup imap request --host imap.example.com --port 993 --tls implicit_tls --login user@example.com --name Personal
cargo run -p hantong-cli -- setup imap status <request_id> --json
cargo run -p hantong-cli -- setup imap resume <request_id> --json
```

`starttls_required`도 선택할 수 있습니다. `--lang ko`는 CLI 출력 언어를 바꾸고 확인 창은 저장된 GUI 언어·테마를 따릅니다. `--no-input`은 터미널 질문을 없애며 사람 확인 창을 생략하지 않습니다. `ready`는 해당 연결 진단 결과이며 CLI 권한을 자동 부여하지 않습니다. 기존 등록이 없으면 별도로 `clients register`를 실행하고, 이미 등록이 있으면 상태·계정 범위 재검토가 필요합니다. 중단/실패한 요청은 `resume`으로 같은 대상의 새 시도를 시작할 수 있습니다. 과거 이력을 보존하고 보호 입력·Hello 확인을 다시 요구합니다. 진행 중인 같은 시도나 이미 연결을 마친 시도는 상태만 반환합니다. 원래 대상이나 계정 버전이 바뀌면 재개를 거부하며 reset은 아직 지원하지 않습니다. 개발 자동 시험에서는 실제 계정이나 Hello를 사용하지 않았고 사람 보안·화면 검토가 남아 있습니다.


IMAP 폴더와 한 페이지 메타데이터 수집의 개발용 명령도 제공됩니다. `folders list --account ACCOUNT --json` 결과의 `wire_hex`와 `encoding`을 그대로 사용하고, `sync cursor`에서 확인한 catalog generation/cursor version을 `sync page`에 지정합니다. 처음부터 전체 mailbox를 자동 수집하지 않습니다.

```powershell
cargo run -p hantong-cli -- sync cursor --account ACCOUNT --wire-hex HEX --encoding utf8
cargo run -p hantong-cli -- sync inspect --account ACCOUNT --wire-hex HEX --encoding utf8
cargo run -p hantong-cli -- sync page --account ACCOUNT --wire-hex HEX --encoding utf8 --expected-catalog-generation 1 --expected-cursor-version 0 --operation-id my_page_1 --idempotency-key my_page_1
cargo run -p hantong-cli -- sync status --account ACCOUNT --execution-id EXECUTION_HEX
cargo run -p hantong-cli -- sync reconcile --account ACCOUNT --wire-hex HEX --encoding utf8 --expected-catalog-generation 1 --expected-cursor-version 1 --operation-id my_reconcile_1 --idempotency-key my_reconcile_1
cargo run -p hantong-cli -- sync reconcile-status --account ACCOUNT --execution-id EXECUTION_HEX
cargo run -p hantong-cli -- sync recover --account ACCOUNT --wire-hex HEX --encoding utf8 --expected-catalog-generation 1 --expected-cursor-version 1 --operation-id my_recovery_1 --idempotency-key my_recovery_1
cargo run -p hantong-cli -- sync recover-status --account ACCOUNT --execution-id EXECUTION_HEX
```

최근 메일부터 확인하려면 초기 미리보기 계획을 시작합니다. 첫 계획의 `--expected-preview-generation`은 `0`이며, `initial-start`는 메일함의 범위만 관측합니다. 작업 결과의 plan ID와 현재 plan version을 사용해 `initial-resume`를 실행하면 최근 UID 범위 한 페이지를 수집합니다. 저장한 메타데이터는 기존 메일 목록 검색에 나타납니다.

```powershell
cargo run -p hantong-cli -- sync initial-start --account ACCOUNT --wire-hex HEX --encoding utf8 --expected-catalog-generation 1 --expected-preview-generation 0 --recent-pages 2 --operation-id initial_1 --idempotency-key initial_1
cargo run -p hantong-cli -- sync initial-status --account ACCOUNT --execution-id EXECUTION_HEX
cargo run -p hantong-cli -- sync initial-plan --account ACCOUNT --wire-hex HEX --encoding utf8 --plan-id PLAN_HEX
cargo run -p hantong-cli -- sync initial-resume --account ACCOUNT --wire-hex HEX --encoding utf8 --plan-id PLAN_HEX --expected-plan-version PLAN_VERSION --expected-catalog-generation 1 --operation-id resume_1 --idempotency-key resume_1
cargo run -p hantong-cli -- sync initial-pause --account ACCOUNT --wire-hex HEX --encoding utf8 --plan-id PLAN_HEX --expected-plan-version PLAN_VERSION --operation-id pause_1 --idempotency-key pause_1
```

최근 범위는 최대8페이지이며 페이지당 UID 폭은64입니다. 페이지 완료는 선택한 최근 범위의 진행 상태를 뜻하며 전체 메일함 동기화 완료가 아닙니다. `initial-status`는 요청 당시의 저장된 작업 결과, `initial-plan`은 현재 진행 상태를 보여줍니다. 같은 작업을 다시 요청해도 자동으로 다음 페이지를 수집하지 않습니다. 다음 페이지는 현재 버전과 새 operation/idempotency 값을 사용한 명시 요청으로 진행합니다.

위 계정·HEX·encoding·버전은 실제 조회 값으로 바꾸는 예시입니다. `inspect`는 저장된 UID 범위·해당 페이지의 관측 상한·로컬 저장 시각을 조회합니다. 미관측과 관측된 빈 메일함을 구분하며, UID 범위는 메일 건수나 현재 서버의 최신 상태를 뜻하지 않습니다. 기존 프로필에 새 권한이 없으면 명시 `clients refresh`와 보호 확인이 필요합니다. 한 번에 UID 폭 최대64를 관측하며 중복 요청은 기존 receipt만 반환합니다. `reconcile`은 이미 저장한 한 페이지의 FLAGS와 원격 부재 관측을 별도로 기록하고 원본 메일은 보존합니다. 다음 페이지도 새 명시 요청으로 진행하며, pass 완료는 시작 때 고정한 기존 페이지 묶음의 관측 완료만 뜻합니다. 종료 후 미완료 작업은 자동 재실행하지 않습니다. UIDVALIDITY가 바뀌면 `recover`로 새 식별 체계의 첫 페이지를 명시 수집합니다. 기존 원본·관측·작업 이력은 보존하고 cursor version은 계속 증가합니다. 복구 성공은 전체 mailbox 수집 완료가 아닙니다. 설정/세대/cursor 충돌은 무반영 실패하며 자동 reset하지 않습니다. 본문·작성·발송은 위 CLI 안내를 따르며, 첨부·전체 mailbox 자동 수집과 실제 제공자 검증은 후속입니다. [공개 계약](docs/contracts/imap-uid-tools.md), [상태 대조 계약](docs/contracts/imap-uid-reconcile.md), [복구 계약](docs/contracts/imap-uid-recovery.md), [관측 요약 계약](docs/contracts/imap-uid-inspection.md), [진행 기록](docs/latest/PROGRESS.md)에 현재 증거와 남은 범위를 기록합니다.

저장된 메일의 제목·발신자는 로컬 메타데이터 조회로 확인합니다. 처음 등록한 CLI나 기존 폴더/UID 권한은 헤더를 읽을 수 없습니다. 사용자가 `clients refresh`의 보호 확인에서 메타데이터 공개 범위를 명시적으로 허용한 뒤 사용할 수 있습니다.

현재 권한 갱신에는 CLI 표시 언어와 자기 실행 이력의 진단 집계 읽기도 포함됩니다. 명시 `--lang`이 우선하며, 일반 메일 명령은 저장된 CLI 언어를 최대 250ms의 비동기 조회 예산으로 읽고 사용할 수 없으면 영어로 표시합니다. `--json`, 도움말, 등록·설정 명령은 이 추가 조회를 하지 않습니다. 진단은 아래처럼 기간과 항목을 지정해 조회하며 식별자·주소·메일 원문을 출력하거나 파일·외부 서버로 전송하지 않습니다.

```powershell
cargo run -p hantong-cli -- diagnostics snapshot --sections sync,recent_work,errors --hours 24 --limit 100 --json
```

```powershell
cargo run -p hantong-cli -- search --account ACCOUNT --wire-hex HEX --encoding utf8 --limit 16
cargo run -p hantong-cli -- search --account ACCOUNT --wire-hex HEX --encoding utf8 --filter unread --lang ko
cargo run -p hantong-cli -- search --account ACCOUNT --wire-hex HEX --encoding utf8 --filter favorite --json
cargo run -p hantong-cli -- search --account ACCOUNT --wire-hex HEX --encoding utf8 --limit 16 --cursor CURSOR
```

한 요청은 현재 UID namespace의 저장된 source 한 페이지를 UID 내림차순으로 조회합니다. 표시된 다음 cursor는 같은 계정·mailbox·filter·limit에 사용하며 5분 뒤나 저장 상태가 바뀌면 만료됩니다. 빈 결과에도 다음 cursor가 있을 수 있습니다. 결과는 로컬의 부분 관측이며 마지막 페이지도 서버 전체 결과를 의미하지 않습니다. `search`는 네트워크 동기화나 자동 다음 페이지 요청을 실행하지 않습니다. 제목·주소는 원시 헤더의 안전 표시이고 MIME encoded-word 해석은 후속 범위입니다. [검색 계약](docs/contracts/imap-metadata-search.md)과 [명시 권한 계약](docs/contracts/client-metadata-scope-refresh.md)에 상세 경계가 있습니다.
