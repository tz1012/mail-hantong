# 충돌 복구·Unix 경로 보호: 사람 보안 검토 범위

최신 상태(2026-11-06): **사용자 검토 완료 확인 접수 / 잔여 조건 판단·조치 미기록으로 공식 배포 보류**. 사용자 원문 ‘검토 완료했어’와 검토 후 배포 승인을 [별도 확인 기록](HUMAN-SECURITY-REVIEW-CONFIRMATION-20261106.md)에 대상 커밋과 함께 보존했다. 아래는 당시 제시한 범위·조건이며, 빈 양식을 제출된 결과로 채우지 않는다. AI 리뷰와 CI 통과를 사람 검토로 대신 기록하지 않는다.

## 근거

- `AGENTS.md` §6: 보안 책임 경계 변경은 “테스트가 통과해도 에이전트가 완료로 표시하지 않는다”, `[사람 검토 필요]`를 붙인다. Rust 대응에는 `hantong-platform`, IPC 인증, Store 등록 전이와 Core 등록 처리가 포함된다.
- `AGENTS.md` §9: 완료 정의에 §6 영역의 사람 검토 필요 표시가 있다. 표시는 검토 완료 증거가 아니다.
- `docs/contracts/terminal-workflows.md` 첫 문단(7–9행): “Human security review required.” 터미널 선택은 의도이며 OS가 검증한 사람 존재 증거가 아니다. 같은 사용자 자동화 위험을 유지한다.
- `docs/adr/proposed/ADR-030-unix-cli-runtime.md` 및 AGENTS의 2026-11-04 승인 항목: Unix 보안 기반 구현 승인은 사람 보안 코드 검토 완료와 구분한다.

문서는 유료 심사자·특정 자격증·두 명의 사람을 요구하지 않는다. 실제 검토자가 고정 커밋을 읽고 판단·발견 사항·조치를 남겨야 한다. 사용자 배포 승인이나 AI APPROVE만으로 이 기록을 채우지 않는다.

## 고정 대상

- 기준 공개 버전 beta.29: `5d1e4734dfe1475c951f8d052079212e1437dc50`.
- 검토 대상 제품 코드 head: `6322b9a1a5f27f7e0ef32a65554ef20a79db0be6`. 후속 보고 문서 커밋은 이 제품 코드 대상과 구분한다. 문서만 추가하면 제품 소스 해시는 바뀌지 않는다.
- 제품 소스 해시: `e5fc5549f41ad5a5767279a3ab3ad63879f3a7cb72efee4014673ce2bfbadb8e`.
- 범위: `git diff 5d1e4734dfe1475c951f8d052079212e1437dc50..6322b9a1a5f27f7e0ef32a65554ef20a79db0be6`와 연결된 기존 Store·IPC·Secrets 호출부. 사용자 DB·키 내용은 자료로 요구하지 않는다.

아래 표는 주요 커밋 요약이다. 생략된 커밋도 위 전체 diff의 검토 범위에 포함된다.

| 주요 커밋 | 검토 범위 |
| --- | --- |
| `add6cbbf7d7a9073b290a42aef51d8353f74db95` | Core unix_scope_refresh/unix_setup/unix_registration: 불일치 항목명 진단, 정상 만료, 명시적 취소, 출처 구분, staged 복구 및 회귀 시험. |
| `22a083c082a69fed9cc78d8b6ab7d52477c262aa` | 플랫폼·Store fixture를 실제 OS 계약에 정렬. 릴리스 루트 symlink 회귀가 먼저 실패했고 후속 커밋으로 수정됐다. 이 시점 전체 검사를 통과로 기록하지 않는다. |
| `28b40a52d8c4b0b0e7a4420829489c80143e3576` | 릴리스 버전 루트 자체의 symlink·타입·쓰기 권한 검사. |
| `857e476a7ac8b28b95ce0790452600f9e5d67a9e` | unix_release/unix_paths/unix_crypto: 중간 경로 no-follow, 소유자·모드 확인, 디렉터리 핸들 기준 openat, 같은 열린 파일로 크기·해시 검증. |
| `5131622996b8453860302fe536cf490babffa423` | Linux 합성 headless 금고 공유, Unix fixture 0700·canonical 경로, Mac CI 전용 Keychain·복원 및 필수 Store 검사. 제품 금고 오류 처리는 유지. |
| `8204aa23f069558033085de3765928066c0b4ca2` | 병렬 fixture 전에 공개 Store API로 합성 DB 키를 한 번 준비한다. Mac은 명시한 시험 Keychain과 기본·검색 목록의 정확한 일치를 확인한 뒤에만 접근한다. Linux는 합성 headless 금고를 유지한다. |
| `012590f4ac04325aa15ee1cf77ef9f1824dc7497` | runner 할당 이후 GITHUB_ENV로 시험 Keychain 경로를 전달하도록 CI 설정을 수정한다. 제품 소스 해시는 직전 커밋과 같다. |
| `eb988a5a66ff168f049aa58962adbebfc3829dca` | Mac 전용 assertion을 참조 비교로 고쳐 엄격한 Clippy 경고를 해결한다. 검사나 경고를 억제하지 않는다. |
| `695524c646a395233aee82de4d7aeb22e8bce358` | Core 프로세스의 시험 금고·사전 키 준비를 공통화하고 Mac 프로필·릴리스 fixture의 병렬 경로 충돌을 원자 카운터로 제거한다. 제품 승인 코드와 시험 assertion은 유지한다. |
| `e37d7dfe58f0a9e927123f3e5f27b5ebd0e56a2f` | 지원 OS의 lifecycle 음성 시험과 실제 시작 gate를 구분한다. Mac CI의 테스트 실행 파일마다 실제 합성 Keychain을 분리하고 잠금·중단 처리·원상 복원·정확한 재검증을 적용한다. ACL 허용 확대나 제품 키 선택자 변경은 없다. |
| `42117910f07e59cae1273447db9dc9355bae5400` | Unix PTY 시험의 종료 대기 중 출력 읽기를 유지한다. 같은 8초 제한과 모든 탐색·화면 복원 assertion을 유지하며 512 KiB 합성 출력 회귀를 필수 실행한다. 제품 권한·메일 경로는 바꾸지 않는다. |
| `b6f6b814e1c54e30415f9fdc85cd34b00964d31b` | Unix 터미널 모드를 기존 libc의 전체 native termios로 저장·복원한다. 실패 경로의 복원 시도와 기존 입력 모드 의미를 보존한다. Windows 경로·권한·비밀 저장 형식은 바꾸지 않는다. |
| `a2c6a2c33e8eba5d3238dd4c1a214f819de84e2d` | 정확한 복원 실패에 합성 상태 필드 진단을 추가하고 Mac 패키지에도 실제 PTY gate를 요구한다. 비밀값·입력 내용은 출력하지 않는다. |
| `6322b9a1a5f27f7e0ef32a65554ef20a79db0be6` | 두 Mac에서 입증된 OS 자체 PENDIN 전환만 독립 native reference에서 유도한다. 전체 속도·CC·제어 플래그 원복을 요구하고 그 외 변화는 거부한다. 마스킹·skip 없이 7개 회귀를 추가한다. |

## 검토 항목

1. **취소 결속·단일 사용**: owner/OS peer, kind/request/hash/account selector/expiry, pending/channel/base identity 확인 후에만 Store CAS deny와 ticket 소비가 일어나는지 확인한다. 다른 등록·재사용·staged 취소를 거부해야 한다.
2. **만료 순서·접근 유지**: 정상 UTC·exact snapshot/version으로 만료한 후 불일치를 판정하는지 확인한다. 승인 전 계정·설정·키·서버 identity·active grants·profile은 유지되어야 한다.
3. **출처 구분**: terminal_cli 검토가 기존 google_oauth 등록 출처를 덮어쓰지 않아야 한다. OAuth 자동 완료가 터미널 pending을 승인하면 안 된다. Confirm은 정확한 대상 계정·도구를 표시한 불변 preview에 결속되어야 한다. 공개 만료 Cancel의 ticket pruning에 따른 CONFLICT와 내부 만료 guard 시험을 구분한다.
4. **staged 복구**: staged를 pending처럼 취소·만료·교체하지 않고 기존 stage→publish→activate의 key/pin/epoch/version/hash와 profile 복구를 유지하는지 확인한다.
5. **릴리스 보호**: 버전 루트·중간 bin·manifest의 symlink 및 다른 UID·쓰기 가능한 경로를 거부하는지, manifest target/caller/regular file/size/hash를 같은 열린 핸들로 확인하는지 검토한다. 비밀 파일의 정확한 0600 정책은 유지해야 한다.
6. **비밀 비노출**: 진단은 닫힌 불일치 항목명만 출력해야 한다. 주소·계정/요청 ID·메일·토큰·키·hash 값·하위 예외 문자열을 출력하지 않아야 한다.
7. **검사 강도**: 기존 symlink/loose-mode/oversize/unsafe path/hash/overlong socket 음성 시험을 유지한다. frozen v8 fixture와 현재 v27 56개 테이블은 해당 버전 계약을 검증해야 한다. ignore·삭제·실패 은폐가 없어야 한다.

연결된 Store expire/deny/stage/activate 트랜잭션, Unix profile publication/rollback, IPC peer·프로세스 결속과 Secrets 경계도 확인한다. 기존 ADR-030 기반의 사람 검토 기록이 있다면 연결하고 없으면 완료로 추정하지 않는다.

## 반드시 판단할 잔여 위험

열린 핸들로 릴리스 파일을 검증하지만 verified_installed_core는 실행 경로를 반환하고 이후 Command가 그 경로를 실행한다. 같은 UID가 검증과 실행 사이 경로를 교체하는 기존 TOCTOU 가능성이 남는다. 이번 수정은 다른 UID·symlink 우회를 막지만 이 창을 제거하지 않는다. AI 리뷰는 기존 MEDIUM 위험으로 기록했다. 실제 검토자가 위협 모델에서 허용 가능한지, 핸들 기반 실행 등 추가 설계가 배포 전 필요한지 명시해야 한다. 이 문서는 위험 수용 승인이 아니다. 미해결로 판단하면 배포를 보류한다.

Mac CI의 실행 파일별 Keychain 분리와 새 설치·같은 버전 재시작 시험은 버전 간 Keychain 접근 호환성을 증명하지 않는다. 이전 버전 Core가 만든 항목을 새 Core가 읽는 경로는 미검증이며, 배포 전 추가 시험 또는 명시적인 판단·조치가 필요하다. 이 문서가 위험을 수용한 것은 아니다.

## 필요한 기록·조치

아래는 미작성 양식이며 사람 승인 기록이 아니다.

- 검토자 이름/역할과 UTC 일시:
- 검토 기록 문서 커밋(제품 코드 대상과 별도):
- 기준·대상 전체 커밋, 제품 소스 해시, 읽은 파일·제외 범위:
- 항목 1–7 결과, 발견 사항 ID·심각도·재현·수정 커밋·재검사:
- 같은 UID 실행 교체 위험 판단·근거·필요한 배포 전 조치:
- 최종 네 플랫폼 전체 CI, 패키지·설치 증거 링크·아카이브 해시:
- 결론 Approve / Request changes / Reject 및 적용 커밋:
- 사용자 배포 승인 근거(보안 검토 결론과 별도):

발견 사항은 수정·검증 후 검토자가 다시 확인한다. 필수 CI 실패·미해결 보안 위험·사람 검토 미충족 중 하나라도 남으면 공식 tag/release/설치 안내 갱신을 보류한다. 합성 CI와 송파 실제 계정 검증은 구분하며 실제 권한을 자동 부여하거나 이전 승인 차단을 우회하지 않는다.
