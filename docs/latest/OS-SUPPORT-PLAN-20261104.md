# Windows·macOS·Linux CLI/TUI 지원 구현과 검증

2026-11-04 사용자 지시: 기존 지원 대상인 Mac·Linux도 실제로 이용할 수 있도록 구현한다.
현재 beta.22는 Windows x64, macOS 15 Intel/ARM, Linux
x86_64 (glibc >= 2.39) 자산을 포함한다.
Linux ARM 자산은 없다. 상세 설치는 [UNIX-INSTALL.md](UNIX-INSTALL.md)를 따른다.

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
| Unix 전체 화면·inline 키 입력 | native smoke 통과 | 화살표·본문/뒤로·마우스·resize·모드 복원 |
| Windows 인증 형식 보존 | v1 golden fixture 추가 | 기존 canonical bytes 불변 |
| Unix 인증 v2 | 구현·회귀 보완 | strict codec·커널 peer·다른 이미지/PID 재사용 차단 |
| Unix 경로·프로필·소켓 | macOS/Linux native 검증 통과 | 실제 제공자 작업은 별도 |
| Keychain·Secret Service·opaque signer | native CI smoke 통과 | desktop Secret Service 미검증·Mac SSH vault 미지원 |
| Unix SQLCipher·Store | native CI smoke 통과 | 평문 대체 금지·과거 schema 이행·FK 검사 |
| Unix Gmail·IMAP·SMTP·Core workers | synthetic provider smoke 통과 | 실제 제공자 로그인·발송·Trash 별도 |
| Unix CLI/TUI·AI/MCP 인증 연결 | startup smoke 통과 | AI provider install/login/capability 별도 |
| Unix 설치·업데이트·제거 패키지 | disposable install smoke 통과 | 최종 공개·서명/공증 별도 |
| Linux headless/SSH | protected headless smoke 통과 | Secret Service 없는 환경에서 vault 1 선택·TTY unlock |

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

## beta.21 외부 확인 반영 — historical — 2026-11-04

Linux 검토자가 설치 전에 태그를 확인했다. 인증정보 저장·암호화 DB·Unix 통신 기반은
추가됐으나 `apps/mail-hantong-core/src/main.rs::start`와
`apps/hantong-cli/src/core_start.rs::ensure_started`의 비-Windows 경로는 아직
`CAPABILITY_UNSUPPORTED`를 반환한다. 기본 IPC 상태 경로도 미연결이며 공개 자산은
Windows용뿐이다. 이는 설치 실패나 세 계정 연결 시험 실패가 아니라 **실행 가능한
Linux 배포본이 없어 설치 단계에 진입하지 못한 것**이다.

Linux에는 저장소 구현이 전혀 없다는 과거 설명은 beta.21 기준으로 정정한다.
반대로 기반 모듈·합성 인증 시험을 Linux 메일 이용 가능으로 설명하지 않는다.
데스크톱과 GUI/Secret Service 없는 headless·SSH를 각각 완료 조건으로 검증한다.
디스플레이 없는 환경에서 IMAP/SMTP 설정·조회·본문·작성·터미널 발송/휴지통 확인이
가능해야 하며, Google은 원격 브라우저의 표준 동의와 안전한 콜백 전달을 별도로 다룬다.

Ponytail 스킬 설치·내용 검증·개발 적용은 완료됐다. 이후 Unix 검증을 재개할 수 있으나
아직 연결하지 않은 실행 경로와 별도 미승인 보안 계약을 검사 성공으로 대체하지 않는다.
우선 연결해야 할 경계는 Core 소유 인증 중계(`unix-core-relay.md` 제안), 실제 Core의
저장소/identity/요청 dispatch/worker 수명, CLI의 패키지 검증·시작·연결과 OS별 설치다.
headless 보호 저장 방식은 ADR-030에서 분리한 별도 제안으로 검토한다. 평문 파일,
비밀값 argv/env 전달, 인증 생략 또는 기존 DB의 키 자동 재생성으로 우회하지 않는다.

## beta22 native verification — 2026-11-04

macOS 15 native CI run `41801224195` passed on both Intel and Apple Silicon,
including workspace Clippy, four SQLCipher tests, Core/socket/stop tests,
cross-process protected-stage checks and the line reader. Disposable package
installation passed first-run encrypted DB/status/stop/reopen using an unlocked
CI Keychain. Linux x86_64 (glibc >= 2.39) passed the installed headless and
GUI/DBus-absent smoke: protected first-start/menu, Core survival after CLI use,
encrypted DB, verified stop, wrong-unlock ciphertext preservation, reopen and
non-TTY `HumanStepRequired`. Windows preserves v1 behavior; isolated
install/uninstall, real pointer/UserPATH and data-sentinel proof passed.

These are startup, storage and synthetic-provider checks. Real Unix provider
login/send/Trash and AI conversation were not tested. Desktop Secret Service
remains unverified, macOS SSH vault is unsupported, and human security review
is pending. macOS signing/notarization is not claimed. Source metadata is
documented in [UNIX-INSTALL.md](UNIX-INSTALL.md).

beta.22 was published from commit `5eb5c93b63549bf8c01b668bd6d3a058e90ff066`.
All ten public assets were downloaded again without authentication and matched
the release checksums. The public Linux installer and interactive
`hantong update` both completed successfully in an isolated installation,
preserving synthetic data and unknown files. Windows final-package isolated
install/uninstall preserved the real installation pointer, User PATH and data.
The website deployment `41804688608` succeeded. These publication checks do not
replace the outstanding real-provider and human-security-review gates above.
