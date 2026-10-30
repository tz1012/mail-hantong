# IMAP 로컬 메타데이터 검색 v1

상태: M2-05/MAIL-03·CLI-01의 첫 로컬 메타데이터 목록 계약 초안. 기준선 `f70cdf3` 이후 작성하며 **사람 검토 필요**다. 이 계약은 `mail.messages.search`의 `imap_metadata` 모드만 정의한다. 통합 검색 문법, 본문 검색, 서버 검색, 다중 계정 검색, 전체 mailbox 동기화 완료는 구현하거나 주장하지 않는다.

## 범위와 권한

`mail.messages.search`의 이 모드는 한 IMAP 계정의 정확한 mailbox에 이미 저장된 현재 UID namespace 메타데이터를 읽는다. 네트워크, Secrets, worker, 자동 동기화, 자동 다음 page, 승인 발급, 메일 변경을 실행하지 않는다. 결과는 항상 로컬 cache 범위이며 원격 mailbox 전체 결과가 아니다.

이 도구는 기존 U 범위의 묵시적 확대가 아니다. 별도 명시 범위 M이 필요하며 M은 정렬된 정확한 도구 집합 `mail.accounts.list`, `mail.folders.list`, `mail.folders.refresh`, `mail.messages.search`, `mail.sync.page`다. 최초 등록과 기존 L/F/U 등록은 `mail.messages.search`를 허용하지 않는다. v4 scope 갱신과 native 영/한 확인, v15 DDL·migration·감사 도구 연결은 각 소유 계약이 정의한다. 이 문서는 그 변경의 승인이나 사람 보안 검토 증거가 아니다.

메일 헤더와 주소는 신뢰할 수 없는 입력이다. 정책, 권한, 승인, 다음 호출을 바꾸는 명령으로 해석하지 않는다. 결과를 이동·삭제·태그·발송 입력으로 사용하는 것은 별도 변경 계약과 승인을 요구한다.

## 공개 요청

공통 envelope의 tool은 정확히 `mail.messages.search`다. params의 정확한 필드는 다음과 같고 unknown, duplicate, missing, 잘못된 null을 거부한다.

| 필드 | 규칙 |
| --- | --- |
| `mode` | 필수 문자열, 정확히 `imap_metadata` |
| `account_id` | 필수 ASCII `[A-Za-z0-9_-]`, 1..128 bytes |
| `wire_hex` | 필수, 1..1024 raw mailbox bytes의 짝수 길이 소문자 hex |
| `encoding` | 필수, `modified_utf7` 또는 `utf8` |
| `filter` | 선택, `all`, `unread`, `favorite`; 생략 시 `all`, null 금지 |
| `limit` | 선택 정수 1..32; 생략 시 16, null 금지 |
| `cursor` | 선택 문자열 1..256 bytes; 생략 또는 null이면 첫 page |

계정은 정확히 하나만 받는다. `account_ids`, 전체 계정 별칭, query 문자열, timezone, 날짜, 본문, endpoint, UIDVALIDITY, UID, source operation, version/hash를 입력받지 않는다. `query-language.md`의 문법은 이 모드에 적용하지 않는다. 클라이언트가 cursor 내용을 해석해 요청 범위나 권한을 넓힐 수 없다.

`hantong_contracts::message_search`는 다음 순수 타입을 제공한다. 모든 필드는 private이며 `Clone`, `Eq`, `PartialEq`, redacted `Debug`와 아래 이름의 getter를 제공한다. 이 타입은 인증이나 승인 증거가 아니다.

- `ImapMetadataFilter::{All,Unread,Favorite}`와 `parse(&str)`, `as_str()`.
- `ImapMetadataSearchCursor::parse(&str)`와 `encode()`. getter는 `digest() -> &[u8;32]`, `source_cursor_version() -> u64`, `resume_uid() -> u32`, `expires_at_utc_ms() -> i64`다.
- `ImapMetadataSearchRequest::new(account_id:&str, mailbox:UidMailboxIdentity, filter:Option<ImapMetadataFilter>, limit:Option<u32>, cursor:Option<ImapMetadataSearchCursor>)`. getter는 `account_id`, `mailbox`, `filter`, `limit`, `cursor`, `account_ids`다.
- `ImapMetadataSearchItem::new(internal_message_id:&str, account_id:&str, uid:u32, internal_date_utc_ms:i64, size_bytes:u32, is_read:bool, is_favorite:bool, header_content_index:u32)`. getter는 같은 이름이다.
- `HeaderAddressProjection::new(host:Option<Vec<u8>>, mailbox:Option<Vec<u8>>, name:Option<Vec<u8>>, route:Option<Vec<u8>>)`. getter는 같은 이름이다.
- `HeaderProjection::new(subject:Option<Vec<u8>>, from:Option<Vec<HeaderAddressProjection>>)`. getter는 `subject`, `from`, `projection_json`이다.
- `ImapMetadataSearchPage::new(account_id:&str, mailbox:UidMailboxIdentity, catalog_generation:u64, binding_current:bool, observed_at_utc_ms:Option<i64>, items:Vec<ImapMetadataSearchItem>, headers:Vec<HeaderProjection>, next_cursor:Option<ImapMetadataSearchCursor>)`. getter는 같은 이름이다.

`ImapMetadataSearchPage`는 다음 구조 불변 조건을 검사한다. items와 headers 길이는 같고 0..32다. 각 item의 account는 page account와 같고 `header_content_index`는 item의 0 기반 위치와 정확히 같다. internal ID와 UID는 중복될 수 없다. `observed_at_utc_ms=None`이면 items/headers/next cursor가 모두 비어야 한다. `Some`은 양수 UTC milliseconds이고 catalog generation은 1..i64::MAX다. 같은 요청 snapshot에서 Store가 cursor를 만들었다는 사실은 순수 생성자가 주장하지 않으며, Store가 아래 digest·source membership 검증 뒤 page를 생성하는 것으로 보장한다.

비Windows의 request 준비·Store API는 기존 Windows 우선 제품 경계에 따라 `CAPABILITY_UNSUPPORTED`다.

## 내부 메시지 ID와 v15 매핑

외부 Message-ID는 식별자가 아니다. v1 `internal_message_id`는 현재 구현이 증명할 수 있는 **불변 IMAP 위치 식별자**이며 cross-mailbox 동일 메시지, thread, move/copy 후 동일성 또는 provider dedup을 뜻하지 않는다. 후속 논리 Message 계층은 발급된 v1 ID를 다시 쓰지 않고 alias/location mapping으로 보존해야 한다.

ID는 정확히 `msg_` 뒤의 소문자 SHA-256 hex 64자다. digest 입력은 다음 canonical bytes다.

```text
"mail-hantong.imap-message-location.v1\0"
|| u32BE(account_id byte length) || account_id ASCII bytes
|| u32BE(mailbox raw byte length) || mailbox raw bytes
|| u32BE(encoding ASCII byte length) || encoding ASCII bytes
|| uid_validity u32BE
|| uid u32BE
```

encoding ASCII는 정확히 `modified_utf7` 또는 `utf8`다. wire hex 문자열 자체가 아니라 검증 후 decode한 canonical raw mailbox bytes를 hash한다. 모든 길이 변환은 overflow를 검사한다.

v15는 `imap_message_identities`를 추가한다. `internal_message_id`가 PK이고 `(account_id,mailbox_key,mailbox_encoding,uid_validity,uid)`가 UNIQUE이며 같은 열의 `imap_uid_messages` PK를 `ON DELETE RESTRICT` FK로 참조한다. ID는 길이68, `msg_` prefix와 뒤 64개 소문자 hex를 DDL과 strict reader가 모두 검사한다. 전역 행 상한은 기존 base message 상한과 같은 1,000,000이며 자동 삭제하지 않는다.

v14→v15 prepared migration은 모든 기존 base message를 한 transaction에서 위 함수로 backfill한다. 기존 `metadata_json`, metadata/page/payload hash, operation, cursor, epoch, reconcile 결과를 재작성하지 않는다. mapping 수와 base message 수가 다르거나 ID/locator 충돌·orphan·hash 불일치가 있으면 원본을 보존하고 migration을 실패시킨다. INSERT 뒤 각 locator와 ID를 per-row readback하거나 locator+ID를 bounded streaming으로 다시 계산·대조해야 하며, count와 anti-join만으로 hash가 맞다고 간주하지 않는다.

v15 이후 direct append, append job, 최초 append, 일반 후속 append와 recovery 성공 transaction은 새 `imap_uid_messages` 행마다 mapping을 함께 삽입한다. reconcile은 base message를 만들지 않으므로 mapping을 만들지 않는다. mapping INSERT 실패는 page/message/cursor/epoch/job/audit 성공 전체를 rollback한다. exact replay는 mapping을 다시 쓰지 않고 strict 검증만 한다. 목록 read, replay, startup은 mapping을 lazy 생성·수정·복구하지 않는다. 기존 `read_operation`의 과거 page/hash 검증 의미는 바꾸지 않고, v15 identity strict 검사는 append/recovery/reconcile/search의 v15 callsite에서 추가한다. 이 slice는 ID 단독 조회를 공개하지 않지만 후속 get은 `internal_message_id` PK lookup으로 locator를 얻어야 하며, mapping miss를 locator 전체 scan이나 hash 재계산 fallback으로 숨기지 않는다.

## Header projection과 신뢰 경계

한 결과 item마다 `untrusted_content` header entry를 정확히 하나 만든다. item의 `header_content_index`는 top-level `untrusted_content` 배열의 같은 위치를 가리킨다. subject와 From이 모두 NIL이어도 entry를 생략하지 않는다.

`HeaderProjection::projection_json()`은 Serde 없이 fixed key 순서, 공백 없음, UTF-8 ASCII JSON을 만든다. 정확한 형태는 다음과 같다.

```json
{"from":null,"subject_hex":null}
```

또는:

```json
{"from":[{"host_hex":null,"mailbox_hex":"","name_hex":"eab080","route_hex":null}],"subject_hex":"7375626a656374"}
```

top-level key 순서는 `from`, `subject_hex`, address key 순서는 `host_hex`, `mailbox_hex`, `name_hex`, `route_hex`다. raw bytes는 소문자 hex이고 NIL은 null, 존재하지만 빈 raw string은 `""`로 구분한다. From NIL은 null이며 존재하는 From은 1..128 addresses다. 각 raw component는 최대8,192 bytes, subject와 모든 From component의 합은 최대65,536 raw bytes다. 임의 JSON 문자열이나 임의 key를 입력받지 않으므로 escaping 규칙을 확장하지 않는다.

Application이 만드는 각 untrusted entry의 정확한 필드는 다음과 같다.

```json
{"content_type":"header","content":"<projection_json>","byte_length":123,"truncated":false}
```

`byte_length`는 projection JSON 문자열의 실제 UTF-8 byte 길이다. 원문을 자르지 않으며 MIME encoded-word/RFC 2047, 주소 정규화, Unicode normalization, thread/message dedup을 수행했다고 주장하지 않는다.

item의 `internal_date_utc_ms`는 기존 Domain metadata와 같은 UTC millisecond 범위 -62,135,596,800,000..253,402,300,799,000이며 1,000으로 나누어떨어진다. Unix epoch 이전과 0은 유효하다. 이는 양수인 로컬 `observed_at` 저장 시각과 구분한다.

구조 item의 정확한 필드는 `internal_message_id`, `account_id`, `uid`, `internal_date_utc_ms`, `size_bytes`, `is_read`, `is_favorite`, `header_content_index`다. raw subject/address, Message-ID, UIDVALIDITY, endpoint, mailbox display name, custom IMAP keyword를 구조 item에 넣지 않는다. `is_read`는 effective flags에 `\seen`, `is_favorite`는 `\flagged`가 있을 때만 true다. 그 밖의 system/custom flag는 이 slice에서 반환하지 않는다.

## Store snapshot과 source page

`Store::search_imap_metadata(&ImapMetadataSearchRequest) -> Result<ImapMetadataSearchPage,ErrorCode>`를 추가한다. 입력 검증 뒤 하나의 Deferred read transaction에서 current account, current catalog와 exact selectable mailbox, current endpoint, strict global cursor, active epoch, optional current reconcile head, selected immutable source operation/page, message children, identity mapping, overlay를 읽는다. 서로 다른 public Store 호출을 Core에서 조합하지 않는다. 기존 strict cursor·epoch·head reader가 결속 경계를 확인하기 위해 수행하는 bounded decode는 허용하지만 전체 source page를 item마다 다시 decode하는 근거로 사용하지 않는다.

첫 요청에서 required current account, current endpoint, current catalog 또는 exact selectable mailbox가 없으면 `NOT_FOUND`다. cursor가 있는 continuation에서 같은 current entity가 없어졌거나 요청 mailbox가 더 이상 exact selectable member가 아니면 `CURSOR_EXPIRED`다. 어느 경우든 존재하는 persisted entity/link/hash가 malformed이면 없음으로 숨기지 않고 `INTERNAL_ERROR`다.

cursor가 없는 첫 요청은 active epoch에서 가장 큰 `result_cursor_version` source를 고른다. cursor 요청은 token의 source version과 exclusive `resume_uid` 위치를 사용한다. source는 active epoch의 `first_cursor_version..=global_cursor_version`에 속하는 정확한 `imap_uid_page_operations` 한 행이다. inserted_count가 0인 page와 messages가 빈 page도 version 순서에 포함한다. 한 호출은 선택한 source page 하나만 소비한다. 선택 source consumer는 그 operation의 canonical result metadata/page JSON 최대8MiB를 정확히 한 번 decode하고 그 결과를 child·hash·coverage 검증과 item projection에 재사용한다. item마다 같은 JSON이나 최대8MiB 자료를 다시 decode하지 않는다. 기존 reconcile strict reader를 재사용할 때도 operation ID로 다시 조회·decode하는 경로보다 이미 고른 source row/result를 인자로 받는 supplied-source parser를 우선한다.

선택 source는 operation/page/payload hash, account/mailbox/encoding/UIDVALIDITY/endpoint, result version, after/covered/UIDNEXT, inserted count와 child rows를 strict 검증한다. active epoch 첫 source는 after UID0 경계여야 한다. 그 밖의 source는 직전 version과 coverage가 이어져야 하며, 다음 version이 active epoch 안에 있으면 선택 source coverage와 다음 after UID도 이어져야 한다. version 누락, 중복, 잘못된 boundary, child 누락/추가, mapping 누락/추가, 다른 locator ID는 `INTERNAL_ERROR`다.

source page의 candidate는 UID 내림차순이다. internal date나 제목으로 정렬하지 않는다. `resume_uid`는 **다음 candidate에 적용할 exclusive upper bound**다. source 시작 값은 정확히 `u32::MAX`다. 그 밖에 발급 가능한 값은 선택 source에 실제 존재하는 UID이면서 그보다 작은 다음 candidate가 같은 source에 남아 있는 값뿐이다. candidate를 반환하거나 filter/`remote_absent`로 검사 완료하면 checkpoint를 그 UID로 내린다. count limit에 도달하거나 다음 완전한 item이 byte budget에 들어가지 않으면 그 candidate를 검사 완료하지 않고 직전 checkpoint를 보존하여 다음 호출이 같은 candidate부터 다시 평가한다. page를 일부만 소비하면 다음 cursor는 같은 source와 이 checkpoint를 담는다. page를 모두 소비하면 다음 작은 source version과 `u32::MAX`를 담는다. 더 작은 source가 없으면 next cursor는 null이다. `resume_uid=0`은 wire parser의 u32 문법상 유효하지만 Store가 발급하지 않으며, 후속 검증에서 0, UID gap, source 밖 UID, 또는 더 작은 candidate가 없는 실제 UID는 `CURSOR_EXPIRED`다. 따라서 items가 빈 page도 next cursor를 가질 수 있다.

다음 작은 source `v-1`로 이동하는 token을 만들 때는 그 source의 bounded scalar identity·operation ID·page hash·result version만 읽어 digest를 만든다. 다음 page의 전체 result JSON, children, identity mapping, overlay는 미리 읽지 않고 그 token을 실제 사용한 다음 호출에서 위 strict 규칙으로 검증한다.

Store는 selected source의 UID 최대64개에 대한 latest overlay를 한 bounded batch로 읽는다. 이 account/mailbox/encoding/active UIDVALIDITY에 scoped reconcile request·successful source result·overlay가 모두 없을 때만 **head 부재 상태에서** immutable base flags로 fallback한다. scoped reconcile 이력이 있는데 current strict head가 없거나 head/result 연결이 끊긴 경우는 `INTERNAL_ERROR`다. valid head는 있지만 선택 source의 successful result가 아직 없고 그 source의 overlay도 없으면 미대조 source이므로 base flags를 사용한다. latest successful source result가 선택 source를 관측했다면 그 source의 요구 overlay set이 반드시 있어야 하며, overlay의 `present` flags가 base를 대체하고 `remote_absent`는 결과에서 제외된다. latest result가 요구하는 overlay child 누락, success 없는 source의 orphan overlay, 잘못된 generation/head/result hash는 base flags fallback이 아니라 `INTERNAL_ERROR`다.

filter는 effective flags에 적용한다. `all`은 present candidate 모두, `unread`는 `\seen`이 없는 candidate, `favorite`는 `\flagged`가 있는 candidate다. 한 source에서 count limit에 도달하거나 byte budget의 다음 완전한 item이 들어가지 않으면 그 앞에서 멈춘다.

## Byte budget

limit은 item 수 상한이지 반환 보장이 아니다. Store는 다음 보수식이 524,288 이하인 item만 page에 추가한다.

```text
8,192 + SUM(each emitted item: 4,096 + 2 * header_projection_json_utf8_length) <= 524,288
```

위 식은 structural item의 `account_results` 복제, untrusted wrapper, cursor와 JSON-RPC 여유를 포함한 보수 한도다. Domain envelope의 raw 합계 상한과 fixed hex JSON으로 최악의 header 한 개는 빈 page budget에 반드시 들어간다. 이 보장이 깨진 입력은 조용히 생략하거나 무한 cursor를 만들지 않고 `QUOTA_EXCEEDED`다.

Application renderer는 완성된 **result object 전체**를 실제 직렬화해 524,288 bytes 이하인지 다시 검사한다. 넘으면 `QUOTA_EXCEEDED`이며 일부 JSON이나 truncated header를 반환하지 않는다. 인증 pipe의 전체 JSON-RPC frame과 IPC client 원본 응답 상한은 1MiB다.

## Cursor와 안정성

cursor wire는 정확히 다음 ASCII 형식이며 전체256 bytes 이하다.

```text
m1:<digest-lower-hex-64>:<source-result-cursor-version-decimal>:<resume-uid-decimal>:<expires-at-utc-ms-decimal>
```

정수는 부호와 선행0이 없는 decimal이다. source version은 1..i64::MAX, resume UID는 0..u32::MAX, expiry는 양수다. malformed token은 `INVALID_INPUT`이다. 첫 요청 시 Store UTC로 `expires=now+300,000ms`를 만들고 모든 후속 page가 같은 expiry를 보존한다. `now>=expires`는 `CURSOR_EXPIRED`다.

digest는 다음 snapshot/position 전체의 domain-separated SHA-256이다. account ID, status, provider, encoding, filter, source operation ID 같은 문자열과 raw mailbox는 각각 u32BE byte length 뒤 canonical bytes를 붙인다. version·generation·limit은 u64BE, UIDVALIDITY·resume UID는 u32BE, UTC milliseconds는 i64BE, boolean은 u8 `0|1`이다. endpoint/catalog/source hash는 length prefix 없는 raw32 bytes다. optional reconcile head는 presence u8 `0|1` 뒤에 present일 때만 generation/version u64BE를 붙인다. 다른 host-width나 JSON 숫자/string encoding을 사용하지 않는다.

```text
"mail-hantong.imap-metadata-search-cursor.v1\0"
account_id, mailbox raw bytes, encoding,
current account status + provider + version,
current endpoint version + endpoint hash,
current catalog generation + catalog snapshot hash,
stored cursor account/catalog/endpoint versions + endpoint hash,
global cursor version + cursor stored_at_utc_ms,
active UIDVALIDITY + epoch generation + epoch first cursor version,
optional reconcile head generation + version,
binding_current,
filter string + limit,
selected source operation ID + source page hash + source result cursor version,
resume UID + expiry UTC milliseconds
```

후속 요청은 같은 transaction snapshot에서 digest를 다시 계산하고 모든 현재 값과 exact source/position membership을 검증한다. account/mailbox/filter/limit/state/source/position 불일치, source가 현재 active epoch에 없거나 resume 위치가 source 범위와 맞지 않음, expiry는 `CURSOR_EXPIRED`다. next cursor는 권한, grant, 인증, 비밀 bearer token이 아니다. digest는 unkeyed integrity/state binding이며 MAC이라고 주장하지 않는다. 매 page마다 현재 등록, 정확한 M tool grant, 계정 scope, 정책을 공통 실행기에서 다시 검사한다.

## 공통 성공 응답

공통 response schema를 바꾸지 않는다. observed cursor가 있을 때 정확한 top-level 필드는 `schema_version`, `ok`, `items`, `next_cursor`, `account_results`, `completeness`, `warnings`, `observed_at`, `metadata`, `untrusted_content`다. 응답에는 mailbox echo, request digest, Store source operation/hash 또는 cursor digest의 별도 증명을 추가하지 않는다.

- `schema_version=1`, `ok=true`.
- `items`는 위 structural items 0..32개다.
- `account_results`는 정확히 한 행이며 `account_id`는 요청과 같고 `status="partial"`, `items`는 top-level structural items와 같은 순서/값이다. raw header bytes는 이 배열에 복제하지 않는다.
- `completeness="partial"`은 next cursor가 null이어도 유지한다.
- `warnings`는 정확히 `LOCAL_METADATA_ONLY`, 뒤에 binding이 stale일 때만 `STALE_BINDING`을 추가한다.
- `observed_at`은 current cursor의 **로컬 저장 UTC milliseconds**를 Domain 공통 UTC RFC3339 helper로 변환한 값이다. 서버 최신 시각이나 reconcile 시각이 아니다.
- `metadata`의 정확한 필드는 `provider="imap"`, `source="local_store"`, `cached=true`다. optional `latency_ms`는 이 응답에서 생략한다. `result_scope`나 임의 metadata 필드를 추가하지 않는다.
- `untrusted_content`는 items와 같은 길이/순서의 header entries다.

catalog와 exact selectable mailbox는 있지만 cursor가 없는 미관측 상태는 성공 result로 반환한다. items/account_results.items/untrusted_content는 빈 배열, next cursor와 observed_at은 null, account_results.status와 completeness는 `unavailable`, warnings는 `LOCAL_CACHE_UNOBSERVED` 뒤에 stale일 때만 `STALE_BINDING`, metadata는 위와 같다. cursor/epoch/operation/message/mapping 손상을 미관측으로 바꾸지 않는다.

cursor가 없는 정상 초기 상태에는 `expected_cursor_version=0`인 append 작업의 `admitted`/`running`/`failed` 이력과 독립 감사 행이 있을 수 있다. 그러나 recovery 작업, `expected_cursor_version>0`인 append 작업, 성공한 append 작업 또는 scoped reconcile head/request/overlay가 있는데 cursor가 없으면 `INTERNAL_ERROR`다. 초기 작업의 존재만으로 미관측 상태를 거부하지 않으며, 과거 성공·후속 작업의 흔적을 미관측으로 숨기지 않는다.

로컬 coverage가 observed ceiling에 도달했거나 마지막 source page여도 `complete`를 반환하지 않는다. 현재 자료는 원격 범위, body, 다른 mailbox, 다른 계정, query-language 전체를 증명하지 못한다.

Application은 `ToolRequest::MessagesSearch { request_id, request: ImapMetadataSearchRequest }`, `ExecutionContext::read_message_search(&ImapMetadataSearchRequest) -> Result<ReadResult<Self::Output>,ErrorCode>`, `render_imap_metadata_search(&ImapMetadataSearchPage) -> Result<ReadResult<serde_json::Value>,ErrorCode>`를 추가한다. 감사 성공 count는 실제 emitted item 수다. provider/worker/Secrets callback은0회다.

## IPC client와 CLI

`hantong_ipc_client::message_search`는 `search_default`, `parse_search_response`, `search_json`을 제공한다. parser는 authenticated JSON-RPC request ID와 원래 `ImapMetadataSearchRequest`를 함께 받아야 한다. client 전용 `ValidatedMessageSearchResponse`는 exact JSON-RPC2.0/request ID/result-or-error, 1MiB 원본 cap, duplicate/unknown/missing/null, 모든 item과 단일 account_result의 exact account ID, item/header/index/count/order, account_results structural duplicate, warnings/metadata/completeness, cursor 문법과 timestamp를 strict 검사한다. emitted count는 request limit 이하여야 하고 `unread` 결과는 모두 `is_read=false`, `favorite` 결과는 모두 `is_favorite=true`여야 한다. 응답에 없는 mailbox/source/digest를 검증했다고 주장하거나 opaque cursor의 Store hash·source membership을 client에서 재계산하지 않는다. UTC RFC3339 검증은 기존 accounts response의 `valid_timestamp`를 공통 private helper로 재사용하며 RFC3339를 milliseconds로 역변환하거나 새 시간 dependency를 추가하지 않는다. Debug는 계정, ID, header content를 숨긴다.

CLI는 한 번만 호출한다.

```text
hantong search --account ID --wire-hex HEX --encoding modified_utf7|utf8 [--filter all|unread|favorite] [--limit 1..32] [--cursor TOKEN]
```

CLI가 `mode=imap_metadata`를 고정하며 query/timezone/body 옵션이나 자동 page loop를 추가하지 않는다. 글로벌 `--lang en|ko`, `--json`, `--no-input`을 유지하고 영어가 기본이다. option duplicate/unknown/missing/null-equivalent와 잘못된 정수/token은 인증 전에 거부한다.

`--json`은 검증한 canonical response를 재직렬화하고 raw header projection을 decode하거나 재배치하지 않는다. 사람용 EN/KO 출력은 internal ID, UID, 시각, 크기, read/favorite와 subject/From의 안전 표시를 제공한다. 이 명령은 기존 범용 `safe_human`을 재사용하지 않고 metadata header 전용 byte escaper를 사용한다. 입력을 strict UTF-8 scalar sequence로 순회하되 invalid UTF-8 byte는 각 원 byte를 ASCII `\xHH`(대문자 hex)로 표시한다. backslash U+005C는 정확히 ASCII backslash 두 개 `\\`로 표시한다. 아래 forbidden scalar는 그 scalar의 원래 UTF-8 각 byte를 각각 `\xHH`로 표시하고, 그 밖의 valid visible Unicode는 원문 그대로 둔다.

- C0·DEL·C1: U+0000..U+001F, U+007F..U+009F.
- separator·bidi control: U+061C, U+200E..U+200F, U+2028..U+202E, U+2066..U+2069.
- 이 slice가 Unicode 15.1 기준으로 고정하는 그 밖의 format-control(Cf) 범위: U+00AD, U+0600..U+0605, U+06DD, U+070F, U+0890..U+0891, U+08E2, U+180E, U+200B..U+200D, U+2060..U+2064, U+206A..U+206F, U+FEFF, U+FFF9..U+FFFB, U+110BD, U+110CD, U+13430..U+1343F, U+1BCA0..U+1BCA3, U+1D173..U+1D17A, U+E0001, U+E0020..U+E007F.
- 선택된 invisible·reserved formatting scalar: U+034F, U+115F..U+1160, U+17B4..U+17B5, U+180B..U+180D, U+2065, U+2800, U+3164, U+FE00..U+FE0F, U+FFA0, U+E0100..U+E01EF.

위 predicate는 exact contract이며 Unicode 이름·폭·화면 렌더링 추정으로 런타임 확장하지 않는다. RFC2047/MIME encoded-word decode, 주소 정규화 또는 완전한 사람용 제목 해석을 주장하지 않는다.

## 필수 검증

- request/schema: mode, single account, canonical mailbox, filter/default, limit/default, cursor omitted/null, unknown/duplicate/type/range rejection.
- identity/migration: fixed hash vectors for case-sensitive raw mailbox, empty mailbox rejection, header NIL/empty preservation, v14 existing rows backfill와 per-row/streaming ID 재대조, count/anti-join만 맞는 잘못된 ID, orphan/collision/hash corruption, old JSON/hash bytes unchanged, rollback and quota.
- write paths: direct/append-job initial and later pages, recovery, exact replay, injected mapping failure full rollback, reconcile no mapping write, read no lazy repair.
- source: active epoch only, UID descending, first/recovery boundary, adjacent coverage/version/hash, empty/gap page, selected source 한 번 decode, next-source scalar-only token 생성, missing/additional child and mapping corruption, 첫 요청 required entity 없음 `NOT_FOUND`와 continuation 없음 `CURSOR_EXPIRED`, malformed persisted link `INTERNAL_ERROR`.
- overlay/filter: reconcile 이력 자체가 없을 때만 base fallback, scoped history인데 head 없음과 latest source success인데 overlay 없음 거부, latest present flags, remote_absent exclusion, unread/favorite, missing latest overlay child와 orphan overlay rejection, head change cursor expiry.
- page/budget: 1/16/32 limits, worst header one item fits, byte cap yields fewer than limit, no truncation, non-emitted budget candidate retried, filtered empty page progresses, final actual result cap.
- cursor: exact integer/hash/string/presence encoding vectors, account status/provider/binding change, every bound field/filter/limit/source/position/expiry mutation, `u32::MAX` source start와 actual checkpoint만 허용, 0/gap/outside/last UID 거부, five-minute expiry preserved across pages, malformed vs expired error, no authority use.
- response/client: exact common fields와 mailbox/source/digest 부재, structural-only account duplication, one header per item/index, NIL vs empty raw fields, partial/unavailable warnings, stale binding, no false complete, 1MiB raw cap, authenticated request/RPC ID·exact account·limit·filter predicate binding, opaque cursor source/hash 비검증.
- CLI: one exchange only, English default/Korean, JSON preservation, backslash 두 개, uppercase `\xHH`, 각 exact forbidden 범위 경계와 바로 바깥 visible scalar, invalid UTF-8/control/bidi/invisible escaping, no ANSI/CRLF injection, no automatic page or MIME decode claim.

시험은 synthetic Store/pipe만 사용한다. 실제 계정, 실제 네트워크, 실제 메일, 실제 사용자 DB, 비밀값을 사용하지 않는다.

## 저장 payload hash 상한

기존 Platform `sha256`의 인증 transcript 32KiB 상한을 유지한다. Store의 canonical page/catalog JSON(최대8MiB), message JSON 및 해당 payload framing은 별도 `sha256_storage_payload`를 사용하며 최대8MiB+4096 bytes를 허용한다. 초과 입력은 INVALID_INPUT이고 무제한 hash 경로를 추가하지 않는다. 두 함수는 같은 CNG SHA-256 primitive를 사용하므로 기존 payload/hash bytes는 변하지 않는다. source/identity/cursor의 작은 hash 입력은 기존 경계를 재사용할 수 있다. 이는 최악의 유효 header가 실제 저장 경계를 통과하도록 하는 수정이며 IPC frame 또는 인증 transcript 상한을 넓히지 않는다.
