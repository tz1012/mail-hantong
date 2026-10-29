# IMAP·SMTP 설정 저장 계약 v1

상태: 2026-10-30 연속 개발 위임에 따른 M2-01 구현 기준. EXT-01·EXT-03, 사람 보안 검토 필요. 인증된 설정 화면·계정 생성·실제 연결을 활성화하지 않는다.

## 범위와 API

Store는 기존 `provider=imap` 계정의 IMAP/SMTP endpoint를 각각 저장한다. 계정 없음은 NOT_FOUND, 다른 provider는 CAPABILITY_UNSUPPORTED다. 계정 생성·삭제·제공자 자동 추정·기존 accounts.settings_version 변경은 없다. SMTP 단독 생성은 CONFLICT이며 IMAP 설정이 먼저 있어야 한다. 기존 설정 삭제는 이 계약 범위 밖이다.

새 IMAP 계정과 최초 endpoint를 함께 만드는 전이는 [계정 생성 계약](account-provisioning.md)을 따른다. 이 계약의 기존 endpoint 변경 API와는 별도 hash domain을 사용한다.

`put_imap_connection(account_id, endpoint, mutation)`과 `put_smtp_connection(...)`은 검증된 Domain endpoint와 MailMutation을 받는다. mutation은 operation_id·idempotency_key(각 ASCII 영숫자/밑줄/하이픈 1..128 bytes), expected_version(0..i64::MAX-1), payload_hash([u8;32])다. 후보 endpoint.settings_version은 expected_version+1이어야 한다. 잘못된 값/해시는 INVALID_INPUT이다. 이 입력은 사용자 권한·승인 증거가 아니며 향후 공통 서비스가 인증/정책을 먼저 검사한다.

`load_mail_connection(account_id)`는 같은 읽기 snapshot에서 IMAP과 선택적 SMTP를 읽는다. 기존 imap 계정에 설정이 없으면 None, SMTP만 있거나 저장 필드가 Domain 경계를 벗어나면 INTERNAL_ERROR다. secret·공급자 주체·다른 계정은 읽지 않는다. 결과의 Debug와 오류에는 host/login/ID/SQL을 넣지 않는다.

## canonical hash와 중복

Store가 hash를 재계산한다. 입력은 `mail-hantong.mail-connection.v1\0` ASCII prefix 뒤에 아래 순서를 붙인 bytes다. 정수는 big-endian, 문자열은 u32 byte length 뒤 UTF-8 bytes다. 문자열은 Domain에서 검증/정규화된 값을 쓴다.

1. account_id 문자열, endpoint 종류 u8(IMAP=0, SMTP=1)
2. expected_version u64, 후보 settings_version u64
3. host 문자열, port u16, TLS u8(implicit_tls=0, starttls_required=1)
4. login_name 문자열, authentication u8(app_password=0, oauth2=1)

SHA-256은 이미 사용 중인 Windows Platform의 bounded CNG 구현을 재사용한다. Store의 Windows 전용 Platform 의존을 허용하며 새로운 암호 구현·패키지는 추가하지 않는다. canonical bytes는 4KiB 이하이고 로그로 내보내지 않는다. 대응하는 hash 계산 helper도 설정값만 받으며 권한을 만들지 않는다.

BEGIN IMMEDIATE 안에서 operation_id 또는 (account_id,idempotency_key)의 기존 행을 먼저 조회한다. operation ID/key/account/endpoint/hash/expected/result가 모두 같으면 당시 result_version을 반환하며 현재 설정이나 관측을 다시 바꾸지 않는다. 이후 다른 설정 변경이 있어도 과거 replay가 이를 되돌리지 않는다. 어느 필드든 다르면 CONFLICT다. 신규 요청은 계정/provider·현재 endpoint version·SMTP 선행·용량을 확인한 뒤 설정 CAS, 해당 endpoint 관측 삭제, operation 삽입을 한 번에 commit한다. 실패는 전체 rollback, 자동 재시도 없음이다.

operation은 최대 100,000행이다. 상한에서 새 작업은 QUOTA_EXCEEDED, 기존 exact replay는 가능하다. 자동 삭제/재사용으로 중복 방지를 약화하지 않는다. 보존 정책은 후속 계약에서 다룬다. operation 표에는 host/login/비밀값을 복제하지 않는다.

## 관측

MailEndpointKind(IMAP/SMTP), MailConnectionState(ready/auth_required/tls_failed/offline/error)를 사용한다. ready는 해당 endpoint의 마지막 연결 관측일 뿐 발송 승인·capability 증거가 아니다.

`record_mail_observation(account_id, endpoint_kind, observed_settings_version, expected_observation_version, state)`는 내부 코어 연결 검사 결과를 저장하는 API다. 외부 도구로 노출하지 않는다. 계정 또는 해당 endpoint 설정이 없으면 NOT_FOUND다. 현재 설정 버전이 일치해야 하며 observation version을 별도 CAS로 증가시킨다. 행이 없을 때 expected=0, 있으면 현재 값과 같아야 한다. 오래된 설정·관측 버전은 CONFLICT, 버전 고갈도 CONFLICT다. 시각은 Store의 UTC epoch ms이며 같은 설정의 기존 관측보다 시계가 뒤로 가면 INTERNAL_ERROR로 보존한다.

`load_mail_observation(account_id, endpoint_kind)`는 설정 행과 관측 행을 동일한 읽기 트랜잭션 snapshot에서 읽고 버전 일치를 확인하여 관측 또는 None을 반환한다. 계정이 없으면 NOT_FOUND, 해당 endpoint 설정 또는 관측이 없으면 None이다. 설정과의 불일치/잘못된 enum·version·timestamp는 INTERNAL_ERROR다. IMAP ready와 SMTP tls_failed를 동시에 보존한다. 설정 변경 시 그 endpoint 관측만 삭제하여 이후 검사를 unchecked로 표시한다. 관측 저장은 비밀번호/토큰/네트워크 예외/서버 문자열을 받지 않는다.

## DB v4

현재 v3 DDL을 migrations/schema-v3.sql로 보존하고 003-to-004.sql에 아래 세 테이블을 추가한다. 기존 등록·감사·계정은 수정하지 않는다. v1→v2→v3→v4 각 단계의 검증과 암호화 백업을 유지한다.

- mail_connection_settings: (account_id, endpoint_kind) PK, accounts FK, host/port/tls/login_name/authentication/settings_version/updated_at_utc_ms. 열의 길이·enum·정수 범위를 DDL과 Domain 모두 검증한다.
- mail_connection_observations: 같은 PK 및 settings 복합 FK, observed_settings_version/observation_version/state/observed_at_utc_ms.
- mail_connection_operations: operation_id PK, UNIQUE(account_id,idempotency_key), account FK, endpoint_kind/hash/expected/result/completed_at_utc_ms. operation_id/idempotency_key는 모두 NOT NULL 및 ASCII 영숫자/밑줄/하이픈 1..128 bytes, endpoint는 imap/smtp다. result=expected+1, hash 32 bytes, 시간 양수.

검증은 fresh v4, 단계별 이전/원본 보존/rollback/backup 충돌, 독립 endpoint 생성·갱신, stale CAS, exact replay·다른 payload 충돌·재개방 replay, 관측 격리·stale 결과·clock rollback, 용량, 손상 읽기와 오류 비노출을 포함한다. 테스트 계정 fixture로 검증하며 실제 계정 연결/설정 마법사 완료로 표시하지 않는다.
