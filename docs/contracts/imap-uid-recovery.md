# IMAP UIDVALIDITY 복구 기반 v1

상태: 2026-10-30 진행 중 M2-04/EXT-04·SYNC-02-AC02의 명시 복구 기반 구현 계약. 기준선 `5f27187`(v13, 전체1,069개 통과). 연속 개발 위임으로 작성하며 **사람 검토 필요**. M2-07을 선행 완료 전에 시작/완료 처리하지 않으며, 아직 구현하지 않은 로컬 초안·대기 변경의 보존 AC 완료도 주장하지 않는다.

## 범위와 권한 판단

서버에서 UIDVALIDITY가 바뀌면 같은 UID 번호는 다른 메일일 수 있다. 기존 namespace의 원본 page/message·관측 overlay·작업·과거 receipt를 보존한 채 새 namespace의 첫 metadata 한 페이지를 저장한다. mailbox 전체 cursor version은 N→N+1로 계속 증가하며 0으로 초기화하지 않는다. 같은 validity, 이전 retired validity 재사용, endpoint 변경, 실제 원격 쓰기·삭제·초안/대기 작업 재매핑은 이번 경로에서 허용하지 않는다.

복구는 별도 명시 요청 한 번당 첫 최대64 UID 관측 한 번만 수행한다. status/replay/startup은 자동 전환·접속하지 않는다. 기존 U의 계정별 한 페이지 metadata 관측/로컬 저장 권한 안에서 동작하며 L/F는 거부한다. 보존된 캐시의 namespace 전환을 발송/원격 대량 변경으로 간주하지 않는다. 이번 기능에 새로운 승인 발급 경로나 추가 native 확인 절차를 넣지 않는다. 발송·전달·대량 원격 변경의 기존 승인 불변 조건은 그대로 적용한다. APR-01은 승인 대상 호출의 결속 규칙이며 캐시 관측 자체를 승인 대상으로 지정하지 않는다. 실제 로컬 작성 내용의 재매핑·충돌 선택은 후속 별도 계약이다.

## Domain·Secrets API — 구현 기준

새 `hantong_domain::imap_recovery` 모듈은 기존 `ImapSyncError`를 사용한다. 모든 타입은 private fields, Clone/Eq/PartialEq/redacted Debug다.

- `ImapUidRecoveryRequest::new(account_id:&str,endpoint_version:u64,mailbox:ImapMailboxName,previous_uid_validity:u32)`. 기존 UID 요청의 account/endpoint/canonical mailbox 검증, previous validity>0. 같은 이름 getter를 제공한다.
- `ImapUidRecoveryPage::new(request:ImapUidRecoveryRequest,page:ImapUidPage)`. getters `request()`와 `page()`. page의 account/endpoint/canonical mailbox가 request와 같고 page validity가 previous와 달라야 한다. page.request는 `Some(page.uid_validity()), after_uid=0`이어야 한다. 기존 page codec/schema1과 metadata bytes는 변경하지 않는다. recovery wrapper는 별도 저장 JSON을 만들지 않으며 Store가 기존 page bytes와 job의 이전 결속을 함께 보존한다.
- `SecretStore::fetch_imap_uid_recovery_page(&ImapUidRecoveryRequest,&ImapEndpointConfig)->Result<ImapUidRecoveryPage,ImapUidPageError>`. 비Windows는 Probe(UnsupportedPlatform). 비밀은 기존 Secrets TLS/AUTH driver 밖에서 읽지 않는다. 임의 session/command/callback은 노출하지 않는다.

고정 흐름은 인증/post-auth CAPABILITY→EXAMINE→첫 유한 범위 UID FETCH 한 번(비어 있으면 생략)→EXAMINE이다. 첫 EXAMINE에서 UIDVALIDITY/UIDNEXT/READ-ONLY를 요구하며 validity가 previous와 같으면 CursorInvalid로 거부한다. 다른 validity의 UIDNEXT는 이전 namespace보다 작아도 정상이다. 새 validity와 UIDNEXT를 고정하여 기존 ImapUidPage 규칙의 `after=0, covered=min(64,UIDNEXT-1)`만 읽는다. 마지막 validity는 첫값과 같고 UIDNEXT는 첫값 이상이어야 한다. 바뀌면 전체 결과 폐기다. retired validity 재사용 검사는 Store가 원장으로 수행한다.

기존 다섯 metadata 필드와 UID 유일성/범위·예상하지 않은 응답 거부·유한 numeric command guard·frame128KiB/literal8192/depth8/총8MiB/30초 제한을 유지한다. sequence number·Message-ID로 old/new 메시지를 연결하지 않는다. TLS 실패·EOF·취소 시 결과를 반환하지 않으며 자동 재시도하지 않는다.

필수 Domain/Secrets 시험: old==new 거부, old→new에서 동일 UID의 다른 envelope 허용(다른 namespace), 이전보다 낮은 새 UIDNEXT 허용, 새 empty mailbox, 첫64개 범위 고정/UIDNEXT 증가, 최종 epoch 변경·UIDNEXT 역행, duplicate/out-of-range/unsafe unsolicited 응답, TLS 전 secret 접근 없음, 취소/deadline/총량, 기존 append/reconcile 회귀. 실제 계정·네트워크는 쓰지 않는다.

## 공개 타입·CLI — 구현 기준

기존 `mail.sync.page`에 `recover`와 `recover-status`만 추가한다. request 필드명·유형은 reconcile과 같고 cursor version1..i64MAX-1이다. 클라이언트가 endpoint·old/new validity·range·epoch generation을 전달하지 않는다. 요청/응답8KiB·공통 U scope/정책/감사·실행 중 철회 규칙을 유지한다. 기존 다섯 action의 wire/hash/의미는 바꾸지 않는다.

새 `hantong_contracts::uid_recovery`는 Domain에 의존하지 않는다.

- `UidRecoveryRequest::new(account_id,mailbox:UidMailboxIdentity,operation_id,idempotency_key,expected_catalog_generation,expected_cursor_version,intent_hash)`는 UidReconcileRequest와 같은 인자 타입/검증/getter/account_ids를 제공한다. `intent_bytes()`는 기존 순서와 u32BE 길이/마지막 두 u64BE를 유지하고 domain만 `mail-hantong.imap-uid-recovery-intent.v1\0`로 구분한다.
- `UidRecoveryProgress::new(epoch_generation:u64,cursor_version:u64,covered_through_uid:u32,inserted_count:u32)`: 2<=generation<=cursor<=i64MAX, coverage<=64, inserted<=coverage. 같은 이름 getter, private fields/Clone/Eq/redacted Debug다.
- `UidRecoveryReceipt::new(account_id:&str,execution_id:[u8;32],state:UidPageState,result:Option<UidRecoveryProgress>,error:Option<ErrorCode>)`. getter account_id/execution_id/state/result/error. 상태별 null 규칙은 reconcile과 같다. status 요청은 `UidPageStatusRequest`를 재사용한다.
- 응답은 정확히 `schema_version:1,ok:true,account_id,execution_id,state,result,error_code`이며 result는 null 또는 위 progress 네 필드 객체다. request 결과의 cursor_version은 요청 expected+1과 같아야 한다. status는 account/execution ID에 결속한다. 완료가 전체 mailbox 재동기화 완료라는 뜻은 아니다.

Application `UidPageAction::{Recover(UidRecoveryRequest),RecoverStatus(UidPageStatusRequest)}`, `ExecutionContext::request_uid_recovery(id,&request)->Result<AuditHandoff<Self::Output>,ErrorCode>`, `read_uid_recovery_status(&UidPageStatusRequest)->Result<ReadResult<Self::Output>,ErrorCode>`, renderer `render_uid_recovery_receipt`를 추가한다. 인증·현재 등록/계정 scope·정책·감사 흐름을 그대로 거친다. durable admission 시도 후 불확실 오류는 handoff로 처리해 두 번째 terminal을 쓰지 않는다.

IPC client 새 uid_recovery 모듈은 `prepare_recovery_request`, `recover_default`, `status_default`, `parse_recovery_receipt`, `parse_status_receipt`, `receipt_json`을 기존 reconcile API와 같은 인자 형태로 제공한다. CLI `hantong sync recover`는 reconcile과 같은 옵션, `recover-status`는 status와 같은 옵션이다. 영/한 도움말은 새 namespace 첫 페이지와 기존 이력 보존을 설명하며 승인 발급이나 전체 완료를 주장하지 않는다. 현재 U의 native 설명도 metadata 한 페이지 관측/복구와 원본 보존을 명확히 하되 도구 집합·기존 scope hash/history·권한은 변경하지 않는다.

## Epoch·Store 불변 조건 — 구현 기준

global cursor와 epoch boundary ledger를 결속한다. 새 첫 operation만 전환 경계에 결속되며 일반 append 경로는 validity 변경을 계속 거부한다. 기존 v13 행의 최초 epoch는 migration에서 보존적으로 기록한다. 과거 page JSON/hash와 source operation의 version은 재작성하지 않는다. reconcile head를 UIDVALIDITY별로 구분하고 현재 epoch의 첫 cursor version부터만 source를 선택한다. 이전 epoch의 receipt/overlay와 미완료 작업은 보존하되 새 namespace에서 재실행하지 않는다.

`imap_uid_epochs`는 전체 account/mailbox wire/encoding/UIDVALIDITY identity를 PK로 가진다. mailbox별 epoch_generation 및 first_cursor_version은 UNIQUE이고 last_cursor_version NULL인 current epoch는 mailbox당 하나뿐이다. 첫 operation ID는 기존 immutable page operation을 FK로 참조한다. 최초 epoch는 generation1/first_cursor_version1/recovery execution NULL, 이후 epoch는 generation>1/first_cursor_version>1/정확한 recovery execution FK다. endpoint version/hash·created_at, nullable last_cursor_version/retired_at을 보존한다. 식별·첫 경계는 불변이고 current→retired만 한 번 허용한다. 전역 epoch quota100000, 자동 삭제·prune는 없다.

retired 마지막 operation ID도 기존 page operation FK로 보존하여 역사적 coverage를 확인한다. `imap_uid_cursors`의 물리 schema/FK는 유지하며 strict reader가 active epoch 존재·동일 identity/version/endpoint를 필수 검증한다. 따라서 기존 base message의 cursor FK까지 불필요하게 재구성하지 않는다. epoch와 recovery job의 provenance 검증은 current cursor를 재귀 호출하지 않는 작은 내부 reader로 수행한다.

v14 이후 새 mailbox의 **일반 최초 append**(expected cursor0, expected validityNone, after0)는 page operation/cursor/message와 함께 generation1/first_cursor_version1/해당 first operation/recovery executionNULL의 initial epoch를 원자 생성한다. direct apply와 append job이 공유하는 private 성공 경계에서 수행하고 epoch quota를 쓰기 전에 검사한다. 어느 쓰기나 마지막 strict reader가 실패하면 전체 rollback한다. 기존 최초 append exact replay는 epoch를 다시 만들거나 중복 계수를 늘리지 않는다. 따라서 migration backfill이 없는 새 mailbox도 active epoch 불변 조건을 만족한다.

v13 backfill은 기존 cursor마다 operation 수·최대 version·최소 version이 1..N 연속인지, operation/message/reconcile head의 scalar validity가 cursor와 같은지 확인한다. 최초 operation과 cursor 결속을 검사하고 최초 epoch를 생성한다. 불일치 시 원본을 보존하고 migration을 실패시킨다. 모든 과거 대용량 page JSON을 일괄 decode했다는 증거로 간주하지 않는다. root가 frozen schema-v13와 v14 fresh/migration DDL 및 보존/실패 시험을 소유한다. Store 담당자의 새 표·재구성 표 DDL 제안을 검토해 db-schema.sql과 013-to-014.sql에 통합했다.

reconcile head의 PK에 UIDVALIDITY를 포함하고 부모 FK는 **mutable current cursor가 아닌 epoch identity**로 옮긴다. request FK도 해당 validity의 head를 참조하며 message overlay FK는 과거 원본을 계속 참조한다. head에 `epoch_first_cursor_version`을 결속한다. 초기 last_source는 first-1이며 그때만 last_execution이 NULL이다. source scan은 (last_source,ceiling]의 해당 epoch에 한정한다. 기존 v13 head는 first=1로 이전하며 나머지 값/결과 JSON/hash는 보존한다. 새로운 값은 과거 receipt/result JSON 형식을 바꾸지 않는다.

strict cursor reader는 mailbox 전체 operation count/MAX==global version을 유지한다. active epoch는 current이고 cursor validity/version/endpoint와 같아야 한다. 직전 operation의 validity/coverage/UIDNEXT 연속성 예외는 **현재 operation이 정확한 새 epoch 첫 operation인 경우만** 허용하며, previous epoch 마지막=N, first=N+1, after0, recovery job/audit/operation/hash 결속을 검증한다. 같은 epoch의 일반 append에는 기존 연속성 검사를 모두 유지한다. read_operation은 epoch reader를 재귀 호출하지 않아 순환 검증을 만들지 않는다.

`load_imap_uid_metadata(account,mailbox,validity,uid)`와 observation getter는 명시 validity의 epoch 경계로 원본을 읽는다. retired epoch에서는 마지막 cursor version/coverage를 사용한다. 관측 범위의 child 누락을 정상 gap으로 숨기지 않으며 과거 receipt는 당시 source/epoch로 검사한다. current cursor와 옛 validity가 다르다는 이유만으로 유효한 과거 결과를 손상으로 판정하지 않는다.

새 `hantong_store::uid_recovery_jobs`는 `UidRecoveryAdmission::{Admitted,Existing,Failed}(UidRecoveryReceipt)/Rejected(ErrorCode)`, `UidRecoveryStart::{Ready(Box<UidRecoveryJob>),Finished(UidRecoveryReceipt)}`를 제공한다. Job getter execution_id/request/fetch_request/endpoint. Store API `admit_uid_recovery(id,&UidRecoveryRequest,UidQueueAdmission)`, `start_uid_recovery(id)`, `check_uid_recovery(id)`, `complete_uid_recovery_success(id,&ImapUidRecoveryPage)`, `complete_uid_recovery_failure(id,error)`, `load_uid_recovery_receipt(owner,&UidPageStatusRequest)`는 기존 작업 소유권·오류 규칙을 따른다. 비Windows는 CAPABILITY_UNSUPPORTED다.

recovery job은 현재 old epoch generation/validity·global cursor N·account version·endpoint/hash·catalog generation·공개 request/intent·원래 audit/owner를 결속한다. 접수·시작·500ms마다·commit 전에 현재 상태와 비교한다. 기존 cursor가 없으면 NOT_FOUND이며 일반 초기 append를 대신하지 않는다. 새 validity가 old와 같거나 이 mailbox의 retired epoch에 이미 있으면 CONFLICT다. old와 다른 new UIDNEXT를 old UIDNEXT와 비교하지 않는다.

현재 Ready account version과 현재 catalog 결속은 접수 시 새로 도출한다. 재인증 후 account version만 증가했다면 과거 cursor의 account version이 다르다는 이유로 복구를 영구 차단하지 않는다. 대신 old cursor/epoch의 endpoint version/hash가 현재 endpoint와 정확히 같아야 하며 old namespace/CAS/현재 계정과 catalog를 검증한다. 새 page/cursor에는 현재 account version을 저장한다. 실행 중 이 결속이 바뀌면 무반영 실패다.

성공 한 transaction은 새 page operation(기존 v1 payload/hash, expected N/result N+1)과 metadata 삽입, old epoch retirement at N, new epoch 생성 first N+1, current cursor 변경, job/result와 audit terminal을 모두 원자 반영한다. 실패·quota·시계 역행·중간 쓰기 오류는 부분 namespace 전환을 남기지 않는다. 일반 append에 validity 우회 boolean을 공개하지 않는다. 검증된 recovery job의 성공 경계 내부에서만 private transition helper를 호출한다. job은 원래 page operation/hash/payload 및 새 epoch generation을 보존하여 receipt를 다시 계산하고, 원래 응답 유실 후 재요청도 기존 결과만 반환한다.

새 epoch가 recovery job의 성공 provenance를 요구하므로 최종 strict cursor 검사는 job/audit 성공 행까지 쓴 뒤 같은 transaction 안에서 수행한다. 그 전에는 private transition helper가 쓰기 순서만 수행하며 외부에 부분 결과를 반환하지 않는다. 마지막 검사 실패는 INTERNAL_ERROR rollback이고 두 번째 terminal을 시도하지 않는다.

direct append, append job, reconcile job, recovery job의 operation ID/account+key/execution 예약은 모든 방향에서 검사한다. 성공한 append/recovery job과 그 자체 page operation의 공동 소유만 허용한다. 반대 원장 충돌 검사는 own exact replay의 조기 반환 전에 한다. 원래 unresolved audit ID를 다른 접수에 재사용해 종료하지 않는다. 새 namespace 활성화 뒤의 옛 pending/running 작업은 현재 CAS/epoch 검사로 차단되며 startup에서 재실행하지 않는다.

한 v14 migration에 epoch 기반과 영속 recovery job을 함께 연결한다. Store 내부의 synthetic page 전환/과거 보존 시험을 먼저 통과시킨 뒤 Core에 연결하며, 기반/공개 활성화마다 추가 migration을 반복하지 않는다. 필수 검증은 동일 UID의 다른 메일 격리·N→N+1/stale N·옛 append/reconcile/recovery replay·old pending 차단·retired validity 재사용·빈 새 epoch·각 쓰기 ABORT/IGNORE rollback·quota·clock·재시작 무재실행·migration 과거 JSON/hash 보존이다. root가 최종 실제 인증 pipe와 전체/cross 회귀를 책임진다.

Core recovery는 기존 folder/append/reconcile과 **동일한 capacity-one network slot·owned permit·UID worker**를 공유한다. 별도 동시 network queue를 만들지 않는다. recovery가 slot을 보유하는 동안 새 folder/append/reconcile/recovery 접수의 queue Full 결과를 시험하며, status/exact replay는 새 permit을 소비하거나 실행을 반복하지 않는다. worker 종료와 미제출 permit 반환은 해당 소유 ID만 해제한다.

근거: [RFC3501 UID 식별](https://www.rfc-editor.org/rfc/rfc3501.html#section-2.3.1.1), [RFC4549 UIDVALIDITY 검증과 재동기화](https://www.rfc-editor.org/rfc/rfc4549.html#section-4.3.1), 기준 [EXT-04](../requirements/ext.md), [SYNC-02](../requirements/sync.md), [APR-01](../requirements/apr.md). 계약 추가는 사람이 보안 코드를 검토했다는 증거가 아니다.
