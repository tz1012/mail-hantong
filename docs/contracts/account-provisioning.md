# 새 IMAP 계정 저장 전이 v1

상태: M2-01의 실제 사용 연결을 위한 ACC-01/ACC-03·M3-10 선행 통합. 연속 개발 위임, **사람 검토 필요**. 계정 추가 UI·실제 로그인·재연결·전체 ACC-01 완료와 별개다.

## 범위와 입력

처음 만드는 범용 IMAP 계정과 endpoint를 원자 저장하고, 첫 probe 결과를 계정/관측에 원자 반영한다. credential 값·승인 bool·CLI assertion을 받지 않는다. Core는 후속 보호된 설정 화면의 일회 사람 확인을 소비한 뒤 이 Store API를 호출해야 한다. API의 데이터 타입과 hash는 권한 증거가 아니다. 이번 저장 전이는 CLI/MCP/외부 IPC에 노출하지 않는다.

Domain `NewImapAccountDraft`는 기존 account ID 형식(ASCII 영숫자/밑줄/하이픈 1..128), 선택 표시명(128문자/512bytes, 빈 값·공백만·제어 문자 금지), 선택 색상(`#RRGGBB`)을 검증한다. ID는 Core CSPRNG가 생성한다. login을 이메일 주소로 추정하지 않는다. primary_address와 provider_subject는 NULL, provider=imap, status=connecting, capabilities/granted_scopes=[], account settings_version=1로 고정한다. 표시 번호는 같은 트랜잭션의 기존 MAX(account_number)+1, 빈 DB면 1이다. i64::MAX면 CONFLICT다. 계정 수 자체에 새 제품 한도를 두지 않는다.

이번 경로는 새 계정 전용이다. 기존 IMAP endpoint의 정규화된 host와 login UTF-8 bytes가 모두 같으면 상태·port/TLS/auth 방식과 무관하게 CONFLICT다. DNS host는 Domain 정규화를 따르고 login의 대소문자는 바꾸지 않는다. disabled/offline/error 계정도 중복 검사에 포함한다. 자동 합치기·기존 계정 덮어쓰기·같은 대상을 별도 ID로 다시 생성하지 않는다. 중복 안내와 기존 account ID를 유지하는 재연결 화면은 후속 구현이며 이 저장 API로 우회하지 않는다. 검사 결과가 없음이 검증된 경우만 생성하므로 별도의 caller 제공 duplicate-confirmed 플래그는 없다.

## stage와 v4 원장 재사용

`imap_account_stage_payload_hash(draft, endpoint)`는 endpoint.settings_version=1만 받는다. canonical bytes는 `mail-hantong.account-provision.v1\0` ASCII prefix(NUL 포함), 다음 순서다. 정수는 big-endian, 문자열은 u32 UTF-8 byte length와 bytes, nullable 문자열은 presence u8(0/1) 뒤 존재할 때 문자열이다.

1. account ID, nullable display_name, nullable color
2. account expected/result version u64 0/1, endpoint expected/result version u64 0/1
3. host, port u16, TLS u8(implicit=0/starttls_required=1), login, authentication u8(app_password=0/oauth2=1)

Windows Platform의 기존 CNG SHA-256을 사용한다. 다른 endpoint 변경 hash와 domain을 분리한다. 외부 패키지·DB migration은 추가하지 않는다.

`Store::stage_imap_account(draft, endpoint, &MailMutation)`는 기존 operation_id/idempotency_key 형식·payload_hash를 검증하고 expected_version=0, endpoint version=1을 요구한다. expected_version은 계정과 endpoint 둘 다 아직 없음을 뜻한다. hash가 다르면 INVALID_INPUT이다. BEGIN IMMEDIATE 안에서 다음을 수행한다.

1. 기존 v4 mail_connection_operations의 operation ID 또는 계정별 idempotency key를 조회한다. ID/key/account/kind=imap/hash/expected=0/result=1이 모두 같으면 연결된 IMAP 계정/endpoint가 존재하고 저장값이 유효한지 확인한 뒤 고정 stage receipt를 반환한다. 현재의 표시값·버전·상태가 최초 값과 같을 필요는 없다. 연결 대상 누락/손상은 INTERNAL_ERROR이고 operation의 비교 필드가 다르면 CONFLICT다. 현재 계약에는 endpoint 삭제 경로가 없다.
2. 신규 작업은 account ID 미존재와 위 host/login 중복 없음을 재검사한다. 기존 IMAP endpoint를 한 행씩 검증하여 메모리 사용량을 제한하고, canonical 저장값·Domain 유효성을 확인한 host/login만 비교한다. 후보와 다른 endpoint라도 손상돼 있으면 INTERNAL_ERROR로 중단한다. 검토 이후 다른 생성이 들어와도 같은 트랜잭션에서 거부하며, 계정 수에 새 제품 한도를 두지 않는다.
3. 기존 원장 전체 100,000행 한도와 account number 고갈을 확인한다. 임의 기록 삭제나 덮어쓰기는 하지 않는다.
4. accounts + IMAP endpoint v1 + operation(expected=0,result=1)을 한 번에 insert/commit한다. 실패는 모두 rollback한다. SMTP는 이후 기존 별도 설정 API를 따른다.

`StagedImapAccount` receipt는 account ID와 account/imap version=1만 포함한다. 최초 표시 번호·이름·상태 snapshot을 replay한다고 주장하지 않는다. 이후 계정/endpoint가 바뀌어도 exact stage replay는 당시 고정 1/1 receipt만 반환하고 현재 데이터를 변경하지 않는다. 원장에 operation_kind를 추가하지 않으므로 DB 행만으로 stage 종류를 역조회하는 기능은 제공하지 않는다. 종류 검색·변경 가능한 최초 snapshot replay가 필요해지면 별도 migration 계약을 만든다.

## 첫 probe 결과

Store의 `InitialImapProbeOutcome`은 Ready(알려진 capability ID 집합), AuthRequired, TlsFailed, Offline, Error다. Ready의 ID는 `imap.condstore`, `imap.idle`, `imap.move`, `imap.qresync`, `imap.special_use`, `imap.uidplus` 중 중복 없는 정렬된 부분집합이다. 서버 원문/자유 텍스트/비밀을 받지 않는다. capability는 표시 자료이며 권한 증거가 아니다.

`Store::finish_initial_imap_probe(account_id, expected_account_version, expected_endpoint_version, expected_observation_version, outcome)`는 양수의 account/endpoint 버전과 0 이상 observation 버전, i64 범위/증가 가능성을 확인한다. 하나의 Immediate 트랜잭션에서 provider=imap, account status=connecting, 세 버전의 정확 일치를 확인한다. 관측이 없으면 version=0이다. 누락은 NOT_FOUND, 다른 provider는 CAPABILITY_UNSUPPORTED, 상태/버전 불일치는 CONFLICT다.

| outcome | account status | IMAP observation state |
| --- | --- | --- |
| Ready | ready | ready |
| AuthRequired | reauth_required | auth_required |
| TlsFailed | error | tls_failed |
| Offline | offline | offline |
| Error | error | error |

account settings_version과 observation_version을 각각 1 올리고 account status/capabilities와 IMAP observation을 함께 commit한다. 성공에만 알려진 capabilities를 저장하고 실패에는 []다. grants/provider_subject/primary_address/SMTP/다른 계정은 변경하지 않는다. Store UTC를 쓰며 기존 관측보다 역행하면 INTERNAL_ERROR로 보존한다. 결과 receipt는 account/endpoint/observation 버전이며 재호출을 허용하는 bearer가 아니다.

결과 데이터 자체로 실제 probe를 입증하지 않는다. Core는 같은 캡처된 account ID/endpoint snapshot으로 Secrets 호출과 이 저장 호출을 연결해야 한다. settings_version 숫자만 같은 다른 계정에 결과를 적용하면 안 된다. 이번 Store 시험은 지정 계정 외 변경이 없음을 검증하고, 실제 probe의 대상 결속은 후속 Core coordinator에서 별도로 검증한다.

`Store::load_imap_provisioning_state(account_id)`는 한 Deferred snapshot에서 account status/version, 검증된 IMAP endpoint, capabilities와 같은 설정 버전의 observation 또는 None을 읽는다. 누락은 NOT_FOUND, 다른 provider는 CAPABILITY_UNSUPPORTED, 저장값/관측 손상·불일치는 INTERNAL_ERROR다. Debug는 ID/표시/endpoint 값을 숨긴다. 전체 계정이나 비밀은 반환하지 않는다.

## 중단·재개

stage 전 실패는 세 행 모두 없고, stage 후 비밀 저장/첫 probe 전 중단은 connecting + endpoint v1 + 원장을 보존한다. 시작 시 네트워크를 자동 재시도하지 않는다. 후속 설정 화면의 명시적 재개에서만 endpoint 결속 credential을 확인하고 probe한다. finalize 성공 뒤 응답이 유실되면 상태 snapshot으로 확인한다. connecting이 아닌 상태에 대한 finalize 반복은 CONFLICT이며 자동 재실행하지 않는다. 첫 인증 실패 뒤 비밀 재입력·재검사, 기존 endpoint 갱신, 계정 해제는 별도 전이로 추가한다.

새 계정 생성은 기존 CLI의 scope를 늘리지 않는다. 등록이 없으면 후속 최초 등록에 포함할 수 있고, 이미 등록된 경우 별도의 사람 검토·profile epoch 변경/보호 파일 교체가 필요하다. 이 작업은 그 절차를 우회하지 않는다.

검증은 입력/해시 경계·host/login 중복·번호/원장 한도·세 행 원자성·stage replay와 다른 payload 충돌·모든 probe 상태 매핑·세 버전 CAS·실패 rollback·한 snapshot 조회·계정 격리를 합성 endpoint/결과와 격리 암호화 DB에서 수행한다. 실제 계정·credential 입력·네트워크를 사용하지 않는다. 비 Windows Store는 CAPABILITY_UNSUPPORTED다.
