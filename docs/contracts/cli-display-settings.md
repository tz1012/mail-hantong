# CLI 표시 언어 읽기 v1 — 검토 초안

상태: M1-05/M1-12의 `CLI-16`, `INST-07`, `OPS-05`, `SEC-03` 후속 **계약 초안·사람 검토 필요**. 연속 개발 위임과 root의 A 범위 설계에 따라 작성했으며 제품 경로 활성화, 새 권한 부여, 사람 보안 검토 완료를 뜻하지 않는다. root가 연관 계약을 교차 검토한 뒤 구현 범위를 배정한다.

기준은 [설정 계약](config.md), [CLI 초기 경계](cli-bootstrap.md), [인증 wire](auth-wire.md), [프로필](client-profile.md), [권한 갱신 v4](client-metadata-scope-refresh.md), [공통 실행기](execution.md)다. 기존 설정 쓰기, GUI 언어/테마, 보호된 정책, 계정·메일 기능은 이 읽기 기능의 범위가 아니다.

## 표시 우선순위와 최소 데이터

CLI 사람용 문구의 우선순위는 유효한 명시 `--lang en|ko` → 인증된 조회로 얻은 저장 `cli.language` → `en`이다. JSON/NDJSON 모드는 저장 언어를 조회하지 않으며, 명시 `--lang`은 기존 error.message의 번역에만 적용할 수 있다. 언어 선택은 호출 한 번 동안 고정하며, 기계용 필드명·오류 코드·도구 ID·명령·메일 원문을 바꾸지 않는다. 새 옵션이나 질문으로 첫 실행을 지연시키지 않는다.

서버가 공개할 설정은 `cli_language` 하나뿐이며 값은 `en`, `ko`, 또는 설정 레코드가 없음을 뜻하는 null이다. timezone, GUI 언어/테마, 정책, settings version, 계정 목록, 경로, 키, 등록 정보는 반환하지 않는다. 저장값이 없을 때 읽기 경로에서 기본 설정을 INSERT하지 않는다. 존재하지만 파싱/버전 결속이 손상된 설정은 `INTERNAL_ERROR`이며 null로 바꾸지 않는다.

CLI는 암호화 DB를 직접 열지 않고, 별도 설정 파일·환경 변수·레지스트리·프로필 필드를 언어 저장소로 만들지 않는다. 기존 고정 경로의 등록 프로필은 인증/권한 판단에만 사용한다.

## A 범위의 전용 권한을 통한 최소 읽기

새 tool ID 제안은 정확히 `app.settings.display.get`이며 이하 D라 부른다. 인증된 요청 scope와 현재 Store 등록의 tool grant 양쪽에 D가 있어야 한다. 계정과 무관한 앱 표시 설정이므로 요청의 계정 목록은 존재하지 않으며, 감사의 계정 참조는 0개다. 이를 전체 계정 권한으로 해석하지 않는다. 최초 구현은 현재 Store 등록의 client kind가 `cli`인 경우만 허용한다. wire의 kind/label/role 주장은 판단에 쓰지 않는다.

기존 L/F/U/M의 정확한 도구 배열, 최초 등록 L, 과거 profile bytes, v1~v4 scope hash/전이/결정은 그대로 둔다. 이들에는 D 권한이 없으므로 조회는 `SCOPE_DENIED`다. 프로필 로드·프로그램 업그레이드·DB migration·기존 pending/staged 재개가 D를 추가하지 않는다. `mail.accounts.list`의 옵션이나 `auth.finish`의 추가 응답 필드로 설정을 끼워 넣지 않는다.

권한 발급은 [앱 읽기 범위 갱신 v5](client-app-read-scope-refresh.md)의 A 집합을 사용한다. A는 정확히 M에 `app.diagnostics.snapshot`, `app.settings.display.get`을 더한 정렬 집합이다. 새 명시 `hantong clients refresh`의 fresh 요청만 전체 A를 native 화면에 제시한다. 최초 L 및 기존 L/F/U/M 프로필은 유지하며, 기존 pending/staged는 저장된 목표를 그대로 재개한다. CLI의 자동 언어 조회가 refresh를 대신 호출하지 않는다.

이는 기존 고정 기본 CLI 갱신 방식의 확장이며 표시 권한만 선택하는 편집 기능은 제공하지 않는다. 따라서 언어만 원하는 L 사용자의 새 refresh에도 메일 메타데이터와 진단 권한이 함께 제시된다. native 화면은 이 선택 폭을 숨기지 않고 전체 old/target 도구와 각 의미를 보여 준다. 거절하거나 refresh하지 않은 사용자는 `--lang` 또는 en을 계속 사용할 수 있다. 이 제약을 표시 설정 조회에 이미 동의했다는 뜻으로 처리하지 않는다.

### 사람 확인과 개발 위임의 구분

언어 읽기 그 자체가 메일 발송·정책 변경과 같은 매번의 사람 승인을 요구하지는 않는다. 기존 권한에 없는 A를 등록에 새로 부여할 때만 기존 native 범위 갱신의 명시 확인·전체 snapshot 결속·opaque 일회 proof·보호 프로필 발행을 거친다. 별도의 승인 프로토콜·D-only 동의 명령을 만들거나 읽을 때마다 Hello/확인 창을 띄우지 않는다. 닫기/거절은 기존 grant와 언어 fallback을 유지한다.

이 요구는 개발 대화에서 다시 허가를 받아야 한다는 뜻이 아니다. 현재 일반 개발 위임으로 계약·코드·가짜 검증을 계속할 수 있으며, 실제 사용자 프로필 변경/Hello 실행이나 사람이 보안 코드를 검토했다는 기록을 허용하지 않는다.

모든 활성 CLI의 세션 보조 정보로 언어를 기본 공개하지 않는다. 인증 callback에서 Store를 직접 호출해 공통 정책·감사를 생략하거나 D를 기존 L/F/U/M의 원래 의미로 간주하는 구현은 금지한다.

## 공개 요청·응답 제안

기존 상호 인증된 연결의 보호 RPC 한 건을 사용한다. app 전용 client는 A 프로필을 확인한 뒤 requested scope를 accounts=[]와 tools=[`app.settings.display.get`]으로 줄인다. 이것은 등록의 grant·프로필을 변경하지 않으며 서버는 기존 교집합 방식으로 검증한다. `auth.begin`/`auth.finish`, transcript 형식, 고정 profile path, server pin, 기존 CNG key, receipt 0x06, 연결 소비 규칙은 변경하지 않는다. bootstrap/system/version/readiness/registration/setup 경로에는 설정 읽기를 추가하지 않는다.

JSON-RPC method는 `app.settings.display.get`이고 params는 다음 공통 도구 envelope다.

```json
{"schema_version":1,"request_id":"display-1","tool":"app.settings.display.get","params":{}}
```

`request_id`는 기존 ASCII ID 형식과 1..64-byte 상한을 따른다. outer RPC ID와 inner request ID는 각각 기존 규칙으로 검증하고, inner tool과 outer method는 정확히 일치해야 한다. 계정·설정 키·version·경로·lang·approved 등의 추가 필드는 거부한다. raw frame을 기준으로 이 메서드의 요청/응답은 각각 최대 4096 bytes다. 공백을 제거한 재직렬화 길이로 원본 상한을 대신하지 않는다.

성공 JSON-RPC result의 정확한 필드는 다음 셋이다.

```json
{"schema_version":1,"ok":true,"cli_language":"ko"}
```

`cli_language`는 필수 nullable이며 null과 누락을 구분한다. object 대신 array, duplicate/unknown/missing field, 잘못된 enum·null·schema_version·ok, BOM·trailing JSON·잘못된 UTF-8을 거부한다. 서버 오류는 기존 `{code,message}`이고 message는 code와 같은 고정 영어 문자열이다. 새로운 ErrorCode/CLI 종료 코드는 추가하지 않는다. client는 원본 bytes에서 strict하게 파싱하고 정확한 outer RPC ID와 result/error 배타성을 검사한다. 이 app 설정 도구에 메일 결과의 items/account_results/completeness를 억지로 붙이지 않는다. 공통 schema 적용 범위를 연관 계약에서 명시적으로 분리해야 한다.

## 실행·정책·감사 경계

기존 connection-owned 인증 → 요청 검증 → 최신 등록 재검증 → 감사 start commit → D scope 검사 → 공통 호출 한도/정책 → 설정 읽기 → 감사 terminal commit 순서로 실행한다. 새 AccountlessDisplayRead 분기를 공통 실행기에 명시적으로 추가하며, IPC callback에서 설정 Store API로 바로 건너뛰지 않는다.

현재 `AuditBegin::new`와 `check_accounts_scope`는 nonempty account를 요구한다. 이 제약을 모든 도구에서 완화하지 않는다. D만 0개 account refs를 허용하는 타입/생성자 및 감사 reader 검증을 추가해야 한다. 기존 모든 mail 도구의 nonempty account 불변 조건을 유지한다. 감사 결과 개수는 메일 건수가 아니므로 `None`이고, 감사에는 언어 값·설정 JSON을 넣지 않는다. 실패는 기존 stable code로 기록한다. 감사 시작/종료 실패를 표시 설정 성공으로 포장하지 않는다.

현재 등록별 `ReadPolicy`의 같은 60초 window와 한도를 공유하고, D도 허용된 조회 시도 한 번을 소비한다. 별도 무제한 display endpoint를 만들지 않는다. 최신 Store 등록의 client kind·활성 상태·epoch·키·scope를 재검증한다. 다른 client kind는 정책에서 `POLICY_BLOCKED`, D grant 없음은 `SCOPE_DENIED`다. 이 한 번의 조회가 이후 메일 요청의 인증/권한 proof를 대신하지 않는다.

Core는 기존 `Store::load_config` 한 번으로 검증한 설정을 읽고 순수 표시 DTO로 투영한다. 읽기 전후 settings·계정·메일·작업·등록·프로필은 같아야 하며 감사 start/terminal만 추가될 수 있다. provider/Secrets/worker/승인 queue를 호출하지 않는다. 비Windows 제품 경로는 기존대로 `CAPABILITY_UNSUPPORTED`다.

감사 tool allow-list와 accountless reader 및 scope 이력 때문에 root 소유의 v15→v16 migration이 필요하다. 현재 v15 DDL과 과거 v1~v4 scope validator를 고정 보존하고 모든 이전 migration chain, 역방향 FK closure, sequence high-water, 감사 capacity, old profile bytes를 보존해야 한다. migration이 grant를 부여하거나 settings를 초기화하지 않는다. 중앙 app 실행·감사 계약과 A 범위 계약에 따라 함께 검증한다.

## CLI 조회 시점과 fallback

먼저 전체 명령·옵션을 검증한다. 도움말/버전 처리나 잘못된 입력을 위해 프로필·CNG·pipe·Store를 열지 않는다. 기존 parser의 중복/미지원 옵션 거부를 유지한다.

| 상황 | 언어와 동작 |
| --- | --- |
| 유효한 명시 `--lang` | 해당 언어. 표시 설정 조회 0회 |
| JSON/NDJSON 모드 | 표시 설정 조회 0회. 명시 언어는 기존 error.message 번역에만 적용 가능 |
| 도움말/버전·빈/잘못된 명령 | 명시 언어 또는 en. 표시 설정 조회 0회 |
| 등록·범위 갱신·setup·core status·clients verify | 이 좁은 구현에서는 명시 언어 또는 en. 표시 설정을 위해 별도 인증/등록/UI를 시작하지 않음 |
| 정상 보호 메일 명령, `--lang` 없음, 기존 프로필에 D 없음 | en. 표시 설정 pipe/서명 조회 0회; 원래 명령은 기존 권한 규칙대로 실행 |
| 정상 보호 메일 명령, `--lang` 없음, 신뢰된 프로필에 D 있음 | 최소 조회 한 번. 검증된 en/ko만 적용 |
| 설정 없음·미기동·미등록·철회·stale profile·인증/정책/전송/파싱 실패·timeout | en. 자동 재조회·Core 실행·등록·프로필 복구·Hello 없음 |

fallback은 **선택적인 표시값을 얻지 못했다는 뜻**이다. 원래 메일 명령의 성공·오류·exit code를 대신 만들거나 auth/scope 실패를 우회하지 않는다. 원래 명령은 별도 기존 인증/공통 실행기로 판단한다. 서버의 `INTERNAL_ERROR` 전파/서비스 중단 규칙도 그대로이며, CLI의 언어 fallback이 DB 손상을 서버에서 정상 처리하도록 바꾸지 않는다.

표시 조회는 사람용 출력의 CLI invocation당 최대 한 번이며 결과는 프로세스 메모리에만 둔다. 새 프로필과 D가 있어도 optional lookup의 전체 비동기 budget은250ms다. 기존 I/O deadline보다 짧은 이 budget이 끝나면 future/connection을 폐기하고 en으로 진행한다. 동기 OS profile/CNG 함수가 멈추는 경우까지 강제 중단한다는 보장은 하지 않으며, 실제 지연은 별도 측정한다. 명시 `--lang`, JSON/NDJSON, 도움말/버전은 추가 조회 비용이 없다. 기존 한 연결 한 RPC 규칙을 유지하므로 설정 조회와 원래 메일 요청은 별개 연결이며 이를 한 호출로 보고하지 않는다.

조회 실패 원문·프로필 경로·서버 message를 stdout/stderr에 추가하지 않는다. JSON 출력에도 별도 언어 조회 응답·경고 행을 섞지 않는다. 기계용 결과의 필드명/ID/상태/오류 코드는 그대로이며 기존 JSON error.message의 번역 정책도 바꾸지 않는다.

## 제안 API와 파일 소유권

다음 이름은 root 계약 검토 뒤 구현 기준으로 확정한다. Contracts/IPC client/CLI는 Domain/Store에 의존하지 않는다.

- `hantong_contracts::display_settings::{CliDisplayLanguage, CliDisplaySettings}`. 언어 enum은 En/Ko와 `parse/as_str`, snapshot은 private `Option<CliDisplayLanguage>`와 `new/language`를 제공한다. Clone/Eq/PartialEq, 고정 또는 redacted Debug이며 인증 proof가 아니다.
- Application의 새 `ToolRequest::DisplaySettingsGet`와 `ExecutionContext::read_cli_display_settings`, `render_cli_display_settings`. 순수 renderer는 위 세 result 필드만 만든다. 계정 없는 실행 분기는 root가 공통 audit/scope/policy와 함께 소유한다.
- `hantong_ipc_client::display_settings::{get_default, parse_display_settings_response}`. 후자는 `(&[u8], expected_rpc_id:&str)->Result<CliDisplaySettings,ErrorCode>`. 전자는 고정 trusted profile에서 D를 사전 확인하고 A 계약의 `authenticate_default_app_read(AppReadTool::DisplaySettingsGet)`과 현재 authenticated exchange를 사용한다. 임의 scope/key/pin/profile 경로를 받지 않는다. 내부 async helper를 사용해 optional CLI budget을 적용한다.
- CLI 신규 `display_language.rs`는 명시 언어/명령 종류/조회 결과를 받아 부작용 없이 언어를 정하는 helper와 시험을 소유한다. `main.rs`의 완전한 명령 검증 뒤 연결 지점은 root 또는 지정된 한 담당자가 소유한다.

권장 분담: 공개 담당은 새 Contracts/Application/client/CLI 모듈과 각각 시험, scope 담당은 A 계약의 auth/helper/profile/native 표시·proof·이력 검증, root는 감사 enum/DDL/migration/정책/공통 실행기/Core/실제 pipe를 맡는다. lib/module export와 CLI main 같은 공유 파일은 root가 순차 통합한다. 기존 `load_config`의 DB 보존 동작은 재사용하고 새 범용 SQL/설정 키 API를 공개하지 않는다.

## 필수 시험과 구현 착수 조건

1. 우선순위 표 전체와 조회 횟수: 명시 en/ko, JSON/NDJSON, 도움말/버전/invalid/bootstrap 경로 0회, D 없는 L/F/U/M은 pipe 0회, 허용된 정상 사람용 명령 최대1회. 영어/한국어/JSON·no-input을 함께 검사한다.
2. strict wire: duplicate/missing/unknown/null/array 대체, RPC ID·inner tool 불일치, 원본 공백 포함4096-byte 경계, malformed error, 잘못된 language·UTF-8·BOM·trailing을 거부한다.
3. 실제 Windows 서명 pipe와 격리 Store: D 없는 L/F/U/M 거부, D 가진 CLI의 en/ko/null, 빈 계정 집합에서 D만 조회 가능, GUI/MCP/철회/epoch/key/pin 변조 거부, 호출 한도 공유, 감사 성공·실패, 조회 중 provider/worker/UI 호출0을 검증한다. 계정 없음이 기존 mail 도구를 허용하지 않음을 함께 검사한다.
4. 존재하는 손상 설정은 INTERNAL_ERROR, 부재는 null이며 자동 초기화 없음. settings·메일·등록·profile bytes 무변경과 audit 예외를 실제 행 snapshot으로 비교한다.
5. 취소/연결 단절/250ms 예산 뒤 다음 메일 요청을 언어 조회의 성공·세션으로 처리하지 않음, 실패 원문 미노출, 단일 RPC/receipt 규칙 보존을 확인한다. 실제 1초 제품 시작 성능 달성으로 간주하지 않는다.
6. A scope 계약의 명시 갱신, 취소/거절, snapshot 대체, 과거 pending/staged 재개, L/F/U/M bytes 보존, 역사적 hash validator·migration rollback/전체 FK 보존을 검증한다. 실제 사용자 profile·Hello·외부 계정/네트워크는 사용하지 않는다.

착수 전 A scope 계약과 root 소유 accountless 공통 감사/정책·v16 DDL 계약의 일관성을 독립 검토한다. 해당 경계 없이 화면 문구만 구현하거나 인증 callback을 우회해 연결하지 않는다. 검토·계약 작성은 현재 개발 위임 범위이며 사용자에게 같은 일반 개발 허가를 다시 묻는 단계가 아니다.
