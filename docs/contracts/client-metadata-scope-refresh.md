# 기본 CLI 로컬 메타데이터 범위 갱신 v4

상태: 기준선 `f70cdf3`의 전체 자동 검증 1,150개 통과 이후 작성하고 독립 검토 보완을 반영한 **구현 기준·사람 검토 필요**. 연속 개발 위임에 따른 설계이며, 실제 사람 보안 검토나 사용자 권한 부여의 증거가 아니다. 관련 요건은 CLI-15·MCP-03·SEC-03·CLI-01·SYNC-02다.

이 문서는 [기존 범위 갱신 계약](client-scope-refresh.md)의 v1/v2/v3 이력·승인·보호 파일 배포 규칙을 보존하면서 새 요청의 M 도구 집합과 v4 hash를 정의한다. 로컬 조회 입력·응답·정보 범위는 별도 [IMAP 로컬 메타데이터 검색 계약](imap-metadata-search.md)이 정한다. 여기서 권한 경로를 정의하는 것은 검색 구현이나 전체 제품 수용 기준의 완료를 뜻하지 않는다.

## M 권한과 기존 권한 보존

도구 집합은 아래의 정확한 정렬 배열만 허용한다. 부분 집합·중복·다른 순서·임의 도구는 거부한다.

| 집합 | 정확한 도구 배열 |
| --- | --- |
| L | `["mail.accounts.list"]` |
| F | `["mail.accounts.list","mail.folders.list","mail.folders.refresh"]` |
| U | `["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]` |
| M | `["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]` |

공통 API `hantong_contracts::auth::default_cli_metadata_tools()->Vec<String>`는 M을 반환한다. 기존 `default_cli_legacy_tools`, `default_cli_folder_tools`, `default_cli_uid_tools`의 값은 바꾸지 않는다. `is_supported_default_cli_toolset`은 L/F/U/M만, `is_supported_default_cli_toolset_transition(old,target)`은 양쪽의 정확한 집합 검증 후 L≤F≤U≤M의 단조 전이만 허용한다. 같은 도구 집합은 계정 목록이 달라질 때만 유효하다. 전체 계정·도구가 같은 no-op과 모든 downgrade는 거부한다. 빈 계정 목록은 계속 계정 권한 0개다.

M의 추가 권한은 정확한 계정·사서함의 **로컬에 저장된 메타데이터를 클라이언트에 공개하는 `mail.messages.search`**다. 제목·보낸 사람 헤더 등의 노출을 포함하므로 기존 U에 이를 암묵적으로 추가하지 않는다. L/F/U는 이 도구에서 SCOPE_DENIED이며 기존 `mail.sync.page`의 입력·응답·권한은 유지한다. 검색 결과를 얻기 위해 자동 page/reconcile/recover를 실행하거나 별도 권한을 자동 요청하지 않는다. 이 권한으로 서버 검색·메일 본문 조회·메일 발송·서버 변경·외부 AI 전송을 허용하지 않는다.

최초 default CLI 등록은 계속 L이다. 기존 등록·프로필을 읽거나 프로그램을 업그레이드하는 것만으로 M으로 변경하지 않는다. 키·pin·최초 등록 request·registration ID·승인 metadata를 보존하며 read_only=true, requests_per_minute=60을 유지한다. 다른 종류·표시명의 등록 정책을 이 변경으로 확대하지 않는다.

## 명시 요청과 기존 결정의 재개

새 `hantong clients refresh`는 기존 `registration.scope.request`와 native 보호 확인 경로를 사용한다. 공개 입력은 기존 protocol_version=1뿐이며 CLI/MCP가 계정·도구·승인 여부·profile 경로를 공급하지 않는다. status 응답·16KiB frame·strict parser·queue·timeout·rate 규칙은 기존 계약 그대로다.

Core는 기존 staged가 있으면 저장된 결정을 복구하고, 만료되지 않은 유효한 pending이 있으면 그 저장된 목표를 다시 표시한다. v1/v2/v3 pending/staged의 hash_version·payload_hash·계정·도구·시각·ID를 바꾸지 않는다. pending 재표시는 current active 등록과 저장된 base 전체의 일치, 현재 registrable 계정 snapshot과 target accounts의 일치, pin을 재검증한다. staged 복구는 exact provisioning 등록·old grants·target epoch/version, 저장된 target 계정의 존재, exact old/new profile·기존 key·pin을 검증한다. 이미 staged된 결정에는 fresh registrable 계정 snapshot 일치를 새로 요구하지 않는다. 각 경로의 기존 재검증이 실패하면 기존 오류로 중단하며 새 M 요청으로 대체하지 않는다.

재사용할 pending/staged가 없는 **fresh 요청만** 현재 등록 가능한 계정 snapshot과 M을 원하는 범위로 만든다. 역사적 결정의 완료 뒤 M이 필요하면 별도의 새 명시 요청과 native 확인을 거친다. 이미 M이고 계정도 같으면 CONFLICT다. status·startup은 pending을 승인하거나 M 요청을 생성하지 않는다. startup은 이미 staged된 exact old/new profile 결정을 끝내는 기존 제한된 복구만 수행한다.

`create_cli_scope_refresh_with_scope`의 API는 유지하며 새로 생성하는 행은 선택된 허용 도구 집합과 관계없이 hash_version=4다. accounts-only `create_cli_scope_refresh`는 L을 요청하는 기존 호환 wrapper로 유지한다. 따라서 F/U/M에서 이 wrapper를 통한 downgrade는 거부한다.

## Native 표시와 승인 증거

`ScopeRefreshReview::new_with_tools`와 private-field/Clone/Eq snapshot을 재사용한다. 전체 old/target 계정·도구, refresh ID/CAS version, base 등록 ID·최초 request ID·version·epoch, client key hash·server pin hash, created/expires를 결속한다. 도구만 U→M으로 바뀌어도 별도 검토 대상이다. 기존 `new`의 L/L 호환 의미는 유지한다.

영어·한국어 화면은 전체 기존/새 계정·도구, epoch 증가와 기존 연결 종료를 표시한다. target이 M이면 추가 설명에 다음 의미를 모두 포함한다.

- 로컬 캐시에 이미 저장된 메타데이터와 제목·보낸 사람 헤더를 CLI 등 해당 등록 클라이언트에 공개한다.
- 본문을 읽거나 서버 검색·발송·변경을 실행하지 않는다.
- 조회 결과는 저장된 범위이며 원격 사서함 전체나 현재 최신 상태를 보장하지 않는다.

기존 U 설명과 L/F/U의 기존 summary bytes는 유지하며 M 설명만 추가한다. Language·Theme 선택은 기존 영/한·라이트/다크/운영체제 설정을 따른다.

전체 exact snapshot과 해당 설명이 native review control에 표시되고 paint 검증을 통과하기 전에는 verification을 시작하지 않는다. 동일 snapshot·OS 사용자/로그온·session/nonce·UTC/단조 시각 유효성을 검증하고 OS Verified 결과에서만 기존 opaque single-use `hantong_approval_windows::VerifiedScopeRefresh`를 만든다. 새로운 공개 `ScopeRefreshProof` 생성 API는 추가하지 않는다. Core의 `consume(expected)`는 M↔U, old 또는 target 도구만 바뀐 경우도 포함하여 모든 snapshot 대체를 거부한다. CLI/MCP·bool·문구·JSON·프로필 파싱·테스트 가짜 결과를 제품 승인 증거로 사용할 수 없다.

닫기/취소는 pending과 기존 grant를 유지하고, 명시 Deny·만료는 기존 전이만 기록한다. 제품 native 경로 구현과 시험은 실제 사용자 프로필·Hello 실행을 허가하지 않는다.

## v4 hash와 과거 이력 검증

새 hash domain은 `mail-hantong.cli-scope-refresh.v4\0`이며 마지막 표기는 NUL byte 하나다. domain 뒤의 필드·순서·길이·정수 encoding은 기존 v3와 동일하다. 즉 기존 identity/version/epoch/key/pin/time·old/target accounts 뒤에 old tools와 target tools를 각각 count(u32BE), 각 ASCII 문자열 byte length(u32BE)+bytes로 붙인 SHA-256이다. hash는 변경 감지 자료이며 승인 증거가 아니다.

저장된 hash_version에 따라 다음 집합을 정확히 검사한다. 내용을 보고 버전을 추정하거나 과거 hash를 새 domain으로 재계산하지 않는다.

| hash_version | 허용 base/target 도구 | hash 의미 |
| --- | --- | --- |
| 1 | L/L만, 서로 다른 계정 목록 | 기존 v1 domain·계정 encoding 그대로 |
| 2 | 각각 L/F, 단조 전이 | 기존 v2 domain·전체 범위 encoding 그대로 |
| 3 | 각각 L/F/U, 단조 전이 | 기존 v3 domain·전체 범위 encoding 그대로 |
| 4 | 각각 L/F/U/M, 단조 전이 | 새 v4 domain·v3와 동일한 후속 encoding |

`scope_refresh/storage.rs`의 `historical_toolset`은 버전별 닫힌 집합을 직접 검사한다. 특히 현재 v3가 공통 최신 allow-list를 호출하는 부분을 **M helper 확장 전에** L/F/U의 명시 비교로 고정한다. v4도 위의 네 집합으로 고정하여 후속 도구가 자동 유입되지 않게 한다. private `historical_transition(old,target,hash_version)`도 버전별 membership과 고정 L≤F≤U≤M 순서를 검사한다. 과거 v1~v4 reader는 변할 수 있는 최신 transition helper를 호출하지 않는다. 새 버전은 기존 분기를 넓히는 방식으로 추가하지 않는다.

기존 `validate_history_v8`은 이전 layout/v1만, `validate_history_v11`은 v1/v2만 유지한다. 새 crate-private `scope_refresh::validate_history_v14(&rusqlite::Connection)->rusqlite::Result<()>`는 현재 열 layout에서 최대 hash_version=3과 L/F/U만 검증한다. Store schema 검증은 v12/v13/v14에 이 함수를 사용하고, v15 이상 현재 validator만 최대4를 허용한다. default 등록의 최초 L 범위와 각 active 이력의 정확한 계정·도구 연속성, staged의 old grant, quota와 phase/시각/CAS 검사는 유지한다.

## DB v15 이전 제안과 효과 순서

DB v14 전체 DDL은 `migrations/schema-v14.sql`에 보존하고 Store의 `SCHEMA_V14`로 참조한다. `CURRENT_SCHEMA_VERSION=15`, `SchemaState::V14` 감지·검증 분기와 v14→v15 migration을 추가한다. v13→v14의 목적 schema도 현재 v15 DDL 대신 고정 `SCHEMA_V14`를 사용한다. 모든 기존 버전의 migration chain은 기존 중간 단계와 검증을 유지한 채 마지막 v14→v15를 거친다. 새 `014-to-015.sql`과 현재 `db-schema.sql`을 일치시키며 v12~v14의 scope validator는 최대3, 현재 v15는 최대4로 구분한다. 기존 암호화 백업→BEGIN IMMEDIATE→고정 DDL 변경→전체 schema/history/FK 검증→commit 순서를 유지한다. 백업 충돌·DDL 실패·검증 실패는 기존 원본과 검증된 백업을 보존한다.

`client_scope_refreshes`는 동일 열을 가진 표로 재구축한다. hash_version CHECK는 1/2/3/4, 양쪽 tools CHECK는 정확한 L/F/U/M canonical JSON이다. 별도 버전 CHECK는 위 표의 제한을 모두 명시하며 `hash_version=3`을 무조건 허용하는 분기를 남기지 않는다. 단조 전이는 L→L/F/U/M, F→F/U/M, U→U/M, M→M이며 전체 scope no-op은 거부한다. DEFAULT hash_version=1과 L/L 기본값, 두 unique index, FK·TTL·quota·시각·phase/CAS·정수 상한을 유지한다. 기존 행은 모든 열·bytes·hash·version을 그대로 복사한다.

`mail.messages.search` 감사 enum을 허용하기 위한 `audit_starts` 재구축은 아래 FK 참조 관계 전체를 함께 보존한다. audit를 참조하는 표만 바꾸고 뒤의 참조를 옛 표에 남기지 않는다.

| 재구축 집합 | 관계 |
| --- | --- |
| audit_starts | 감사 도구 allow-list 확장 대상 |
| audit_start_accounts, audit_terminals | audit_starts 자식 |
| folder_refresh_requests, imap_uid_page_requests, imap_uid_recovery_requests, imap_uid_reconcile_requests | audit_starts를 참조하는 영속 작업 |
| imap_uid_epochs | recovery_requests를 참조 |
| imap_uid_reconcile_heads | epochs 및 reconcile_requests를 참조 |
| imap_uid_reconcile_messages | reconcile_requests를 참조 |

위 열 개 표에 별도 `client_scope_refreshes`가 더해진다. reconcile_heads↔reconcile_requests의 순환 FK를 포함하므로 기존 deferred-FK migration 방식으로 처리하고 foreign_keys를 끄거나 FK를 제거하지 않는다. 모든 데이터·원장 상태·미완료 audit·원본 metadata·epoch·관측을 보존하며 자동 실행하지 않는다. audit AUTOINCREMENT high-water와 원래 sequence 행 부재, audit_capacity 값, `imap_uid_epochs_current`·`imap_uid_reconcile_source_latest`·`imap_uid_reconcile_messages_execution` index와 두 scope unique index도 보존한다. 새 grant를 migration으로 INSERT하지 않는다.

전이 API는 기존 `stage_cli_scope_refresh`·`activate_cli_scope_refresh`를 재사용한다. native proof 소비 후 stage는 epoch/version을 증가시키고 provisioning으로 바꾸되 **old 계정·도구 grant를 유지**한다. exact old/new 보호 프로필 발행과 guard 확보 후 activate만 target grant를 원자 교체한다. tool INSERT 실패도 전체 rollback한다. 최초 request/key/pin/승인 metadata는 유지하고 최초등록 activate 경로로 이 전이를 우회하지 않는다.

## 프로필 호환

`ParsedClientProfile::new_with_scope`, `from_json`, `to_json`과 `AuthenticatedDefaultProfile`의 기존 전체 scope 보존 경로를 재사용한다. profile schema_version=1, scope RPC protocol_version=1, 32KiB profile 상한, strict required/unknown/duplicate/null/type 검사를 유지한다. 공통 정확한 toolset 검증에 M을 추가하되 기존 L/F/U profile의 canonical bytes는 바꾸지 않는다. `new`는 L wrapper 그대로다.

Core는 과거 pending/staged 복구에서도 저장된 base.scope와 target_scope만으로 old/new bytes를 만든다. 현재 desired M helper로 이 bytes를 다시 구성하지 않는다. 기존 보호 publisher의 ACL/reparse/handle/no-replace/guard·backup 복구·activation 후 정리 규칙을 유지한다. 프로필이 M으로 파싱되더라도 현재 Store 등록의 grant·epoch·상태와 인증된 요청 scope 검증을 통과하기 전에는 권한을 얻지 못한다.

## 수용 시험과 병렬 소유권

구현은 시험을 먼저 작성하고 다음을 합성 fixture로 검증한다.

1. 정확한 M 순서와 전체 L/F/U/M 전이 행렬, no-op/downgrade/부분/중복/send 거부, 최초 등록 L 및 L/F/U의 search 거부.
2. 실제 v1/v2/v3 pending/staged/active 이력의 hash·전체 열·old/new profile bytes 보존. 새 fixture를 v4로 바꿔 과거 시험의 기대값만 수정하는 것으로 대체하지 않는다. v1/v2/v3에 M을 넣고 해당 옛 domain으로 hash를 재계산해도 거부한다.
3. U→M tool-only 전이, stage old grant 유지, active target 정확 교체, search-tool INSERT 실패를 포함한 rollback, 이력 provenance·구 epoch·stale/expired/revoked/suspended 거부.
4. 영/한 M 설명, U summary 보존, paint 전 verification 금지, M/U snapshot 대체 거부, 기존 opaque proof/OS context/시간/단일 소비 경계.
5. L/F/U/M strict profile round-trip, 기존 canonical bytes 고정, parsed profile만으로 권한을 얻지 못함, 기존 pending/staged 재개와 startup에서 M 자동 확대 없음.
6. v14→v15 실제 SQLCipher migration의 모든 재구축 표/순환 FK/index/audit sequence 보존, 대기 job 무실행, backup 충돌·DDL/검증 실패 원본 보존, fresh DDL과 migration 후 DDL 일치.

scope 담당은 `hantong-contracts/src/auth.rs`, Store의 `scope_refresh.rs`·하위 storage/tests, approval review/resources, approval-windows의 proof/paint 시험, IPC profile/trusted-profile 시험을 소유한다. 기존 native/보호 publisher 제품 경로는 필요한 변경이 명시되기 전까지 재사용한다. root는 AuditTool enum·정책·감사 DDL·migration·Store lib schema 선택·Core·통합 검증·진행 기록을 소유한다. Core의 fresh desired helper 변경은 root가 별도 위임할 수 있다. 검색 담당은 별도 검색 계약과 조회 구현을 소유한다. 공유 파일 변경이 필요하면 담당자에게 먼저 알린다.

독립 검토 보완을 반영한 이 계약에 따라 각 담당자가 부여받은 범위만 구현한다. 실제 사용자 Core/DB/profile·Hello·계정 로그인·메일·외부 공개는 개발 검증에 사용하지 않는다. 새 의존성은 추가하지 않으며 보안 경계는 자동 시험 통과 후에도 **사람 검토 필요**로 기록한다.
