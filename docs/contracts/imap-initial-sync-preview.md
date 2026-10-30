# 최초 동기화 preview 영속 연결 준비안
상태: schema17 구현 기준·사람 검토 필요. 아래 확정 선택은 준비안의 상충 문장보다 우선한다. 요건 SYNC-01/02·EXT-04·MAIL-03.

## 이번 구현의 확정 선택
- `initial-start`는 bounded EXAMINE만 수행하고 Ready/Complete 계획을 저장한다. FETCH/자동 다음 실행은 하지 않는다. `initial-resume`가 최근 높은 범위의 한 page를 명시 수집한다. 각각 기존30초 네트워크 예산을 적용한다.
- start 입장에는 request만 저장한다. plan ID는 그 원래 audit execution ID의32bytes이며, 관측 성공 transaction에서만 plan을 생성한다. 관측 전에는 execution-status로 admitted/running을 표시하고 plan 상태는 null이다. plan이 생기기 전 pause는 NOT_FOUND다. 실제 GUI의 전체 취소 흐름은 후속이다.
- public status는 `initial-status`의 account_id/execution_id(기존 UidPageStatusRequest)를 사용한다. 원래 작업 receipt는 불변이며, 별도 `initial-plan`은 account_id/mailbox/plan_id의 현재 상태·진행을 조회한다. 조회는 무변경이며 원래 audit를 채우지 않는다.
- plan의 public version과 Domain model version은 별도 값이다. public version은1부터 시작하고 쓰기 CAS마다 증가한다. Domain은 target 관측 후1에서 시작한다. 검증된 `InitialSyncPlan::from_stored_parts(validity,uid_next,recent_pages,completed_pages,state,model_version)`로 복원하며 산술·상태/진행·최소 도달 version을 확인한다. 이는 권한 proof가 아니다.
- 계획이 생성된 뒤에는 원래 Core instance와 다른 실행만 명시 resume에서 CORE_UNAVAILABLE로 정리한다. 같은 instance의 admitted/running은 살아있는지 추정하지 않고 CONFLICT로 보존한다. 정상 worker 실패는 즉시 저장하며, 같은 instance에서 원인 불명의 소실이 발생하면 명시 Core 재시작 후 재개한다. instance는 wire에 없고 실제 Core가 주입한다.
- current plan 교체는 이전 plan이 Running/Pausing 상태가 아닐 때만 generation CAS로 허용한다. 원래 plan/page/receipt는 남긴다. active request가 따로 남아 있으면 교체를 거부한다.
- bootstrap validity는 마지막 관측과 같으면 재사용할 수 있지만, 그 사이 다른 validity가 관측된 과거 값으로 돌아가면 CONFLICT다. base active epoch가 있으면 그 validity와 일치해야 하며 자동 recovery는 없다.
- quota는 모든 preview request100,000개, plan4,096개 및 mailbox별128개, plan별8page/512metadata다. 자동 삭제·eviction은 없다. source/hash/receipt/clock 불일치는 기존 INTERNAL_ERROR, stale CAS는 CONFLICT, 보존 한도는 QUOTA_EXCEEDED다.
- m2 검색 cursor는 `m2:<digest64>:<p|b>:<source_decimal>:<resume_uid_decimal>:<expiry_decimal>`로 최대256bytes, 소문자hex·canonical unsigned decimal이다. p source는 현재plan page ordinal+1(1..8), b는 기존base source cursor version(1..i64MAX). 만료는기존5분. digest는 고정v2 domain·기존요청/현재base상태·현재plan ID/version·page hash·source kind를 길이 prefix와 big-endian 정수로 결속하며, 정확한 digest byte 순서는 검색 연결 구현 전에 추가한다. m1은 preview가 전혀 없을 때 기존 의미를 유지하고 preview가 생기면 CURSOR_EXPIRED다.

## 보존할 불변 조건
- 기존 append는 UID0에서 연속 상승한다. 최근 높은 UID를 기존 cursor/epoch/base에 삽입해 낮은 범위가 관측된 것처럼 만들지 않는다.
- 원문 page JSON/hash, 과거 receipt, base identity mapping과 retired epoch를 보존한다. preview만으로 recovery나 append를 실행하지 않는다.
- 목표는 검증된 EXAMINE의 UIDNEXT−1로 고정한다. 높은 canonical64 UID block부터 최근1..8개를 선택하며 이후 신규 메일로 목표를 늘리지 않는다.
- 한 명시 start/resume는 FETCH page 최대1개·UID span64·응답/저장 JSON 각각8MiB·전체 네트워크30초다. 성공 후 남은 page는 PausedBudget이고 다음 page를 자동 실행하지 않는다.
- 남은 UID span/page 수를 표시하며 메일 건수·메일 백분율·INTERNALDATE 정렬로 표시하지 않는다. 본문/첨부는 가져오지 않는다.

## 최소 재사용 경로
- 기존 mail.sync.page 도구의 명시 U/M/A 권한, 공통 인증·감사·정책·rate 경로를 재사용한다. 최초 등록이나 기존 권한을 확대하지 않는다.
- folder/append/reconcile/recovery와 같은 capacity-one slot·UidQueueAdmission·UID worker를 공유한다. 별도 network queue는 만들지 않는다.
- fixed-range FETCH는 기존 ImapUidReconcileRequest/Page와 Secrets fetch_imap_uid_reconcile_page를 재사용할 수 있다. preview 저장은 reconcile 원장에 넣지 않는다.
- 기존 ImapUidPage는 최신 UIDNEXT로 covered를 계산하므로 frozen ceiling의 마지막 partial block에 그대로 쓰면 안 된다.
- start는 별도 bounded EXAMINE-only Secrets API를 사용한다. 후속 명시 resume는 기존 fixed-range FETCH를 사용하며 각 호출은 자기30초 예산만 가진다.
- 원문 credentials/session handle은 Secrets 밖으로 내보내지 않는다. page parser·TLS·literal·metadata 한도는 기존 구현을 재사용한다.

## 공개 action 초안
- `initial-start`: account/mailbox, operation_id/idempotency_key/intent_hash, expected_catalog_generation, recent_pages1..8, expected_preview_generation(첫 계획은 명시 값0).
- `initial-resume`: account/mailbox, plan_id, expected_plan_version, expected_catalog_generation, operation/key/intent. 한 page만 명시 재개한다.
- `initial-pause`: account/mailbox, plan_id, expected_plan_version, operation/key/intent. 네트워크 permit을 요구하지 않는다.
- `initial-status`: 같은 owner의 account/mailbox와 plan_id 또는 execution_id 중 하나. 호출 자체의 감사만 만들며 원래 작업을 변경하지 않는다.
- ID/hash/버전/원문 frame은 기존 strict 입력 규칙을 따른다. endpoint·UIDVALIDITY·UID 범위·worker instance·소유 registration은 wire 입력으로 받지 않는다.
- start/resume receipt는 execution ID·plan ID·고정 작업 상태·plan version·수치 진행·안정 오류만 포함한다. 메일은 별도 M 검색으로 조회한다.
- exact replay는 원래 결과를 반환하고 이번 audit만 완료한다. 원래 unresolved audit ID 재사용과 다른 원장 op/key/execution 충돌은 CONFLICT다.

## 실행·중단
- admission은 Immediate transaction에서 unresolved 원래 audit, owner/현재 epoch/grant, Ready 계정, exact selectable mailbox, 현재 catalog와 endpoint를 검증한다.
- start의 목표 관측 전에는 request 상태만 존재한다. 관측 성공 때 순수 InitialSyncPlan과 current plan을 원자 생성한다.
- start 완료는 preview generation CAS, step begin/page 성공은 plan version CAS를 검사한다. Store의 외부 CAS version과 순수 모델 version은 별도 열이다.
- worker는 네트워크 전·500ms마다·commit 전에 등록/계정/endpoint/catalog/namespace/plan step을 재검사한다. page는 정확한 after/through/validity와 일치해야 한다.
- pause가 먼저 commit되면 이전 step은 무효다. provider future를 폐기한 뒤 PausedUser·원래 job 실패(CONFLICT)·원래 audit terminal을 원자 저장한다.
- page commit이 먼저면 그 page만 보존한다. 남은 page가 있으면 다음 pause가 PausedBudget→PausedUser, 이미 Complete면 기존 모델대로 CONFLICT다.
- startup/status/replay는 enqueue·자동 resume·원래 audit terminal 채우기를 하지 않는다. 새 명시 resume만 중단된 run을 처리한다.
- 이전 Core instance의 admitted/running을 명시 resume로 인수할 때 old run/audit 실패(CORE_UNAVAILABLE), interrupt 전이, 새 run 입장을 한 transaction으로 묶는다.
- 같은 Core instance의 살아 있는 run은 중복 재개하지 않는다. DB commit 불확실성은 자동 재접속/두 번째 terminal 쓰기의 근거가 아니다.
- timeout은 OFFLINE, 크기/보존 예산 초과는 QUOTA_EXCEEDED로 무진행 실패하며 재시도는 명시 resume다. PausedBudget은 성공한 한 page 실행 예산을 뜻한다.

## namespace와 검색
- base cursor가 없으면 active epoch도 없다. preview는 별도 bootstrap namespace에 관측 validity를 고정하며 가짜 base operation/epoch를 만들지 않는다.
- base가 있으면 현재 active validity/generation에 결속한다. namespace 변경은 NeedsRecovery이며 기존 recovery를 자동 실행하지 않는다.
- bootstrap 이후 첫 base append가 같은 endpoint/validity의 epoch1을 만들면 새 명시 resume에서 결속을 채택할 수 있다. 기존 preview bytes는 바꾸지 않는다.
- catalog만 바뀌면 현재 run은 CONFLICT로 멈추고, 다음 명시 resume에서 동일 endpoint·exact mailbox·같은 namespace일 때만 새 catalog를 결속한다. endpoint 변경은 StaleBinding이다.
- preview 메시지 ID는 기존 mail-hantong.imap-message-location.v1 NUL domain과 locator encoding을 그대로 사용한다. 별도 Message-ID dedup은 없다.
- 기존 identity table은 base FK를 유지한다. preview child에 같은 계산 ID를 보관하고 양쪽 삽입/선택 읽기에서 동일 ID의 다른 locator 충돌을 거부하는 안이다.
- 검색은 현재 선택 preview의 UID 중 base covered prefix 밖만 읽고 이후 base source로 내려간다. base coverage 안의 preview는 base gap/remote_absent라도 되살리지 않는다.
- 한 검색 호출은 source 하나·최대64 metadata decode·기존512KiB result/1MiB frame budget을 유지한다. filter·checkpoint·빈 page의 다음 cursor 의미를 재사용한다.
- 새 cursor version은 source_kind, plan ID/version, page ordinal/hash, current base cursor/epoch/overlay head 및 현재 binding을 결속한다. 기존 m1 cursor bytes의 의미를 바꾸지 않는다.
- preview만 있는 정상 상태를 현재 no_cursor corruption 검사에서 구분해야 한다. 다른 orphan base/reconcile/recovery 이력은 계속 INTERNAL_ERROR다.

## schema17 최소 표 제안
- `imap_initial_preview_plans`: plan_id PK(32bytes), owner_registration FK, account/mailbox, generation, is_current, external version, nullable target/model fields, binding versions/hashes/epoch, active_execution FK, UTC 시각. mailbox별 current partial UNIQUE.
- `imap_initial_preview_requests`: execution PK/FK audit, plan FK, action, operation UNIQUE, account+key UNIQUE, canonical intent/hash, expected/result versions, 고정 binding/step, admitted/running/succeeded/failed, 시각, 안정 오류, bounded 불변 receipt JSON/hash.
- `imap_initial_preview_pages`: (plan_id,ordinal0..7) PK, source execution UNIQUE/FK, fixed after/through, validity/observed UIDNEXT, canonical fixed-range JSON≤8MiB/hash/payload hash, child count0..64, 저장 시각. INSERT만 허용한다.
- `imap_initial_preview_messages`: (plan_id,ordinal,uid) PK/FK page, locator와 internal_message_id, canonical metadata JSON/hash. plan+UID UNIQUE, locator/ID lookup index. 큰 JSON은 시작 검증에서 일괄 decode하지 않는다.
- FK는 RESTRICT, ID ASCII/고정길이·소문자 hex·정수·시각·state/nullable 조합 CHECK를 기존 표와 동일하게 적용한다. 최종 SQL과 quota 수치는 root가 확정한다.
- 성공 transaction은 page+children+identity 검증+plan 진행+job receipt+원래 audit terminal 전체다. 실패에는 기존 page/검색 결과를 변경하지 않는다.
- v16을 동결하고 한 v16→17 migration으로 빈 새 표/index를 추가한다. 기존 행/hash/sequence/capacity를 보존하며 preview나 권한을 자동 생성하지 않는다.

## 필수 통합 검증
- 최근 높은 page가 실제 M 목록에 먼저 보이고 base cursor는0/prefix 그대로임, sparse/max UID/빈 사서함/target 고정, preview→base 동일 ID·gap/absent 중복 억제.
- 두 pause/commit 순서, crash 전후 명시 resume, 원래 audit 보존·exact replay·교차 원장 충돌·shared slot Full/Closed·500ms 철회·전체30초 한도.
- 각 쓰기의 ABORT/IGNORE/변조 rollback, quota/clock/손상 strict 읽기, source 한 번 decode와 큰 header byte 진행, migration bytes 보존, provider/queue 무호출 status/search.
- 실제 사용자 DB·계정·네트워크 없이 격리 SQLCipher/가짜 제공자/실제 signed pipe로 검증한다. 이번 안은 전체 최초 동기화·본문/첨부·GUI 완료를 뜻하지 않는다.

## 검색 m2 digest 고정 순서

`mail-hantong.imap-metadata-search-cursor.v2`+NUL로 시작한다. frame은 u32 big-endian byte length와 원문 bytes다. 다음 순서를 유지한다: frame(account_id), frame(raw mailbox), frame(encoding), frame(account status), frame(provider), u64 account version, u64 current endpoint version, endpoint hash32, u64 catalog generation, catalog hash32. 이어 base presence byte0/1을 쓰고1이면 frame(기존 m1 snapshot prefix 전체)를 쓴다. current preview가 있으면 presence1+plan ID32+u64 public version+u64 generation+u32 validity+u32 target ceiling+u8 completed pages+binding_current byte를 쓰고 없으면0만 쓴다. frame(filter), u64 limit, source kind byte(p=1/b=2), u64 source position, source page hash32, u32 resume UID, i64 expiry 순서로 마친다. SHA256 digest를 cursor의 hex로 사용한다. 이 prefix는 권한 token이 아니며 매 조회에서 현재 DB snapshot과 exact 재계산 비교한다.


## 공개 응답과 저장된 상태
- receipt 결과는 `{schema_version:1,ok:true,account_id,execution_id,action,state,plan,error_code}`다. `plan`과 `error_code`는 생략하지 않으며 없으면 null이다. terminal 성공은 plan을 포함한다. 초기 admitted/running receipt는 plan이 null이다.
- initial-plan 결과는 `{schema_version:1,ok:true,plan}`이다. plan 필드는 `plan_id,account_id,wire_hex,encoding,generation,version,is_current,binding_current,stored_at_utc_ms,progress`다.
- progress 필드는 `uid_validity,uid_next,recent_pages,completed_page_count,state,model_version,target_uid_ceiling,selected_lower_uid,selected_page_count,remaining_page_count,remaining_uid_span,not_selected_before_uid`다. 파생 값은 원래 계획의 산술과 일치해야 한다. nullable UID 필드는 생략하지 않는다.
- receipt/plan 응답은8KiB 이내이며 클라이언트는 추가 필드·자료형·일관성·요청 identity 결속을 검사한다. admitted/running은 **저장된 작업 상태**이며 현재 프로세스가 실행 중이라는 증거가 아니다. 상태 조회는 자동 재시작하지 않는다.
- preview와 base active UIDVALIDITY가 다르면 preview 행을 섞지 않고 base만 반환하며 `binding_current=false`다. current preview ID/version은 m2 digest에 계속 결속하며 기존 m1은 만료한다.
- app.diagnostics.snapshot의 기존 v2 sync 집계는 folder/append/reconcile/recovery 네 종류의 작업에 한정한다. 초기 preview는 그 sync 계수에 포함하지 않는다. recent_work/errors는 공통 SyncPage 감사 기록으로 집계한다. 이 제한은 초기 preview 상태 화면을 대신하지 않는다.

검색의 binding_current는 bootstrap preview 뒤 같은 UIDVALIDITY의 base epoch 생성도 호환으로 인정한다. public initial-plan의 엄격한 저장 binding_current는 명시 resume에서 새 base 결속을 채택하기 전 false일 수 있다. 검색은 이 경우에도 base covered prefix를 제외하고 최근 preview의 나머지만 보여준다.


## 구현 후 검토에서 보완한 중단 복구

- EXAMINE 완료 전에는 plan이 없으므로 새 명시 `initial-start`가 같은 registration/account/mailbox의 다른 Core instance에 속한 미완료 start를 `CORE_UNAVAILABLE`로 terminal 처리하고 새 요청을 동일 트랜잭션에서 생성할 수 있다. 현재 권한·현재 결속·generation·의도·quota 검증을 먼저 만족해야 하며 실패 시 부분 정리를 남기지 않는다. 원래 요청과 receipt는 삭제하지 않는다.
- 원래 start의 정확한 replay는 항상 과거 receipt만 반환한다. 같은 Core instance의 미완료 start, 다른 registration의 미완료 start, plan에 속한 active resume은 새 start가 정리하지 않는다. 시작 시 자동 takeover는 없다.
- 이전 Core instance의 active resume이 남은 plan에 `initial-pause`를 요청하면 `CONFLICT`로 거부하고 plan/원 실행은 보존한다. 이 경우 명시 `initial-resume`만 이전 실행을 정리하고 현재 instance에서 한 페이지를 시작할 수 있다.
- endpoint 결속과 catalog가 함께 바뀐 실패는 endpoint 불일치를 먼저 판별해 plan을 `stale_binding`으로 저장한다. 실패한 원래 작업의 안정적인 오류 코드는 그대로 보존한다.
