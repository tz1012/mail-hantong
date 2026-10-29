# ADR-008 Windows 비밀 저장소

상태: 2026-10-29 연속 개발 위임에 따른 구현 결정. **사람 보안 검토 필요**.

Windows 제품은 DPAPI CurrentUser로 비밀을 암호화한다. 머신 전체 보호 플래그는 사용하지 않고 `CRYPTPROTECT_UI_FORBIDDEN`으로 예기치 않은 대화창을 막는다. 계정과 비밀 종류를 추가 엔트로피에 결속한다. OS가 복호화한 값은 Secrets crate 안에서만 사용하고 외부에는 불투명 핸들만 제공한다.

선택지: 자격증명 관리자는 OS 저장 정책을 활용할 수 있지만 큰 키/여러 종류의 수명과 백업 제외 제어가 다르다. 암호화 파일에 별도 소프트웨어 키를 보관하면 키 보호 문제를 다시 해결해야 한다. 현재는 작은 DPAPI 암호문 파일을 사용자 전용 앱 데이터 아래 관리한다. 같은 사용자 권한의 악성코드나 커널 침해 방어 수단으로 DPAPI를 설명하지 않는다.

Windows API는 Microsoft의 `windows-sys 0.61.2`를 선택해 필요한 Foundation·Cryptography 기능만 사용한다. 이 crate는 MIT/Apache-2.0 조건을 유지하며 제품 자체 라이선스로 대체하지 않는다. `hantong-secrets`의 Windows 모듈에만 필요한 FFI를 두고 `unsafe_op_in_unsafe_fn`을 거부한다. 나머지 제품 crate의 unsafe 금지는 유지한다. FFI와 평문 수명은 사람 검토 대상이다.

초기 범위는 저장·존재 확인·내부 무결성 검증·삭제다. 실제 OAuth/HTTP 자격증명 사용은 이후 Secrets 내부 경로로 연결한다. Linux/macOS에서는 보호를 가장한 평문 파일로 대체하지 않고 미지원 오류를 반환한다. Mac의 OS keychain 구현은 후속이다.

공식 근거: [Microsoft CryptProtectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata), [Windows API bindings](https://docs.rs/windows-sys/0.61.2/windows_sys/). CurrentUser 보호는 같은 사용자/컴퓨터 범위이며 로그인된 사용자 프로세스 전체를 서로 격리하는 수단이 아니다.

## M2 역할별 자격 증명 보완

IMAP와 SMTP는 MailCredentialSlot의 독립된 비밀을 사용한다. 새 종류는 Windows case-insensitive 경로 충돌을 피하는 v2 base32hex 파일명을 쓰고 기존 종류 이름/경로는 자동 변경하지 않는다. 공개 resolver는 mail slot만 받으며 plaintext getter나 임의 AUTH callback은 추가하지 않는다. 세부 동작은 [비밀 계약](../contracts/secrets.md)을 따른다. 기존 네 종류의 raw account 파일명은 case-safe 이전 결정 전 새 제품 인증 경로에서 사용하지 않는다. 검토 과정에서 발견한 호환성/격리 과제로 추적하며 사람 검토 의무를 유지한다.

제품 AUTH는 [메일 자격 증명 envelope](../contracts/mail-credentials.md)에 endpoint 전체와 exact settings_version을 결속한다. 해시나 평문 sidecar 없이 검증된 필드의 bounded canonical prefix를 DPAPI 내부에서 비교한다. 이를 위해 Secrets→Domain 의존을 허용하고 Domain은 OS/Secrets/Store를 참조하지 않는다. 설정을 바꾼 뒤 이전 암호가 다른 서버로 자동 전달되는 경로를 차단하며, 비밀 사용은 향후 Secrets 소유 TLS broker 안에서만 구현한다.

## INST-05 네이티브 보호 입력

Core 소유 native UI의 같은 thread에 Secrets가 표준 password EDIT child를 생성·소유하는 안을 선택한다. Secrets 자체에 별도 top-level 설정 창을 만들면 언어·테마·DPI·취소·동시 창 gate를 중복하게 된다. arbitrary 기존 EDIT에서 값을 읽는 안은 대상과 수명을 보장하기 어렵다. 선택한 경로는 parent HWND를 배치 locator로 검증하고 plan→child→pending의 불변 target/nonce를 유지한다. HWND나 UI 클릭은 권한이 아니며 Hello 확인 결과는 별도로 결속한다.

기존 windows-sys 0.61.2의 UI/Controls/Shell/Threading feature를 사용한다. UI는 plaintext API를 받지 않고, Secrets는 volatile-wiped UTF-16/UTF-8 buffer로 읽어 endpoint-bound DPAPI 저장에만 사용한다. Windows EDIT/IME 내부 메모리까지 지운다고 보장하지 않으며 같은 사용자/프로세스 침해를 격리한다고 주장하지 않는다. API·시간·정리·실패 경계와 Microsoft 근거는 [보호 입력 계약](../contracts/protected-mail-input.md)에 둔다.
