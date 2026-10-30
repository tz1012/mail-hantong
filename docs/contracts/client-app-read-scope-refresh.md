# 기본 CLI 앱 읽기 범위 갱신 v5

상태: M1-05/M1-10/M1-12의 표시 언어·비민감 진단 읽기를 위한 **계약 초안·사람 검토 필요**. 연속 개발 위임과 root의 A 범위 설계에 따라 작성했다. 실제 권한 부여, 실제 native 확인 실행, 사람 보안 검토 완료의 증거가 아니다. 관련 요건은 CLI-15·CLI-16·MCP-03·SEC-03·OPS-05다.

[v4 범위 갱신](client-metadata-scope-refresh.md)의 등록/proof/보호 프로필/중단 복구 절차를 재사용하고 v1~v4 이력의 의미·bytes를 고정한다. [표시 설정 조회](cli-display-settings.md)는 `app.settings.display.get`의 데이터와 fallback을 정의한다. `app.diagnostics.snapshot`의 입력·비민감 결과·본인 등록/현재 scope 필터는 별도 진단 계약이 정의한다. 공통 app 실행·감사·v16 migration은 root 소유의 연관 계약과 함께 검토한다.

## A 집합과 최초 등록 보존

기존 L/F/U/M의 정확한 값은 v4 계약 그대로다. 새 A의 canonical 배열은 다음과 같으며 부분 집합·중복·다른 순서·임의 도구는 지원하는 default profile로 인정하지 않는다.

```json
["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]
```

`hantong_contracts::auth::default_cli_app_read_tools()->Vec<String>`는 A를 반환한다. 기존 `default_cli_legacy_tools`, `default_cli_folder_tools`, `default_cli_uid_tools`, `default_cli_metadata_tools`는 변경하지 않는다. 최신 `is_supported_default_cli_toolset`은 L/F/U/M/A, 최신 transition helper는 각각의 정확한 membership과 L≤F≤U≤M≤A를 검사한다. 도구 downgrade와 전체 scope no-op은 거부하며, 같은 도구 집합의 계정 변경은 기존 규칙을 따른다. 빈 계정은 계정 권한 0개이고 전체 계정의 별칭이 아니다.

최초 default CLI 등록은 L, read_only=true, requests_per_minute=60을 유지한다. 현재 기존 L/F/U/M은 두 app 도구를 허용하지 않는다. migration·시작·프로필 파싱·CLI 실행이 A를 부여하거나 기존 프로필을 다시 쓰지 않는다. 현재 제품의 등록 종류·이름·키 selector·server pin 규칙은 바뀌지 않는다.

A는 기존 M의 메일 권한과 두 앱 읽기 권한을 하나의 명시 native 검토에서 제시하는 고정 묶음이다. 표시 설정 하나만 선택하는 권한 편집은 이번 범위가 아니다. L/F/U에서 A로 갱신할 때 기존 M의 제목·보낸 사람 헤더 공개까지 포함된다는 사실을 화면에서 숨기지 않는다. 거절하면 기존 권한·`--lang` 사용·en fallback이 유지된다.

## Fresh 요청과 과거 결정

기존 `hantong clients refresh` → `registration.scope.request`의 정확한 protocol_version=1 입력과 status/16KiB frame/strict codec/단일 UI queue/응답을 유지한다. CLI가 임의 계정·도구·scope·승인 bool·경로를 보내지 않는다. 새 CLI 옵션·동의 프로토콜·approval 발급 API는 추가하지 않는다.

새로 시작할 pending/staged가 없는 **fresh 요청만** 현재 registrable 계정 snapshot과 A를 목표로 만든다. 기존 pending/staged가 있으면 저장된 목표를 그대로 사용하며 v1~v4의 L/F/U/M을 A로 바꾸지 않는다. 동일 A와 동일 계정은 `CONFLICT`다. 언어 fallback·진단 조회가 이 명시 refresh를 자동 실행하지 않는다.

pending의 재표시는 현재 active 등록과 저장된 base 전체, 현재 registrable 계정 snapshot과 저장 target accounts, 기존 pin을 검증한다. staged 복구는 exact provisioning 등록/old grant/target epoch/version, 저장 target 계정의 존재, exact old/new profile bytes/기존 key/pin을 검증한다. 이미 staged된 동의에 fresh registrable 계정 snapshot 일치를 추가하지 않는다. 과거 결정을 새 fresh 요청으로 교체하거나 실패를 새 동의로 숨기지 않는다.

기존 `create_cli_scope_refresh_with_scope` API는 유지하고 새로 생성하는 행의 hash_version은5다. accounts-only `create_cli_scope_refresh`는 기존 L 호환 wrapper로 남기며 downgrade를 허용하지 않는다. fresh Core의 desired helper만 M에서 A로 바꾸고, 역사적 old/new profile을 만드는 경로는 저장 snapshot만 사용한다.

## 표시와 native proof

기존 `ScopeRefreshReview::new_with_tools`는 base/target 전체 계정·도구, refresh ID/version, 등록 ID/최초 request ID/version/epoch, key hash/pin hash, created/expires를 그대로 결속한다. 기존 opaque `VerifiedScopeRefresh`의 paint/OS context/nonce/UTC·단조 만료/일회 consume 규칙을 재사용한다. A↔M, 두 app tool 중 하나 삭제/추가, account 교체도 snapshot 대체로 거부한다.

L/F/U/M의 기존 summary bytes는 보존한다. A summary는 기존 M의 메타데이터 설명과 다음 두 의미를 **영어·한국어**로 함께 표시한다.

- 표시 설정: “Read the saved CLI language (English or Korean). This does not change settings.” / “저장된 CLI 표시 언어(영어·한국어)를 읽습니다. 설정을 변경하지 않습니다.”
- 진단: “Read bounded diagnostic counts and status codes for this registered client within its current account scope. No mail content, headers, account identifiers, file paths, or raw logs are returned or transmitted.” / “이 등록 클라이언트와 현재 계정 범위에 한정된 진단 건수·상태 코드를 읽습니다. 메일 본문·헤더·계정 식별자·파일 경로·원시 로그를 반환하거나 전송하지 않습니다.”

이 범위는 보호 정책 변경·로그 파일 읽기/내보내기·외부 전송·모델 사용·발송·메일 변경·승인 발급을 허용하지 않는다. 전체 exact target과 설명이 실제 control에 표시되고 paint 검증을 통과하기 전 verification을 시작하지 않는다. 닫기/취소/거절의 기존 상태 전이를 유지하고 자동 승인하지 않는다.

사용자가 앱에서 A를 부여할 때의 기존 native 확인은 실제 제품 권한 결속이다. 개발 중 계약/구현/가짜 검증을 진행하기 위한 채팅 승인을 새로 요청하는 절차가 아니며, agent가 실제 사용자 Hello/profile을 실행하는 근거가 아니다.

## v5 hash와 역사적 validator

새 hash domain은 `mail-hantong.cli-scope-refresh.v5\0`이며 끝은 NUL byte 하나다. 그 뒤 필드·순서·길이·정수 encoding은 v4와 같다. ID/version/epoch/key/pin/time·old/target accounts·old/target tools를 같은 순서로 SHA-256에 넣는다. hash 자체는 승인 proof가 아니다.

| hash_version | 고정 허용 집합과 전이 |
| --- | --- |
| 1 | 기존 L/L, 서로 다른 계정 목록 |
| 2 | 각각 L/F, 기존 고정 단조 순서 |
| 3 | 각각 L/F/U, 기존 고정 단조 순서 |
| 4 | 각각 L/F/U/M, 기존 고정 단조 순서 |
| 5 | 각각 L/F/U/M/A, 고정 L≤F≤U≤M≤A |

`historical_toolset`과 `historical_transition(old,target,hash_version)`는 v1~v5를 각각 닫힌 집합으로 검사한다. v1~v4 reader에 최신 helper를 호출해 A가 유입되는 분기를 만들지 않는다. v5도 이번 다섯 집합으로 고정하며 미래 도구가 자동 추가되지 않는다. 과거 행의 domain/version/hash/전체 열을 다시 계산하거나 수정하지 않는다.

새 crate-private `scope_refresh::validate_history_v15(&rusqlite::Connection)->rusqlite::Result<()>`는 현재 layout에서 최대 hash_version=4만 허용한다. 기존 v8/v11/v14 validator의 의미는 유지한다. Store의 schema v15 검증 분기는 새 v15 validator를 사용하고 v16 현재 validator만 최대5를 허용한다. 등록의 최초 L, active chain, staged old grants, quota/TTL/CAS/시각은 기존대로 strict하게 검증한다.

## v16 DDL·전이·프로필

root는 현재 v15 DDL을 `migrations/schema-v15.sql`/`SCHEMA_V15`로 동결하고 `SchemaState::V15`를 추가한다. v14→v15의 목적을 고정 SCHEMA_V15로 바꾸고 모든 옛 chain 뒤에15→16을 붙인다. 새15→16 SQL과 현재 DDL의 일치를 검증한다. 기존 SQLCipher 백업→IMMEDIATE transaction→DDL/data copy→strict schema/history/FK 검증→commit 경계를 유지한다.

scope table의 hash_version CHECK는1..5, 양쪽 tool CHECK는 정확한 다섯 canonical JSON, 버전별 membership/단조 제약은 위 표대로다. DEFAULT1과 L/L 기본값, unique/FK/TTL/quota/phase/시각/CAS를 보존한다. 기존 모든 rows/JSON/hash/versions는 그대로 복사한다. 기존 v15의 A 없는 row를 v5로 downlabel하거나 fixture 기대값만 갱신해 과거 보존을 주장하지 않는다.

두 app 감사 enum 추가에 필요한 audit_starts 및 역방향 FK closure 재구축은 root의 중앙 계약을 따른다. v15의 audit_children/영속 jobs/epochs/reconcile 순환 FK, sequence high-water와 row 부재, audit capacity, indexes를 유지하며 foreign_keys를 끄지 않는다. app 감사의 계정 참조0 예외는 두 app tool에만 적용한다. mail 도구의 기존 nonempty 검사/과거 audit 해석을 완화하지 않는다. migration은 grant·새 scope 요청·감사 사건·settings·자동 네트워크 작업을 생성하지 않는다.

stage는 native proof 소비 뒤 version/epoch를 증가시키고 provisioning 상태에서 **old grant를 유지**한다. exact 보호 new profile 발행과 guard 확보 후 activate만 target grant로 바꾼다. app tool INSERT 실패를 포함해 부분 전이를 rollback한다. 최초 request/key/pin/승인 metadata는 보존한다.

`ParsedClientProfile::new_with_scope/from_json/to_json`은 A를 추가로 허용하되 schema_version1/32KiB/strict required/duplicate/unknown/null/type 검사를 유지한다. new의 최초 L wrapper와 L/F/U/M canonical bytes는 바꾸지 않는다. profile을 파싱했다고 A를 얻는 것이 아니며 Store 현재 등록·epoch/state/key와 상호 인증 교집합을 통과해야 한다.

## App 요청의 최소 인증 scope

`hantong_contracts::auth::AppReadTool::{DisplaySettingsGet,DiagnosticsSnapshot}`는 고정 두 ID를 `as_str()`로 제공하는 순수 enum이다. 도구가 인증 proof라는 뜻은 아니다. `hantong_ipc_client::authenticate_default_app_read(AppReadTool)`는 기존 고정 trusted profile/key/pin loader를 재사용하고 profile에 해당 tool이 없으면 서명/pipe 전에 `SCOPE_DENIED`다.

이 helper의 requested scope는 accounts=[]와 tools=[선택한 app tool]이다. arbitrary string/scope/path/key/pin을 받지 않는다. `AuthenticatedDefaultProfile`은 실제 요청 subset을 보관하고 기존 handle 수명·one exchange 규칙을 유지한다. 기존 `authenticate_default()`의 전체 profile scope 의미와 mail 호출은 그대로다. 두 helper가 같은 내부 profile 검증을 사용하되 profile bytes를 새 subset으로 저장하지 않는다.

Core는 app의 명시적인 global-scope 검사에서 현재 등록 재검증과 session∩current tool grant를 확인한다. 여기서 계정0은 전역 설정/본인 진단에 대한 고정 모델이며 다른 도구의 전체 계정 접근이 아니다. 기존 `check_account_scope/check_accounts_scope`는 빈 목록을 계속 거부한다. app 전용 helper는 최소 subset을 요청하지만 서버가 보관한 최신 등록의 계정 scope를 없애는 것은 아니다.

특히 진단은 current authenticated registration ID와 **현재 Store 등록의 허용 계정 집합**으로 제한한다. app session의 빈 requested accounts를 전체 계정으로 취급하거나 진단 범위를 빈/전체로 임의 변환하지 않는다. 정확한 필터와 malformed history 처리·결과 budget은 진단 소유 계약에서 고정한다. CLI/MCP가 registration ID나 임의 계정 목록을 진단 입력으로 넘기지 않는다.

두 도구는 공통 coordinator의 최신 등록 재검증→감사→tool grant→공유 ReadPolicy→서비스→감사를 따른다. 같은 등록의 app/mail 호출은 같은60rpm window를 공유한다. 공통 정책을 우회하는 직접 callback·별도 unlimited limiter·등록별 예외를 만들지 않는다. app read를 성공했다고 후속 mail 권한이 생기지 않는다.

## 시험·소유권

1. exact A와 전체5×5 전이, no-op/downgrade/partial/duplicate/send 거부, 최초L와 기존4집합의 두 app 도구 거부.
2. 실제 v1~v4 pending/staged/active fixtures의 전체 hash/row/old-new profile bytes 보존. 옛 domain으로 A를 재해시해도 거부. fresh만A, 옛 pending/staged target 그대로 재개. 같은 계정 M→A도 별도 확인.
3. stage old grant 유지, target tool INSERT ABORT/IGNORE rollback, epoch/key/pin/등록 상태 변경 차단, no-op 거부, quota와 시간 경계.
4. A 영/한 설명·기존M summary 보존, paint 전 verification 금지, A↔M/두 tool 교체/계정 교체·nonce·time·단일 consume 거부. 실제 사람 Hello는 실행하지 않음.
5. old profile bytes 보존과 A strict roundtrip, subset helper가 exact app1개/accounts0만 서명, 기존 mail helper가 전체 scope 유지, 다른 key/pin/시각/registration 거부. profile의 grant 유무는 서버 현재 grant 검사를 대체하지 않음.
6. SQLCipher15→16 전체 FK/rows/index/sequence/capacity 보존, backup 충돌·DDL/검증 실패 원본 보존, empty/current schema 일치, 과거 mail audit의 nonempty 유지와 app-only0 허용. startup에서 pending 동의/네트워크 재실행0.

scope 담당의 예정 소유권은 Contracts auth/helper, Store scope_refresh와 storage/tests, Approval review/resources, ApprovalWindows proof/paint 시험, IPC profile/trusted-profile helper/tests다. root가 Store lib/DDL/AuditTool/정책/공통 실행기/Core desired helper/통합 검증을 소유한다. 실제 파일별 구현 배정 전에는 제품 파일을 수정하지 않는다. 다른 담당자가 표시/진단 DTO 및 app service를 소유한다. shared module export는 지정된 한 담당자가 통합한다.

기존 오류 코드·의존성만 사용한다. 보안 경계는 자동 시험이 통과해도 **사람 검토 필요**이며, 실제 사용자 DB/profile·Core·Hello·계정·메일·외부 공개를 개발 검증에 사용하지 않는다.
