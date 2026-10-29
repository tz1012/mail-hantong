# IMAP·SMTP 연결 설정 계약 v1

상태: M2-01의 순수 설정 모델 구현 기준. 2026-10-30 연속 개발 위임에 따른 결정, 사람 보안 검토 필요. EXT-01·EXT-03. 이 문서와 모델만으로 연결·비밀 입력·계정 생성·발송이 가능해지지 않는다.

## 설정 모델

`MailConnectionConfig`는 schema_version=1, account_id, imap, smtp를 가진다. IMAP은 필수이며 SMTP는 null/생략할 수 있다. account_id는 기존 계정 ID와 같은 ASCII 영숫자·밑줄·하이픈 1..128 bytes다. 각 endpoint가 별도의 settings_version(1..i64::MAX)을 가진다. 회사 도메인에서 Google/Microsoft를 자동 선택하지 않는다. 제공자 선택과 자동 발견 제안은 설정 마법사의 별도 단계이며 이 모델에 추정 권한을 넣지 않는다.

각 endpoint의 필드는 settings_version, host, port, tls, login_name, authentication뿐이다. IMAP·SMTP는 서로 다른 host/port/TLS/login/authentication을 보존하며 한쪽의 수정·실패로 다른 쪽을 덮어쓰지 않는다. 권장 포트는 마법사 제안이고 모델은 1..65535의 사용자 지정 포트를 허용한다. 포트 번호로 TLS 종류를 추정하지 않는다.

host는 ASCII DNS 이름 또는 표준 IPv4/IPv6 주소다. DNS 이름은 전체 최대 253 bytes, label 1..63 bytes, 영숫자와 내부 하이픈만 허용하며 소문자로 정규화한다. localhost 같은 단일 label을 허용한다. IPv4/IPv6는 Rust 표준 IP parser로 검증·정규화한다. 표준 IP로 해석되지 않고 모든 label이 숫자 또는 0x 접두 16진수인 비표준 IP 표기(127.1, 0x7f000001, 0x7f.1 등)는 거부한다. 0xdead.example처럼 보통 DNS label과 결합한 이름은 허용한다. URL/사용자 정보/경로/포트 결합/괄호/zone ID/공백/제어 문자/후행 점은 거부한다. 국제 도메인은 확인된 ASCII IDNA 이름을 마법사에서 전달한다. 모델은 DNS 조회나 연결을 하지 않는다.

login_name은 1..320 Unicode 문자·최대 1280 UTF-8 bytes, 제어 문자 없음, 앞뒤 공백 없음, 공백만 있는 값 금지다. 실제 로그인 이름을 보존하며 이메일 주소라고 가정하지 않는다. authentication은 `app_password` 또는 `oauth2`다. 비밀번호·토큰·secret path/ID·공급자 client secret을 설정에 넣는 필드는 없다. 비밀은 INST-05 입력과 Secrets가 관리하며 현재 모델은 그 값을 읽거나 핸들을 발급하지 않는다.

## TLS 경계

tls는 `implicit_tls` 또는 `starttls_required`만 허용한다. 평문/자동/opportunistic/인증서·호스트명 검증 끄기 값은 없다. 실제 transport는 인증서와 접속 host 검증 후에만 AUTH를 수행한다. STARTTLS 거절·인증서 오류·TLS handshake 실패는 연결을 폐기하며 login/AUTH/평문 재접속을 하지 않는다. 이 동작은 향후 sealed transport의 실행 시험으로 검증한다. enum 입력 거부만으로 EXT-03의 실제 TLS 수용 기준을 완료 처리하지 않는다.

## 직렬화·진단·저장

JSON은 최대 16KiB이며 BOM·잘못된 UTF-8·중복/알 수 없는 필드·형식 불일치를 거부한다. 공백 뒤 추가 JSON도 거부한다. 모든 제품 타입의 필드는 private이며 생성/역직렬화 검증을 통과해야 한다. Debug/오류에 host·login·계정 ID를 출력하지 않는다. 직렬화한 설정에는 해당 개인정보가 있으므로 로그·진단으로 사용하지 않고 암호화 Store에만 저장한다.

저장 API는 [설정 저장 계약](mail-connection-store.md)의 operation_id/expected_version/payload_hash·CAS·중복 처리를 따른다. IMAP와 SMTP의 자격 증명은 독립적으로 교체할 수 있어야 하며 기존 account+AppPassword 단일 slot을 양쪽의 공통 비밀번호로 묵시 재사용하지 않는다. Secrets 밖으로 평문을 전달하는 callback/임의 socket auth 인터페이스를 추가하지 않는다.

관측 상태는 설정과 별개이며 endpoint별 observed_settings_version이 현재 설정과 다르면 오래된 결과로 폐기한다. IMAP ready와 SMTP auth_required/tls_failed를 동시에 유지할 수 있어야 하며, 한쪽 성공으로 전체 연결/발송 가능 상태를 만들지 않는다. 실제 상태 저장·설정 UI와 TLS 연결은 후속 구현이다.
