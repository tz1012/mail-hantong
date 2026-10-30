# 최초 동기화 최근 범위 계획 모델 v1

상태: `SYNC-01`/M2-05의 내부 순수 모델 구현 기준·**사람 검토 필요**. 기존 append cursor는 UID0부터 연속 상승만 허용하므로 최근 높은 UID를 그 원장에 넣어 낮은 범위가 관측된 것처럼 만들지 않는다. 이 계약의 첫 구현은 범위 계산·CAS·일시정지 상태 모델이며, 네트워크·영속 저장·목록 연결 완료를 뜻하지 않는다.

## 고정 범위와 표시 의미

Core가 별도 bounded EXAMINE에서 검증한 `uid_validity>0`, `uid_next>0`, `recent_pages=1..8`을 모델 생성자에 전달한다. 모델은 외부 연결이나 현재 시각을 읽지 않는다. target ceiling은 `uid_next-1`로 고정하고 이후 새 메일이 생겨도 바꾸지 않는다. UID0 기준64개씩 나눈 canonical block 중 높은 block부터 최대 recent_pages개를 선택한다. uid_next=1이면 관측된 빈 범위로 Complete이며 미관측 상태를 이 생성자로 만들지 않는다.

ceiling>0일 때 highest ordinal은 `(ceiling-1)/64`, 선택 page 수는 `min(recent_pages, highest+1)`이다. 첫 page의 after는 highest*64, covered는 ceiling이다. 다음 page는 직전 after보다 낮은64개 block이다. 마지막 선택 범위의 lower는 `(highest+1-selected_pages)*64+1`이다. 산술은 u64 중간값과 검사된 변환을 사용하며 u32::MAX UIDNEXT에서도 overflow하지 않는다.

출력은 `target_uid_ceiling`, `selected_lower_uid:Option<u32>`, `selected_page_count`, `completed_page_count`, `remaining_page_count`, `remaining_uid_span`, `not_selected_before_uid:Option<u32>`이다. 빈 범위는 lower/older=None이다. nonempty라도 selected_lower_uid=1이면 not_selected_before_uid=None이며, lower>1이면 Some(lower-1)이다. remaining_uid_span은 아직 미완료인 step마다 (covered_through_uid-after_uid)를 더한 값이며 메일 건수가 아니다. 최상위 partial block 때문에 단순 remaining_page_count*64와 다를 수 있다. uid_next=u32::MAX, recent_pages=8의 최초 span은510이다. UID gap·삭제된 UID가 있어도 page 성공 한 번은 page 하나 진행이다. “남은 메일” 또는 메일 기준 백분율을 만들지 않는다. 높은 UID는 도착 순서의 근사이며 INTERNALDATE 정렬이라고 표시하지 않는다.

## 순수 상태·CAS

`hantong_domain::initial_sync_plan`은 private 필드, Clone/Eq/PartialEq, redacted Debug를 가진 `InitialSyncPlan`, `InitialSyncStep`과 고정 `InitialSyncState`를 제공한다. 모든 선택자와 값은 데이터이며 권한/승인 proof가 아니다. plan version은1부터 i64::MAX이고 성공한 상태 변경 때만1씩 증가한다. Domain은 Contracts에 의존하지 않는다. 자체 `InitialSyncPlanError::{InvalidInput,Conflict}`를 제공하고 std::error::Error 및 고정 Display/Debug를 구현한다. 상위 adapter가 기존 안정 ErrorCode로 매핑한다. expected_version 불일치·overflow·잘못된 상태/step·UIDVALIDITY 불일치는 Conflict이고 생성 입력 오류는 InvalidInput이다. Err이면 원래 모델 전체를 Eq 기준으로 그대로 유지하며 자동 reset하지 않는다. 생성자 signature는 `new(uid_validity:u32,uid_next:u32,recent_pages:u8)->Result<Self,InitialSyncPlanError>`이고 모든 전이 메서드는 `&mut self`와 명시 expected_version(u64) 또는 발급 step 참조를 받는다.

고정 상태는 Ready, Running, Pausing, PausedUser, PausedBudget, Interrupted, Complete, Failed, StaleBinding, NeedsRecovery다. 빈 범위는 처음부터 Complete, 나머지는 Ready다.

- `begin(expected_version)`은 Ready/PausedUser/PausedBudget/Interrupted/Failed에서만 가능하다. 다음 page 한 개를 가진 step을 반환하고 Running으로 전이한다. step에는 증가한 plan_version, ordinal(0부터), uid_validity, after_uid, covered_through_uid가 결속된다. step 생성자는 공개하지 않는다. 각 begin은 명시적인 별도 실행 의도일 뿐 provider 호출을 만들지 않는다.
- `complete_step(&step, observed_uid_validity)`는 Running과 exact 현재 step/version/validity에서만 한 page를 진행한다. 남은 page가 있으면 PausedBudget, 없으면 Complete다. 자동 다음 begin은 없다.
- `pause(expected_version)`는 Running→Pausing, Ready/PausedBudget/Interrupted/Failed→PausedUser다. 이미 Pausing/PausedUser면 같은 version에서 no-op이다. 다른 상태에서는 Conflict다. 첫 pause가 version을 올리므로 이미 발급된 step은 commit할 수 없다.
- `acknowledge_pause(expected_version)`는 Pausing→PausedUser이며 page 진행은0이다. 실행 중 provider가 종료/취소된 뒤 future Store worker가 호출할 경계다.
- `interrupt(expected_version)`는 Running→Interrupted, Pausing→PausedUser다. page 진행은0이다. 이 순수 전이가 재시작 시 자동 DB 쓰기/재시도 권한을 주지 않는다. 향후 Store의 명시 재개 원장이 이전 run을 처리해야 한다.
- `fail(expected_version)`는 Running→Failed이며 page 진행은0이다. 새 begin은 사용자 명시 재개를 전제로 한다. 모델이 자동 retry하지 않는다.
- `invalidate(expected_version, needs_recovery:bool)`는 아직 terminal 아닌 상태를 NeedsRecovery 또는 StaleBinding으로 바꾸고 page 진행은0이다. 두 상태와 Complete는 begin할 수 없다. 실제 endpoint/catalog/epoch 비교는 미래 trusted adapter가 하고 bool 자체가 보안 proof는 아니다.

진행 getter와 `version`, `state`, `uid_validity`를 제공한다. step getter는 위 결속 필드를 제공한다. 메서드가 Err를 반환하면 모든 필드를 보존해야 한다. `fail()`의 성공 전이는 version/state만 바꾸고 범위·진행은 보존한다. 같은 숫자 범위를 가진 오래된 step도 pause/resume 이후 version이 다르면 거부한다.

## 후속 연결의 필수 경계

최근 범위 page는 기존 cursor·epoch·base page/hash를 수정하지 않는 별도 immutable preview 원장에 저장해야 한다. locator 기반 현재 msg_ ID를 그대로 계산하여 추후 base와 겹칠 때 동일 ID를 유지한다. preview를 실제 목록 검색에 연결하고 중복 locator는 base 우선으로 합쳐야 최근 우선 목록을 제공했다고 할 수 있다. source_kind/plan version/page hash를 검색 cursor에 결속해야 한다.

향후 start/resume의 네트워크 예산은 한 page·64 UID·8MiB·30초로 한정한다. plan 목표·endpoint hash/version·catalog generation/hash·exact mailbox·active epoch를 입장/실행/commit 때 확인한다. pause가 commit보다 먼저면 무반영, commit이 먼저면 그 page만 진행 후 pause다. crash/startup의 자동 enqueue·implicit resume·recovery 호출은 금지한다. catalog-only rebind도 다음 명시 resume에서 exact endpoint/mailbox가 같을 때만 허용한다. 실제 Store/Secrets/CLI·검색 연결에는 별도 원장/원자 commit/권한 계약과 통합 시험이 필요하다.

필수 모델 시험: empty/sparse/max-u32 범위, recent1/8 경계, ceiling 고정, descending 연속 block과 전체선택합, 성공 한 page 예산, 두 pause/commit 순서, no-op pause, crash/interrupt 명시재개, fail의 무진행, stale/recovery terminal, stale step/version/validity와 overflow 시 무변경. 메일 건수나 네트워크 성공을 모델만으로 주장하지 않는다. 새 dependency·I/O·DB·시간·sleep을 추가하지 않는다.
