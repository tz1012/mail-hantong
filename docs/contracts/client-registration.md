# 기본 CLI 등록 요청과 진행 조회 v1

상태: 2026-10-30 연속 개발 범위의 구현 계약. 사람 보안 검토 필요. 관련 요건: CLI-15, MCP-03, SEC-03, INST-04. 등록 요청은 권한이나 발송 승인이 아니다.

## 등록 전 IPC

OS가 같은 사용자·로그온 세션으로 검증한 기존 로컬 pipe에서 다음 두 메서드만 추가한다. 기존 엄격한 JSON-RPC decoder, 프레임 상한, 5초 I/O deadline, 응답 ACK를 그대로 사용한다. 메일·auth 메서드를 등록 callback으로 전달하지 않는다.

| 메서드 | params 전체 |
| --- | --- |
| `registration.request` | `{"protocol_version":1}` |
| `registration.status` | `{"protocol_version":1,"request_id":"<32 bytes lowercase hex>"}` |

알 수 없는 필드·메서드, null, 타입 오류, 중복 필드, 전부 0인 request_id는 INVALID_INPUT이다. protocol_version의 다른 정수는 VERSION_MISMATCH다. CLI 명령은 `hantong clients register`, `hantong clients status <request_id>`이며 `--client default`만 지원한다. CLI는 요청/상태 조회만 수행하고 승인 결과·키·pin·scope·표시 이름을 입력받거나 발급하지 않는다. `--no-input`은 CLI 콘솔 입력 금지이며 등록을 승인하는 의미가 없다.

성공 result에는 다음 필드만 있다. 모두 필수다: `protocol_version:1`, `request_id`(32 bytes lowercase hex), `expires_at_utc_ms`(양의 i64), `state`(`pending`, `denied`, `expired`, `provisioning`, `ready`). 등록 ID, 키, 서버 pin, 계정 목록, Windows Hello 결과는 응답에 없다. 오류는 기존 안정적 코드만 반환하며 내부 오류 내용은 노출하지 않는다. 이 unauthenticated 상태 응답을 인증 증거로 사용하지 않는다. `ready` 이후에도 고정 경로의 프로필과 기존 CNG 키를 확인하고 서버 서명을 검증해야 한다.

## CLI 인증 확인

`hantong clients verify`는 고정 `default` 프로필을 읽고 기존 CNG 키의 식별자·공개값과 pinned 서버 서명을 검증하여 상호 인증을 수행한 뒤 연결을 닫는다. 프로필 경로·키·pin을 입력받지 않으며 `--client default`만 허용한다. 프로필/키를 생성·수정·복구하거나 등록 권한·정책·사람 승인을 발급하지 않는다. 보호된 메일 도구는 호출하지 않는다. 초기 `accounts list`는 읽기 정책 통합 전까지 기존 명시적 거부를 유지한다.

성공 시 일반 출력은 기본 영어 또는 `--lang ko`의 한국어 확인 문구다. `--json`은 공통 성공 envelope를 사용하며 `items`는 정확히 `[{"profile":"default","authenticated":true}]`다. `schema_version:1`, `ok:true`, `next_cursor:null`, `account_results:[]`, `completeness:"complete"`, `warnings:[]`, `observed_at:null`을 포함한다. 계정 목록·등록 ID·공개키·개인키·서버 pin은 출력하지 않는다. 실패는 기존 안정 오류와 종료 코드를 유지한다.

`--no-input` 여부와 관계없이 콘솔 입력이나 Hello를 요청하지 않는다. 누락되거나 신뢰할 수 없는 프로필, 기존 키 부재, 잘못된 서버 서명은 성공으로 표시하지 않는다. Windows 이외의 현재 빌드는 `CAPABILITY_UNSUPPORTED`를 반환한다. CLI 테스트는 비공개 주입 경계로 성공·실패를 검증하며 일반 사용자 프로필이나 실제 등록을 조작하지 않는다.

## 코어와 신뢰 창

코어는 현재 Store 계정 ID의 명시적인 snapshot으로 CLI/default/read-only/mail.accounts.list/분당 60회의 요청을 생성한다. 빈 계정 목록은 권한 0개다. 외부 입력으로 권한을 늘리지 않는다. 사람에게 보여 줄 요청·서버 공개 키 hash·만료 시각을 `registration-review.md`의 불변 snapshot에 결속한다. UI 완료를 기다리며 IPC 응답을 막지 않는다.

동시에 기본 등록 창은 하나만 연다. 현재 pending 요청에 대한 반복 request는 같은 ID와 원래 만료 시각을 반환하며 TTL을 연장하지 않는다. 창 닫기나 OS 취소는 승인/거절로 저장하지 않는다. 명시적인 새 request로 원래 pending 요청을 다시 검토할 수 있다. 재시작 시 pending 요청을 자동 승인하거나 창을 자동으로 열지 않는다. 이미 default 등록이 provisioning/active/suspended/revoked 상태면 새 등록은 CONFLICT이며 이전 공개 pin/키를 자동 교체하지 않는다. 완료 요청은 요청 ID로만 조회한다. 알 수 없는 요청 ID는 NOT_FOUND다.

`VerifiedRegistration`을 소비하여 현재 요청/버전/로그온/시각/서버 pin을 재검증한 뒤에만 키를 생성하고 Store stage를 호출한다. 승인됨+provisioning 저장 → 프로필 발행 → active CAS 순서다. 명시적인 Deny만 denied로 저장한다. 만료는 Store의 현재 시각으로 처리한다. 실패는 자동 승인·재시도로 바꾸지 않는다.

## 중단 복구

키 생성 전에 이미 저장된 32-byte 요청 ID를 내구성 있는 selector 예약으로 사용한다. 등록 ID는 `SHA256("mail-hantong/registration-id/v1\0" || request_id)`의 앞 16 bytes로 고정하며, domain의 끝은 문자열 역슬래시가 아닌 NUL byte다. 플랫폼의 `registration_id_for_request`가 계산하고 Store stage가 같은 값을 다시 검사한다. 임의의 새 등록 ID를 먼저 생성한 뒤 키를 만드는 경로는 없다. 요청 행은 정리 성공 후에도 결정 이력과 명시적 Deny를 유지한다.

시작 시 provisioning 등록을 제한된 수로 조회한다. 등록 ID에서 고정 selector를 계산해 기존 CNG 키를 열고, 공개 키·현재 서버 pin·고정 권한 snapshot을 검증한다. 키가 없으면 새 키를 생성하지 않는다. 프로필이 없으면 같은 공개 데이터로 발행하고 정확히 같은 canonical bytes면 재사용한다. 다른 내용은 덮어쓰지 않는다. publisher 성공 이후에만 Store active CAS가 가능하다. 실패한 항목은 provisioning으로 남고 인증을 거부한다.

프로필에는 개인 키나 승인 토큰이 없다. 발행 전 stage 실패에 한해 이번 시도가 막 생성한 키만 삭제한다. stage 이후 실패에서는 복구에 필요한 키를 보존한다. 모든 작업은 개발 중 가짜 Store/키/파일 경계와 격리된 OS fixture로 검증하며 실제 사용자 등록·Hello·메일 계정은 자동 조작하지 않는다.

시작과 명시적인 재시도에서는 최대 1,024개의 검증된 미등록 기본 요청을 조회하고 요청 ID에서 유도한 정확한 기존 selector만 연다. 키가 없는 경우에만 정리 성공으로 간주하며, 다른 OS 오류를 부재로 해석하지 않는다. 정리는 키를 생성하거나 등록을 활성화하지 않는다. 삭제 직전에 Store snapshot에서 요청·유도 등록 ID·실제 공개 키 중 하나라도 연결된 등록이 있는지 검사한다. 연결, 충돌, 손상 또는 조회 오류에서는 키를 보존한다. stage가 오류를 반환했더라도 다시 연결을 읽어 불확실한 commit으로 저장된 provisioning 키를 삭제하지 않는다.

정리 실패는 요청과 키를 보존하고 새 키 생성을 차단하며 다음 시작/명시적 재시도에서 같은 selector를 다시 확인한다. 공개 키를 읽을 수 없으면 소유권을 확인할 수 없으므로 삭제하지 않는다. 원래 stage 오류와 추가 정리 오류는 각각 안정적 오류 코드로 남기며 원래 오류를 바꾸지 않는다. 복구 목록 조회 실패는 시작 실패로 전달하고, 개별 항목의 실패는 provisioning/요청 상태를 보존하면서 `DiagnosticEvent`의 `operation_failed`와 안정적 오류 코드만 기록한다. ID·키·경로·메일 데이터는 진단에 넣지 않는다.
