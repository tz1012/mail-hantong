# Windows·macOS·Linux CLI/TUI 지원 구현과 검증

2026-11-04 사용자 지시: 기존 지원 대상인 Mac·Linux도 실제로 이용할 수 있도록 구현한다.
현재 공개 제품은 Windows beta.20이다. 아래 작업은 진행 중이며 완료 표시가 없는
OS에서 단순 빌드·TUI demo 성공을 메일 지원 완료로 안내하지 않는다.

## 공통 완료 조건

설치 → 실행 → Google/일반 IMAP·SMTP 계정 연결 → 목록·본문·봉투 정보 → 작성 →
터미널 내용 확인 → 발송·단일 휴지통 이동 → AI Codex/Claude와 MCP → 업데이트 →
데이터 보존 제거가 실제 OS에서 동작해야 한다. 다계정과 기존 계정 보존이 필수다.
별도 한통 GUI나 Windows Hello를 CLI/TUI 필수 흐름에 넣지 않는다.

표시 설정·번역·계정 별칭·통합 Inbox·키보드·마우스도 같은 범위다. 발송/Trash는
계정·내용·epoch 결속, 단일 claim, 감사, unknown 무재시도를 유지한다.

## 구현 순서와 현재 상태

| 묶음 | 상태 | 검증 게이트 |
| --- | --- | --- |
| Windows 구버전 업데이트 복구 | 안내 공개·합성 재설치 검증 | 회사 PC 결과는 별도 |
| Unix 전체 화면·inline 키 입력 | 구현·Linux native 검증 중 | 화살표·본문/뒤로·마우스·resize·모드 복원 |
| Windows 인증 형식 보존 | v1 golden fixture 추가 | 기존 canonical bytes 불변 |
| Unix 인증 v2 | 구현·회귀 보완 | strict codec·커널 peer·다른 이미지/PID 재사용 차단 |
| Unix 경로·프로필·소켓 | 기반 구현·Linux 합성 검사 통과 | Mac native 검사·실제 실행 검증 필요 |
| Keychain·Secret Service·opaque signer | 구현 중 | 합성 crypto·가짜 backend·잠김/누락·키 연속성 |
| Unix SQLCipher·Store | 기반 연결·native 검증 중 | 평문 대체 금지·과거 schema 이행·FK 검사 |
| Unix Gmail·IMAP·SMTP·Core workers | 드라이버 연결·Core 미통합 | 합성 제공자 E2E·복구·다계정 격리 |
| Unix CLI/TUI·AI/MCP 인증 연결 | 중계 계약 제안·미통합 | Secrets 경계·기능 전환·제공자별 외부 전송 설정 |
| Unix 설치·업데이트·제거 패키지 | 미완료 | 새 설치·상향·실행 중 교체·데이터 보존 |
| Linux headless/SSH | 후속 구현 필요 | 암호화 금고·잠금 해제·SSH/서비스 재시작 계약 |

## OS별 native 검증

- Windows x64: 기존 동작·계정·CNG·ACL·DPAPI를 유지한다.
- macOS 15 이상 Apple Silicon/Intel: 각각 native CI와 실제 Keychain/실행 시험을 수행한다.
- Ubuntu 24.04 x86_64 desktop: 실제 Secret Service와 사용자 런타임을 검증한다.
- headless/SSH: 데스크톱 Secret Service 성공으로 대체하지 않는다. 별도 암호화
  저장소 구현 전에는 원인을 명시하고 실패해야 하며, 평문 파일을 자동 사용하지 않는다.

WSL은 Linux compiler/PTY/합성 검사의 보조 환경이다. 데스크톱 Keychain/Secret Service와
실제 서비스 로그인 검증을 대신하지 않는다. CI의 native 검사는 아직 미구현인
제품 경로를 자동으로 완료 판정하지 않는다.

## 승인과 보존

[ADR-030](../adr/proposed/ADR-030-unix-cli-runtime.md) 및
[Unix v2 계약](../contracts/proposals/unix-cli-runtime-v2.md)의 구현·명시 의존성 추가는
사용자가 승인했다. 사람 보안 코드 검토는 별도로 필요하다. 기존 Windows 데이터·키를
변환하거나 내보내지 않는다. 새 OS에는 자체 OS 저장소를 사용한다.
실제 로그인·비밀 입력·발송/휴지통 시험은 기존 사용자 경계에 따라 별도로 진행한다.

실행 결과와 변경 사항은 [PROGRESS.md](PROGRESS.md)에 계속 누적한다.
