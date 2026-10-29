# IMAP 설정 요청·효과 저널 v1 (DB v5)

상태: SETUP-01/INST-09·M9-01의 신규 IMAP 계정 선행 구현. 사용자 연속 개발 위임, **사람 검토 필요**. 전체 마법사, 기존 계정 재설정/재개, 공개 IPC 활성화를 뜻하지 않는다.

## 범위와 소유권

Core가 생성한 신규 계정 설정 의도와 처리 이력을 암호화 Store에 보존한다. 등록된 client와 이미 존재하는 account가 필요한 일반 AuditBegin을 위조하지 않는다. origin은 `local_same_logon_bootstrap` 고정이며 Core의 동일 로그온 로컬 IPC 경계만 연결할 수 있다. 이 문자열, hash, 상태, Store 데이터 객체는 사람 확인 증거가 아니다. CLI/MCP는 저널 변경·완료·비밀 입력 API를 갖지 않는다.

비밀번호·credential nonce·Hello 결과·승인 bool·OS 원문·임의 오류 문자열은 저장하지 않는다. 사람이 확인한 native 결과를 현재 review로 한 번 소비한 Core만 stage를 호출한다. [설정 확인](imap-setup-review.md)의 request ID는 이 저널 ID를 사용하며, [stage hash](account-provisioning.md)는 draft/endpoint에 정확히 결속한다.

## 물리 스키마와 한도

v4 전체 DDL은 [schema-v4.sql](migrations/schema-v4.sql)에 고정한다. 3→4는 정확히 버전 4/고정 DDL을 검증하고, 별도의 [004-to-005.sql](migrations/004-to-005.sql)이 요청·이벤트 두 테이블과 인덱스를 추가한다. 기존 1→2→3→4→5 각 단계의 암호화 백업·전체 DDL 검증을 유지한다. v4 기존 계정에 가짜 요청·사람 확인 이력을 backfill하지 않는다.

`imap_setup_requests`는 0이 아닌 request ID(32 bytes), Core 생성 account/operation/idempotency ID, nullable display/color, version=1 app_password endpoint, 0이 아닌 stage hash, 고정 origin, phase/CAS version, created/expires/updated UTC를 저장한다. account ID에는 stage 전 FK가 없고 operation ID는 전역 유일, account/idempotency는 조합 유일하다. 진행 중 같은 account ID는 partial unique index로 겹치지 않는다. 내부 typed 생성자는 Domain/ID/hash를 검증하며 Store가 생성 시각과 5분 만료를 정한다.

`imap_setup_events`는 request FK, seq(1..8), 고정 kind, UTC, 선택된 결과 enum/안정 오류 코드/첫 probe outcome·알려진 capabilities·결과 버전을 가진다. `(request_id,seq)` PK, `(request_id,kind)` UNIQUE다. 일반 append/update/delete API는 없다. 요청 phase/version은 최종 이벤트와 일치하고 version=마지막 seq다. stage 이벤트는 신뢰 Core가 확인을 소비한 후 원자 생성했다는 처리 기록이며 독립 승인 발급물이 아니다.

보존 요청 최대 **1,024개**, 요청당 이벤트 최대 **8개**, 전체 최대 **8,192개**다. 초과는 QUOTA_EXCEEDED, 자동 삭제/eviction/조용한 잘라내기는 없다. 요청 ID를 먼저 검사해 exact create replay는 quota에서도 현재 검증된 snapshot을 반환할 수 있다. 같은 ID의 다른 payload, 다른 요청의 operation/key 충돌은 CONFLICT다. 새 요청은 이미 있는 account ID를 받지 않는다. 이 한도는 setup 이력 한도이며 계정 수 제품 한도와 별개다.

새 create transaction은 accounts 존재, 진행 중 같은 account ID, 기존 v4 mail_connection_operations의 operation ID 충돌도 검사한다. journal stage는 같은 Immediate transaction 안에서 account 미존재와 v4 operation ID 또는 account/idempotency 미존재를 다시 확인한다. 다른 경로가 create 이후 같은 대상/operation을 먼저 만들었으면 CONFLICT이며 public stage의 exact replay를 신규 journal 성공으로 채택하지 않는다. 이전 public stage의 replay 동작은 유지하고 journal 경계에서 새-only 조건을 추가한다. v4 원장 100,000행 한도도 별도로 유지한다.

## 전이와 원자성

| 이전 phase | 작업·이벤트 | 다음 phase | seq |
| --- | --- | --- | --- |
| 없음 | create / request_created | review_pending | 1 |
| review_pending | stage / account_staged | account_staged | 2 |
| review_pending | close / review_closed | review_closed (terminal) | 2 |
| account_staged | begin credential / credential_store_started | credential_store_started | 3 |
| credential_store_started | finish credential / credential_store_returned | credential_store_returned | 4 |
| credential_store_returned (returned_ok만) | begin probe / probe_started | probe_started | 5 |
| probe_started | finish probe / probe_finished | probe_finished (terminal) | 6 |

credential returned_error는 terminal로 취급한다. 나머지 미완료 phase는 중단된 위치를 나타낼 수 있으며, 저장된 started 자체는 다시 실행할 권한이 아니다. review_closed는 취소(APPROVAL_REQUIRED), 명시 거절(APPROVAL_DENIED), 만료(APPROVAL_EXPIRED), 또는 검토 실패의 허용된 안정 코드다. 취소를 명시 거절로 바꾸지 않는다. credential 결과는 returned_ok 또는 제한된 안정 오류뿐이며 probe 결과는 [첫 probe 계약](account-provisioning.md)의 다섯 enum/정렬된 capability 집합만 허용한다.

review_closed의 허용 코드는 APPROVAL_REQUIRED, APPROVAL_DENIED, APPROVAL_EXPIRED, APPROVAL_RATE_LIMITED, HUMAN_STEP_REQUIRED, CAPABILITY_UNSUPPORTED, SCOPE_DENIED, INVALID_INPUT, INTERNAL_ERROR다. credential returned_error는 INVALID_INPUT, SCOPE_DENIED, APPROVAL_EXPIRED, CAPABILITY_UNSUPPORTED, INTERNAL_ERROR만 허용한다. SEND_UNKNOWN·네트워크·정책/등록 오류를 credential 저장 결과로 혼용하지 않는다. 입력 생성자는 whitelist 밖을 INVALID_INPUT으로 거부하고, 저장된 위반은 INTERNAL_ERROR다.

모든 변경은 전체 expected snapshot(ID·불변 draft/endpoint/hash/operation/key·phase/version/시각·이벤트에서 나온 결과)을 현재 DB의 검증된 snapshot과 비교한다. 불일치는 CONFLICT, 없음은 NOT_FOUND, 저장 손상은 INTERNAL_ERROR다. snapshot은 권한이 아니다. 입력 오류와 저장 손상을 구분하고 대상/원문을 오류에 넣지 않는다.

- stage: BEGIN IMMEDIATE 안에서 요청 CAS, 기존 account/endpoint/operation 생성, account_staged 이벤트를 함께 commit한다. 기존 stage의 transaction body만 내부 helper로 재사용하고 public primitive의 동작은 유지한다. 신규 대상/중복/원장 검사는 같은 transaction에서 실행한다.
- begin credential/probe: 현재 connecting account v1, exact endpoint v1, observation 없음과 요청을 재검사하고 시작 이벤트를 **먼저 commit**한다. 그 commit이 이번 호출에서 새로 성공한 경우만 Core가 외부 효과를 수행한다. 같은 expected snapshot의 재호출은 CONFLICT이며 성공 receipt를 재전달하지 않는다.
- credential returned: 시작 이후 결과를 기록한다. DB 반환이 불확실하면 다음 효과를 시작하지 않는다. 성공 결과만 probe 시작을 허용한다.
- probe finished: 요청 CAS, 기존 첫 probe의 account/observation CAS, 결과 이벤트를 함께 commit한다. 계정 버전 2/endpoint 1/observation 1이 결과이며 기존 grants/SMTP/다른 계정은 보존한다. 결과 commit 실패는 전부 rollback한다.

create/replay 외에 변경 API는 idempotent success를 반환하지 않는다. 완료 응답이 유실되면 조회로만 확인하고 외부 효과를 자동 반복하지 않는다. 기존 public stage/finish와 동일한 rollback 경계를 유지한다.

## 시각·읽기·중단

Store UTC를 쓰며 모든 전이는 created/updated/event UTC보다 역행하지 않아야 한다. stage와 두 begin은 만료 미만일 때만 가능하고 만료/역행은 APPROVAL_EXPIRED다. 시작된 외부 효과의 returned/finished는 만료 이후에도 실제 결과를 기록할 수 있어야 하며 역행이면 INTERNAL_ERROR로 보존한다. review 만료 정리는 만료 이상에서만 허용한다.

한 요청 조회는 같은 Deferred snapshot에서 전체 request+최대 9개 event를 읽어 Domain/canonical endpoint/hash/enum/nullable 조합/연속 seq/전이/UTC/최종 phase를 재검증한다. 손상된 연결·순서·누락을 성공으로 잘라내지 않는다. 목록/시작 검증은 최대 1,025개 ID를 읽어 한도 초과를 검출한다. 반환 타입은 Clone/Eq가 가능한 자료이며 Debug는 target과 결과를 가린다. 공개 raw SQL·credential getter는 없다.

Store::open의 v5 검증은 모든 bounded history와 stage operation/account 연결의 구조적 무결성을 확인한다. operation completed 시각은 account_staged 이벤트 시각과 정확히 같아야 한다. 이후 정상 계정 편집을 과거 review와의 불일치로 오판하지 않는다. 외부 효과 직전에만 현재 연결 상태·endpoint snapshot을 정확히 다시 확인한다. 시작 검증은 데이터 수정·비밀 덮어쓰기·UI·네트워크를 실행하지 않는다.

중단된 요청과 connecting 계정은 보존한다. 명시적 재개에서 새 사람 확인/비밀번호를 받는 후속 계약 전에는 resume/reset을 노출하지 않는다. 이전 v4의 connecting 계정도 자동 재접속하지 않는다. 이 신규 전용 slice에 가짜 resume 성공을 넣지 않는다.

## 시험

fresh v5와 이전 전체 ladder, populated v4 보존, 백업 충돌/손상/rollback; create/replay/quota; 모든 순서·skip/역행/중복 start 거부; request/account/endpoint/operation/event 중간 실패 원자성; credential/probe commit 실패 뒤 자동 효과 0; exact 대상/버전/시각/만료; 이벤트 gap/unknown/nullable/hash/phase/UTC 손상과 reopen 거부를 격리 DB/합성값으로 검증한다. 비 Windows는 CAPABILITY_UNSUPPORTED다. 실제 사용자 DB·계정·Hello·네트워크는 시험하지 않는다.
