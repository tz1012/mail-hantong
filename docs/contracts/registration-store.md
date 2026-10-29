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
