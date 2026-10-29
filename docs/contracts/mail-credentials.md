# 연결 설정에 결속한 메일 자격 증명 v1

상태: 2026-10-30 M2-01 구현 기준, SEC-01·EXT-01·EXT-03. 사람 보안 검토 필요. 이 저장 API는 로그인·승인·실제 네트워크 접속을 수행하지 않는다.

## 목적과 경계

계정과 slot만 일치하는 비밀번호를 변경된 host/login으로 재사용하면 이전 서버의 자격 증명이 다른 서버로 전달될 수 있다. 따라서 제품의 메일 인증은 아래 envelope에 저장된 자격 증명만 사용한다. 설정 변경 뒤에는 새 버전에 대해 신뢰할 수 있는 설정 화면에서 명시적으로 입력해야 한다. 자동 복사/재결속/legacy fallback은 없다.

`SecretStore::store_imap_credential(account_id, &ImapEndpointConfig, reader)`와 `store_smtp_credential(...)`는 기존 mail slot에 DPAPI로 보호한 envelope를 저장하고 opaque SecretHandle을 반환한다. authentication=app_password는 해당 role의 app slot, oauth2는 access slot만 선택한다. refresh slot은 사용하지 않는다. reader 실패·형식 오류·상한 초과 시 기존 암호문 bytes를 보존한다. 임의 slot/path/hash를 caller가 지정하는 API는 없다.

`verify_imap_credential(account_id, endpoint)`와 SMTP 대응 메서드는 내부 복호화·형식·결속을 검사한 뒤 상태/고정 오류만 반환한다. 이 성공은 인증 성공·권한·발송 승인 또는 나중에 사용할 비밀값의 불변성을 보장하지 않는다. 향후 broker는 TLS 인증서와 hostname 검증 뒤 비밀을 다시 읽고 같은 결속을 확인해야 한다. 공개 plaintext getter/callback/socket/session은 없다.

## 입력과 암호화 형식

app password는 UTF-8 1..1024 bytes이며 Unicode 제어 문자를 거부한다. 앞뒤 및 내부의 일반 공백은 그대로 보존한다. OAuth access token은 ASCII 0x21..0x7e의 1..16384 bytes이며 공백·제어 문자·비ASCII를 거부한다. 일반 SecretStore의 64KiB 한도는 유지한다. slot 입력은 목적별 상한+1까지만 읽고 고정 allocation을 사용하여 이전 plaintext allocation이 재할당으로 남지 않게 한다.

DPAPI optional entropy는 기존 account+slot을 유지한다. DPAPI 내부 평문은 아래 bytes의 정확한 연결이며 file/DB/sidecar에는 평문 metadata를 쓰지 않는다. 정수는 big-endian이고 문자열은 u32 UTF-8 byte length와 bytes다.

1. ASCII magic `mail-hantong.mail-credential.v1\0`
2. account_id 문자열, role u8(IMAP=0, SMTP=1), settings_version u64
3. normalized host 문자열, port u16, TLS u8(implicit_tls=0, starttls_required=1)
4. login_name 문자열, authentication u8(app_password=0, oauth2=1)
5. credential byte length u32, credential bytes

현재 typed endpoint에서 예상 prefix를 다시 생성해 암호문을 해제한 prefix와 정확히 대조한다. account/role/version/host/port/TLS/login/authentication 중 하나라도 달라지거나 magic이 없으면 CredentialMismatch다. 설정을 원래 값으로 되돌려도 version이 달라지므로 재입력이 필요하다. credential length와 남은 전체 bytes가 일치해야 하며 trailing bytes·잘못된 UTF-8/문자·상한 위반도 CredentialMismatch다. 파일의 DPAPI 손상은 기존 InvalidCiphertext다.

입력 형식 오류는 새 내부 SecretError::InvalidCredential(고정 문구), 결속 오류는 CredentialMismatch(고정 문구)다. 선택한 slot 자체가 없으면 기존 NotFound이며 반대 인증 방식의 slot을 찾아 대신 읽지 않는다. 새로운 공용 도구 오류 코드를 만들지는 않는다. 향후 코어 연결 단계에서 재입력 안내로 매핑한다. Debug/오류에 비밀·host·login·account를 넣지 않는다. 자체 입력/envelope/DPAPI 평문 buffer는 반환·오류·폐기 때 지우며, 미래 라이브러리 내부 복사까지 지워진다고 주장하지 않는다.

기존 `store_from_reader(..., SecretKind::Mail(...))`의 일반 raw payload는 verify(handle)의 DPAPI 무결성 검사만 통과할 수 있으며 bound verification에는 실패한다. 이전 시험·저장 API와의 호환을 위해 보존하지만 제품 AUTH에는 사용하지 않는다. v2 case-safe filename과 IMAP/SMTP slot 격리는 그대로 유지한다.

## 코어 통합 순서와 시험

설정 DB CAS commit → 새 endpoint/version 재조회 → 신뢰 UI 입력 → 결속 저장 → 일회성 probe 순서다. 초기 코어는 설정 변경·입력·probe를 mail connection gate로 직렬화하고 DB transaction을 네트워크 동안 유지하지 않는다. stale 입력은 쓰기 전에 거부한다. 중단·stale 쓰기가 있더라도 결속 불일치로 사용이 차단되며 원래 값을 자동 재결속하지 않는다.

시험은 fixture만 사용한다. Windows 왕복·재개방·정확한 바이트 보존·각 endpoint 필드/version 변경 거부·다른 account/role/access/refresh 격리·raw legacy payload 거부·손상/trailing/length 거부·입력 최대 경계/실패 시 이전 암호문 보존·Debug 비노출을 확인한다. 이 시험은 실제 TLS/AUTH 또는 설정 GUI 완료를 뜻하지 않는다.
