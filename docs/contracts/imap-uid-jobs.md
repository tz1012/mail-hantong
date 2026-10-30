# IMAP UID 페이지 영속 작업 v1

상태: 2026-10-30 M2-04/EXT-04·SYNC-02·SEC-03의 공개 연결 선행 계약. 연속 개발 위임으로 작성하며 **사람 검토 필요**. [UID 페이지](imap-uid-page.md)의 읽기 전용 단일 페이지를 인증·권한·감사와 결속한다. 발송·승인 발급·자동 재개·기존 메일 변경 대조는 추가하지 않는다.

공개 `mail.sync.page` request의 intent hash는 클라이언트가 정한 account/operation/key/canonical mailbox identity/expected catalog generation/cursor version에 대한 것이다. 서버가 관측한 page/payload hash와 다르다. Core는 기존 인증·등록 재검증·감사 시작·scope·policy 경로를 거친 뒤 실제 worker queue permit을 예약하고 Store admission을 호출한다. 공개 도구 권한은 기존 folder 권한에서 자동 확대하지 않는다.

## 접수와 고정 상태

`imap_uid_page_requests`는 execution_id(원래 audit FK), operation_id(전역 UNIQUE), account_id+idempotency_key(UNIQUE), mailbox_key/encoding, intent_hash, expected_catalog_generation/cursor_version, bound_account_version/endpoint_version/endpoint_hash, expected_uid_validity/after_uid, state, 접수/시작/완료 UTC 시각, 결과 cursor/coverage/count, page_operation_id/result_page_hash/result_payload_hash, error_code를 가진다. 보관 상한은 100,000개다. DDL의 CHECK·FK도 동일한 상태 제약을 강제한다.

admission은 Immediate transaction에서 unresolved인 원래 audit가 SyncPage tool·정확한 단일 계정·현재 active 등록/epoch/tool/account에 속하는지 확인한다. mailbox wire는 Domain에서 decode하고 재인코딩 bytes/encoding이 입력과 정확히 같아야 한다. 현재 Ready IMAP 계정·catalog generation·selectable mailbox·endpoint version/hash·cursor version을 확인한다. catalog와 cursor의 binding_current가 필요하다. cursor가 없으면 version0/validityNone/after0, 있으면 그 validity/covered를 고정한다. 클라이언트가 endpoint/validity/after를 주입하지 않는다.

먼저 operation 또는 account+key 충돌을 조회한다. 같은 request와 같은 소유 registration의 exact replay는 원래 receipt를 반환하고 이번 audit만 returned_ok로 종료한다. 현재 endpoint/cursor가 달라져도 과거 receipt는 다시 실행하지 않는다. 다른 내용/owner/교차 충돌은 CONFLICT이며 이번 audit만 오류 종료한다. direct UID 적용 원장과 job 원장은 operation/key를 상호 예약 검사하므로 같은 키를 두 경로가 소비하지 못한다.

재접수 시도에는 새 audit ID가 필요하다. 원래 job의 unresolved audit ID 자체를 다시 admission에 넣으면 CONFLICT를 반환하며 그 audit를 종료하지 않는다. 같은 내부 호출을 반복해서 live job의 audit를 receipt 반환 성공으로 바꿀 수 없다.

새 정상 binding에서 queue Full/Closed는 각각 RATE_LIMITED/CORE_UNAVAILABLE failed job과 audit terminal을 원자 저장한다. 일반적인 입력·권한·CAS 거부는 audit terminal만 남긴다. INTERNAL_ERROR·DB 오류·불확실 commit은 별도 재시도나 네트워크 작업을 유발하지 않는다. `Ok(UidPageAdmission)`은 audit 소유권을 Store에 넘겼다는 뜻이며 Rejected도 포함한다. 외부 Result 오류는 원자 commit 여부를 추측하는 근거가 아니다.

Core는 durable admission 호출 후 외부 Result 오류도 `AuditHandoff::error`로 전달하며 Application이 감사 terminal을 다시 쓰지 않게 한다. 이는 commit 성공의 증명이 아니라 불확실한 결과에 추가 변경을 하지 않는 경계다. admission 호출 전 worker 부재 등의 오류만 Application이 종료한다.

## 실행과 완료

state는 admitted→running→succeeded|failed, admitted→failed만 허용한다. start는 admitted에서 한 번만 가능하고 running은 다시 시작하지 않는다. worker는 네트워크 전·실행 중 최대500ms 간격·결과 commit 전에 등록/epoch/scope와 계정/endpoint/catalog/cursor를 다시 대조한다. 변경 시 future를 폐기한다. UIDVALIDITY 변화/불가능한 cursor는 CONFLICT로 매핑하고 reset하지 않는다. 종료·startup·status·replay는 admitted/running을 재접속시키지 않는다.

성공은 job에 고정한 fetch request와 결과 page.request가 정확히 같아야 한다. 기존 v11 적용 검사를 같은 transaction 내부 helper로 수행하며 page operation+message rows+cursor+job succeeded+원래 audit success를 한 번에 commit한다. 결과 원장은 원본 canonical page와 두 결과 hash를 보존한다. 성공 receipt는 과거 결과 version/coverage/count이며 현재 cursor를 대신 조회해 바꾸지 않는다. 읽을 때 job/audit/operation/page/metadata의 키·hash·시각·결과를 대조한다.

실패는 이전 UID 데이터를 보존하고 job failed+audit error만 원자 commit한다. DB 내부 오류나 불확실 commit 뒤 두 번째 terminal 기록을 시도하지 않는다. 자원 quota·CAS 등 효과 전 검증 실패만 명시적인 failed terminal이 될 수 있다. 완료된 job에 다시 terminal을 붙이지 않는다.

## Store API

`hantong_store::uid_page_jobs`는 `UidQueueAdmission::{Reserved,Full,Closed}`, `UidPageAdmission::{Admitted,Existing,Failed}(UidPageReceipt)`/`Rejected(ErrorCode)`, `UidPageStart::{Ready(Box<UidPageJob>),Finished(UidPageReceipt)}`를 제공한다. Job은 private fields와 redacted Debug이며 `execution_id()`, `request()`, `fetch_request()`, `endpoint()` getter만 둔다.

Store 메서드: `admit_uid_page(audit_id, &UidPageRequest, UidQueueAdmission)`, `start_uid_page(id)`, `check_uid_page(id)`, `complete_uid_page_success(id, &ImapUidPage)`, `complete_uid_page_failure(id, ErrorCode)`, `load_uid_page_receipt(owner_registration_id, &UidPageStatusRequest)`. receipt 조회는 같은 owner/account만 허용하고 다른 경우 NOT_FOUND다. 비Windows에서는 CAPABILITY_UNSUPPORTED다. 공개 정책을 우회하는 endpoint/credential/session API는 없다.

v12 시작은 DDL·FK·job 개수 상한을 검사하고, 개별 strict row/operation 검증은 조회·접수·실행·완료 때 수행한다. 과거 100,000개 job의 최대8MiB page를 시작마다 전부 decode하지 않는다. 미조회 행까지 의미 무결성을 확인했다고 주장하지 않는다. migration은 v11 모든 행과 미완료 audit/job, scope history, audit sequence를 보존한다.

필수 검증은 exact replay/후속 cursor 보존·ID/key/owner 충돌·계정/encoding/CAS·실행 중 epoch/endpoint/catalog/cursor 철회·empty cursor·queue full/closed·단일 시작·실패 원자성·각 쓰기 trigger rollback·과거 결과/metadata 손상·quota·중단 재접속 없음이다. 가짜 제공자와 격리 SQLCipher/DPAPI만 사용한다.
