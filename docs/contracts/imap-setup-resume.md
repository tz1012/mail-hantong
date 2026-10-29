# IMAP 설정의 명시 재개 v1

상태: SETUP-01/INST-09 후속 구현 계약, **사람 검토 필요**. 2026-10-30 연속 개발 위임에 따라 설계를 기록한다. 신규 계정 단일 시도의 v5 계약을 보존하면서 v6에 재개 시도를 추가한다. 실제 로그인·Hello는 개발 자동 시험에서 실행하지 않는다.

## 범위와 의도

`setup.imap.resume`은 protocol_version=1과 기존 request_id만 받는다. CLI는 `hantong setup imap resume REQUEST_ID`다. 부모의 endpoint·draft를 그대로 사용하며 target/비밀/approved/버전/경로 입력을 추가하지 않는다. endpoint는 원래 canonical 값과 version 1/app_password를 유지한다. 임의 계정 설정 편집·기존 v4 계정 이력 생성·Ready 재진단은 이 범위가 아니다.

매 재개는 새 request/operation/idempotency ID와 새 300초 TTL을 갖는 child다. 이전 요청 마지막에 `superseded(child_request_id)`를 추가하며 부모 변경과 child 생성은 한 Immediate transaction이다. 이전 이벤트/시각/대상/결과를 수정하거나 지우지 않는다. 기존 요청을 완료 처리하거나 과거 확인을 재사용하지 않는다. 새 child의 native 보호 입력과 Hello를 다시 통과해야 credential/probe 효과가 가능하다.

Core의 살아 있는 등록/설정 job이 있으면 새 child를 만들지 않는다. 단, 요청한 chain의 최신 leaf가 현재 IMAP job이면 해당 receipt만 반환하고 창/효과 0회다. job이 없고 최신 leaf가 이미 ProbeFinished(Ready)이면 lost ACK 복구를 위해 저장된 leaf receipt만 반환한다. 새 child·UI·Secrets·네트워크 효과는 없다. 그 외 job이 없을 때만 명시 resume가 최신 leaf를 대상으로 prepare를 호출한다. startup/status는 child 생성·UI·Secrets·네트워크를 실행하지 않는다. 이전 효과의 callback은 부모 CAS가 달라져 실패한다.

## v6 저장 모델

v5 DDL은 `migrations/schema-v5.sql`에 고정한다. requests의 phase CHECK도 바뀌므로 requests/events 두 테이블을 동일 migration transaction에서 새 CHECK 구조로 옮긴다. 검증된 temp_store=MEMORY 아래 임시 테이블로 기존 값을 보관하고 events→requests 순서로 제거한 뒤 정확한 v6 DDL로 재생성·복사한다. 임시 테이블은 commit 전에 없애며 평문 디스크 임시파일을 쓰지 않는다. 과거 행/이벤트의 기존 column 값은 그대로 복사한다. 기존 부모 FK와 child event FK를 포함한 전체 연결을 검증한 뒤 commit한다. backup-before-v5와 손상/충돌/rollback 원칙은 기존과 같다.

request에 다음 nullable column을 추가한다.

- resume_of_request_id: 부모 request 32-byte ID, UNIQUE이며 request FK
- resume_parent_version: superseded 직전 부모 version
- resume_account_version: 기존 계정 재개에만 존재하는 expected account version
- resume_observation_version: 같은 경우 expected observation version(없으면 0)
- resume_payload_hash: 새 resume hash domain의 32-byte 값

최초 요청은 모두 NULL이다. child는 parent/version/hash가 전부 필수다. 기존 계정 child는 account/observation 두 값이 모두 존재하고, 계정 생성 전 child는 둘 다 NULL이다. endpoint expected version은 기존 endpoint_version=1로 고정되어 중복 column을 두지 않는다. account/observation version은 이후 +2/+1 연산이 i64 범위를 넘지 않아야 한다.

기존 payload_hash와 MailMutation은 신규 account stage용 원래 hash다. resume_payload_hash는 `mail-hantong.imap-setup-resume.v1\0` domain, parent ID/version, 동일 account/draft/canonical endpoint의 stage hash, 기존 계정 여부와 expected account/endpoint/observation version을 길이/고정 정수 형식으로 결속한다. 정확한 hash bytes는 domain bytes(끝 NUL 포함) || parent ID 32 bytes || parent version u64BE || stage hash 32 bytes || existing flag u8이며, flag=1일 때 account version u64BE || endpoint version 1 u64BE || observation version u64BE를 덧붙인다. 이 hash는 데이터 일치 검사용이며 승인 증거가 아니다. `review_payload_hash()`는 child에서 resume hash, 최초 요청에서 stage hash를 반환한다. native review/capture plan/pending 검사는 이 선택된 hash를 사용한다.

새 phase/event는 `superseded`, `resume_staged`다. superseded 이벤트는 child_request_id만, resume_staged는 account/endpoint/observation 결과 version만 가진다. 기존 probe_finished 결과 version CHECK는 각 시도의 기대 version에 맞게 일반화한다. 요청 1,024개·이벤트 각 8개/총 8,192개 상한은 유지한다. 어떤 정상 시도도 superseded를 포함해 7개를 넘지 않는다.

## 준비와 실행

`prepare_imap_setup_resume(expected_leaf, new_request_id, operation_id, idempotency_key)`는 부모 snapshot을 정확히 재검사하고 새 ID/기존 v4·setup operation 및 account+key 충돌을 거부한다. caller는 account/endpoint/hash/expected version을 임의로 공급하지 않는다. Store가 정확한 현재 상태와 부모에서 계산한다. 부모가 이미 superseded이거나 ProbeFinished(Ready)이면 CONFLICT다. 현재 leaf에서만 준비할 수 있다.

부모/조상 어디에서도 account가 stage되지 않은 경우 동일 draft/endpoint의 신규 child이며, 같은 account가 이미 존재하면 CONFLICT다. child는 기존 AccountStaged 전이를 사용하여 account1/endpoint1과 v4 operation을 원자 생성한다. 기존 public provisioning replay 의미는 바꾸지 않는다.

이미 stage된 계정 또는 기존 계정 child가 review 전 중단된 경우 기존 계정 child다. 현재 계정은 IMAP이며 원래 draft 표시값/endpoint와 일치해야 하고 ready/disabled가 아니어야 한다. account/endpoint/observation version을 저장한다. 현재 version은 최신 leaf의 이력에서 도출한 값과도 정확히 같아야 한다. 기존 계정 child의 review_pending/closed는 binding base account/observation, ResumeStaged부터 probe_started까지는 base account+1/observation, probe_finished는 base account+2/observation+1이다. 최초 신규 stage는 account1/observation0, 첫 probe 결과는 account2/observation1이다. 원래 표시/endpoint가 같더라도 외부 version 변경을 채택하지 않는다. 현재 데이터가 달라졌으면 CONFLICT이며 과거 endpoint로 덮어쓰지 않는다.

새 native proof를 소비한 뒤 `stage_imap_setup_resume`가 현재 snapshot을 다시 검사하고 status=connecting/account version+1과 resume_staged 이벤트를 원자 commit한다. endpoint/SMTP/identity/grants/v4 operation은 생성·변경하지 않는다. 다른 account와 표시값도 보존한다. 이후 begin credential/probe의 fresh CAS, 외부 효과 전 시작 commit, 성공 반환 뒤 다음 효과, 만료 이후 실제 결과 기록 원칙을 그대로 적용한다. probe finish는 stage 결과 account version+1/observation version+1과 journal을 원자 commit한다. 새 password를 받지 않고 저장돼 있던 credential로 자동 접속하는 경로는 없다.

superseded는 시작/완료와 별개인 명시 중단 사건이다. 만료 이후에도 append할 수 있지만 시각 역행은 거부한다. parent updated/event UTC와 child created UTC가 정확히 같다. 살아 있는 job 여부는 Core gate가 책임지고 Store는 exact CAS로 이미 교체된 부모의 모든 후속 변경을 거부한다.

## 연결 검증과 응답

parent의 마지막 Superseded(child)와 child의 resume_of_request_id가 양방향 일치해야 한다. child resume_parent_version+1은 parent superseded seq와 같다. parent/child account·draft·endpoint가 같아야 하고 hash·nullable 조합·result version을 읽을 때 재검증한다. 신규 stage 사건은 기존 v4 operation 링크를 계속 검증한다. resume stage는 해당 v4 operation을 새로 만들지 않으며 조상의 원래 신규 stage 연결을 검증한다.

부모를 탐색하거나 leaf를 찾을 때 재귀 호출을 쓰지 않고 최대 1,024개의 ID와 visited set으로 cycle/orphan/mismatch를 거부한다. 단일 조회는 필요한 ancestor/child 연결을 bounded하게 검사하고, startup/read_all은 전체 이력을 검사한다. 정상 후속 account 변경을 과거 snapshot 손상으로 오판하지 않도록 역사 검증과 효과 직전 현재 snapshot 검증을 구분한다.

`resolve_imap_setup_leaf(request_id)`는 검증된 최신 leaf snapshot만 반환한다. lost ACK 뒤 resume(parent)를 다시 호출하면 살아 있는 leaf의 상태를 돌려주거나, 그 leaf가 중단된 경우 새 child의 사람 확인을 시작한다. Ready leaf는 재실행하지 않는다. status(parent)는 요청한 ID를 그대로 반환하며 superseded를 interrupted/resume_required로 표시한다. resume 성공 응답은 새/현재 leaf ID를 반환할 수 있어 status와 달리 입력 ID echo를 강제하지 않는다. ready는 연결 진단 이력이며 등록/권한 확대를 뜻하지 않는다.

## 검증

v5→v6 실제 데이터 보존/backup 충돌/실제 DDL rollback; pre-stage/credential-started/probe-started/각 실패 결과의 fresh child; Ready/edited endpoint/version/다른 active job 거부; 한 부모 한 child·lost ACK·다단 chain·cycle/orphan/null 조합/시각/hash/버전 손상 거부; 부모 effect CAS 무효화; 기존 계정·SMTP·grants·v4 operation 보존; resume stage와 probe 결과의 원자 rollback; 비밀 저장/네트워크 전 시작 commit; 중단 후 자동 효과 0을 시험한다. native 모델은 신규 생성/기존 연결 재확인 문구를 영/한으로 구분한다. CLI/MCP는 완료 증거를 만들지 않는다.

v5 migration 전에는 고정 NULL alias를 사용하는 별도 v5 journal reader로 기존 데이터를 검증한다. v6 정상 reader는 새 column을 엄격히 요구하며 런타임 column 탐지로 검증을 완화하지 않는다.
