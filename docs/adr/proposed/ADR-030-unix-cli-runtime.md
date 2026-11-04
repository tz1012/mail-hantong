# ADR-030 제안: macOS·Linux CLI/TUI의 네이티브 실행 기반

상태: 2026-11-04 사용자가 보안 기반·명시된 의존성 추가 승인 / 구현 중 /
사람 보안 검토 필요. 승인 자체는 제품 지원 완료나 사람 보안 코드 검토를 뜻하지 않는다.

## 결정안

Windows와 같은 CLI/TUI 메일 기능을 macOS와 Linux에 제공한다. 기존 Windows
인증 v1·CNG 키·계정·DB 형식은 유지하고 Unix용 인증 v2와 OS 비밀 저장소를
별도로 추가한다. `cfg`만 제거하거나 평문 개발용 저장소를 제품에 연결하지 않는다.

첫 네이티브 검증 조합은 Windows x64, macOS 15 이상 arm64/x86_64,
Ubuntu 24.04 x86_64 데스크톱이다. Linux headless/SSH도 후속 지원 범위에 남긴다.
Secret Service 없는 환경을 지원 완료로 표시하지 않으며, headless용 암호화
금고·잠금 해제 설계는 별도 제안으로 검증한다. WSL 컴파일은 제품 E2E의 대체가 아니다.

## 보호 방식

- macOS: 사용자 Keychain. Linux 데스크톱: freedesktop Secret Service.
- Unix 서명 개인키는 PKCS#8으로 OS 저장소에 보관하고 Secrets 내부 메모리에서만
  읽어 서명한다. CLI·IPC 응답에 개인키를 반환하지 않는다. Windows CNG 비수출 키는 유지한다.
- DB 키도 Secrets가 OS 저장소에서 읽어 기존 SQLCipher 연결에만 제공한다.
- 저장소 없음·잠김·키 손상은 실패로 처리한다. 평문 파일 대체, 기존 키 누락 시
  자동 재생성, 프로필 자동 재고정은 하지 않는다.
- Unix AF_UNIX 연결은 커널 peer 자격·프로세스 인스턴스·실행 이미지에 결속한다.
  같은 UID라는 사실만으로 터미널 도우미를 신뢰하지 않는다.
- 공개키 72바이트 ECS1 호환 형식과 서명 64바이트 형식을 유지한다.
  Windows 기존 인증 transcript는 바이트 단위로 보존한다.

## 추가할 의존성

target별로 고정 버전과 Cargo.lock을 검증한다.

| 의존성 | 용도 |
| --- | --- |
| `keyring-core =1.0.1` | OS 저장소 공통 인터페이스 |
| `apple-native-keyring-store =1.0.2`, `keychain` | macOS Keychain |
| `zbus-secret-service-keyring-store =1.0.1`, `crypto-rust` | Linux Secret Service |
| `ring =0.17.14` (기존 lockfile) | Secrets 내부 P-256 고정 형식 서명 |
| `libc =0.2.191` (기존 lockfile) | Unix peer credential·안전한 파일 열기 OS API |

`ring`과 `libc`도 새 직접 의존성으로 취급한다. `crypto-rust`는 Linux 저장소
crate의 기능 이름이며 별도 제품 라이브러리 이름이 아니다. 직접·전이 의존성의
라이선스와 최소 Rust 버전은 실제 lock 생성 시 재확인한다.
기존 `rusqlite =0.41.1` SQLCipher 기능은 Unix target에도 연결한다.
Unix Secrets는 Platform의 비밀값 없는 경로/파일 handle 검증을 재사용한다.
Platform은 Secrets에 의존하지 않고, 서명 인터페이스는 Contracts에 둔다.

## 구현 순서와 완료 기준

1. Windows v1 golden bytes 및 기존 계정 보존 회귀를 고정한다.
2. Unix 경로·프로필 검증·서명·OS 저장소·SQLCipher를 추가한다.
3. Unix 인증 v2와 AF_UNIX를 연결한 뒤 Core의 공통 workers를 활성화한다.
4. 동일한 Gmail/IMAP/SMTP 다계정 읽기·본문·작성·발송·휴지통, AI/MCP 흐름을 연결한다.
5. OS별 설치·업데이트·데이터 보존 제거와 native E2E를 통과한 조합만 배포한다.

각 OS에서 저장소 잠김/누락, symlink·다른 실행 이미지·PID 재사용,
과거 DB migration/FK, 발송·MOVE 응답 유실 후 unknown 무재시도 시험이 필요하다.
실제 로그인·비밀 입력·발송 시험의 기존 사용자 경계는 유지한다.

## 영향

HOST-02, INST-01/03, SEC-03, AI-07/09, APR-01, SYNC-07, EXT-03.
관련 코드는 `hantong-platform`, `hantong-secrets`, `hantong-contracts`,
`hantong-ipc-client/server`, `hantong-store`, Core 및 OS별 패키징이다.
세부 계약 제안은 [Unix 실행 계약](../../contracts/proposals/unix-cli-runtime-v2.md)에 있다.

## 검토 근거

- [Keychain Services](https://developer.apple.com/documentation/security/keychain-services/)
- [Secret Service 명세](https://specifications.freedesktop.org/secret-service/latest/)
- [keyring-core](https://docs.rs/crate/keyring-core/1.0.1)
- [Apple native store](https://docs.rs/crate/apple-native-keyring-store/1.0.2)
- [zbus Secret Service store](https://docs.rs/crate/zbus-secret-service-keyring-store/1.0.1)
- [공식 CI runner 목록](https://github.com/actions/runner-images)

대안인 직접 OS FFI만으로 비밀 저장소 전체를 구현하는 방식은 새 의존성을 줄이지만
메모리·D-Bus·Keychain 오류 처리의 자체 구현과 보안 검토 부담이 커 우선 선택하지 않는다.
