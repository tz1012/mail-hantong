# IMAP 로컬 UID 관측 요약 v1

상태: M2-04/SYNC-02·CLI-01의 관측 범위 공개 계약. 기준선 `1e332aa`의 v14 복구 자동 검증 이후 작성한다. 연속 개발 위임 범위이며 **사람 검토 필요**. DB schema는 v14를 유지한다.

## 의미와 범위

`mail.sync.page action=inspect`는 현재 저장된 cursor 페이지의 수집 범위와 그 페이지가 처음 관측했던 UID 상한을 반환한다. 마지막 append 또는 recovery 페이지에 결속한 값이며, 이후 reconcile이나 실제 원격 서버의 현재 최신성을 나타내지 않는다. 상한에 도달해도 현재 mailbox와 완전히 동기화됐다는 뜻이 아니다. UID 번호 사이에는 빈 구간이 있을 수 있으므로 UID 범위를 메일 건수나 남은 메일 수로 표시하지 않는다.

기존 일곱 action의 입력·응답·hash·동작은 유지한다. 기존 U의 `mail.sync.page`/계정 grant, 정책, 호출 제한, 감사 흐름을 그대로 사용한다. L/F에는 권한이 없다. 네트워크·worker·작업 접수·자동 재개·새 승인을 추가하지 않는다. 메일/cursor/catalog/epoch/job 자료는 읽기만 하며 공통 감사 시작/종료 기록은 정상적으로 남긴다.

## 공개 계약

입력의 정확한 필드는 `action:"inspect",account_id,wire_hex,encoding`이며 기존 `UidCursorRequest`를 재사용한다. canonical mailbox와 계정 형식·strict unknown/duplicate/missing/null 거부·요청/응답8192-byte 제한을 유지한다. 계약 모듈은 `hantong_contracts::uid_inspection`이다. private fields·검증 생성자·Clone/Eq/PartialEq·redacted Debug를 제공한다.

- `UidObservationSummary::new(cursor_version:u64,epoch_generation:u64,covered_through_uid:u32,observed_uid_ceiling:u32,stored_at_utc_ms:i64)`. `1<=epoch_generation<=cursor_version<=i64MAX`, `covered<=ceiling<u32MAX`, 저장 시각은 양수다. 동일 이름 getter와 `coverage_at_observed_ceiling()->bool`을 제공한다. bool은 covered==ceiling로 계산하고 별도 상태로 저장하지 않는다.
- `UidInspectionSnapshot::new(account_id:&str,mailbox:UidMailboxIdentity,catalog_generation:u64,binding_current:bool,observation:Option<UidObservationSummary>)`. 계정/identity는 기존 검증, catalog generation은 1..i64MAX다. 동일 getter와 `observation()->Option<&UidObservationSummary>`를 제공한다.

성공 응답의 정확한 필드는 `schema_version:1,ok:true,account_id,wire_hex,encoding,catalog_generation,binding_current,observation`이다. observation은 cursor가 없으면 null, 있으면 정확히 `cursor_version,epoch_generation,covered_through_uid,observed_uid_ceiling,stored_at_utc_ms,coverage_at_observed_ceiling` 객체다. 모든 nullable/타입/범위를 검사하며 bool 값이 두 UID 값의 비교 결과와 다르면 거부한다. client는 account와 전체 mailbox identity 및 RPC ID를 요청에 결속한다. endpoint·UIDVALIDITY·제목·주소·본문을 노출하지 않는다.

cursor 없음은 미관측 상태다. 빈 mailbox를 관측한 경우에는 observation이 존재하고 coverage=ceiling=0, bool=true다. 서로 구분한다. stored_at_utc_ms는 cursor 페이지의 **로컬 저장 시각**이며 서버의 정확한 관측 시각이나 최근 reconcile 시각으로 표현하지 않는다.

## Store와 Core

새 `hantong_store::uid_inspection` 모듈에 `Store::inspect_imap_uid(&UidCursorRequest)->Result<UidInspectionSnapshot,ErrorCode>`를 추가한다. 비Windows는 CAPABILITY_UNSUPPORTED다. 입력 검증 후 단일 Deferred read transaction 안에서 기존 strict catalog/cursor/epoch reader를 사용한다. 서로 다른 public Store 호출 결과를 Core에서 합치지 않는다.

catalog 또는 정확한 selectable mailbox가 없으면 NOT_FOUND다. 손상된 cursor/epoch/operation 연결은 INTERNAL_ERROR로 전파하며 null로 숨기지 않는다. cursor가 없을 때도 같은 account/mailbox/encoding의 epoch·page operation·message 존재를 bounded EXISTS로 확인한다. 하나라도 남아 있으면 INTERNAL_ERROR다. 최초 수집 전의 pending job/audit만 존재하는 것은 미관측으로 허용한다. observation은 현재 active epoch만 사용한다. `observed_uid_ceiling=cursor.uid_next_at_start()-1`, stored_at은 기존 cursor 시각, epoch_generation은 동일 namespace의 strict epoch 값이다. 응답 catalog_generation은 현재 catalog의 세대다. binding_current는 현재 catalog 결속과 기존 StoredImapUidCursor::binding_current의 AND이며, cursor가 없을 때는 catalog 결속만 사용한다. 기존 cursor 계약대로 cursor.catalog_generation과 현재 catalog 세대의 숫자 일치를 추가로 요구하지 않는다. 같은 계정/endpoint/선택 가능한 mailbox를 보존한 catalog 갱신 뒤에도 binding_current는 true일 수 있다. 이 값은 최신성이나 cursor 페이지 관측 시각을 나타내지 않는다. 낡은 결속은 false로 표시하며 자동 갱신하거나 성공 상태로 덮어쓰지 않는다.

Application은 `UidPageAction::Inspect(UidCursorRequest)`, `ExecutionContext::read_uid_inspection(&UidCursorRequest)->Result<ReadResult<Self::Output>,ErrorCode>`, `render_uid_inspection`을 제공한다. Core는 위 Store API 한 번과 renderer를 연결한다. worker/Secrets를 호출하지 않는다.

## IPC client와 CLI

`hantong_ipc_client::uid_inspection`은 기존 cursor API와 같은 인자 형태의 `inspect_default`, `parse_inspection_snapshot`, `inspection_json`을 제공한다. CLI는 `hantong sync inspect --account ID --wire-hex HEX --encoding ENCODING`이다. 글로벌 EN/KO·json·no-input 규칙과 기존 bootstrap help 검증을 유지한다.

영어 기본 출력과 한국어 출력은 미관측, 저장된 UID 범위/관측 상한, epoch/cursor 세대, 로컬 저장 UTC milliseconds, 현재 결속 여부를 표시한다. `complete`나 남은 메일 건수로 표현하지 않는다. CLI가 조회 결과를 받아 자동 page/recover/reconcile을 호출하지 않는다.

## 검증과 병렬 소유권

Store 담당은 요약 모듈/시험과 모듈 선언을, 공개 담당은 Contracts/Application/client/CLI/schema를, root는 Core/실제 인증 pipe 연결과 문서를 소유한다. 작성자와 다른 담당자가 공개/Store 경계를 검토한다. 새 의존성·migration·scope 변경은 없다.

시험은 미관측과 관측된 빈 mailbox 구분, sparse UID와64 경계, 더 큰 UIDNEXT를 관측한 append, recovery 후 새 epoch 및 과거 원본 보존, stale binding 및 같은 mailbox를 유지한 catalog 세대 증가, 잘못된 입력/손상 거부, 조회 전후 모든 제품 데이터 행 불변을 포함한다. 실제 서명 pipe에서는 provider 호출0·queue 예약0·공통 감사 terminal, 계정/scope 거부와 strict response binding을 확인한다. 실제 계정·메일·사용자 DB는 사용하지 않는다.
