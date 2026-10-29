# 신규 IMAP 설정 요청·진행 조회 v1

상태: SETUP-01/SETUP-06·INST-05의 Core/CLI 단일 시도 연결 계약, **사람 검토 필요**. [설정 저널](imap-setup-journal.md)과 native 확인·Core 효과 순서에 따라 기존 로컬 pipe에 연결한다. 실제 사용자 Hello/계정 검증은 별도다.

## 요청과 형식

기존 OS 동일 사용자/로그온 검증, 엄격한 JSON-RPC 중복 필드 거부, 5초 I/O timeout, 응답 ACK를 재사용한다. 설정 요청/응답 frame은 16KiB다. 현재 인증용 최초 frame 상한 128KiB와 구분하며 dispatcher는 원본 설정 frame 길이를 callback 전에 검사해야 한다. Value 재직렬화 길이만으로 원래 공백을 포함한 한도를 검사했다고 주장하지 않는다. 새 계정 연결은 아직 등록된 CLI scope가 없을 수 있으므로 별도 bootstrap 작업이다. mail/auth 도구로 위장하지 않으며 등록과 설정이 같은 단일 UI 작업 queue/gate를 사용한다.

| 메서드 | params의 필수 전체 필드 |
| --- | --- |
| `setup.imap.request` | protocol_version=1, display_name(string 또는 null), color(string 또는 null), host(string), port(정수), tls(`implicit_tls` 또는 `starttls_required`), login_name(string) |
| `setup.imap.status` | protocol_version=1, request_id(0이 아닌 32-byte lowercase hex) |
| `setup.imap.resume` | protocol_version=1, request_id(0이 아닌 32-byte lowercase hex); [재개 계약](imap-setup-resume.md) 적용 |

account ID, operation/idempotency ID, 설정 version, hash, authentication은 Core가 정한다. 이 경로의 인증은 app_password로 고정한다. password/token/secret, approved, scope, endpoint version, 임의 경로 등 추가 필드는 INVALID_INPUT이다. CLI 인자·stdin·환경변수로 비밀을 받지 않는다. null/누락/중복/알 수 없는 필드, object 대신 배열, float/bool 정수, 잘못된 enum을 거부한다. 다른 protocol 정수는 VERSION_MISMATCH다. `setup.imap.complete/approve/execute`는 사람 확인을 대체하지 못하고 HUMAN_STEP_REQUIRED로 거부하며 callback을 호출하지 않는다. 미지원 reset 메서드는 INVALID_INPUT이다. resume는 기존 ID만 받으며 새 사람 확인을 요구한다.

전송 DTO는 target의 byte/문자 한도를 먼저 검사한다. display_name은 선택값, 존재하면 1..128문자/512 UTF-8 bytes이고 공백만·제어 문자를 거부한다. color는 선택 `#RRGGBB`. host 1..253 ASCII bytes, 공백·제어 문자 금지. port=1..65535, login=1..320문자/1280 bytes, 공백만·제어 문자 금지이며 정상 앞뒤 공백은 trim하지 않는다. DNS/IP 정규화와 실제 endpoint 유효성은 Core가 Domain의 기존 검증을 다시 적용한다. host를 임의 URL/명령으로 실행하지 않는다. DTO는 데이터이며 Debug에서 target을 숨긴다.

## 응답

성공 result는 `protocol_version:1`, `request_id`, 양의 i64 `expires_at_utc_ms`, `state`, `next_action` 다섯 필드만 포함한다. 모든 필드는 필수다. account/endpoint/login/operation/hash/nonce/Hello 결과/비밀은 반환하지 않는다. request/status/resume 응답은 인증 또는 승인 증거가 아니다. status는 요청 ID를 정확히 대조하며 resume는 검증된 chain의 새/현재 leaf ID를 반환할 수 있다.

| state | 허용 next_action |
| --- | --- |
| pending | helper_required |
| connecting | wait |
| ready | client_registration_required 또는 profile_scope_review_required |
| auth_required, tls_failed, offline | resume_required |
| error | resume_required 또는 new_request_required |
| denied, cancelled, expired | new_request_required |
| interrupted | resume_required |

`helper_required`는 사람이 native 확인 화면에서 이어갈 차례이며 완료 표시가 아니다. 실행 중인 job이 없는데 진행 중처럼 wait를 반환하지 않는다. 시작 뒤 중단된 작업은 interrupted/resume_required로 표시한다. 자동 재접속은 없으며 명시 resume와 새 native 확인을 거친다. ready여도 기존 CLI scope를 자동 확장하지 않으며 별도 등록/범위 검토를 안내한다. 일반 오류 envelope는 기존 안정 code와 같은 고정 message만 포함한다. 내부 원문이나 extra data는 반환하지 않는다.

client decoder는 원본 bytes의 strict map visitor로 envelope/result/error의 중복·알 수 없는 필드·필수 누락·positional array·모순되는 state/action·잘못된 ID/버전/시각을 거부한다. result와 error를 동시에 받거나 RPC ID/status request ID가 다르면 CORE_UNAVAILABLE다. 정상 안정 오류 code만 그대로 반환한다. 16KiB 초과/빈/trailing data/잘못된 UTF-8은 거부한다.

## Core 연결 순서와 검증

Core는 Domain 검증 후 자체 난수 ID와 stage hash로 저널 request를 만들고 단일 native queue에 넣는다. 요청/조회 I/O는 사람 확인을 기다리지 않는다. native plan/review request ID·대상·nonce를 결속하고 Verified 결과를 소비한 후 journal stage → credential 시작 commit → Secrets 저장 → 반환 commit → probe 시작 commit → Secrets 일회 probe → account/observation/journal 결과 commit 순서다. 어느 경계의 오류도 다음 효과 실행으로 바꾸지 않는다. status는 조회와 만료된 review의 제한된 종료만 가능하며 UI/credential/network를 실행하지 않는다.

codec 시험은 정상 roundtrip과 중복/누락/unknown/타입/경계/권한 필드, callback 0, 응답 privacy·상태/행동 조합·target Debug 마스킹을 검사한다. 기존 등록 codec/인증/mail 동작을 유지한다. 실제 pipe routing·단일 UI queue·Core 효과 순서·CLI 영/한 출력은 이후 통합 시험으로 구분하며, 실제 사용자 계정/Hello/메일은 개발 시험에 사용하지 않는다.


## Core/CLI 활성화 구현 범위

CLI는 `hantong setup imap request --host HOST --port PORT --tls implicit_tls|starttls_required --login LOGIN [--name NAME] [--color #RRGGBB]`, `hantong setup imap status REQUEST_ID`, `hantong setup imap resume REQUEST_ID`를 제공한다. 영/한 문구와 기존 JSON envelope를 사용한다. `--no-input`은 터미널 질문을 없애며 native 사람 확인을 승인하거나 건너뛰지 않는다. 다른 명령에 setup flag를 섞거나 duplicate/unknown/missing 값을 주면 모든 효과 전에 거부한다.

동일 UI queue는 등록/IMAP job을 구분한다. 다른 job이 있으면 RATE_LIMITED이며 요청을 추가하지 않는다. 진행 중인 IMAP 요청과 canonical endpoint·표시값이 같은 재요청은 같은 ID의 상태만 돌려주고 창이나 효과를 중복 실행하지 않는다. worker가 사라졌거나 시작 후 중단된 요청은 자동 재개하지 않는다. 영속 review pending이라도 현재 job이 없으면 interrupted/resume_required로 안내하고, 만료되면 제한된 review close로 expired를 기록한다. 명시 resume의 이력·계정 버전·새 확인·중복 방지 규칙은 재개 계약을 따른다. 전체 설정 마법사와 reset은 미구현이다.

Core의 Secrets 루트는 시작 때 검증한 AppDirectories.secrets로만 정한다. CLI·IPC 입력으로 경로를 받지 않는다. native VerifiedImapSetup 소비에서 얻은 불투명 pending만 실행 함수에 전달하며 가짜 native token 생성은 없다. 이 함수의 private 효과 경계는 실패 순서 시험에 사용하고 제품 구현은 Store/Secrets 하나뿐이다. 실제 probe 성공의 settings_version이 snapshot과 다르면 error로 기록한다. 인증 거부는 auth_required, TLS 거부는 tls_failed, 연결 실패/시간초과는 offline, 나머지는 error이며 capabilities는 알려진 6개를 정렬해 저장한다.

Core 종료/취소는 queue gate를 해제하지만 영속 started 사건을 성공으로 바꾸지 않는다. 조회는 UI/Secrets/네트워크를 부르지 않는다. Ready 응답의 다음 행동은 현재 default CLI 등록 존재 여부에 따라 신규 등록 또는 범위 재검토이며 자동 scope 변경은 없다.

기존 default 등록이 provisioning/suspended/revoked여도 새 등록을 중복 생성하지 않는다. `profile_scope_review_required`는 해당 등록의 상태와 범위를 다시 검토하라는 안내이며 활성/인증 성공을 뜻하지 않는다. 등록이 없는 경우에만 `client_registration_required`를 반환한다. 기존 등록 재검토 기능은 후속 구현이다.
