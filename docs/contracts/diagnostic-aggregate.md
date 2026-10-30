# 비민감 진단 집계 계약 초안 v1

상태: **M1-10 후속 구현 제안 · 사람 검토 필요**. 요건은 OPS-05, SEC-08이다. 이 초안은 [진단 묶음 v1](diagnostic-export.md)의 실제 원천 연결 범위를 제안한다. 이번 병렬 묶음에서 root가 맡는 `app.diagnostics.snapshot`·명시 scope v5·공통 정책/감사·CLI 연결과 함께 검토한다. 이 문서만으로 도구 권한이나 파일 내보내기 경로를 활성화하지 않는다. 작성자와 다른 담당자의 검토 및 구현 범위 확정 전에는 구현 완료로 표시하지 않는다.

## 1. 범위와 기존 계약

기존 Application `DiagnosticSnapshot`의 v1 생성 경로와 JSON bytes는 보존한다. 원천이 제공되지 않은 기존 `sync`·`recent_work`는 계속 `"unavailable"`이다. 수집 성공을 표시할 새 경로만 `schema_version: 2`를 사용한다. v1의 문자열을 같은 schema version에서 객체로 바꾸지 않는다.

v2는 암호화 Store에 이미 보존된 감사 실행과 네 종류의 IMAP 작업 상태를 읽는다. `sync`는 선택된 실행 집합에 속한 **영속 작업의 저장 상태**이고, `recent_work`는 그 실행 집합의 감사 결과이며, `errors`는 그 실행 집합의 오류 terminal이다. 동기화 지연 시간, 전체 계정의 동기화 완료율, 메일 수, 디스크 여유, 실제 코어 실행 상태, 제공자 연결 상태는 이 자료만으로 알 수 없다. 이들을 추정하거나 0/정상으로 채우지 않는다. OPS-05 전체 수용 기준과 M1-10의 사용자 확인·내보내기 완료를 이 집계로 선언하지 않는다.

`admitted`·`running`은 저장 원장의 상태다. 현재 프로세스의 queue 점유나 worker 실행을 뜻하지 않으며, 재시작 후 남은 `running`을 자동 실행하거나 실패로 바꾸지 않는다. `succeeded`는 한 번의 제한된 작업 성공이며 전체 사서함의 동기화 완료를 뜻하지 않는다.

## 2. 입력과 소유 범위

내부 수집 요청은 다음 값만 받는다. wire JSON에서 곧바로 역직렬화하는 인증 principal이 아니다.

| 값 | 규칙 |
| --- | --- |
| `registration_id` | 인증된 호출 문맥에서 얻은 정확한 16 bytes. 묶음에 포함하지 않는다. |
| `expected_profile_epoch` | 1..i64::MAX. 현재 등록 epoch와 일치해야 한다. |
| `from_utc_ms` | 포함 시작 시각, 0 이상. |
| `to_utc_ms` | 제외 종료 시각. `from < to`, `to - from <= 2_678_400_000`(31일). |
| `max_executions` | 1..1,000. 호출 계층 기본값은 100. |
| `selected_sections` | `sync`, `recent_work`, `errors` 중 1..3개. 입력 순서는 임의로 허용하되 빈 값·중복·다른 값은 거부한다. DTO는 입력 순서와 무관하게 정확히 `sync`, `recent_work`, `errors` 순서로 정렬해 보관한다. |
| `excluded_execution_id` | 독립 collector에는 None. 공개 호출에서는 이번 `app.diagnostics.snapshot`의 32-byte audit ID를 Core가 주입한다. 사용자 입력에는 없고 출력에도 포함하지 않는다. |

호출 계층의 기본 기간은 수집 요청 시점 직전 24시간이다. [공개 실행 계약](app-read-execution.md)에 따라 CLI는 자신의 시계로 구체적 시작·종료 값을 한 번 정한 뒤 전달하며, 그 시각을 신뢰 시각으로 취급하지 않는다. Store는 단일 read transaction의 최초 읽기로 snapshot을 정한 직후 자신의 UTC 시각을 `generated_at_utc_ms`로 한 번 잡으며 `to <= generated_at`을 검사한다. 음수·역전·기간 초과·미래 종료·수치 변환 overflow는 `INVALID_INPUT`이다. 시계 읽기 실패는 `INTERNAL_ERROR`다. 31일보다 오래된 유효한 과거 구간도 조회할 수 있다. 완료 시각이 아닌 **감사 시작 시각**이 `[from, to)`에 들어가는 실행을 선택한다.

같은 Deferred transaction 안에서 기존 엄격한 등록 reader로 등록 이력·현재 scope를 확인한다. 없음은 `CLIENT_NOT_REGISTERED`, active가 아닌 provisioning/suspended/revoked는 `CLIENT_SUSPENDED`, 현재 epoch 불일치는 `AUTH_REQUIRED`다. 등록 ID만 아는 것은 호출 권한이 아니다. 현재 계정 scope가 비어 있어도 명시 global 도구의 자기 실행은 조회할 수 있지만 계정별 실행은 모두 제외한다. 빈 계정 범위를 전체 계정으로 해석하지 않는다.

선택 가능한 실행은 `audit_starts.registration_id`가 현재 등록과 정확히 같고, 계정별 도구라면 그 실행의 **모든** 감사 계정 참조가 현재 등록의 계정 scope에 포함되는 행이다. 다른 등록의 같은 계정 실행도 제외한다. 일부 계정만 허용된 다중 계정 실행은 전체를 제외하며 허용 계정 부분만 따로 세지 않는다. 현재 scope 밖 실행의 수·존재 여부·계정 목록은 별도 출력하지 않는다. 과거 profile epoch의 자기 실행은 현재 계정 범위 조건을 만족하면 포함한다. 과거 도구 grant의 사용 기록을 현재 실행 권한으로 해석하지 않는다.

범위 판별은 `audit_start_accounts`의 복합 PK와 현재 등록 계정 집합을 사용한다. 기존 여섯 계정별 도구는 canonical account ID 1..128개이며, 새 `app.settings.display.get`·`app.diagnostics.snapshot` 두 global 도구는 정확히 0개다. 계정별 도구의 참조 누락이나 global 도구의 계정 참조는 `INTERNAL_ERROR`다. 판독하는 자기 실행의 참조를 검증한 뒤 현재 범위 밖의 정상 실행만 제외한다. 저장 오류를 scope 밖 실행으로 숨기지 않는다. 원문 요청자가 전달한 계정 목록·`all_accounts`·임의 등록 ID를 범위 선택 수단으로 공개하지 않는다.

`excluded_execution_id`가 있으면 같은 등록·현재 epoch·`app.diagnostics.snapshot` 도구·계정 참조0·terminal 없음의 이번 audit인지 같은 transaction에서 검증한 뒤 선택 대상에서 제외한다. 누락이나 불일치는 `INTERNAL_ERROR`다. 이번 진단이 자기 unresolved 실행을 집계하지 않도록 하는 내부 결속이며 임의 실행을 숨기는 사용자 필터가 아니다. 과거 진단 호출의 완료/미완료 감사는 일반 자기 실행과 동일하게 선택한다.

## 3. 선택 상한과 일관된 snapshot

조건에 맞는 실행을 `audit_seq DESC`로 최신 `max_executions`개 선택한다. UTC 시작 시각이 같거나 시계가 과거에 움직였어도 순서는 audit sequence로 정한다. 같은 기준의 한 행을 추가로 확인하여 초과 시 `truncated: true`로 표시한다. 추가 확인 행은 집계하지 않는다. 초과가 없으면 `truncated: false`다. 정확한 전체 건수나 다음 cursor는 제공하지 않는다.

출력의 `included_executions`는 선택된 실행 수다. `truncated: false`는 현재 보존 원장 중 해당 등록·현재 계정 범위·요청 구간에 맞는 실행을 모두 포함했다는 뜻이다. 전체 계정·전체 기간·삭제 이전 이력·원격 제공자까지 완전하다는 뜻이 아니다. 상한 초과 결과는 최신 실행 부분집합의 정확한 집계이며 전체 구간의 비율이나 추정치로 보정하지 않는다.

등록·scope, 후보 실행·계정 참조, terminal 및 선택한 작업 원장의 scalar는 **같은 Deferred transaction/connection**에서 읽는다. 여러 Store 공개 조회를 반복 호출해 서로 다른 snapshot을 합치지 않는다. 읽는 중 뒤에서 terminal이나 job 상태가 commit되어도 현재 결과는 최초 read snapshot을 유지한다. snapshot 시각 이후의 상태를 재조회하여 섞지 않는다. 미래 완료 시각처럼 판독 snapshot 시각과 모순되는 선택 행은 성공으로 포장하지 않고 `INTERNAL_ERROR`로 거부한다.

collector는 DB·등록·감사·작업·capacity·원장 상태를 쓰지 않는다. provider 호출, DNS/TLS, 비밀 조회, queue 입장, worker 깨우기, reconcile/recovery/append 자동 시작, pruning 및 손상 복구는 모두 0회다. 공개 도구의 audit 시작/terminal은 root가 연결하는 기존 공통 실행기가 담당하며 collector가 중복 생성하지 않는다. 공개 호출의 감사 쓰기와 collector의 쓰기0회는 구분하여 시험한다.

## 4. 집계 정의와 중복 계수

### recent_work

감사 실행 하나는 `execution_id` 하나이며 정확히 한 번 센다. 고정 상태는 `unresolved`(terminal 없음), `returned_ok`, `returned_error`다. 성공 terminal의 `result_count`는 검증만 하고 합산하거나 공개하지 않는다. 이를 읽은 메일 수·가져온 메일 수로 해석하지 않는다.

도구별 허용 목록은 `mail.accounts.list`, `mail.folders.list`, `mail.folders.refresh`, `mail.sync.page`, `mail.messages.search`, `mail.send.execute`, `app.settings.display.get`, `app.diagnostics.snapshot` 여덟 개다. 알려지지 않은 저장 도구는 `INTERNAL_ERROR`다. 도구 목록과 상태 목록은 0건도 포함하고 JSON object key는 ASCII 순서로 정렬한다. 다음 등식이 성립한다.

`recent_work.count = included_executions = sum(state_counts) = sum(tool_counts)`

status/cursor/inspect/재실행 확인 등 각각 새 감사 실행을 만드는 요청은 각각 센다. 정확한 재실행 요청 때문에 **기존 작업**을 다시 세지는 않지만, 그 요청 자체의 새 감사 실행은 최근 요청에 포함된다. `recent_work`의 이름은 사용자 메일 변경 횟수나 고유 operation 수를 뜻하지 않는다. 저장된 감사에 없는 작업은 지어내지 않는다.

### sync

선택된 audit execution 각각에 대해 아래 네 원장을 execution PK로 조회한다. `sync`를 선택하지 않았으면 이 조회를 생략할 수 있다.

| 고정 종류 | 원장 | 원래 audit 도구 |
| --- | --- | --- |
| `folder_refresh` | `folder_refresh_requests` | `mail.folders.refresh` |
| `uid_append` | `imap_uid_page_requests` | `mail.sync.page` |
| `uid_reconcile` | `imap_uid_reconcile_requests` | `mail.sync.page` |
| `uid_recovery` | `imap_uid_recovery_requests` | `mail.sync.page` |

상태는 `admitted`, `running`, `succeeded`, `failed` 네 개뿐이다. 종류와 상태는 JSON object key의 ASCII 순서로 직렬화하며 0건도 포함한다. job이 없는 일반 읽기·입장 전 거부·status·cursor·inspect·replay 확인 요청과 두 global 도구의 감사는 정상이며 sync에 더하지 않는다. 다른 실행이 소유한 기존 job을 operation ID나 idempotency key로 역추적해 더하지 않는다.

한 선택 execution이 둘 이상의 job 원장에 나타나면 `INTERNAL_ERROR`다. job의 account는 원래 audit의 유일한 계정과 같아야 하고, audit 도구도 표와 같아야 한다. `audit.started <= admitted <= started(있을 때) <= completed(있을 때) <= generated_at`이며 실패는 worker 시작 전에도 완료될 수 있다.

`admitted`는 started/completed/error가 없고, `running`은 started만 있으며, 둘 다 audit terminal이 없어야 한다. `succeeded`는 started/completed가 있고 error가 없으며, audit가 같은 완료 시각의 returned_ok여야 한다. `failed`는 completed와 허용 error가 있고, audit가 같은 완료 시각·같은 error의 returned_error여야 한다. 이 네 종류 job의 원래 terminal `result_count`는 NULL이어야 한다. 불명 상태·시각 역전·누락 audit 결속·terminal 불일치는 `INTERNAL_ERROR`다.

이 검증은 집계에 쓰는 scalar 상태·소유·시각·오류의 일관성 검증이다. 기존 성공 receipt reader를 호출해 최대8MiB page/catalog/result JSON을 복호화·디코드하거나 전체 UID/epoch/overlay 이력을 검증하지 않는다. 진단 성공을 원장 전체의 무손상 판정으로 제시하지 않는다. 선택 집합 밖의 orphan job을 전체 검색해 보고하는 DB 검사 도구도 아니다.

`sync.count = sum(sync.state_counts) = sum(sync.kind_counts) <= included_executions`

sync와 recent_work는 **같은 실행을 다른 관점에서 센다**. 두 count를 더하여 전체 작업 수라고 표시하지 않는다. 정상 선택 집합에 job이 없으면 source가 연결된 0건이며 `unavailable`이 아니다.

### errors

선택된 audit의 `returned_error` terminal만 센다. failed job의 error를 별도로 더하지 않는다. terminal이 없는 실행은 오류가 아니며 `SEND_UNKNOWN` terminal과도 구분한다. 한 실패 job+그 terminal은 errors에서 1건, exact replay의 성공 응답은 0건 추가다. 별도 요청 자체가 실패 terminal을 만들었다면 그 실행은 추가 1건이다.

`errors.count = recent_work.state_counts.returned_error`이며 recent_work를 선택하지 않아도 같은 규칙을 사용한다. `by_code`는 양수 건수만 포함하고 아래 고정 허용 목록 순서로 나열한다. `last_error_code`는 선택 집합에서 `(completed_at_utc_ms DESC, audit_seq DESC)` 첫 오류 코드이며 오류가 없으면 NULL이다. 구간 밖에서 시작한 실행의 최근 오류는 포함되지 않는다. 구간 안에서 시작한 실행이 구간 종료 뒤에 완료되었더라도 현재 snapshot에서 완료되어 있으면 포함된다.

허용 오류는 현재 22개로 동결한다: `INVALID_INPUT`, `AUTH_REQUIRED`, `SCOPE_DENIED`, `POLICY_BLOCKED`, `CONFLICT`, `RATE_LIMITED`, `OFFLINE`, `NOT_FOUND`, `SEND_UNKNOWN`, `APPROVAL_REQUIRED`, `APPROVAL_EXPIRED`, `APPROVAL_DENIED`, `APPROVAL_RATE_LIMITED`, `CLIENT_NOT_REGISTERED`, `CLIENT_SUSPENDED`, `HUMAN_STEP_REQUIRED`, `CORE_UNAVAILABLE`, `VERSION_MISMATCH`, `CURSOR_EXPIRED`, `CAPABILITY_UNSUPPORTED`, `QUOTA_EXCEEDED`, `INTERNAL_ERROR`. 임의 문자열이나 새로운 enum 추가만으로 출력 허용 목록을 자동 확장하지 않는다. 불명 저장 코드는 `INTERNAL_ERROR`로 수집 전체를 거부한다.

## 5. v2 JSON과 동일 bytes

최상위 허용 키는 `schema_version`, `generated_at_utc_ms`, `selected_sections`, `window`, 선택한 `sync`·`recent_work`·`errors`뿐이다. 선택하지 않은 section은 NULL 또는 빈 객체로 남기지 않고 생략한다. `window`는 요청 구간과 상한, `included_executions`, `truncated`만 가진다. 아래는 형식 예이며 어떤 실제 계정이나 실행도 가리키지 않는다.

```json
{
  "schema_version": 2,
  "generated_at_utc_ms": 1000,
  "selected_sections": ["sync", "recent_work", "errors"],
  "window": {"from_utc_ms": 0, "to_utc_ms": 1000, "max_executions": 100, "included_executions": 1, "truncated": false},
  "sync": {
    "source": "durable_job_ledgers",
    "count": 1,
    "state_counts": {"admitted": 0, "running": 0, "succeeded": 0, "failed": 1},
    "kind_counts": {"folder_refresh": 0, "uid_append": 1, "uid_reconcile": 0, "uid_recovery": 0}
  },
  "recent_work": {
    "source": "audit_executions",
    "count": 1,
    "state_counts": {"unresolved": 0, "returned_ok": 0, "returned_error": 1},
    "tool_counts": {"mail.accounts.list": 0, "mail.folders.list": 0, "mail.folders.refresh": 0, "mail.sync.page": 1, "mail.messages.search": 0, "mail.send.execute": 0, "app.settings.display.get": 0, "app.diagnostics.snapshot": 0}
  },
  "errors": {"count": 1, "last_error_code": "OFFLINE", "by_code": [{"code": "OFFLINE", "count": 1}]}
}
```

JSON은 UTF-8 compact 형식이며 BOM·마지막 줄바꿈이 없다. object key는 ASCII 오름차순, `selected_sections`와 `by_code` 배열은 위 고정 순서로 직렬화한다. 위 예시의 들여쓰기와 object key 순서는 표시용이며 canonical bytes가 아니다.

집계 값은 private field와 검증된 enum/정수로만 만든다. 최종 묶음은 최대16,384 UTF-8 bytes다. Application은 Store 결과를 받은 뒤 한 번 직렬화하고 불변 bytes로 보관한다. `preview_json`과 `export_json`은 **그 bytes를 그대로** 반환한다. 호출할 때마다 시각·DB·상태·언어·정렬을 다시 계산하지 않는다. 같은 snapshot을 복제해도 같은 bytes를 돌려준다. 미리보기 뒤 terminal이 생기거나 registration이 바뀌어도 기존 묶음 bytes는 변하지 않는다. 새로운 내용을 보려면 새 수집·새 미리보기가 필요하다. 화면 번역은 bytes 밖의 UI 리소스에서 하며 JSON key·code는 번역하지 않는다.

기존 v1의 `with_error_summary`는 기존 동작을 유지한다. 이미 생성한 v2에 이 v1 builder를 적용하면 `SnapshotError::ImmutableSnapshot`으로 거부하고 캐시 내용을 바꾸지 않는다. 이 내부 오류 enum은 새 wire 오류 코드가 아니다.

공개 반환 DTO·Debug·JSON·오류·로그에는 account/registration/execution/operation/idempotency ID, mailbox·UID·UIDVALIDITY, account 수나 이름, email·제목·원문 Message-ID·본문·header·첨부, provider 호스트/응답, key/pin/hash·토큰·자격증명, display label, 파일 경로, 자유 설명·SQL·예외 문자열이 들어갈 수 없다. 구간/생성 시각 외 개별 실행 시각과 sequence도 출력하지 않는다. 수집에 필요한 내부 ID는 인증/조인에만 사용하고 요청 타입의 Debug에서도 숨긴다. 임의 `serde_json::Value`나 자유 map을 채워 넣는 공개 builder를 두지 않는다.

## 6. 내부 인터페이스 제안과 구현 경계

Store에서 Application을 참조하는 의존성을 추가하지 않는다. 공통 validated 요청/집계 DTO는 `hantong_contracts::diagnostic_aggregate`, Store 수집은 새 모듈, 직렬화·기존 v1 유지·불변 묶음은 Application의 진단 모듈이 담당한다. 최소 호출 형태 제안은 다음과 같다. 최종 타입·getter 목록은 교차 검토에서 확정한다.

```rust
DiagnosticAggregateOwner::new(
    registration_id: [u8; 16],
    expected_profile_epoch: u64,
    excluded_execution_id: Option<[u8; 32]>,
) -> Result<DiagnosticAggregateOwner, ErrorCode>

DiagnosticAggregateQuery::new(
    owner: DiagnosticAggregateOwner,
    from_utc_ms: i64,
    to_utc_ms: i64,
    max_executions: u32,
    selected_sections: &[DiagnosticSection],
) -> Result<DiagnosticAggregateQuery, ErrorCode>

Store::collect_diagnostic_aggregate(
    &self,
    query: &DiagnosticAggregateQuery,
) -> Result<DiagnosticAggregate, ErrorCode>

DiagnosticSnapshot::from_aggregate(
    aggregate: DiagnosticAggregate,
) -> Result<DiagnosticSnapshot, SnapshotError>
```

`DiagnosticAggregateOwner`는 Core가 신뢰 문맥을 전달하는 내부 값이며 자체적으로 인증을 증명하지 않는다. owner/query의 Debug는 소유 ID와 제외 execution ID를 가린다. `DiagnosticAggregate`는 생성 시각·선택 항목·window·고정 집계만 소유하며 등록/계정/실행 식별자를 소유하지 않는다. 기존 Application `SnapshotSection`은 새 공통 section으로의 명시적 변환 또는 재export로 호환할 수 있으나 기존 v1 API와 byte 의미를 깨지 않는다. 유효 입력의 비Windows Store 결과는 기존 방식의 `CAPABILITY_UNSUPPORTED`다. 수집 성공은 권한·승인 증거가 아니며 Store 직접 호출 API를 공개 IPC 우회 경로로 연결하지 않는다.

이번에 독립 구현 가능한 범위는 요청/집계 타입, Windows Store read-only collector, Application v2 불변 snapshot, 격리 fixture 시험이다. 신규 DB 표·migration·영속 snapshot·새 의존성은 필요하지 않다.

공개 연결은 이번 병렬 묶음에서 root가 소유한다. 예정 도구는 `app.diagnostics.snapshot`이며 registration/epoch/이번 execution ID는 Core 신뢰 문맥에서 주입하고 IPC 입력으로 받지 않는다. root의 중앙 계약·감사 v16 변경은 두 global 도구의 명시적 계정0형 감사, 전용 tool grant, 공통 rate policy를 정의한다. native 명시 scope v5의 A는 기존 M에 `app.settings.display.get`·`app.diagnostics.snapshot`을 더한 집합이다. 최초 등록은 L을 유지하고 기존 L/F/U/M 등록을 자동 확대하지 않는다. collector 타입이 인증·정책·rate 검사나 native scope 갱신 증거를 대신하지 않는다.

CLI·IPC의 strict 입력/응답과 포함 항목 미리보기는 그 공개 계약에 맞추어 연결한다. 파일 내보내기까지 연결할 경우 포함 항목과 실제 v2 내용 표시, 동일 bytes에 대한 사용자의 내보내기 의사 결속, 확인 시점 재검증, 안전한 저장 경로·덮어쓰기 정책도 root의 공개 계약에서 고정해야 한다. 이 collector 계약은 그 절차를 우회할 권한을 주지 않는다. 파일 저장·외부 업로드·공유·메일 발송의 허용 범위를 서로 혼동하지 않고, 진단 파일 저장을 메일 발송 승인으로 쓰지 않는다.

## 7. 비용과 기존 인덱스

v15에는 registration+시간 전용 감사 index가 없다. 기존 `audit_starts.audit_seq` PK 역순 스캔, execution unique index, `audit_start_accounts(execution_id, account_id)` PK, 네 job의 execution PK, terminal execution PK를 사용한다. 전역 audit scalar 스캔은 최대100,000개이며 추가 한 행은 상한 손상 sentinel로만 확인한다. 그 안에서 현재 등록·구간·계정 범위를 판별하고 최대1,001개 eligible 후보를 확인해 최종1,000개까지만 집계한다. `max_executions+1`번째 eligible 행을 만나면 더 과거를 스캔할 필요 없이 `truncated=true`로 끝낼 수 있다. sentinel을 제외한100,000개를 소모해도 eligible 초과 여부가 결정되지 않았는데 전역 다음 행이 더 있으면 정상 quota를 벗어난 원장이므로 `INTERNAL_ERROR`다. 전역 스캔 제한을 실행 부분집합 truncation으로 숨기지 않는다.

후보와 계정 참조는 bounded iterator/batch로 소비하며 전체 audit/계정/작업 목록을 Vec에 담지 않는다. 다른 등록의 문자열/계정/terminal/job을 읽거나 검증할 필요 없이 registration 일치 여부로 건너뛴다. 각 자기 실행 계정 참조는 최대129개를 읽어 상한 위반을 확인할 수 있다. 원본 header·body·catalog/page/result JSON 열은 SELECT하지 않는다. 스캔한 전역 행 수와 다른 등록의 행 수는 출력하지 않는다.

유효한 Store의 전체 감사 보존 상한은100,000개, 계정 참조 상한은1,000,000개다. 현재 index로는 희소한 등록·과거 구간 요청에서 최대 이 보존 원장을 스캔할 수 있으므로 작업량이 항상 `O(max_executions)`라고 주장하지 않는다. 이 요청 상한은 반환/집계 메모리 상한이지 SQL이 확인하는 전체 scalar 행 수 상한과 같지 않다. 전역 보존 quota·DDL·시작 검증은 기존 Store 계약을 유지한다. 별도 index가 필요하다는 성능 근거가 나오면 migration/과거 schema 보존 제안을 먼저 하고 이 초안 구현 중 임의로 추가하지 않는다.

## 8. 검증 조건

1. 기간의 포함/제외 경계·동일 시각의 sequence 순서·31일/1,000개 최대값과 초과·빈/중복 section·미래 시각·overflow를 검사한다. limit+1에서만 truncated가 켜지고 상한 밖 행은 계산에 들어가지 않는다.
2. 다른 registration의 같은 계정 실행, 자기 실행의 일부만 허용된 다중 계정, scope 밖 실행, active 아닌 등록·구 epoch·미등록·빈 현재 계정 scope를 검사한다. 빈 scope는 자기 global 실행만 허용하고, 제외한 행의 수/존재가 출력되지 않으며 정상 과거 epoch의 자기 실행은 현재 계정 범위 안에서 포함된다. 이번 진단 execution만 정확히 제외하고 제외 ID의 owner/tool/epoch/terminal 결속 손상은 거부한다.
3. audit의 세 상태, 네 종류 job의 네 상태, 입장 전 실패, worker 시작 전 실패, 재시작 후 미완료, status/inspect/replay 확인을 포함한다. 오류 job+terminal은 오류1건이고 sync/recent_work 중복은 정의한 등식과 일치한다. 선택 section별로 불필요한 원천 조회와 출력이 없다.
4. audit/job 소유·도구·account·시각·terminal/error 불일치, 두 원장의 execution 중복, 미허용 enum/code, 누락·초과 account 참조를 주입하여 안정 `INTERNAL_ERROR`와 쓰기0회를 확인한다. 이 검사는 전체 JSON/원장 provenance 검증을 대신한다고 주장하지 않는다.
5. 단일 snapshot에서 동시 terminal commit의 앞/뒤가 섞이지 않는 것을 격리 연결로 확인한다. 수집 후 원장을 바꿔도 기존 preview/export bytes가 같고 새 수집만 새 상태를 반영한다. v1 fixture bytes와 unavailable 동작을 보존한다.
6. ID·주소·제목·메일 원문·토큰·경로·display label·예외를 synthetic sentinel로 넣어 DTO/Debug/JSON/오류에 나타나지 않는 것을 검증한다. JSON key/enum allow-list, canonical 순서, 선택 누락,16KiB 상한을 검증한다.
7. raw row snapshot 및 `total_changes`로 collector 전후 쓰기0회, provider/secret/queue/worker adapter 호출0회를 확인한다. 최대 보존량·희소 등록·과거 구간에서도 큰 JSON을 읽지 않고 bounded 메모리를 사용하는 계획/시험 근거를 남긴다. 실제 사용자 DB·계정·비밀·네트워크를 사용하지 않는다.

이 초안과 구현 검증은 에이전트 검토이며 실제 사람의 보안 검토나 진단 묶음 공유 승인을 의미하지 않는다.
