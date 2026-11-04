# ADR-031 제안: Linux headless의 암호화 비밀 저장

상태: 변경안 제시 후 사용자의 실제 이용 구현 계속 지시(2026-11-04). 로컬 구현·검증 진행,
사람 보안 검토 필요. 실제 비밀값 이전이나 계정 작업은 별도로 유지한다.

## 문제와 결정안

Linux CLI/Core와 메일 기능은 디스플레이·데스크톱 세션·Secret Service가 없는 SSH
환경에서도 사용할 수 있어야 한다. ADR-030의 데스크톱 Secret Service 구현만으로
이 조건을 충족하지 못한다. Secret Service의 Unlock은 GUI Prompt 객체를 반환할 수
있고 표준에 터미널 입력 방식이 정해져 있지 않다.

권고안은 기존 OS 저장소를 보존하고, 사용자가 명시적으로 선택하는 **headless
암호화 금고** backend를 Secrets 안에 추가하는 것이다. 자체 암호 알고리즘이나 KDF를
설계하지 않고 `age = "=0.12.1"`, `default-features = false`의 passphrase 기반
바이너리 포맷을 사용한다. CLI-common, plugin, SSH-key, 외부 pinentry 실행 기능은
활성화하지 않는다. 프로젝트 Rust 코어·기존 SQLCipher DB는 유지한다.

이 버전의 workspace MSRV는 1.74, 라이선스는 MIT OR Apache-2.0이다. upstream은
1.0 이전 crate를 beta로 명시한다. 따라서 포맷 상호운용·변조·상한·오류·복구 회귀와
사람 보안 검토를 거쳐야 하며 검증된 보안 제품이라는 보장은 하지 않는다.
승인 시 고정 직접 의존성 및 그 전이 의존성의 lock/license 검증을 포함한다.

## 대안 비교

| 대안 | 판단 |
| --- | --- |
| Secret Service만 필수화 | 기존 desktop에는 적합하지만 서비스/GUI 없는 서버 요구를 충족하지 못함 |
| headless Secret Service 설치 안내 | 이미 준비된 서비스는 사용 가능하나 터미널 unlock·D-Bus 수명을 모든 환경에서 보장하지 못함 |
| 명시적인 age 암호화 금고 | 추가 의존성·복구 설계가 필요하지만 GUI 없이 사용자 암호로 해제 가능 |
| 기존 ring으로 자체 금고 제작 | 의존성은 줄지만 자체 파일 암호 포맷·KDF 운용의 보안 부담이 커 비권고 |
| 평문 파일·환경변수 비밀값 | 채택하지 않음 |

## 필수 경계

- Windows DPAPI/CNG와 macOS Keychain, 기존 Linux Secret Service 항목은 변경하지 않는다.
- backend는 첫 초기화 전에 명시적으로 고정한다. OS 저장소 실패를 금고 자동 생성이나
  다른 backend로의 자동 폴백으로 처리하지 않는다. 기존 계정·키의 자동 복사도 없다.
- 사용자 암호와 앱 비밀번호는 같은 SSH TTY의 Core 소유 Secrets 보호 입력에서만
  받는다. CLI/MCP는 비밀값을 읽거나 전달할 API를 갖지 않는다. argv·env·로그·JSON·
  임시 평문 파일을 입력 경로로 제공하지 않는다.
- Core 종료/잠금 시 Secrets가 해제 상태를 폐기한다. 서버 재시작 뒤에는 다시 사용자
  해제가 필요하다. TTY가 없는 서비스의 무인 잠금 해제는 이번 제안에 포함하지 않는다.
  headless 지원을 사람 없는 무인 발송 허가로 해석하지 않는다.
- 사용자 암호 분실은 복구 불가일 수 있다. 암호화 백업은 제공하되 평문 복구키나
  하드코딩된 master key를 넣지 않는다. OS/하드웨어 결속 저장소와 같은 보호라고 표시하지 않는다.
- 기존 DB·등록/키 메타데이터가 있을 때 금고 누락·손상·암호 불일치는 실패다.
  새 DB 키·서명 키 생성이나 pin 교체로 복구하지 않는다.
- 고정 경로·0700 디렉터리·0600 파일·UID·symlink 거부·배타 잠금·파일 identity 검증을
  기존 Unix guard로 수행한다. 암호문 전체 인증 및 크기/KDF 자원 상한을 검증한 후
  항목을 사용한다. 비밀 selector는 기존 Unix 계약을 따른다.
- 변경은 보호된 동일 디렉터리 임시 **암호문** → fsync → atomic rename → 디렉터리
  fsync를 사용한다. 중단된 저장·기존 DB와의 결속 불일치는 fail closed로 복구한다.
- 오류 코드는 기존 안정 오류 계약을 우선 사용한다. 새 코드가 필요하면 별도 계약
  변경을 먼저 제시하며 라이브러리 원문 오류를 사용자에게 노출하지 않는다.

## 실행과 완료 조건

Core의 Unix storage/identity/runtime/socket/dispatcher/workers를 연결하고 CLI는
[Core 소유 relay 제안](../../contracts/proposals/unix-core-relay.md)으로 인증한다.
금고 선택·생성·해제·잠금 수명과 영속 결속 스키마는
[headless 금고 계약 변경안](../../contracts/proposals/linux-headless-vault.md)을 따른다.
이 ADR 승인만으로 임의의 새 공개 RPC나 권한을 추가하지 않는다.

GUI/D-Bus 없는 실제 Linux에서 설치·실행·보호 입력·세 계정 격리·IMAP/SMTP
목록/본문/봉투 정보·작성·발송/휴지통 확인·재시작/잠금·업데이트를 검증한다.
기존 fake 제공자로 회귀를 수행하고 실제 계정·로그인·발송 시험은 별도 사용자 경계를
지킨다. Google OAuth는 표준 브라우저 동의·원격 콜백 연결을 별도로 검증하며
IMAP/SMTP의 headless 성공으로 대체하지 않는다. 이 제안은 아직 Linux 배포본이 아니다.

## 영향과 근거

HOST-02, INST-01/03, SEC-03, AI-07/09, APR-01, EXT-03, SYNC-07.
대상: Secrets backend/SQLCipher 키 수명, Unix Core 시작·종료, 보호 입력,
설정/프로필 결속, CLI 안내, native 설치·복구·검사. 사람 보안 검토 필요.

- [Secret Service 잠금 해제](https://specifications.freedesktop.org/secret-service/latest/unlocking.html)
- [age 0.12.1 API](https://docs.rs/age/0.12.1/age/)
- [crate 기능과 의존성](https://github.com/str4d/rage/blob/v0.12.1/age/Cargo.toml)
- [MSRV와 라이선스](https://github.com/str4d/rage/blob/v0.12.1/Cargo.toml)
