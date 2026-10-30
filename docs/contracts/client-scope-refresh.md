# 기본 CLI 계정·도구 범위 갱신 v3

상태: 2026-10-30 v12 UID 페이지 공개 경로의 권한 선행 계약, **사람 검토 필요**. 연속 개발 위임에 따라 구현한다. v1/v2의 기존 이력·hash·복구 의미는 보존하고, 아래 v3 규칙을 새 요청에 적용한다. UID 페이지 구현 완료나 실제 사람 보안 검토를 뜻하지 않는다.

## v3 고정 도구 집합과 명시 확인

공통 `hantong_contracts::auth::default_cli_uid_tools()`는 정렬된 정확한 `["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]`를 반환한다(U). 기존 legacy(L)와 folder(F) 함수의 값은 바꾸지 않는다. `is_supported_default_cli_toolset`은 L/F/U만 허용하며 순서·중복·부분 집합·임의/send 도구는 거부한다. `is_supported_default_cli_toolset_transition(old,target)`은 두 집합을 먼저 검증한 뒤 L≤F≤U의 단조 전이만 허용한다. 동일 집합은 계정 범위가 달라질 때만 유효하며 전체 scope no-op은 별도로 거부한다.

최초 등록은 L을 유지한다. 새 명시 `hantong clients refresh`만 현재 등록 가능한 계정과 U를 native 화면에 제안한다. 화면은 영/한으로 기존/새 계정·전체 도구를 표시하고, U에는 IMAP 메타데이터 한 페이지를 읽어 로컬에 저장하는 권한이라는 설명을 함께 표시한다. 기존 paint/전체 snapshot/OS 사용자·로그온/Hello opaque proof 경계는 그대로다. CLI·MCP·status·startup·프로필 파싱은 이 확인을 대신하지 않는다.

프로필 schema_version=1과 scope RPC protocol_version=1은 유지한다. 기존 L/F 프로필은 동일 canonical bytes로 유효하며 `mail.sync.page`를 허용하지 않는다. 새 U도 고정 read_only/60rpm·계정별 scope를 유지한다. scope refresh는 UID 작업을 실행하거나 자동 enqueue하지 않는다.

## v3 이력과 v12 이전

새 요청 행은 hash_version=3이다. hash domain은 `mail-hantong.cli-scope-refresh.v3\0`이며 domain 이후 필드·순서·길이 encoding은 v2와 동일하다. v1은 기존 v1 domain/encoding과 L/L만, v2는 기존 v2 domain/encoding과 L/F 집합만 정확히 검증한다. v3은 L/F/U를 허용한다. 버전을 내용으로 추정하지 않는다. phase CAS version(pending1→staged2→active3)은 hash_version과 별개이며 바뀌지 않는다.

v11 DDL을 고정하고 v12가 `client_scope_refreshes`의 CHECK만 확장하여 표를 재구축한다. 모든 기존 열·행·ID·hash·시각·phase·version 및 unique index/FK/개수·TTL 규칙을 보존하며 hash를 다시 계산하거나 toolset을 바꾸지 않는다. 기존 DEFAULT hash_version=1과 L/L 기본값은 유지한다. 허용 hash_version은 1/2/3이다. base/target tools는 L/F/U의 canonical JSON이며 버전별 제한과 단조 전이를 DDL·strict reader 양쪽에서 검사한다. v9/v10/v11 검증용 `scope_refresh::validate_history_v11`은 v1/v2와 L/F만 허용하고, 현재 v12 validator만 v3/U를 허용한다. v8의 과거 layout validator도 유지한다.

단조 전이는 L→L/F/U, F→F/U, U→U이며 어떤 계정 변경도 tool downgrade를 허용하지 않는다. 계정·도구가 모두 같은 no-op은 거부한다. create의 accounts-only wrapper는 계속 L을 요청하는 호환 API이며 F/U에서 호출하면 downgrade로 거부한다. 새 with_scope/create가 만드는 이력은 v3이며 기존 성공 이력 전체 scope의 연속성을 그대로 검증한다. stage는 old grants를 보존하고 activate만 target accounts/tools로 정확히 교체한다.

기존 pending/staged는 저장된 목표와 hash를 바꾸지 않는다. 명시 refresh는 유효한 기존 pending을 같은 L/F/U snapshot으로 다시 표시하거나 기존 staged 결정을 그대로 복구한다. 기존 v1/v2 결정의 완료가 U 권한을 뜻하지 않는다. U가 필요하면 해당 완료 뒤 별도의 새 명시 refresh와 native 확인을 거친다. startup은 staged의 exact old/new profile 발행·활성화만 복구하고 pending에는 효과를 주지 않는다. 키·서버 pin·등록 ID를 새로 만들지 않는다.

시험은 v1/v2 실제 pending/staged/active hash·프로필 bytes 보존, F→U tool-only 전이, U→F/L 거부, v1/v2 행에 U를 주입한 손상, phase별 old/target grants와 rollback, 이력 provenance, 영/한 UID 권한 설명·snapshot 변조, L/F/U strict profile, old pending/staged 재개에서 자동 확대 없음, 초기 등록 L 유지 및 구 epoch 거부를 포함한다. 격리 synthetic fixture만 사용하며 실제 사용자 프로필·Hello·계정·비밀은 사용하지 않는다.

---

# 기본 CLI 계정·도구 범위 갱신 v2

상태: 2026-10-30 v9 폴더 도구 활성화 선행 계약, **사람 검토 필요**. 아래 v2 절은 v1의 계정만 갱신/도구 고정 부분을 대체한다. 그 외 TTL·phase·stage/publish/activate·파일 보호·중단 복구 규칙은 유지한다.

## v2 공통 인터페이스와 의미

`hantong_contracts::auth::{default_cli_legacy_tools,default_cli_folder_tools,is_supported_default_cli_toolset}`을 단일 도구 목록 기준으로 사용한다. legacy는 정확히 `["mail.accounts.list"]`, folder는 정렬된 `["mail.accounts.list","mail.folders.list","mail.folders.refresh"]`다. 최초 등록은 legacy로 유지한다. 명시 `hantong clients refresh`만 현재 등록 가능한 계정과 folder 도구를 native 확인으로 요청한다. 기존 사용자에게 자동 grant·자동 확인을 추가하지 않는다. v1 profile의 구조는 그대로이며 tool_ids는 이 두 정확한 집합만 허용한다. partial/arbitrary/send 도구나 folder→legacy downgrade는 거부한다.

v8 DDL을 보존하고 v9 migration이 `client_scope_refreshes`만 재구축한다. 기존 열/행/ID/hash/시각/phase는 보존하며 `hash_version=1`, `base_tools_json`과 `target_tools_json`은 canonical legacy 배열로 보강한다. 새 열은 NOT NULL이고 이 legacy 기본값을 유지한다. hash_version은 1 또는 2다. 양쪽 tools는 두 canonical JSON 중 하나이며 v1은 둘 다 legacy와 서로 다른 계정 목록이어야 한다. v2는 전체 계정+도구 범위가 달라야 하고 downgrade를 금지한다. 기존 unique indexes와 FK·수치/phase 제약은 보존한다.

새 row는 hash_version2다. `mail-hantong.cli-scope-refresh.v2\0` 뒤에 v1과 동일한 identity/version/epoch/key/pin/time 및 old/target accounts encoding을 넣은 뒤 old tools, target tools를 각각 count(u32BE), 각 ASCII 문자열 length(u32BE)+bytes로 덧붙여 SHA-256한다. v1은 기존 domain/encoding/hash를 그대로 검증한다. v2와 v1을 추정하지 않고 저장 hash_version에 따라 엄격히 검증한다. 과거 성공 이력의 계정뿐 아니라 도구도 다음 base와 연결되어야 한다.

`CliScopeRefresh`는 target_scope() 및 target_tools()를 제공하고 target_accounts()는 유지한다. `create_cli_scope_refresh_with_scope(refresh_id, RequestedScope, expected_server_public_key_hash)`를 추가한다. 기존 accounts-only create는 legacy 범위를 만드는 호환 wrapper로 유지하되 downgrade를 허용하지 않는다. 실제 Core의 새 요청은 with_scope와 folder 목록을 사용한다. stage는 기존 accounts/tools grant를 모두 유지하며 epoch를 올려 비활성화한다. activate는 두 grant 표를 target_scope와 정확히 같게 원자 교체한다. 도구 INSERT 실패도 전부 rollback한다.

`ScopeRefreshReview::new_with_tools`는 기존 new 인자 중 old/target accounts 다음에 old/target tools를 받는다(등록 ID는 기존 16 bytes). new는 legacy wrapper다. 전체 snapshot Eq에 tools를 포함하고 영/한 summary는 old/target 도구 전체를 표시한다. 같은 accounts라도 tools가 다르면 유효하며 완전히 같은 scope만 거부한다. native opaque proof는 기존 전체 snapshot 결속을 재사용한다. v1 pending/staged의 명시 재개/복구는 기존 legacy 결정을 보존하고 새 권한으로 바꾸지 않는다.

`ParsedClientProfile::new_with_scope`는 기존 new의 accounts 대신 RequestedScope를 받는다. new는 legacy wrapper이고 schema_version1 유지. strict parser는 두 정확한 정렬 toolset만 허용한다. `AuthenticatedDefaultProfile`은 전체 scope를 보존하고 scope()/tools() 및 기존 accounts() getter를 제공한다. Core는 이전 프로필을 base.scope, 새 프로필을 target_scope로 만들어 재개 때도 동일 bytes를 발행한다.

시험은 v1 history/hash 보존, tool-only upgrade, no-op/downgrade/arbitrary 거부, stage의 old grants 유지, 두 grant 표 활성화와 rollback, 전체 이력 결속, 영/한 도구 표시/변조, legacy/folder 프로필 strict decode, Core 중단 복구와 최초등록 legacy 보존을 포함한다. 실제 사용자 profile/Hello/계정은 사용하지 않는다.

---

# 기본 CLI 계정 범위 갱신 v1

상태: M6-02/CLI-15·MCP-03·SEC-03의 한정된 후속 구현 계약, **사람 검토 필요**. 2026-10-30 연속 개발 위임에 따라 작성한다. 최초 등록·IMAP 설정은 기존 계약을 유지한다. 이 범위는 default CLI의 읽기 전용 `mail.accounts.list` 계정 목록만 갱신한다. 도구·쓰기 권한·키·서버 pin·철회 복구는 추가하지 않는다.

## 요청과 사람 확인

`registration.scope.request`는 `protocol_version=1`만 받는다. Core가 현재 등록 가능한 계정의 정렬된 정확한 snapshot을 만들고 기존 default CLI 범위와 함께 신뢰 창에 표시한다. CLI 명령은 `hantong clients refresh`다. `registration.scope.status`는 protocol_version과 0이 아닌 32-byte 소문자 hex refresh_id만 받으며 CLI는 `hantong clients refresh-status REFRESH_ID`다. 임의 계정 목록·등록 ID·key/pin·version·approved·경로·비밀 입력은 거부한다. 향후 선택적 범위 편집은 별도 계약이다. 빈 snapshot은 0개 권한이며 전체 계정의 별칭이 아니다.

기존 bootstrap의 동일 OS 사용자/로그온 검증·16KiB frame·strict JSON·5초 timeout·ACK와 Core 단일 등록/설정 queue를 사용한다. 같은 pending 갱신 job은 상태만 재전달하고 다른 job은 RATE_LIMITED다. 현재 target과 base가 같은 요청은 CONFLICT이며 새 승인 이력을 만들지 않는다. status와 startup은 확인 창을 열거나 pending을 승인하지 않는다. pending 만료만 제한적으로 기록할 수 있다.

응답은 protocol_version, refresh_id, expires_at_utc_ms, state(`pending|provisioning|ready|denied|expired|interrupted`)의 제한된 값만 갖는다. ID·상태는 승인 증거가 아니다. status는 입력 ID를 정확히 대조한다. 계정·키·pin·비밀·승인 nonce는 응답하지 않는다. 공개 approve/complete/execute 메서드는 HUMAN_STEP_REQUIRED이며 callback을 호출하지 않는다. `--no-input`은 native 확인을 건너뛰지 않는다.

현재 queue에 같은 job이 있을 때 pending/staged는 각각 pending/provisioning으로 표시한다. job이 없는 pending/staged는 interrupted다. 명시 refresh 요청은 유효한 동일 pending을 다시 표시하거나 이미 staged인 결정을 복구할 수 있다. ready는 갱신 이력의 활성화 완료이며 현재 등록이 철회되지 않았다는 보증은 아니다. 이후 인증은 매번 현재 등록 상태를 검사한다.

새 `ScopeRefreshReview`는 전체 base 등록 ID/최초 request ID/version/epoch, client public-key hash, server pin hash, 정렬된 old/target accounts, refresh ID/CAS version, created/expires를 결속하는 순수 private-field/Clone/Eq 자료다. default label/CLI/read-only/accounts.list/60 rpm은 고정이다. native 화면은 영/한으로 기존 범위·새 범위·epoch 증가와 기존 연결 종료를 표시하고 전체 snapshot paint/OS user/logon/nonce/UTC·단조 만료를 확인한 후 Hello를 수행한다. OS Verified 결과만 opaque single-use token을 만들며 Core가 exact snapshot으로 소비한다. CLI/MCP·bool·파일·문구로 token을 만들 수 없다. 닫기/취소는 grant 변경 없이 pending을 보존하고 Deny만 denied로 기록한다.

## DB v7 전용 이력

v6 DDL을 `migrations/schema-v6.sql`로 보존한다. v7은 전용 `client_scope_refreshes` 표를 추가한다. 기존 `client_registration_requests`에 갱신 요청을 넣지 않는다. 최초 request는 기존 CNG selector와 orphan 복구의 예약이므로 변경하거나 대체하지 않는다. refresh ID를 등록 ID나 키 selector 유도 함수에 넣지 않는다.

필드는 refresh_id(32-byte nonzero PK), registration_id(FK), base_registration_version, base_profile_epoch, target_profile_epoch(정확히 base+1), base_accounts_json, target_accounts_json, client_public_key_hash, server_public_key_hash, payload_hash, phase(`pending|staged|active|denied|expired`), version, created/expires/updated UTC, decided_at UTC와 activated_at UTC다. initial 승인 채널은 기존 등록에 유지하고 이 표의 stage 결정은 고정 approval_helper 의미다. pending에는 decision/activation이 NULL, denied/expired에는 decision만, staged에는 approval decision만, active에는 approval decision과 activation이 존재한다. created>0, TTL=300,000ms; 결정/갱신 시각 역행 금지, 승인·거절은 만료 미만, 만료 사건은 expires 이상, activation은 decision 이상이며 TTL 이후도 허용한다. CAS version은 pending1→terminal/staged2→active3이다.

각 목록은 ASCII account ID 최대 128개, 정렬·중복 금지이며 old와 target은 달라야 한다. epoch≤i64::MAX-1, base version≤i64::MAX-2다. registration별 pending/staged는 최대 하나다. `(registration_id,target_profile_epoch)` 유일성은 staged/active에만 적용한다. denied/expired 후 같은 목표 epoch로 새 사람 확인을 요청할 수 있어야 한다. 총 1,024행 상한이며 초과 시 자동 삭제하지 않고 QUOTA_EXCEEDED다.

payload hash는 `mail-hantong.cli-scope-refresh.v1\0` || refresh ID32 || registration ID16 || 최초 request ID32 || base version u64BE || base epoch u64BE || client hash32 || server hash32 || created i64BE || expires i64BE || old count u32BE || 각 old ID의 byte length u32BE+ASCII bytes || target count u32BE || 각 target ID의 같은 encoding을 SHA-256으로 계산한다. target epoch는 base+1, 고정 정책은 domain의 의미다. hash는 변경 감지 자료이며 승인 증거가 아니다. 읽을 때 hash/nullable/type/상한/시각/버전/이력 연결을 재검증한다.

## Store API와 전이

`CliScopeRefresh`는 private fields와 getter를 제공한다. base_registration()은 최초 request·key·고정 정책을 보존한 과거 active `RegistrationRecord` snapshot이며 target_accounts(), refresh_id(), phase(), version(), created/expires/updated/decided/activated, server_public_key_hash(), payload_hash()를 조회할 수 있다. 자료를 생성·파싱하는 것은 사람 확인이 아니다.

`create_cli_scope_refresh(refresh_id,target_accounts,expected_server_public_key_hash)`는 현재 active default CLI와 active 서버 pin을 같은 snapshot에서 검사한다. 유효한 최초 등록, 현재 scope provenance, 실제 target 계정 존재·정렬·상한·old와 차이·증가 가능한 version/epoch를 요구한다. pending/staged 충돌은 CONFLICT이며 기존 요청을 덮어쓰지 않는다. 없는 등록/계정은 NOT_FOUND, 비활성/변경된 등록·pin은 CONFLICT, 손상은 INTERNAL_ERROR다. fresh 요청만 생성하며 키·파일·grant는 변경하지 않는다.

`load_cli_scope_refresh(id)`·`load_pending_cli_scope_refresh()`·`list_staged_cli_scope_refreshes()`는 검증된 자료만 읽는다. default 등록의 pending/staged 중복은 충돌/손상이며 staged 조회는 LIMIT 2로 초과를 탐지한다. `deny_cli_scope_refresh(expected)`와 `expire_cli_scope_refresh(expected)`는 exact pending CAS와 Store UTC를 사용한다. expired가 아닌 pending에 expire를 호출하면 변경 없이 false이며 다른 phase/stale snapshot은 CONFLICT다.

`stage_cli_scope_refresh(expected)`는 Core가 새 native proof를 소비한 뒤 호출한다. BEGIN IMMEDIATE 안에서 exact pending row, base active registration 전체 값, 실제 target 계정, 같은 client key와 active server pin, 유효 TTL을 다시 검증한다. 하나의 commit으로 refresh staged/version2/decision을 기록하고 registration을 provisioning, epoch=base+1, version=base+1로 바꾼다. **grant는 old를 유지한다.** 최초 request/승인 metadata·key·label·도구·read-only·한도는 보존한다. 이 시점부터 기존 epoch와 모든 비활성 등록 세션은 거부된다. 중복 stage를 성공으로 재전달하지 않는다.

`activate_cli_scope_refresh(expected_staged)`는 Core가 exact 새 프로필을 발행하고 파일 guard를 보유한 뒤 호출한다. 현재 registration이 provisioning/target epoch/base version+1/old grants이고 최초 identity·client key·server pin이 같은지 다시 확인한다. 하나의 commit으로 grant를 정확히 target으로 교체하고 registration active/version=base+2, refresh active/version3/activation 시각을 기록한다. 실패하면 전체 rollback하며 staged를 유지한다. Store API 자체는 파일 배포 증거가 아니며 Core가 효과 순서를 강제한다. 기존 최초등록 activate API는 epoch1만 허용하여 scope 갱신을 우회할 수 없다.

## 이력과 인증 읽기

이 절은 고정 default CLI 등록에 적용하며 다른 종류·표시명의 기존 등록 정책을 바꾸지 않는다. 최초 등록 ID/요청/key·승인 metadata는 불변이다. 갱신이 없는 epoch1 default 등록의 grant는 최초 approved scope와 일치한다. 현재 active epoch>1에는 해당 target epoch의 active 갱신이 정확히 하나 있고 grants=target이다. 갱신 중 provisioning에는 staged 갱신 하나, target epoch, base version+1, grants=old가 일치한다. 최초 provisioning epoch1에는 갱신 이력이 없어야 한다. 과거 active 이력은 뒤의 epoch가 생겨도 덮어쓰지 않는다. 이어지는 성공 이력의 base epoch/accounts는 앞 성공 결과와 일치해야 하며 pending/denied/expired가 성공 이력을 대신하지 않는다. 역사 snapshot과 현재 registration 일치 검사는 구분하고 재귀 없이 1,024행 내에서 검증한다.

기존 초기 등록 복구는 갱신 중 provisioning을 초기 발행/활성화 경로로 보내지 않는다. 별도 갱신 복구는 이미 staged된 사람 결정만 끝낼 수 있고 pending에는 효과 0회다. 일반 인증과 공통 실행기의 매 요청 active/epoch 검사는 유지한다. suspended/revoked에서 갱신 요청/활성화로 우회하지 않는다.

중지·철회 기록은 마지막 승인 이력의 epoch/accounts를 유지하며 그 상태 변경으로 version과 updated 시각이 증가할 수 있다. 읽기 검증은 이 경우 이력의 최소 version/시각 이상을 허용하지만 active로 해석하지 않는다. staged 이후 중지·철회도 old grants와 target epoch를 그대로 검증하며 복구 활성화는 거부한다.

## 보호 프로필 배포와 중단

DB와 파일을 하나의 원자 트랜잭션이라고 주장하지 않는다. 순서는 native proof 소비 → stage commit → 기존 등록 CNG 키 reopen/public 대조 → exact old/new canonical profile 생성 → 보호 파일 배포 → activate commit이다. stage 이후 실패하면 provisioning으로 남아 인증을 거부하고 명시 요청 또는 startup의 제한된 복구가 이어간다. 새로운 키를 만들거나 pin을 바꾸거나 실패를 이유로 과거 권한을 복원하지 않는다.

배포는 고정 profiles 디렉터리와 `default.json`, 해당 refresh ID에서 유도한 `default.refresh-<hex>.bak`만 사용한다. 조상·DACL·reparse 검사를 기존 publisher와 동일하게 적용한다. exact old default를 DELETE+READ 권한, share READ만으로 열어 다른 writer/delete를 막고 같은 열린 handle을 backup으로 no-replace rename한다. 파일을 닫고 경로로 교체하지 않는다. 새 canonical bytes를 보호 temp handle에서 write/flush 후 default로 no-replace rename하며 guard를 activation까지 유지한다. 기존 CLI reader가 파일을 잡고 있으면 실패 상태를 보존하며 강제 종료·권한 완화하지 않는다.

복구 허용 상태는 default=old/backup없음, default없음/backup=old, default=new/backup=old 또는 없음이다. 모든 존재 파일은 exact canonical bytes와 ACL/type/size를 확인한다. 양쪽 없음, 다른 내용, default=old와 backup동시존재, reparse·권한오류는 보존 후 실패한다. old→backup 뒤 중단은 default 부재로 안전하게 거부되고, new default 뒤 중단은 provisioning으로 안전하게 거부된다. 정확한 new가 이미 있으면 재발행 없이 guard만 얻는다. backup 정리는 activation 성공 후 exact owned handle로만 하며 실패해도 active 권한을 되돌리지 않는다. inert public-data backup의 잔존은 비밀 노출이나 승인 증거가 아니다.

기존 read-only loader guard를 연 채 별도 DELETE handle로 승격하지 않는다. publisher가 처음부터 rename 가능한 handle을 열고 대조한다. DELETE를 가진 guard가 유지되는 동안 일반 reader가 공유 충돌로 실패할 수 있으며 activation 뒤 guard를 반환/해제하면 정상화된다. loader의 공유 권한을 완화하지 않는다. 이는 [CreateFile의 access/share 규칙](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea)과 [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)에 따른다. 대상 경로는 기존 검증된 절대 경로 rename 구현과 조상 handle 고정을 재사용한다. 이 저장소에서 RootDirectory 상대 rename의 실제 실패가 기록되어 있으므로 검증 없이 바꾸지 않는다. 새 파일은 rename 후에도 쓰기 handle로 flush하며 실패는 staged 복구 대상으로 남긴다. [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)는 전원 차단 내구성의 절대 증명이 아니다. [GetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo)는 동시 ACL 변경의 잠금이 아니며 같은 사용자 악성코드 격리 한계를 유지한다.

## 검증 범위

시험 먼저 작성한다. pending no-effect·동일 target 거부·빈 범위·없는 계정·stale/expired/denied·CAS/epoch overflow·client/server 변경·중복·quota·hash/시각/이력 손상·새 키 0회·원본 request와 다른 계정 보존·stage/activate 각 rollback·초기 activate 우회 거부·구 epoch session 거부를 검사한다. 실제 암호화 v6→v7 보존/backup 충돌/DDL rollback을 검증한다. 보호 파일은 격리 경로에서 old→backup→new 각 중단·예상치 못한 내용·다른 reader·ACL/reparse·guard 유지와 복구를 검사한다. 실제 사용자 프로필·Hello·메일·네트워크는 개발 시험에 사용하지 않는다.
