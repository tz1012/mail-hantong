# CI 기준

M0-02에서 `.github/workflows/check.yml`을 추가했다. `push`, `pull_request`, 수동 실행을 대상으로 Ubuntu 24.04와 Windows Server 2025를 각각 검사한다. 두 플랫폼은 같은 `scripts/check_all.py`를 실행하며, `fail-fast: false`로 한 플랫폼의 실패가 다른 플랫폼 결과를 숨기지 않는다. 마지막 `CI required` 작업은 항상 실행되어 두 플랫폼 작업 중 하나라도 실패하면 실패한다.

각 작업은 저장소를 읽기 전용으로 체크아웃하고, Python 3.12와 Rust 1.95.0의 rustfmt·clippy를 사용한다. Cargo 의존성은 먼저 `cargo fetch --locked`로 가져온 뒤, 공통 검사기가 `--offline`으로 빌드·테스트·fmt·clippy를 실행한다. Tokio·Serde·Windows API 등 외부 의존성의 실제 해상 결과는 `Cargo.lock`으로 고정한다.

Windows는 `scripts/prepare_native_tools.py`로 고정된 NASM·Strawberry Perl을 저장소의 무시된 `.tools/`에 준비한다. 다운로드·소스 빌드가 끝난 뒤 `cargo_native.py`의 검증을 거쳐 자식 프로세스 PATH에만 연결한다. SQLCipher·OpenSSL은 vendored 소스로 빌드한다. 최초 네이티브 빌드를 위해 전체 job은 45분, 공통 검사의 build 명령은 30분으로 제한하며 나머지 명령은 각 10분 제한을 유지한다. 이 시간은 제품 시작 시간 목표와 별개다.

워크플로는 `contents: read`만 허용하며 checkout의 credential 저장도 끈다. 메일 계정·OAuth·비밀값·배포 토큰을 사용하지 않는다. 실제 GitHub 실행, 저장소의 기본 브랜치 보호 규칙, `CI required`를 병합 필수 상태 검사로 등록하는 작업은 저장소 관리자 설정이므로 이 폴더에서 완료로 주장하지 않는다.

## 액션 버전 근거

현재 액션 참조는 공식 릴리스 버전으로 고정했다.

- `actions/checkout@v6.1.0` — [공식 v6.1.0 릴리스](https://github.com/actions/checkout/releases/tag/v6.1.0)
- `actions/setup-python@v7.1.0` — [공식 v7.1.0 릴리스](https://github.com/actions/setup-python/releases/tag/v7.1.0)

액션 버전을 바꿀 때는 해당 공식 릴리스의 변경·서명·러너 요구 사항을 확인하고 이 문서와 구조 검사를 함께 갱신한다. GitHub의 workflow syntax에서 matrix `fail-fast`, job `needs`, 최소 권한을 확인할 수 있다: [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax).

## 관리자 설정 후 확인

GitHub에 저장소를 만든 뒤 첫 workflow 실행을 확인하고, Branch protection 또는 Rulesets에서 `CI required`를 병합 필수 검사로 등록한다. Ubuntu와 Windows 두 개의 matrix 검사도 모두 필수로 등록한다. 이 단계는 GitHub 계정 권한과 실제 저장소 주소가 필요하므로 코드 변경의 검증 결과와 분리한다.
