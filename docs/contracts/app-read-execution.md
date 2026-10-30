# 인증된 앱 읽기 실행 계약 v1

상태: 연속 개발 위임에 따른 v16 구현 기준·독립 검토 중·**사람 검토 필요**. `CLI-16`, `INST-07`, `OPS-05`, `SEC-03`, `SEC-08`에 관련된다. 이 문서는 [표시 설정](cli-display-settings.md), [진단 집계](diagnostic-aggregate.md), [명시 범위 v5](client-app-read-scope-refresh.md)의 공통 실행 경계를 정의한다. 기존 메일 도구와 상호 인증 bytes는 보존한다.

## 명시적인 전역 도구

전역 도구는 정확히 `app.settings.display.get`, `app.diagnostics.snapshot` 두 개다. `app.*` wildcard를 허용하지 않는다. 도구별 요청 DTO는 계정 필드를 받지 않고, 감사 계정 참조는 정확히 0개다. 이 값은 전체 계정이 아니라 해당 호출이 계정 작업이 아님을 뜻한다. 기존 모든 `mail.*` 도구는 계정 1..128개와 기존 scope 검사를 그대로 요구한다.

`AuditTool`에 `DisplaySettingsGet`, `DiagnosticsSnapshot`과 `is_global_read()`를 추가한다. `AuditBegin::new`는 이 두 enum에만 빈 계정 배열을 허용하고, 다른 enum의 빈 배열과 전역 enum의 비어 있지 않은 배열을 거부한다. 저장된 감사 reader도 같은 생성자를 사용하므로 임의 계정 참조 삽입/삭제를 손상으로 거부한다. 전역 성공 감사의 `result_count`는 None이며 언어·집계 내용은 감사에 넣지 않는다.

전역 도구도 `execute_tool` 한 경로를 사용한다. 엄격한 입력 검증 → 실제 connection-owned 세션 및 최신 등록 재검증 → 감사 start commit → 정확한 tool grant 검사 → 공통 정책/호출 한도 → 서비스 → 감사 terminal commit 순서다. IPC dispatcher에서 Store로 직접 우회하지 않는다. `read_only=true`가 허용하며 현재 제품에서는 신뢰된 등록의 `client_kind=cli`만 정책상 허용한다. GUI/MCP의 wire 주장·label·profile 자체는 권한 증거가 아니고 다른 kind는 `POLICY_BLOCKED`다.

인증 scope는 계정 `[]`, tool `[해당 app 도구 하나]`만 요청한다. 서버의 `check_app_read_scope(current, AppReadTool)`는 최신 등록을 재검증하고 connection에 부여된 tools 및 현재 등록 tools 양쪽의 정확한 ID를 검사한다. 계정 검사 API를 빈 목록으로 호출하거나 빈 계정을 모든 계정으로 바꾸지 않는다. 이 API는 고정 enum만 받아 임의 `mail.*` 도구에 사용할 수 없다. 공통 `ReadPolicy`의 기존 등록별 60초 bucket을 함께 사용하고 기본 한도60/min을 유지한다. 표시 조회와 원래 명령은 별개의 인증 연결·감사·요청 한도를 소비한다.

등록·epoch·키·scope 변경 시 기존 연결 종료 규칙을 유지한다. 초기 L과 기존 L/F/U/M은 두 app grant를 갖지 않으며 migration이나 프로필 로드로 권한을 얻지 못한다. A의 실제 부여는 v5 native 확인 경로를 거친다. 개발 시험에서 실제 Hello·사용자 프로필·사용자 DB를 사용하지 않는다.

## Wire와 입력 한도

상호 인증과 0x06 receipt, 한 연결 한 protected request 규칙은 그대로다. 보호 dispatcher/client의 `mail.` prefix 허용에 두 app ID만 추가한다. 미인증 두 app 요청은 `CLIENT_NOT_REGISTERED`이며 Store·설정·집계를 호출하지 않는다. 다른 `app.*`는 `INVALID_INPUT`이다.

두 요청은 공통 `{schema_version:1, request_id, tool, params}` envelope이고 outer method와 inner tool이 정확히 같아야 한다. duplicate/unknown/missing/type/null/array 대체를 거부한다. 두 app 원본 요청 frame 상한은 각각4096 bytes이며 공백도 포함한다. 표시 응답의 원본 frame 상한은4096, 진단 응답은36864 bytes다. 기존 mail 한도와 안정 error `{code,message}`를 유지하고 message는 code와 같은 고정 문자열이다. 원본 frame 한도는 JSON 재직렬화 크기로 대체하지 않는다. 공통 transport는 최대1MiB의 기존 hard frame 상한 안에서 읽고 strict 구문 해석으로 method를 식별한 뒤, raw length의 더 작은 도구별 상한을 감사·서비스 callback 전에 검사한다. method를 아직 알 수 없는 시점에 두 app 전용 상한을 적용했다고 주장하지 않는다.

표시 요청 params는 빈 object이며 결과는 표시 설정 계약의 세 필드만 가진다. Core는 기존 `load_config()` 한 번을 호출하고 검증된 `cli.language`만 투영한다. 설정 없음은 null, 손상은 `INTERNAL_ERROR`다. CLI optional 표시 lookup의 전체 비동기 budget은250ms이며 실패 시 영어 fallback 후 원래 명령의 기존 인증/정책을 별도로 수행한다. 명시 언어·도움말·버전·잘못된 입력·bootstrap·JSON/NDJSON은 이 조회를 하지 않는다. 동기 OS 함수의 강제 중단이나 제품 전체1초 목표 달성을 이 budget만으로 주장하지 않는다.

`--json`/NDJSON 등 기계 출력에서는 저장 언어 lookup을 실행하지 않고 추가 연결·감사·한도 소비를 만들지 않는다. 명시 `--lang`은 기존 고정 error.message 번역에 적용할 수 있지만 기계 필드/코드를 바꾸지 않는다. 저장 언어 조회는 유효한 사람용 출력 경로에서만 한 번 허용한다.

진단 요청 params의 정확한 필드는 다음과 같다.

| 필드 | 규칙 |
| --- | --- |
| `from_utc_ms` | 필수 정수, 0 이상 |
| `to_utc_ms` | 필수 정수, from보다 크며 기간31일 이내 |
| `max_executions` | 선택 정수1..1000, 생략100 |
| `sections` | 필수 배열, sync/recent_work/errors 중1..3개, 중복/빈 배열 금지 |

서버는 등록 ID·epoch와 이번 감사 execution ID를 실제 인증 문맥에서 내부 `DiagnosticAggregateOwner`에 주입한다. 클라이언트는 이 세 값을 보내지 못한다. collector는 현재 진단의 미완료 감사 결속을 검증하고 집계에서 제외한다. 서비스 포트는 `read_diagnostic_snapshot(audit_id, request)`로 audit ID를 명시적으로 받는다. 공개 요청 기간은 Store의 시계와 대조해 미래 종료를 거부한다.

진단 결과의 정확한 세 필드는 `schema_version:1`, `ok:true`, `snapshot_json:string`이다. snapshot_json은 진단 집계 계약의 canonical v2 JSON bytes(최대16384 bytes)를 UTF-8 문자열로 담는다. wrapper는 별도 조합 정보를 추가하지 않는다. IPC client는 outer ID, wrapper, 내부 JSON의 exact key/type/enum/정수/집계 등식/선택 항목/요청 구간/limit 결속과 canonical bytes를 검사한다. 내부 snapshot의 unknown/duplicate/null/array 대체도 거부한다. string의 이중 인코딩을 승인·신뢰 경계로 취급하지 않는다.

`hantong diagnostics snapshot --sections sync,recent_work,errors`는 명시 미리보기 명령이다. `--hours 1..744`(기본24), `--limit 1..1000`(기본100), `--json`, 기존 `--lang`/`--no-input`를 지원한다. CLI 시계로 기간을 한 번 계산하며 음수/overflow는 거부한다. `--json`은 검증한 내부 snapshot JSON 한 개만 stdout에 쓴다. 사람용 출력은 고정 영/한 label과 숫자·안정 코드만 사용한다. 이 명령은 파일 내보내기·덮어쓰기·업로드·전송을 하지 않는다. 실제 내보내기는 선택 및 동일 bytes의 미리보기/확인·안전한 저장 경로 계약과 UI를 후속 구현한다.

## v16 저장 호환과 소유권

현재 v15 DDL은 `migrations/schema-v15.sql`로 고정하고 모든 기존 migration chain 끝에 v15→v16을 추가한다. audit tool CHECK에 두 app ID를 추가한다. 기존 audit FK closure 열 개 표와 client_scope_refreshes를 기존 deferred-FK 방식으로 재구축하며 모든 기존 열/bytes/hash, 순환 FK, index, audit sqlite_sequence 존재 여부/high-water, capacity를 보존한다. scope 이력의 v5/A 허용과 v15까지 max4 validator는 v5 계약을 따른다. message identity·page·epoch의 뜻이나 기존 승인 bytes를 바꾸지 않는다. 새 grant와 기본 설정을 migration으로 넣지 않는다.

root는 이 중앙 계약, AuditTool/감사, v16 DDL/migration/Store lib, 정책·공통 실행기·Core·실제 pipe 시험을 소유한다. scope 담당은 auth의 AppReadTool/A helper, v5 이력, native 표시/결속, profile와 최소 app 인증 client helper를 소유한다. 진단 담당은 공통 신규 집계 DTO와 Store collector를 소유한다. 신규 display/diagnostic client·CLI·Application renderer는 지정된 공개 담당 또는 root가 구현한다. 공유 module export는 root가 통합한다. 작성자와 다른 검토 후 행동 RED/GREEN과 전체 검사를 수행한다.
