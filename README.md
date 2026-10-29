# mail-hantong

A multi-account mail hub for Windows, with a shared core service, the `hantong` CLI, MCP access, and an optional GUI. GUI languages are Korean and English; themes are light, dark, and system. The CLI defaults to English and supports Korean.

**Status: active development. The Rust workspace and initial bilingual CLI run locally. Mail integration, GUI and production installers are not complete.**

Windows ships first. Planned macOS support starts with the local core, CLI, and a minimal approval helper; the full GUI follows later.

The core service and CLI use **Rust 1.95.0**, selected to balance startup speed and memory use. See [ADR-001](docs/adr/ADR-001-core-runtime.md) and the [workspace structure](docs/latest/RUST-STRUCTURE.md). The GUI framework remains a separate decision.

여러 메일 계정을 함께 사용하는 메일 허브를 개발하고 있습니다. 원문 170개 요건을 보존한 [Markdown 분할본](docs/requirements/README.md)을 사용자 승인으로 개발 기준 문서에 채택했습니다. GitHub 공개와 명령어 설치·개발 환경 구성을 목표로 합니다.

- [개발 진행 기록](docs/latest/PROGRESS.md): 현재 상태, 변경 내용, 검증 결과, 다음 작업
- [최신 작업 문서](docs/latest/README.md): 요건·디자인·작업 카드
- [GitHub 공개와 설치 계획](docs/latest/DISTRIBUTION.md): 사용자 설치와 개발자 작업 환경의 목표
- [작업 규칙](AGENTS.md): 이 폴더에서 개발할 때 적용할 규칙

Rust/rustup과 Python 3.10 이상이 있는 개발 환경에서 다음 명령을 실행할 수 있습니다. Windows는 MSVC C++ build tools와 Windows SDK가 필요합니다. 네이티브 도구 준비 명령은 검증된 NASM·Perl을 `.tools/`에만 설치하며 시스템 PATH를 바꾸지 않습니다. 제품용 설치 패키지는 아직 없으며 아래 명령은 개발용입니다.

```powershell
cargo fetch --locked
python scripts/prepare_native_tools.py
python scripts/cargo_native.py build --workspace --all-targets --locked --offline
cargo run -p hantong-cli -- --help
cargo run -p hantong-cli -- --lang ko --help
./scripts/check-all.ps1
```

Python 실행 파일을 지정하려면 `./scripts/check-all.ps1 -Python <python-path>`를 사용합니다. Windows의 첫 전체 빌드는 SQLCipher·OpenSSL을 소스에서 컴파일하므로 수 분 이상 걸릴 수 있습니다. 다음 빌드는 Cargo 캐시를 사용합니다. Linux 개발 환경에서는 Windows 도구 준비를 건너뛰고 `cargo fetch --locked` 후 `sh scripts/check-all` 또는 `python3 scripts/check_all.py`로 검사합니다. OS 비밀 저장소의 Linux/macOS 제품 구현은 아직 제공하지 않습니다. Windows의 보호된 계정 조회에는 기존 등록 프로필이 필요합니다.

Windows 코어의 연결 상태를 확인하려면 첫 터미널에서 `python scripts/cargo_native.py run -p mail-hantong-core --locked --offline -- run`을 실행하고 다른 터미널에서 `cargo run -p hantong-cli -- core status --json`을 실행합니다. `--lang ko core status`로 한국어 상태를 볼 수 있습니다. 코어는 Windows Known Folder의 사용자 로컬 앱 데이터 아래 `mail-hantong/data`와 `mail-hantong/secrets`에 암호화 DB·키를 초기화하며, 환경 변수나 명령 인자로 경로를 바꾸지 않습니다. 기존 설정·키·스키마가 올바르지 않으면 시작을 거부합니다. 지원하는 이전 DB는 검증과 단계별 암호화 백업 후 현재 스키마로 이전하고, 서버 서명 키는 Windows 현재 사용자의 CNG에 보관해 다음 실행에도 유지합니다. 활성 서버 키 누락·불일치 시 자동으로 다른 키를 만들지 않습니다. 현재 응답은 전송 준비 상태만 뜻하며 `mail_ready`와 `authenticated`는 false입니다. 코어는 Ctrl+C로 종료합니다. 실제 계정·메일 접근은 구현 중이며 기본 클라이언트 등록은 아래의 별도 개발 명령으로 제공합니다.

코드나 시험이 바뀌면 `python scripts/traceability.py`로 추적표를 갱신한 뒤 전체 검사를 실행합니다. 태그나 문서 연결만으로 요건 통과를 표시하지 않습니다. [CI 구성과 미검증 외부 상태](docs/latest/CI.md)도 확인하십시오. 공개 저장소 주소·릴리스·패키지 ID는 아직 없습니다.

Licensed under the custom [mail-hantong Source-Available License, Version 1.0](LICENSE). Personal and internal business use, private modifications, and free redistribution are permitted. Selling, renting, charging for access or use, and paid hosted or managed provision of the software are prohibited. Preserve license/copyright notices and identify modified versions when redistributing. Third-party licenses apply separately. [한국어 라이선스 안내](docs/latest/LICENSING.md).

현재 실행할 수 있는 문서 도구 검사는 Python 3.10 이상에서 외부 패키지 없이 동작합니다. 아래 명령은 프로젝트 루트에서 실행하며, 제품 빌드·설치 시험과는 별개입니다.

```powershell
python -X utf8 -m unittest discover -s tests/Docs -v
python -X utf8 scripts/prepare_requirements.py --check
python -X utf8 scripts/check_requirements.py
```

[채택된 기준 요건](docs/requirements/README.md)의 무결성은 `check_requirements.py`로 검사합니다. [과거 요건 분할 검토본](docs/proposals/M0-00/README.md)은 원문 대응 증거로 보관하며 `prepare_requirements.py --check`로 별도 검증합니다. 후자는 기준 요건을 덮어쓰지 않습니다.

등록 연결 개발 명령은 Windows 11에서 `cargo run -p hantong-cli -- clients register`, `cargo run -p hantong-cli -- clients status <request_id>`, `cargo run -p hantong-cli -- clients verify`입니다. 먼저 코어를 실행해야 합니다. 등록 요청은 코어 소유 창을 열며 사용자의 Windows Hello 확인 후 기본 읽기 전용 프로필을 발행합니다. CLI는 승인하지 않습니다. `verify`는 기존 프로필과 서버 신원만 확인합니다. 현재 등록 권한은 요청 당시 존재하는 계정 snapshot의 accounts.list로 제한되고, 계정이 없으면 권한도 0개입니다. 계정 설정·전체 GUI와 실제 Hello·사람 보안 검토가 남아 있습니다. 개발 자동 시험은 실제 사용자 등록이나 계정 로그인을 실행하지 않습니다.

등록된 계정 조회는 `cargo run -p hantong-cli -- accounts list --json` 또는 `cargo run -p hantong-cli -- --lang ko accounts list`로 실행합니다. `--include-capabilities`를 추가하면 저장된 기능 식별자도 반환합니다. 상호 인증·현재 계정 범위·등록별 호출 한도·감사를 거친 로컬 계정 표시 자료이며, 지금 외부 메일 서버에 접속했다는 뜻은 아닙니다. 빈 권한은 SCOPE_DENIED이고 계정을 자동 추가하지 않습니다. 아래 신규 IMAP 설정은 단일 연결 시도까지 제공하며 전체 설정 마법사·기존 프로필 범위 갱신은 개발 중입니다. 발송 명령은 계속 차단됩니다.


신규 IMAP 연결 개발 명령은 다음과 같습니다. 코어를 먼저 실행하고 실제 서버 정보로 바꿉니다. 요청은 세션 ID를 반환하고 코어의 영/한 설정 확인 창을 엽니다. 비밀번호는 그 보호 입력 창에서만 입력하며 Windows Hello 확인 뒤 저장과 1회 TLS/AUTH 진단을 실행합니다. 이 기능은 메일 수신·발송을 수행하지 않습니다.

```powershell
cargo run -p hantong-cli -- setup imap request --host imap.example.com --port 993 --tls implicit_tls --login user@example.com --name Personal
cargo run -p hantong-cli -- setup imap status <request_id> --json
cargo run -p hantong-cli -- setup imap resume <request_id> --json
```

`starttls_required`도 선택할 수 있습니다. `--lang ko`는 CLI 출력 언어를 바꾸고 확인 창은 저장된 GUI 언어·테마를 따릅니다. `--no-input`은 터미널 질문을 없애며 사람 확인 창을 생략하지 않습니다. `ready`는 해당 연결 진단 결과이며 CLI 권한을 자동 부여하지 않습니다. 기존 등록이 없으면 별도로 `clients register`를 실행하고, 이미 등록이 있으면 상태·계정 범위 재검토가 필요합니다. 중단/실패한 요청은 `resume`으로 같은 대상의 새 시도를 시작할 수 있습니다. 과거 이력을 보존하고 보호 입력·Hello 확인을 다시 요구합니다. 진행 중인 같은 시도나 이미 연결을 마친 시도는 상태만 반환합니다. 원래 대상이나 계정 버전이 바뀌면 재개를 거부하며 reset은 아직 지원하지 않습니다. 개발 자동 시험에서는 실제 계정이나 Hello를 사용하지 않았고 사람 보안·화면 검토가 남아 있습니다.
