# IMAP UID 페이지 공개 도구 v1

상태: 2026-10-30 M2-04/EXT-04·SYNC-02·CLI-01 연결 계약. 연속 개발 위임에 따른 구현이며 **사람 검토 필요**. 기존 [UID 페이지 저장/전송 계약](imap-uid-page.md)에 공개 접수·상태 조회를 연결한다. 한 번의 명시 요청은 최대64 UID 범위만 관측한다. 본문·첨부·메일 변경·발송·자동 반복·전체 mailbox 완료 판정은 포함하지 않는다.

## 요청과 공통 타입

공통 envelope는 schema_version=1, request_id, tool="mail.sync.page", params다. 외부 JSON-RPC method와 tool은 같아야 한다. params는 다음 정확한 필드로 구분한다. unknown/duplicate/missing/null, 숫자 문자열·소수·음수, 다른 action의 필드는 거부한다.

| action | 정확한 params 필드 |
| --- | --- |
| cursor | action, account_id, wire_hex, encoding |
| request | action, account_id, wire_hex, encoding, operation_id, idempotency_key, expected_catalog_generation, expected_cursor_version, intent_hash |
| status | action, account_id, execution_id |

account_id/operation_id/idempotency_key는 ASCII `[A-Za-z0-9_-]` 1..128 bytes다. wire_hex는 1..1024 raw bytes를 나타내는 짝수 길이의 소문자 hex이며 encoding은 정확히 modified_utf7 또는 utf8다. expected_catalog_generation은 1..i64::MAX, expected_cursor_version은 0..i64::MAX-1이다. intent_hash는 소문자64 hex, execution_id는 0이 아닌32 bytes의 소문자64 hex다. 계정 목록이나 전체 계정 별칭은 받지 않는다.

Contracts의 순수 타입은 `uid_page::{UidMailboxEncoding,UidMailboxIdentity,UidCursorRequest,UidPageRequest,UidPageStatusRequest,UidCursorSnapshot,UidPageState,UidPageReceipt}`다. private 필드·검증 생성자·getter·Clone/Eq·redacted Debug를 제공한다. Contracts/IPC client/CLI는 Domain에 의존하지 않으며 mailbox 식별자를 어휘적으로만 검증한다. 서버 Application/Core 경계에서 Domain `ImapMailboxName::from_wire`로 검증한 후 canonical wire가 입력과 정확히 같은지 확인한다. 비정규 INBOX 표기를 자동 정규화하거나 encoding을 자동 연결하지 않는다. CLI는 folders list의 wire_hex/encoding을 그대로 사용한다.

외부 request에 endpoint/UIDVALIDITY/after_uid/range/limit/메일 metadata를 넣지 않는다. 서버가 현재 계정·endpoint·선택 가능한 catalog mailbox·cursor에서 도출한다. expected_cursor_version=0은 cursor가 없는 경우만 허용한다. 기존 cursor의 UIDVALIDITY/coverage/endpoint 결속 불일치는 CONFLICT이며 자동 reset/재동기화하지 않는다.

## Intent와 결과 hash

`UidPageRequest::intent_bytes()`는 `mail-hantong.imap-uid-page-intent.v1\0` 뒤에 account_id, operation_id, idempotency_key, encoding 문자열, wire_hex 문자열을 이 순서로 각각 u32BE byte 길이+ASCII bytes로 붙이고, expected_catalog_generation과 expected_cursor_version을 각각 u64BE로 붙인다. 이 bytes의 SHA-256이 intent_hash다. hash 필드 자체는 포함하지 않는다. 기존 Platform SHA-256을 사용하며 비Windows 준비/실행은 CAPABILITY_UNSUPPORTED다.

intent_hash는 접수하려는 의도의 결속이다. 전송 결과가 나온 뒤 생성하는 page_hash 및 Store의 결과 payload_hash와 서로 바꾸어 쓰지 않는다. 이 hash나 DTO가 인증·승인 proof를 발급하지 않는다.

## Cursor 조회

cursor는 로컬 저장소만 읽으며 네트워크·작업 접수·자동 재개를 실행하지 않는다. 성공 결과의 정확한 필드는 schema_version=1, ok=true, account_id, wire_hex, encoding, catalog_generation, cursor_version, covered_through_uid, binding_current다. catalog_generation은 현재 catalog의 양수 세대다. cursor_version은 0..i64::MAX, coverage는 0..u32::MAX-1이다. cursor 없음은 version=0과 coverage=0으로 표현한다. 손상된 cursor/operation/message 연결을 없음으로 바꾸지 않는다.

binding_current는 현재 계정/endpoint/catalog의 정확한 selectable mailbox 및 cursor 결속을 모두 만족하는지 뜻한다. cursor가 없더라도 catalog 결속이 낡으면 false다. catalog나 요청한 selectable mailbox가 없으면 NOT_FOUND다. false인 결과로 새 네트워크 작업을 접수하지 않는다. 응답은 UIDVALIDITY·endpoint·제목·주소·본문을 포함하지 않는다.

## 접수와 상태 receipt

request와 status의 성공 결과는 정확히 schema_version=1, ok=true, account_id, execution_id, state, cursor_version, covered_through_uid, inserted_count, error_code다. state는 unresolved/succeeded/failed다. 이 receipt는 전송 승인이나 전체 동기화 완료 증거가 아니다.

- unresolved: cursor_version/covered_through_uid/inserted_count/error_code 모두 null.
- succeeded: cursor_version은 1..i64::MAX, covered_through_uid는 0..u32::MAX-1, inserted_count는 0..64이며 coverage보다 클 수 없다. error_code는 null. 즉시 성공 응답은 요청의 expected_cursor_version+1과 같아야 한다.
- failed: 세 결과 필드는 null이며 error_code는 고정 ErrorCode다. UIDVALIDITY 변경/after 불일치는 CONFLICT다.

status는 같은 account와 소유 registration의 작업만 읽는다. 다른/없는 작업은 NOT_FOUND다. 상태 조회·과거 request replay·시작 시 미완료 작업을 다시 enqueue하지 않는다. client는 account와 RPC id를 확인하고, status에서는 execution_id까지 정확히 비교한다. 반환된 문자열을 오류/로그에 그대로 넣지 않는다.

Core 접수·worker·원장 구현은 기존 folder 공개 도구의 인증→등록 재검증→audit start→scope→policy→durable admission 순서와 소유권 규칙을 따른다. 명시 요청만 한 번의 bounded network operation을 실행한다. 등록/tool/account·계정/endpoint/catalog/cursor 버전 결속은 네트워크 전·실행 중·commit 직전에 재검증한다. 실패·취소·불확실 commit은 메타데이터를 부분 저장하거나 자동 재시도하지 않는다. 결과 메타데이터+cursor+operation+job/audit terminal은 원자 commit해야 한다. 별도 durable admission/DDL 부분은 해당 Store 통합 계약으로 기록한다.

## Wire와 CLI 한도

공통 bounded strict IPC decoder를 유지한다. 이 도구의 client 요청/응답 JSON-RPC 프레임은 최대8192 bytes다. 성공 결과에 raw mail이나 대형 배열이 없으므로 초과는 INVALID_INPUT으로 거부한다. client는 원본 JSON에서 중복·unknown·누락 필드와 null/scalar/enum 조합을 검사한다. 응답 DTO의 Debug는 redacted이며 터미널은 검증된 고정 상태와 숫자만 출력한다.

CLI:

```
hantong sync cursor --account ID --wire-hex HEX --encoding ENCODING
hantong sync page --account ID --wire-hex HEX --encoding ENCODING --expected-catalog-generation N --expected-cursor-version N --operation-id ID --idempotency-key KEY
hantong sync status --account ID --execution-id HEX
```

글로벌 --lang en|ko, --json, --no-input를 유지한다. 영어가 기본이며 영/한 resource를 함께 제공한다. CLI의 정수는 부호·선행0 없는 십진수이며 0 자체는 cursor version에 허용한다. 잘못된 옵션·중복·누락은 인증 전에 거부한다. 자동 cursor 조회·자동 페이지 루프·자동 retry·임의 Core 경로가 없다. profile의 정확한 mail.sync.page/account grant를 확인한 뒤 한 번 교환한다. 새 tool grant는 명시 범위 갱신과 native 확인 없이 기존 등록에 자동 부여하지 않는다.
