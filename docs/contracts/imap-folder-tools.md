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

worker는 네트워크 전과 저장 전 등록 active/epoch/tool/account·계정 상태/version·정확한 endpoint·catalog generation을 검사한다. 실행 중 철회는 future를 폐기한다. 성공 catalog+operation+audit terminal은 한 Store transaction으로 commit한다. 실패는 안정적 오류 terminal만 기록하고 이전 catalog를 보존한다. 전송/파싱 실패 자동 재시도 없음. queue full은 RATE_LIMITED terminal을 기록한다. 아직 Core 활성화되지 않은 동안 이 계약/API가 권한 검사를 우회하는 직접 adapter 경로를 만들지 않는다. operation replay/접수 중복과 상태 복구의 세부 원자 결속은 Core/Store 통합 구현 전에 추가 검증한다.

## CLI

`hantong folders list --account ID [--limit N] [--cursor CURSOR]`, `hantong folders refresh --account ID --expected-generation N --operation-id ID --idempotency-key KEY`, `hantong folders refresh-status --account ID --execution-id HEX`다. 글로벌 --lang en|ko, --json, --no-input를 기존 방식으로 지원한다. 기본 영어와 한국어 리소스를 함께 제공한다. 잘못된 옵션은 인증 전에 거부한다. 자동 페이지 루프·자동 refresh·임의 Core 경로 입력은 없다. client는 현재 profile의 정확한 계정/tool grant를 확인한 후 한 번 교환한다.

공통 순수 request/cursor 타입은 `hantong_contracts::folders`에 두며 private fields와 redacted Debug를 사용한다. 이 타입은 인증 proof가 아니다. client의 typed parser와 CLI 테스트는 가짜 callback/원본 JSON fixture로 진행한다. Store/Core 연결 시험은 별도 담당자가 수행하며 실제 사용자 계정/Hello/메일은 사용하지 않는다.
