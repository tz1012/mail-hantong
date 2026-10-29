# 서버 신원·클라이언트 등록 저장 계약 v1

상태: M1-02의 영속 기반, 사람 보안 검토 필요. DB v2 기반을 만들며 등록 승인 UI·제품 인증 활성화를 뜻하지 않는다. 일반 CLI/MCP 도구에는 활성화·승인 발급 작업을 추가하지 않는다.

## DB v2와 이전

기존 `app_metadata`, `settings`, `accounts`는 유지한다. [migrations/schema-v1.sql](migrations/schema-v1.sql)은 v1 전체 DDL의 보존본이다. 기존 v1의 전체 DDL·metadata·user_version을 검증하고 암호화 백업을 만든 다음 [001-to-002.sql](migrations/001-to-002.sql)을 한 트랜잭션으로 적용한다. metadata/user_version을 같이 2로 올리고 전체 v2 DDL을 검증한다. 실패하면 원본을 보존한다. 기존 backup-before-v1.db를 덮어쓰지 않으며 충돌 시 이전을 중단한다. v2 전체 DDL은 [schema-v2.sql](migrations/schema-v2.sql)에 보존한다. 현재 신규 DB는 [감사 계약](audit.md)의 v3로 생성하며 기존 v1은 v2 검증을 거쳐 v3로 이전한다. 마이그레이션은 server/client 키나 active 등록을 생성하지 않는다.

## 서버 신원

singleton `server_identity`는 OS CNG의 공개 선택자 key_id(32 bytes), 상태 `creating|active`, 공개 키(72 bytes), key_epoch, CAS version, UTC 시각을 가진다. 개인 키와 raw secret은 DB에 없다. 고정 namespace `mail-hantong-server-<key_id hex>`만 사용하고 클라이언트 키와 구분한다. 공개 키 hash는 사용 시 계산해 중복 저장 값의 불일치를 피한다.

코어는 새로운 key_id를 OS 난수로 만들고 `creating` 행을 먼저 CAS로 저장한다. DB 트랜잭션 밖에서 같은 key_id의 CNG 키를 생성하고, key usage/export policy/public point를 검증한 뒤 공개 키를 넣어 `active`로 전환한다. `creating` 복구 때는 동일 key_id를 재개방하거나 없을 때만 동일 ID로 생성한다. `active`인데 키가 없거나 공개 키가 다르면 자동 재생성·재pin하지 않는다. 초기 key_epoch/version은 1이며 상태 변경은 version을 증가시킨다. 키 회전은 이 단계에서 제공하지 않는다.

## 등록 요청·권한

`client_registration_requests`는 32-byte opaque 요청 ID, 종류(cli/mcp/gui), 표시명, 요청 account/tool 목록, 5분 이하의 만료, 상태·결정 채널·CAS version을 기록한다. 요청 목록은 권한이 아니며 `RequestedScope`의 ASCII 형식·중복 금지·각 128개 제한을 통과해야 한다. 표시명은 신원 증거가 아니고 진단에 출력하지 않는다. pending 상태만으로 메일 접근이 생기지 않는다.

저장 API는 표시명을 1..128 Unicode 문자·최대 512 UTF-8 bytes로 제한하고 제어 문자·공백만 있는 값을 거부한다. 요청 ID 중복은 CONFLICT다. 한 번에 유효한 pending 요청은 최대 64개, 보존 요청 총수는 최대 1,024개이며 초과하면 RATE_LIMITED로 변경 없이 거부한다. 보존 한도 도달 시 자동으로 기존 결정 이력을 삭제하지 않는다. 새 요청 만료는 저장 시각+5분으로 고정하며, 만료 갱신은 승인 권한 없이 pending→expired만 수행한다. network 제출/상태 응답의 별도 제한과 최소 공개 형식이 정의되기 전에는 외부 메서드로 연결하지 않는다.

`client_registrations`는 16-byte 등록 ID, 원본 요청, CNG 공개 키, epoch, `provisioning|active|suspended|revoked`, read_only, 분당 요청 한도(1..600), 사람 확인 채널·시각·CAS version을 가진다. 같은 공개 키는 중복 등록하지 않는다. raw 72-byte 공개 blob에 UNIQUE 제약을 걸고 좌표와 형식은 CNG로 확인한다. `approved_channel` 문자열은 승인 증거를 대신하지 않으며 승인 서비스가 신뢰 채널을 검증한 결과만 저장한다.

허용 계정과 도구는 `client_registration_accounts/tools`에 각각 저장한다. 빈 계정 집합은 0개 권한이다. 계정 제거·범위/키/상태 변경은 관련 행 변경과 epoch 증가를 같은 트랜잭션으로 처리한다. 철회는 tombstone으로 남긴다. FK는 자동 cascade로 권한 변경을 숨기지 않고 RESTRICT한다. 모든 읽기에서 ID 길이·enum·scope 개수/형식·버전·공개 키 형식을 다시 검증한다.

등록 읽기는 등록·원본 요청·허용 범위를 한 DB snapshot에서 확인한다. 원본 요청이 approved이며 종류·표시명·결정 채널·결정 시각이 등록의 승인 정보와 일치해야 한다. 승인/거절 결정은 요청 생성 이상·만료 미만 시각에만 유효하고 expired 시각은 만료 이상이어야 한다. 등록 갱신 시각은 승인 이상이며 revoked 시각은 승인과 갱신 사이에 있어야 한다. 누락된 FK 대상·잘못된 metadata는 INTERNAL_ERROR로 거부한다. 문자열·blob은 빌린 DB 값을 길이 확인한 뒤 복사하며, grant 조회는 종류별 최대 129개를 읽어 128개 초과를 거부한다. 요청 scope는 실제 허용 grant를 대신하지 않는다.

만료 API는 Store가 읽은 UTC만 사용한다. 유효한 현재 pending 요청이 아직 만료되지 않았을 때만 false를 반환한다. 없는 ID는 NOT_FOUND, 상태/버전 불일치와 증가 불가능한 최대 버전은 CONFLICT이며 DB 오류를 충돌로 바꾸지 않는다. 승인·거절 상태 변경 API는 이 단계에서 제공하지 않는다.

## 활성화 경계

이번 구현은 서버 identity의 복구와 등록부 read model 및 pending 요청 저장까지만 연결한다. active 등록 생성은 승인 도우미·일회 결정 결속·공개 프로필의 원자 배포가 연결될 때 별도 서비스로 추가한다. 테스트의 synthetic active 행은 fixture 안에서만 직접 구성한다. 제품의 등록 우회 플래그·seed 계정·자동 승인 경로는 없다.

Store의 공개 키 읽기는 길이/header를 검증한 비밀이 아닌 metadata를 반환한다. 실제 P-256 좌표와 pinned key의 검증은 platform CNG import와 IPC `RegisteredClient` 생성자가 수행한다. Store metadata 객체를 그대로 인증 principal로 취급하지 않는다.

승인 도우미는 신뢰된 서버 키를 공개 프로필에 고정하고 기존 클라이언트 키 선택자·등록 ID·epoch·요청 상한을 원자적으로 배포한다. CLI의 unauthenticated readiness나 TOFU로 pin을 만들지 않는다. 공개 프로필의 경로/형식과 helper 확인 계약은 별도 정의한다. software CNG·파일 ACL은 같은 사용자 악성코드 격리의 증거가 아니다.

## 2026-10-30 default CLI 등록 전이 추가

이 절은 위의 “승인·활성화 API 없음” 상태를 다음 한정된 영속 API로 확장한다. 발급 UI는 [registration-review.md](registration-review.md), 파일 배포는 [client-profile.md](client-profile.md)를 따른다. Store는 OS 사람 확인기를 갖지 않고 bool/Verified/임의 승인 채널을 입력받지 않는다. 신뢰 Core coordinator가 일회성 token과 현재 snapshot을 검증한 후에만 이 API를 호출한다. CLI/MCP는 Store에 의존하거나 직접 호출할 수 없다.

고정 기본 프로필 표시명은 `hantong CLI (default)`다. 초기 grant는 kind=cli, tool_ids=[mail.accounts.list], read_only=true, requests_per_minute=60, channel=approval_helper다. 같은 표시명의 기존 CLI 등록이 provisioning/active/suspended/revoked 중 어느 상태로든 있으면 새 기본 등록을 거부한다. 철회 후 재등록/프로필 교체는 별도 복구 UI 계약 전까지 자동 수행하지 않는다.

`CliRegistrationStage::new(expected_request, registration_id, client_public_key, expected_server_public_key_hash)`는 private 필드의 입력 자료를 만든다. 예상 요청은 유효한 pending CLI 요청·고정 표시명·고정 도구여야 한다. 요청/등록 ID와 서버 hash는 0 금지, request version은 증가 가능한 양수, account 목록은 최대 128개다. 이 입력 객체는 사람 승인 증거가 아니다.

`stage_cli_registration(&stage)`는 client public key의 실제 CNG P-256 점을 확인하고 BEGIN IMMEDIATE 안에서 다음을 수행한다.

stage 전 등록 ID가 플랫폼 `registration_id_for_request(expected_request.request_id)`의 결과와 같은지 검사한다. 일치하지 않으면 INVALID_INPUT이다. 이미 저장된 요청 ID가 키 selector의 내구성 있는 예약이며 새 DDL은 필요하지 않다. `create_registration_request`도 전부 0인 요청 ID를 INVALID_INPUT으로 거부한다.

- current pending request의 모든 공개 metadata(request ID/version/kind/label/scope/생성/만료)가 expected_request와 일치하는지 다시 읽고 검사한다.
- Store UTC가 생성 이상·만료 미만인지, active 서버 신원의 public-key hash가 예상값과 같은지, 허용 계정이 모두 실제 accounts 행에 있는지 확인한다. 빈 scope는 0개 grant다.
- 중복 registration ID/request/public key/default 프로필, 상태·버전·scope 변경, 계정 소실, 잘못된 서버 pin을 거부한다. 불일치는 CONFLICT, 없는 요청/계정은 NOT_FOUND, 만료/시계 역행은 APPROVAL_EXPIRED다. 손상된 저장 metadata와 DB 실패는 INTERNAL_ERROR다.
- pending 요청을 approved로 바꾸고 Store UTC/고정 channel을 기록하며 요청 version을 증가시킨다. 같은 트랜잭션에서 provisioning 등록, epoch=1/version=1, 고정 정책, 정확한 계정/도구 grant를 넣는다. 오류면 전체를 rollback한다. active 등록은 만들지 않는다.

`activate_cli_registration(registration_id, expected_version, expected_server_public_key_hash)`은 Core가 공개 프로필을 성공적으로 발행/대조한 후 호출한다. provisioning 상태와 정확한 양수 CAS version, 고정 기본 프로필 정책, 원본 approved 요청/정확한 grant/계정 일관성, 현재 active 서버 pin 일치, Store UTC가 승인/마지막 갱신 이후인지 검증한다. active로 바꾸며 version만 증가시키고 epoch/grant는 유지한다. 이미 active/suspended/revoked거나 버전이 다르면 CONFLICT다. 이 API의 존재 자체가 파일 발행 증거는 아니며 Core가 순서를 강제한다.

`deny_cli_registration_request(request_id, expected_version)`은 명시적인 UI Deny에만 사용한다. 현재 pending 기본 CLI 요청, 버전과 유효 시각을 확인한 뒤 denied/channel/time/version을 원자적으로 바꾼다. 닫기/취소에는 호출하지 않는다. denied를 다시 승인하는 전이는 없다.

`load_registration_by_request(request_id)`는 기존 validated snapshot 읽기와 같은 기준을 사용한다. `list_provisioning_registrations(limit)`는 1..128 입력만 받고 LIMIT limit+1로 읽어 초과 시 QUOTA_EXCEEDED로 거부하며, 모든 반환 행을 검증한다. `list_registrable_account_ids()`는 정렬된 실제 ID를 최대 129개 읽어 128개 초과/손상 시 오류를 반환하고, 빈 DB는 빈 목록이다. 이것은 Core의 사용자 검토 snapshot 구성용이며 외부 열거 API가 아니다.

기본 등록 전이는 기존 registration 테이블/상태를 사용하므로 그 전이 자체에는 별도 DDL migration을 만들지 않는다. 범위 갱신 이력은 [클라이언트 범위 갱신 계약](client-scope-refresh.md)의 DB v7 전용 `client_scope_refreshes` 표에 저장하며 기존 최초 request/key/pin을 변경하지 않는다. 실패 rollback, stale/expired/wrong-scope/deleted-account, duplicate ID/key/default profile, wrong pin, invalid public point, activation 순서/CAS, denial terminal, bounded recovery 조회를 합성 fixture로 검증한다. 실제 등록/Windows Hello를 자동 실행하지 않는다.

### Core의 기본 프로필·pending 조회

`load_default_cli_registration()`은 고정 표시명과 cli 종류의 모든 상태를 대상으로 최대 2개 ID를 읽고 validated snapshot으로 반환한다. 2개가 있으면 CONFLICT이며 임의로 하나를 선택하지 않는다. `load_pending_default_cli_request()`는 Store UTC에 아직 만료되지 않은 동일 기본 pending 요청을 최대 2개 읽는다. 중복은 CONFLICT, 생성 시각이 미래이면 APPROVAL_EXPIRED, 손상된 저장 metadata는 INTERNAL_ERROR다. 정상적으로 만료된 과거 pending 요청은 새로운 요청을 영구 차단하지 않으며, 조회 자체는 TTL·상태를 변경하지 않는다. 두 조회는 Core 내부 중복 방지·복구용이며 외부 등록부 열거 API가 아니다.

### 미등록 키의 제한된 복구 조회

`list_orphan_cli_registration_requests()`는 고정 기본 CLI 요청을 LIMIT 1025로 읽고 1,024개 초과 시 QUOTA_EXCEEDED를 반환한다. 요청·연결된 등록을 같은 snapshot에서 검증하고 등록이 없는 pending/denied/expired 요청만 반환한다. approved인데 등록이 없거나 ID/정책/scope가 손상되면 INTERNAL_ERROR, 기본 등록이 중복되거나 서로 다른 요청의 유도 등록 ID가 충돌하면 CONFLICT다. 생성 시각이 현재 Store UTC보다 미래이면 APPROVAL_EXPIRED로 멈춘다. 조회는 요청을 삭제하거나 결정을 변경하지 않는다.

`registration_key_is_linked(request_id, public_key)`는 같은 제한된 예약 목록을 검증하고 유도 등록 ID·요청 ID·공개 키 중 하나라도 연결된 등록이 있으면 true를 반환한다. 일치 후보는 유일 제약에 따른 최대 3개와 초과 탐지 1개만 읽어 모두 검증한다. false는 검증된 snapshot에 연결이 없다는 데이터일 뿐 임의 키 삭제 권한이 아니다. Core만 정확한 요청 유도 selector를 열어 얻은 공개 키로 호출하며, Store 조회부터 삭제까지 동일한 직렬 실행 구간을 유지한다. 조회 실패를 false로 바꾸거나 key namespace를 열거하지 않는다.

### 기본 CLI 범위 갱신과 기존 등록 보존

범위 갱신은 별도 [client-scope-refresh.md](client-scope-refresh.md)의 전용 이력으로만 기록한다. `create_cli_scope_refresh`가 만드는 입력은 현재 active default CLI 등록의 최초 request/key/pin과 고정 read-only `mail.accounts.list` 정책을 보존하고, 당시 실제 존재하는 정렬된 계정 snapshot만 새 target으로 검토한다. 빈 snapshot은 권한 0개이며 전체 계정의 별칭이 아니다. revoked/suspended 등록은 갱신 대상으로 되살리지 않으며, 최초 등록 activate 경로로 갱신을 우회하지 않는다.

stage 전에는 grant·등록 상태·파일을 바꾸지 않는다. native 사람 확인 뒤 Core가 v7 이력과 현재 등록을 같은 snapshot에서 재검증하고 stage/activate를 수행한다. 성공한 갱신만 target snapshot으로 새 epoch를 활성화하며, 원래 request ID·registration ID·client key·server pin·승인 metadata는 유지한다. pending/denied/expired 이력은 성공한 권한을 대신하지 않으며, 실패 시 기존 등록 권한을 자동 복원하거나 revoked 상태를 재활성화하지 않는다.
