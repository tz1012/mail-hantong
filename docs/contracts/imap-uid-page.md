# IMAP UID 메타데이터 페이지 v1

상태: 2026-10-30 M2-04/EXT-04·SYNC-02의 제한된 선행 구현. 연속 개발 위임에 따라 작성하며 **사람 검토 필요**. 폴더 v10 기준선은 `a946075`다. 이 계약은 읽기 전용 새 UID 페이지와 원자 cursor 저장만 다룬다. 기존 메일 FLAGS/expunge 대조, UIDVALIDITY 변경 후 재동기화, 최근 메일 우선 최초 동기화, 본문/첨부, 공개 도구/자동 실행은 후속이다. 전체 mailbox completeness나 M2-04 전체 완료를 주장하지 않는다.

## 관측과 식별

영속 identity는 account_id + mailbox wire bytes/encoding + UIDVALIDITY + UID다. 기존 `ImapMailboxName`으로 이름을 검증하며 INBOX의 canonical wire 규칙을 유지한다. 표시명·Message-ID·응답 sequence number는 identity가 아니다. Request 생성 시 canonical wire+encoding에서 이름을 재구성해 INBOX 표시 대소문자를 의도적으로 버린다. 다른 encoding으로 같은 이름을 자동 연결하지 않는다. 등록·권한은 이 DTO가 발급하지 않는다.

새 private Secrets API는 `SecretStore::fetch_imap_uid_page(&ImapUidPageRequest, &ImapEndpointConfig) -> Result<ImapUidPage, ImapUidPageError>`다. `ImapUidPageError`는 `Probe(MailProbeError)`, `UidValidityChanged`, `CursorInvalid`이며 민감값이 없는 오류다. 비Windows는 Probe(UnsupportedPlatform). 요청의 endpoint_version은 endpoint.version과 같아야 하며 실제 비밀은 기존 Secrets 소유 TLS/AUTH driver 안에서만 읽는다. 임의 session/command/credential callback은 공개하지 않는다.

고정 흐름은 인증→post-auth CAPABILITY→EXAMINE→UID FETCH 한 번(필요한 경우)→EXAMINE 재검증이다. EXAMINE은 UIDVALIDITY/UIDNEXT와 READ-ONLY를 요구한다. UIDVALIDITY/UIDNEXT는 양의 u32이며, 요청 expected_uid_validity가 Some일 때 다르면 UidValidityChanged로 결과를 폐기한다. None이면 after_uid=0이어야 한다. Some(validity)+after_uid=0은 비어 있던 기존 cursor에 유효하다.

첫 EXAMINE의 UIDNEXT를 `uid_next_at_start`, ceiling을 UIDNEXT-1로 고정한다. after_uid>ceiling은 CursorInvalid다. `covered_through_uid=min(u64(after_uid)+64, ceiling)`이며 UID gap/빈 응답도 이 범위 전체를 관측한 것으로 기록한다. after==ceiling이면 FETCH를 생략한다. 나머지는 명시된 유한 범위 `UID FETCH <after+1>:<covered> (UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)`만 요청한다. `*`, UID SEARCH, BODY/RFC822 본문, SELECT, STORE, EXPUNGE는 없다. 마지막 EXAMINE에서 validity가 바뀌면 전체 페이지 폐기, UIDNEXT가 감소하면 CursorInvalid다. 증가하더라도 start ceiling을 바꾸거나 자동 반복하지 않는다.

FETCH는 UID/FLAGS/INTERNALDATE/RFC822.SIZE/ENVELOPE를 각각 정확히 하나 요구한다. 반환 UID는 (after,covered] 범위의 양의 u32이며 중복은 전체 실패다. 응답 순서는 바뀔 수 있으므로 UID순으로 정렬한다. 바깥 sequence number는 폐기한다. 누락 UID는 정상이며 본 slice에서 기존 행 삭제 근거로 쓰지 않는다. 범위 밖/UID 없는 FETCH는 거부한다. EXAMINE의 일반 상태·FLAGS·EXISTS·RECENT·UNSEEN·PERMANENTFLAGS는 제한된 문법으로 소비하되 영속 identity에 사용하지 않는다. 예상하지 않은 응답·tag/상태/필수 필드 중복·truncated/oversize는 전체 Protocol 실패이며 자동 재시도하지 않는다.

## 공통 Domain API와 상한

새 모듈은 `hantong_domain::imap_sync`다. 모든 validated 타입은 private fields, getter, Clone/Eq/PartialEq와 redacted Debug를 가진다. 내부 오류 `ImapSyncError::{InvalidInput,UidValidityChanged,CursorInvalid,TooLarge,InvalidStorage}`는 원문을 포함하지 않는다. 기존 serde/serde_json/jiff만 사용한다.

- `ImapUidPageRequest::new(account_id:&str, endpoint_version:u64, mailbox:ImapMailboxName, expected_uid_validity:Option<u32>, after_uid:u32)`. account ASCII ID 1..128, endpoint version1..i64MAX, Some validity>0. getters는 같은 필드명, mailbox는 &ImapMailboxName이다.
- `ImapEnvelopeAddress::new(name,route,mailbox,host)`는 각각 Option<Vec<u8>>를 받는다. getters도 같은 이름으로 Option<&[u8]>다. NIL과 빈 bytes, group marker 조합을 보존한다. 주소의 deliverability/신뢰를 뜻하지 않는다.
- `ImapEnvelopeInput`은 public 필드 date/subject/in_reply_to/message_id: Option<Vec<u8>>, from/sender/reply_to/to/cc/bcc: Option<Vec<ImapEnvelopeAddress>>를 갖는 unvalidated 입력이다. Default와 Clone, redacted Debug. `ImapEnvelope::new(input)` 검증 후 `fields()->&ImapEnvelopeInput`만 제공한다. None과 Some(empty bytes)는 다르며 Some(empty address list)는 거부한다. 주소/목록 순서와 raw bytes를 보존하고 MIME decode는 하지 않는다.
- 각 raw field 최대8192 bytes, 한 address list 최대128주소, envelope 전체 최대256주소/65536 raw bytes. NUL/8bit를 임의 손실 변환하지 않는다. 모든 예외/Debug/로그는 redacted다.
- `ImapMessageMetadata::new(uid:u32, flags:Vec<String>, internal_date_utc_ms:i64, size:u32, envelope:ImapEnvelope)`와 같은 이름의 getters. uid>0. INTERNALDATE는 초 단위 UTC epoch milliseconds(-62135596800000..253402300799000, 1000의 배수)로 변환한다. size0 허용. 날짜 표현/원문 주소는 identity가 아니다.
- FLAGS 최대64개/각128ASCII bytes/총4096bytes, RFC atom의 printable ASCII만 허용한다(공백·control·괄호·중괄호·%·*·quote·backslash·] 제외; 시스템 flag의 첫 backslash만 허용). 첫 backslash가 있으면 known `answered,flagged,deleted,seen,draft,recent`만 허용한다. lower-case 정규화 후 정렬·중복 제거하고 recent는 폐기한다. keyword/flags를 쓰기 권한으로 해석하지 않는다.
- `ImapUidPage::new(request:ImapUidPageRequest, uid_validity:u32, uid_next_at_start:u32, messages:Vec<ImapMessageMetadata>)`는 모든 결속/범위를 검사하고 UID순 정렬한다. getters request/uid_validity/uid_next_at_start/covered_through_uid/messages. 메시지 최대64개. covered는 고정 공식으로 도출하며 입력받지 않는다.
- Page `to_storage_json()->Result<Vec<u8>,ImapSyncError>`, `from_storage_json(&[u8])->Result<Self,...>`. Metadata에도 같은 codec. canonical JSON은 struct 필드 순서 고정, raw bytes는 소문자 hex 또는 null, flags는 정규 순서, message는 UID순, mailbox encoding은 modified_utf7/utf8 + wire_hex만 저장하고 이름을 재검증한다. Unknown/duplicate/missing/null scalar/비정규 정렬·hex·공백·키 순서는 거부한다(parse→재직렬화 byte equality). Page 전체 최대8MiB, metadata 최대256KiB. JSON 크기 검사 전에도 필드/주소 수/bytes 상한을 검사한다. schema_version=1을 최상위 첫 필드로 두며 Page 안의 각 Metadata도 같은 standalone 표현을 사용한다.

## Transport 메모리 경계

고정 imap-codec 2.0.0-alpha.9 / imap-types 2.0.0-alpha.7의 sequence-set 인코더는 단일 범위를 축약하고 최대 UID 근처의 끝을 `*`로 정규화한다. 따라서 typed 고정 FETCH 명령을 인코딩한 뒤 prefix/suffix를 검증하고, 그 사이의 UID token만 검증한 십진수 유한 범위로 복원한다. 전체 명령을 다시 decode해 원래 typed command와 같고 잔여 bytes와 `*`가 없는지 확인한 경우만 전송한다. 임의 command나 문자열 치환 API를 공개하지 않는다. 단일 UID와 u32 상한 근처 범위를 실제 in-memory 전송 시험으로 고정한다.

codec에 전달하기 전에 응답 frame128KiB, literal8192bytes, 괄호 nesting8, token/주소 수를 제한한다. 리터럴은 ENVELOPE nstring 자리만 허용하며 길이 선언 즉시 검증한다. 한 FETCH 최대64응답, 전체 페이지 응답8MiB, 상태 응답 각 exchange128개, 기존30초 deadline을 유지한다. 기존 probe/SMTP/LIST의 제한을 완화하지 않고 이 읽기 operation에만 별도 bounded stream 한도를 지정한다. 입력 chunk가 작아도 총량/깊이 제한이 같아야 한다. EOF/timeout/cancel은 session을 폐기하고 페이지를 반환하지 않는다. tests는 기존 in-memory TLS/격리 DPAPI fixture만 사용한다.

## v11 저장 계약과 API

Root가 v10 DDL을 고정하고 `010-to-011.sql`로 `imap_uid_cursors`, `imap_uid_messages`, `imap_uid_page_operations` 세 표만 추가한다. 기존 행은 변경하지 않는다. v10검증→암호화backup→DDL/metadata/user_version→v11검증은 기존 migration 원자성 규칙을 따른다. 실제 사용자 DB는 열지 않는다.

Store public 모듈은 `imap_sync`다. `imap_uid_page_payload_hash(expected_account_version:u64, expected_catalog_generation:u64, endpoint:&ImapEndpointConfig, page:&ImapUidPage, expected_cursor_version:u64)->Result<[u8;32],ErrorCode>`. `Store::apply_imap_uid_page(expected_account_version, expected_catalog_generation, endpoint, page, &MailMutation)->Result<ImapUidApplyReceipt,ErrorCode>`이며 mutation.expected_version은 cursor version이다. API는 내부 저장 경계이고 공개 정책 우회 경로로 연결하지 않는다.

hash는 `mail-hantong.imap-uid-page-payload.v1\0` + account_version u64BE + catalog_generation u64BE + expected_cursor_version u64BE + 기존 catalog endpoint_hash32 + page JSON length u64BE + page canonical JSON의 SHA256이다. page_hash는 canonical JSON의 SHA256. mailbox_key는 canonical wire bytes의 소문자hex, encoding은 별도 key column이다. operation_id는 전역 UNIQUE, (account,idempotency_key)는 UNIQUE. 다른 기능의 원장과 합치지 않는다.

Immediate transaction에서 strict operation reader로 exact replay를 먼저 판정한다. operation/key/account/mailbox/encoding/expected+result cursor versions/account+catalog+endpoint version/hash/page+payload hash/validity/coverage가 같으면 저장된 과거 receipt를 반환하고 현재 cursor/message를 변경하지 않는다. 다른내용/교차충돌은 CONFLICT, 손상은 INTERNAL_ERROR다. 이후 신규 적용만 현재 Ready IMAP account/version, exact endpoint/hash/version, current catalog generation/binding_current, exact selectable mailbox wire+encoding을 확인한다.

cursor absent면 expected_cursor_version=0, request.expected_uid_validity=None, after_uid=0. 기존 cursor면 expected version/Some(validity)/after==stored covered가 정확히 같아야 한다. 비어 있는 cursor도 endpoint hash/version 변경 시 CONFLICT이며 자동 reset하지 않는다. page.validity는 기존과 같고 UIDNEXT는 기존보다 작아질 수 없다. catalog generation은 이번 apply의 CAS 전제이며 cursor identity가 아니다. 관련없는 폴더 갱신 후 새 expected generation으로 같은 cursor를 이어갈 수 있다.

한 페이지 메시지 삽입+cursor 전진+operation receipt를 한 transaction으로 commit한다. 동일 UID의 기존 행을 덮어쓰지 않으며 exact operation replay 외의 기존 UID 충돌은 CONFLICT다. 이후 FLAGS 대조는 별도 경로다. empty page도 유효한 observation이며 cursor version을 +1한다. message_id 유일성 제약은 없다. quota는 전역 cursor100000/message1000000/operation100000이며 실제 신규 행 수를 쓰기 전 검사한다. 자동 prune/삭제는 없다. UTC 저장 시간이 같은 cursor보다 뒤로 가면 INTERNAL_ERROR다.

`ImapUidApplyReceipt` getters cursor_version:u64, covered_through_uid:u32, inserted_count:u32. `Store::load_imap_uid_cursor(account:&str,mailbox:&ImapMailboxName)->Result<Option<StoredImapUidCursor>,ErrorCode>` getters version/account_version/endpoint_version/catalog_generation/uid_validity/uid_next_at_start/covered_through_uid/stored_at_utc_ms/binding_current. 읽기는 current cursor·last_operation·hash·필드 관계를 다시 검증하고, binding_current는 계정/endpoint/현재 catalog의 해당 selectable 이름 결속만 뜻한다. 계정 설정 변경 후에도 과거 cursor를 보존하며 false를 반환한다. `Store::load_imap_uid_metadata(account,mailbox,validity:u32,uid:u32)->Result<Option<ImapMessageMetadata>,ErrorCode>`는 strict metadata JSON/hash/키/operation 관계를 검증한다. 모든 비Windows Store/hash API는 CAPABILITY_UNSUPPORTED다.

DDL의 필드/constraint는 SQL이 기준이다. v11 시작 검사는 외래키·표별 count 한도를 확인한다. cursor/operation/message의 canonical JSON/hash·연결은 해당 행을 읽거나 적용할 때 strict reader로 검증한다. 모든 과거 메시지 JSON을 시작 때 일괄 decode하지 않으며 미조회 행까지 무결성을 확인했다고 주장하지 않는다. operation에는 과거 exact replay 검증용 page_json도 보존한다. 정상 값으로 위장한 모든 DB 변조를 암호학적으로 증명한다고 주장하지 않는다.

현재 validity의 관측 완료 범위에서 metadata 행이 없으면 coverage index로 해당 UID를 관측한 operation을 찾아 snapshot과 자식 행을 검증한다. 정상 UID gap만 None이며, snapshot에 있던 메시지의 삭제·누락은 INTERNAL_ERROR다. 미관측 UID나 다른 validity는 None이다. 이렇게 누락 행을 서버의 정상 UID gap으로 오인하지 않는다.

## 병렬 소유권과 검증

Domain 담당: 새 imap_sync 모듈/순수 시험과 lib 모듈 선언. Transport 담당: private uid_page_gate/driver와 fixture, 최소 Secrets 연결. Store 담당: 새 imap_sync 모듈/시험만. root: 이 계약·DDL/migration/Store lib 연결·전체 회귀. 공통 API 변경은 root에게 먼저 보고하며 다른 담당자 파일을 바꾸지 않는다. 작성자와 별도 검토자가 identity/CAS/replay/범위·메모리/취소 경계를 대조한다.

필수 시험: UID gaps/empty/overflow·None/Some0·encoding/계정 경계, NIL/empty/group·raw byte 보존과 canonical 거부, hostile FETCH/중복/누락/범위/tag/literal/depth/EOF, 최종 validity 변경/UIDNEXT 증가·감소, plaintext fallback 없음, atomic rollback/exact replay/후속 cursor 보존/stale/CAS/endpoint/cross-account/quota/손상, migration 기존 v10행 보존/backup 충돌/DDL rollback. 전체 기능 완료나 실제 제공자 시험으로 확대하지 않는다.

근거: [RFC3501 UID](https://www.rfc-editor.org/rfc/rfc3501.html#section-2.3.1.1), [EXAMINE](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.3.2), [FETCH](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.4.5), [UID command](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.4.8), [유한 범위 문법](https://www.rfc-editor.org/rfc/rfc3501.html#section-9), [RFC4549 새 UID 관측](https://www.rfc-editor.org/rfc/rfc4549.html#section-4.3.1), [고정 io-imap0.7.1 소스](https://github.com/pimalaya/io-imap/tree/v0.7.1). 고수준 client를 노출하지 않고 ADR-006의 codec-only Secrets 경계를 유지한다.
