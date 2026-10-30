# IMAP 기존 UID 페이지 관측 대조 v1

상태: 2026-10-30 M2-04/EXT-04·SYNC-02의 제한된 추가 구현 계약. 연속 개발 위임으로 작성하며 **사람 검토 필요**. v12 기준선은 `94fd693`이다. 기존 append 페이지·원본 metadata·receipt는 불변이다. FLAGS와 서버에서 관측되지 않은 상태만 별도 overlay에 보존한다. 본문·첨부·삭제 명령·자동 재시도·자동 반복·UIDVALIDITY reset은 포함하지 않는다.

## 관측의 의미

동일 UIDVALIDITY에서 UID는 재사용되지 않으며 FLAGS 외 metadata는 불변이다. 성공한 bounded UID FETCH에서 빠진 **기존에 저장한 UID**만 `remote_absent` 관측으로 기록한다. 정상 UID gap에는 행을 만들지 않는다. 이는 원격 사서함의 원자 snapshot이나 영구 삭제 이벤트 수신을 뜻하지 않는다. 원본은 지우지 않는다. 반환된 UID가 원본 페이지에 없거나, non-FLAGS metadata가 다르거나, 이전 remote_absent UID가 다시 나타나면 전체 CONFLICT다. `\\Deleted` flag와 remote_absent는 별개다.

근거: [RFC3501 UID 불변성](https://www.rfc-editor.org/rfc/rfc3501.html#section-2.3.1.1), [UID FETCH](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.4.8), [RFC4549 기존 메일 대조](https://www.rfc-editor.org/rfc/rfc4549.html#section-4.3.1), [RFC9051 unsolicited 응답](https://www.rfc-editor.org/rfc/rfc9051.html#section-2.2.2), [EXPUNGE](https://www.rfc-editor.org/rfc/rfc9051.html#section-7.5.1). 현재 dispatcher가 처리하지 못하는 unsolicited FETCH/EXPUNGE/EXISTS는 전체 실패로 처리하며 sequence number를 UID로 쓰거나 조용히 무시하지 않는다. 최종 EXAMINE은 namespace 검증이지 현재 상태 전체의 증명이 아니다.

## 고정 범위 Domain·Secrets API

새 `hantong_domain::imap_reconcile` 모듈:

- `ImapUidReconcileRequest::new(account_id:&str, endpoint_version:u64, mailbox:ImapMailboxName, uid_validity:u32, after_uid:u32, through_uid:u32, minimum_uid_next:u32) -> Result<Self,ImapSyncError>`. private fields, 같은 이름 getter, Clone/Eq/PartialEq/redacted Debug. account/endpoint/mailbox 검증은 기존 UID request와 동일하다. validity>0, after<through<u32MAX, through-after<=64, minimum_uid_next>through. minimum은 접수 때 current append cursor.uid_next_at_start에서 정한다.
- `ImapUidReconcilePage::new(request, uid_next_at_start:u32, messages:Vec<ImapMessageMetadata>)`. 첫 UIDNEXT>=minimum, 메시지<=64, UID는 고정 (after,through] 안에서 유일하고 UID순 정렬한다. getters request/uid_next_at_start/messages. `to_storage_json/from_storage_json`은 기존 canonical codec 규칙·상한8MiB·unknown/duplicate/비정규 bytes 거부를 따른다. 기존 page/metadata schema1 bytes는 변경하지 않는다.
- `SecretStore::fetch_imap_uid_reconcile_page(&request,&endpoint) -> Result<ImapUidReconcilePage,ImapUidPageError>`. 기존 오류·TLS/AUTH 소유 경계와 비Windows UnsupportedPlatform을 유지한다.

EXAMINE→정확한 유한 범위 UID FETCH 한 번→EXAMINE이다. 시작 validity 일치·UIDNEXT>=minimum, 끝 validity 일치·UIDNEXT>=시작을 요구한다. UIDNEXT가 커져도 범위를 넓히지 않는다. 고정 다섯 필드 UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE, 기존 frame/literal/depth/총량/30초 한도와 typed command 검증을 재사용한다. 임의 session/command/secret callback을 노출하지 않는다.

## 공개 요청·결과

기존 `mail.sync.page`에 action `reconcile`, `reconcile-status`를 추가한다. 기존 cursor/request/status wire·intent·scope history는 변경하지 않는다. 기존 U 권한의 명시적인 메타데이터 한 페이지 관측/로컬 저장 범위다. L/F 등록은 계속 거부하고 권한을 자동 확대하지 않는다. CLI는 `hantong sync reconcile` 및 `hantong sync reconcile-status`다. request와 같은 account/wire-hex/encoding/catalog/cursor/operation/key 옵션, status와 같은 account/execution 옵션을 사용한다. 영/한 도움말·오류·출력을 제공한다. 프레임은 요청/응답8KiB다.

`hantong_contracts::uid_reconcile`:

- `UidReconcileRequest::new(account_id,mailbox:UidMailboxIdentity,operation_id,idempotency_key,expected_catalog_generation,expected_cursor_version,intent_hash)`는 기존 UidPageRequest와 같은 타입·getter·account_ids/intent_bytes를 제공하되 cursor version은 1..i64MAX-1이다. intent bytes는 기존 길이 prefix/필드 순서와 동일하고 domain만 `mail-hantong.imap-uid-reconcile-intent.v1\0`이다. Domain 의존은 없다.
- status는 `UidPageStatusRequest`를 재사용하지만 action/함수/응답 타입은 분리한다.
- `UidReconcileProgress::new(generation:u64,frozen_cursor_version:u64,source_cursor_version:u64,after_uid:u32,through_uid:u32,observed_count:u32,flags_changed_count:u32,newly_absent_count:u32,remaining_pages:u32,pass_complete:bool)`. getters 같은 이름. 양수 generation/source/frozen<=i64MAX, source<=frozen, 유한 폭1..64, observed1..64 및 <=폭, changed+newly_absent<=observed, remaining<=100000, pass_complete==(remaining==0).
- `UidReconcileReceipt::new(account_id:&str,execution_id:[u8;32],state:UidPageState,result:Option<UidReconcileProgress>,error:Option<ErrorCode>)`. getter account_id/execution_id/state/result/error. unresolved는 result/error 없음, succeeded는 result만, failed는 error만. private fields/redacted Debug.
- JSON 응답은 정확히 `schema_version:1,ok:true,account_id,execution_id,state,result,error_code`다. result null 또는 위 progress 필드 순서의 객체다. 성공해도 append cursor를 전진시키지 않는다. `pass_complete`는 고정된 기존 source 페이지 묶음의 관측 완료만 뜻한다.

## source 선택·pass 상태

서버가 immutable append operation 중 result_cursor_version 순으로 **after<covered이고 messages가 비어 있지 않은** 다음 source를 strict reader로 선택한다. 클라이언트는 UID range/validity/endpoint/head generation을 넣지 않는다. 첫 pass의 ceiling은 접수 시 current append cursor version이며 이후 새 append가 생겨도 고정한다. 각 후속 요청은 그 시점의 current append cursor CAS와 현재 catalog generation을 제공해야 하고 cursor>=ceiling이어야 한다. 동일 validity/endpoint hash/version 결속을 유지한다.

선택·진행 수 계산은 기존 mailbox/version index의 수치 열과 operation별 indexed child count를 대조하여 후보와 남은 수를 찾고 선택된 source의 canonical page와 자식만 읽는다. declared inserted_count와 실제 child count가 다르거나 child가 있는데 span이 비어 있으면 INTERNAL_ERROR다. 과거 전체 page JSON을 매 요청/500ms 재검증마다 decode하지 않는다. 남은 수는 저장된 수치 index를 기준으로 하며 아직 선택하지 않은 모든 과거 JSON의 의미 무결성을 검증했다는 주장이 아니다.

head는 generation, version, frozen ceiling, last completed source version, open/complete 상태를 보존한다. 첫 eligible job 접수 때만 새 head/generation을 만들거나 완료된 pass를 다음 generation으로 전환한다. 완료 성공 transaction에서 남은 nonempty source를 확인하여 마지막이면 complete로 바꾼다. 실패는 source 진행을 바꾸지 않는다. eligible source가 하나도 없으면 NOT_FOUND·audit terminal만 기록한다. head/job/op/key는 만들거나 변경하지 않으며 queue Full/Closed보다 이 판정이 먼저다. 따라서 응답 유실 후 같은 no-work 요청이 retry만으로 다음 pass를 여는 일은 없다.

job은 공개 request/owner뿐 아니라 head generation+version+ceiling, source operation ID/page hash/cursor version/정확한 range, account version/endpoint version+hash/catalog generation/current append cursor version/UIDVALIDITY/minimum UIDNEXT를 결속한다. 접수·네트워크 직전·최대500ms마다·commit 직전에 검증한다. source는 같은 pass 안에서 ceiling 이하만 선택한다. 다른 cursor/catalog/epoch 변경은 현재 job을 취소하고 자동 재시도하지 않는다.

## v13 저장과 원자성

세 표만 추가한다: `imap_uid_reconcile_heads`, `imap_uid_reconcile_requests`, `imap_uid_reconcile_messages`. root가 v12 snapshot과 `012-to-013.sql` 및 migration 검증을 소유한다. Store 담당자가 새 모듈 옆 `schema-proposed.sql`에 필요한 정확한 DDL을 제안하고 root가 통합 후 제거한다. 기존 UID/audit/scope 표의 데이터와 schema 의미는 그대로 보존한다.

head는 mailbox identity별 PK/FK와 위 상태를 가진다. requests는 audit execution FK, operation UNIQUE/account+key UNIQUE, 모든 결속과 상태·UTC 시각, immutable canonical result JSON/hash·derived receipt를 가진다. messages는 전체 immutable base message identity PK/FK, presence present|remote_absent, canonical flags(JSON 또는 absent일 때 NULL), observation generation/time, last successful execution FK다. 없는 overlay는 미대조이며 present라고 꾸며내지 않는다. quota는 head100000/request100000/message1000000, result JSON<=1MiB다.

result JSON은 **source의 모든 기존 UID**에 대한 present+flags 또는 remote_absent 관측과 source/head 결속을 보존한다. gap은 포함하지 않는다. strict reader가 hash/원본 source/모든 overlay child를 확인한다. 후속 관측이 있는 과거 receipt는 immutable result에서 반환하며 최신 overlay와 동일해야 한다고 강제하지 않는다. 반대로 최신 성공 결과가 있는 UID의 overlay가 누락된 것을 원본 flags로 fallback하지 않는다. 해당 source의 최신 성공 job index로 판정한다. 시작은 count/FK/DDL 검사, 개별 의미 검증은 해당 조회·실행 때 한다.

접수는 기존 UID job의 audit/owner/grant 검증·새 attempt audit·exact replay·queue Full/Closed terminal 원칙을 따른다. **operation/account+key/execution 충돌은 direct append apply, append job, reconcile job의 모든 방향에서 검사한다.** exact reconcile replay는 같은 owner/공개 request만 원래 receipt를 반환하며 현재 overlay를 바꾸거나 재접속하지 않는다. 요청이 원래 unresolved audit ID를 재사용하면 그 audit를 종료하지 않는다.

성공 시 overlay 전부+head 진행+job/result+audit terminal을 한 Immediate transaction으로 commit한다. 실패는 overlay/base를 보존하고 job+audit만 원자 종료한다. 시계 역행·손상·commit 불확실성은 INTERNAL_ERROR이고 뒤따르는 두 번째 terminal을 쓰지 않는다. startup/status/replay는 unresolved를 재실행하지 않는다.

Core가 durable admission을 호출한 뒤 받은 외부 Result 오류도 감사 소유권을 Application에 돌려주는 근거가 아니다. commit 여부가 불명확하므로 `AuditHandoff::error`로 전달하여 coordinator의 추가 terminal 쓰기를 막는다. 이 규칙은 기존 folder/append admission에도 동일하게 적용한다. worker 부재처럼 admission 호출 전 오류만 coordinator가 종료한다. 오류 handoff는 DB commit 성공을 주장하지 않는다.

`hantong_store::uid_reconcile_jobs` API는 `UidReconcileAdmission::{Admitted,Existing,Failed}(UidReconcileReceipt)/Rejected(ErrorCode)`, `UidReconcileStart::{Ready(Box<UidReconcileJob>),Finished(UidReconcileReceipt)}`. Job getters execution_id/request/fetch_request/endpoint. Store `admit_uid_reconcile(id,&request,UidQueueAdmission)`, `start_uid_reconcile(id)`, `check_uid_reconcile(id)`, `complete_uid_reconcile_success(id,&ImapUidReconcilePage)`, `complete_uid_reconcile_failure(id,error)`, `load_uid_reconcile_receipt(owner,&UidPageStatusRequest)`는 기존 UID job과 같은 오류/감사 소유권 규칙이다. current observation 조회 API는 Store 담당자가 구체적 타입을 보고하여 이 계약에 추가한다.

관측 조회는 `Store::load_imap_uid_observation(account:&str,mailbox:&ImapMailboxName,validity:u32,uid:u32)->Result<Option<StoredImapUidObservation>,ErrorCode>`다. outer None은 미대조 또는 원본 없음이며 strict base/최신 observation 검사 후에만 반환한다. getter `flags()->Option<&[String]>`의 None과 `remote_absent()==true`는 원격 부재 관측이다. 그 밖에 generation/observed_at_utc_ms/execution_id getter가 있다. 값은 private fields/redacted Debug이고 기본 메일 원본·현재 서버의 확정 상태를 대신하지 않는다.

## 병렬 소유권과 검증

Domain/Secrets 담당은 새 fixed-range DTO/transport/fixture, Store 담당은 새 저장 모듈·시험·기존 원장 예약 상호 검사, 공개 경계 담당은 Contracts/Application/client/CLI/schema·영/한, root는 이 계약/DDL/migration/Core worker/실제 pipe 통합을 소유한다. 다른 담당자 변경을 되돌리지 않는다. 의존성 추가는 없다. root가 native Cargo cache 순서를 배정한다.

필수 연결: 두 append source 후 flags/부재 대조, tail1..10에서 UIDNEXT100이어도 범위1:10, pass중 append 증가의 ceiling 보존, 실행 중 epoch/catalog/cursor 변경 무반영, 과거 replay의 최신 overlay 보존, folder/append/reconcile 공유 slot, 실패한 각 쓰기 rollback, 누락·손상·quota·재시작 무재실행. 실제 계정/네트워크/사용자 DB/Hello는 사용하지 않는다.
