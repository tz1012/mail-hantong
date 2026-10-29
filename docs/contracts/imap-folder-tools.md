# IMAP 폴더 공개 도구 v1

상태: M2-03/EXT-06·CLI-01·SEC-03 연결 계약, 2026-10-30 연속 개발 위임. **사람 검토 필요**. 승인·발송·UID/메일 변경 권한을 추가하지 않는다. 기존 인증된 공통 실행 경로와 명시 account_id를 사용한다. 기본 CLI는 [범위 갱신 v2](client-scope-refresh.md)에서 native 확인을 거친 정확한 folder toolset만 허용한다.

## 목록 요청과 페이지

`mail.folders.list`는 공통 envelope(schema_version1, request_id, tool, params)를 받는다. params의 필수 account_id는 ASCII `[A-Za-z0-9_-]` 1..128 bytes, 선택 limit은 정수1..32(기본32), 선택 cursor는 문자열이다. unknown/duplicate 필드·null·빈 cursor는 거부한다. account_ids/전체 계정 별칭은 없다. 로컬 catalog만 읽고 네트워크를 호출하지 않는다. catalog 없음은 NOT_FOUND다.

cursor는 `v1:<account_id>:<generation decimal>:<after_wire_hex>`다. generation은 1..i64::MAX, 선행0/+부호 금지. wire_hex는 1..1024 bytes를 나타내는 소문자 hex다. 전체 길이 최대2220 bytes. 계정은 요청과 같아야 하며 generation이 현재 catalog와 다르면 CURSOR_EXPIRED다. after_wire는 현재 canonical 목록의 정확한 원소여야 하며 다음 원소부터 반환한다. 다른 계정/잘못된 encoding은 INVALID_INPUT, 같은 generation에서 없는 원소는 CURSOR_EXPIRED다. cursor는 권한이나 비밀 토큰이 아니다.

결과는 공통 성공 형태의 다음 정확한 필드다: schema_version1, ok=true, items, next_cursor(string|null), account_results(정확히 한 행), completeness(`partial` if next_cursor else `complete`), warnings, observed_at(UTC RFC3339 milliseconds), metadata. account_results의 한 행은 account_id, status(동일 completeness), items(상위 items와 정확히 같음)다. warnings는 binding_current=true이면 [], false이면 정확히 ["STALE_BINDING"]이다. observed_at은 catalog 저장 시각이다.

metadata의 정확한 필드: source="local_store", cached=true, account_id, generation(>=1), account_version(>=1), endpoint_version(>=1), binding_current(bool), special_use_complete=false. items는 canonical wire 순이며 최대 요청 limit개다. 각 item 필드는 wire_hex, display_name, encoding(`modified_utf7|utf8`), delimiter(ASCII printable 한 문자|null), no_select, non_existent, children(`unknown|has_children|has_no_children`), roles(Domain 고정 순서 all/archive/drafts/flagged/junk/sent/trash). wire/display는 [저장 계약](imap-folder-store.md)과 같은 의미/한도다. selectable은 !no_select&&!non_existent로 도출하며 별도 wire 필드를 만들지 않는다. 다음 cursor의 after_wire는 마지막 반환 item과 같아야 한다. 빈 page는 next_cursor=null이다.

서버는 JSON result를 최대1,040,000 bytes로 제한하며 IPC 전체 1MiB를 넘기지 않는다. 과대 결과는 QUOTA_EXCEEDED이며 조용히 자르지 않는다. client는 원본 JSON 스트림에서 duplicate/unknown/누락 필드와 잘못된 scalar/enum/시간·ID·계정/세대/순서·account_results 불일치를 거부한다. Debug·진단에는 이름/원문을 넣지 않는다. 터미널 출력은 기존 안전 escaping을 사용한다.

## 명시 갱신과 상태

`mail.folders.refresh` params는 action으로 구분한다. request는 정확히 action="request", account_id, operation_id, idempotency_key, expected_generation(0..i64::MAX-1), payload_hash(소문자64 hex)다. operation/key는 ASCII `[A-Za-z0-9_-]` 1..128 bytes. status는 정확히 action="status", account_id, execution_id(0이 아닌32 bytes 소문자 hex)다. 같은 tool 권한을 요구하며 status는 네트워크나 재개를 실행하지 않는다.

intent hash는 `mail-hantong.imap-folder-refresh-intent.v1\0` + account/operation/key 각각 u32BE 길이와 ASCII bytes + expected_generation u64BE의 SHA-256이다. 이는 [catalog 결과 hash](imap-folder-store.md)와 별개다. 공통 순수 타입이 intent bytes를 만들고 Windows platform SHA 구현을 사용한다. 다른 OS는 현재 CAPABILITY_UNSUPPORTED이며 약한 hash/평문 우회를 만들지 않는다.

request는 인증·재검증·audit start·scope·policy admission 후 단일 capacity-one Core worker로 전달하고 5초 IPC 제한 안에 receipt를 반환한다. receipt의 정확한 필드는 schema_version1, ok=true, account_id, execution_id, state(`unresolved|succeeded|failed`), generation(null|positive integer), error_code(null|고정 ErrorCode)다. unresolved는 generation/error 둘 다 null, succeeded는 generation만 양수, failed는 error_code만 고정 오류다. receipt는 발송 승인이나 완료 증거가 아니다. status는 동일 account와 소유 registration의 audit 작업만 조회한다. 없는/다른 작업은 NOT_FOUND다. 갑작스런 종료는 unresolved로 남기며 status/startup이 자동 네트워크 재실행하지 않는다.

worker는 네트워크 전과 저장 전 등록 active/epoch/tool/account·계정 상태/version·정확한 endpoint·catalog generation을 검사한다. 실행 중 철회는 future를 폐기한다. 성공 catalog+operation+audit terminal은 한 Store transaction으로 commit한다. 실패는 안정적 오류 terminal만 기록하고 이전 catalog를 보존한다. 전송/파싱 실패 자동 재시도 없음. queue full은 RATE_LIMITED terminal을 기록한다. Core의 공통 인증·정책·감사 경로를 거치며 직접 adapter를 호출하는 공개 우회 경로는 없다. operation replay/접수 중복과 상태 복구는 아래 v10 작업 이력에 결속한다.

## CLI

`hantong folders list --account ID [--limit N] [--cursor CURSOR]`, `hantong folders refresh --account ID --expected-generation N --operation-id ID --idempotency-key KEY`, `hantong folders refresh-status --account ID --execution-id HEX`다. 글로벌 --lang en|ko, --json, --no-input를 기존 방식으로 지원한다. 기본 영어와 한국어 리소스를 함께 제공한다. 잘못된 옵션은 인증 전에 거부한다. 자동 페이지 루프·자동 refresh·임의 Core 경로 입력은 없다. client는 현재 profile의 정확한 계정/tool grant를 확인한 후 한 번 교환한다.

공통 순수 request/cursor 타입은 `hantong_contracts::folders`에 두며 private fields와 redacted Debug를 사용한다. 이 타입은 인증 proof가 아니다. client의 typed parser와 CLI 테스트는 가짜 callback/원본 JSON fixture로 진행한다. Store/Core 연결 시험은 별도 담당자가 수행하며 실제 사용자 계정/Hello/메일은 사용하지 않는다.


## v10 영속 작업과 audit 소유권

공통 순서는 parse → 등록 재검증 → durable audit start → scope → policy다. list/status는 그 시도의 audit를 동기로 종료한다. request만 이미 시작된 audit ID를 Store admission에 넘긴다. admission은 그 audit가 unresolved, 정확한 folder refresh tool·단일 계정·현재 active 등록/epoch/grant인지 확인한다. 이 값은 외부 입력의 승인이나 인증 proof가 아니다.

Core는 capacity-one channel permit을 DB admission 전에 예약하고, 실행 중에도 별도 scheduled 값을 유지해 전체 활성 작업을 1개로 제한한다. Store는 같은 Immediate transaction에서 operation_id 또는 account+idempotency_key를 조회한다. 정확한 request/intent hash와 같은 소유 registration이면 기존 receipt만 반환하고 이번 시도 audit를 returned_ok로 종료한다. 이는 receipt 반환 성공이며 기존 작업의 성공을 뜻하지 않는다. 다른 owner·내용·교차 충돌은 CONFLICT로 이번 시도 audit만 종료한다. 중복에는 새 job·network·catalog 변경이 없다. 같은 owner의 나중 epoch도 현재 scope/policy를 통과하면 과거 receipt를 조회할 수 있다.

새 작업은 현재 Ready IMAP 계정 version·정확한 endpoint hash/version·catalog generation에 결속한다. 계정 설정 변경은 자동 보정하지 않는다. 유효한 binding에서 queue full/closed이면 failed job과 RATE_LIMITED/CORE_UNAVAILABLE audit terminal을 같은 transaction에 기록한다. binding/hash/권한 거부는 이번 audit terminal만 기록한다. 새 admitted job과 해당 audit 연결은 원자 저장한다. audit start 직후 crash는 unresolved audit만 남기며 effect는 없다. DB 오류나 불확실한 commit은 재시도/네트워크 실행을 유발하지 않는다.

`folder_refresh_requests`는 execution_id(PK/FK audit), operation_id UNIQUE, account_id, idempotency_key(계정과 UNIQUE), intent_hash, expected_generation, bound_account_version, bound_endpoint_version/hash, state, admitted/started/completed 시각, result_generation, error_code, catalog_operation_id, result_snapshot_hash, result_payload_hash를 가진다. 성공 행의 두 결과 hash는 32 bytes로 필수이며 다른 상태에서는 null이다. 성공 reader는 catalog operation의 모든 열·결과 hash와 job 결속을 대조한다. 현재 last operation은 실제 catalog snapshot도 재검증한다. 과거 결과의 원본 snapshot은 보관하지 않으므로 동시에 조작된 여러 행의 진본성을 hash만으로 증명하지 않는다. ID/hash/enum/시간 범위는 DDL과 strict reader 모두 확인한다. 보관 상한은 100,000개이며 audit 상한도 그대로다. 상태는 admitted → running → succeeded|failed이며 admitted → failed도 허용한다. wire에서 admitted/running은 unresolved다. 시작은 한 번만 가능하며 persisted running/admitted를 startup/status/replay에서 다시 enqueue하지 않는다.

worker 시작·실행 중(최대 500ms 간격)·결과 commit 직전에 등록 active/epoch/tool/account와 계정 Ready/version, endpoint hash/version, generation을 재검증한다. 철회/변경은 discovery future를 폐기하고 안정적 오류로 종료한다. 성공은 catalog+catalog operation+job succeeded+원래 audit success를 한 transaction으로 저장한다. 실패는 이전 catalog를 유지하며 job failed+audit error만 원자 저장한다. job 성공 audit의 result_count는 null이다. 동일 operation/key를 기존 direct catalog API와 job API가 각각 소비하지 못하도록 양방향 충돌을 검사한다. job completion의 내부 helper만 예약을 소비한다.

Store admission 결과는 Admitted/Existing/Failed(receipt) 또는 Rejected(error, 이번 audit 이미 종료)다. 외부 Result 오류는 반환된 소유권 인수가 없다는 뜻이며 불확실한 commit도 포함한다. Application은 명시 `AuditHandoff`를 받은 경우 audit를 다시 종료하지 않는다. handoff 전 오류만 공통 실패 종료를 시도하며 이미 종료된 경우 INTERNAL_ERROR로 fail closed한다. receipt는 private fields의 검증된 공통 타입이며 JSON 생성은 실패하지 않는 고정 필드 구성이다. 외부 입력이나 직접 adapter 호출로 이 경계를 우회하지 않는다.

v9→v10은 이전 DDL을 고정 보존하고 audit의 tool CHECK만 확장한다. 기존 audit 행·child 행·capacity·sqlite_sequence high-water를 보존하는 transaction으로 재구성한 후 빈 folder ledger를 만든다. 보호된 사전 backup, foreign_key_check, 기존 행/DDL 비교와 실제 실패 rollback을 검증한다. 기존 미완료 audit를 성공으로 바꾸지 않는다. 보안/계약 변경은 계속 사람 검토 필요다.
