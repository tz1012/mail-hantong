# 비밀 저장소 계약 v1

상태: 연속 개발 위임에 따른 구현 기준. SEC-01, M1-03. **사람 보안 검토 필요**.

Secrets module만 평문을 읽는다. 입력 스트림에서 읽고 즉시 암호화하며 외부에는 계정과 종류에 결속한 불투명 `SecretHandle`을 반환한다. 공개 API에는 평문 getter, Serialize, 비밀값 Debug/Display가 없다. 실제 로그인·토큰이나 계정 정보를 개발 테스트에 넣지 않는다.

- `SecretStore::open(root)`는 전용 저장 루트와 안전한 경로를 확인한다. 심볼릭 링크·Windows reparse point는 거부한다. OS별 전용 구현이 없으면 명시적으로 미지원 오류다.
- `store_from_reader(account_id, kind, reader)`는 1~65536바이트만 허용한다. 계정 ID는 ASCII 영숫자·하이픈·밑줄 1~128자다. 종류는 고정 enum이며 사용자가 파일 경로를 정할 수 없다. 입력 실패·빈 입력·상한 초과 때 암호문을 생성하지 않는다.
- Windows DPAPI CurrentUser와 계정/종류 엔트로피를 사용한다. 파일에는 암호문만 쓰고 같은 디렉터리의 새 임시 파일에 쓰기·동기화한 후 원자적으로 교체한다. 실패 시 이전 값이 유지돼야 한다. 삭제는 같은 루트 안 해당 암호문만 제거한다.
- `contains(handle)`과 `verify(handle)`는 상태/오류만 반환한다. verify는 내부에서 복호화와 결속을 검사하고 즉시 평문 버퍼를 지운다. 실제 토큰 사용은 후속 통합에서 Secrets 내부에 둔다.
- 임시 평문 버퍼와 OS 복호화 버퍼는 모든 반환 경로에서 지운다. 계정·비밀값·파일 원문·Windows 상세 오류를 Debug/Display/오류 메시지로 노출하지 않는다.
- 시험: 합성 비밀의 Windows 왕복·암호문 평문 불포함·변조 거부·다른 계정 결속 거부·갱신·삭제·과대 입력·잘못된 ID. 다른 실제 Windows 사용자 실험과 실제 제공자 통합은 별도 확인 사항이며 단일 사용자 시험으로 완료를 주장하지 않는다.

저장소 root는 설치 시 현재 사용자 앱 데이터 아래 만들어야 한다. 로그인된 사용자 악성 프로세스와 디스크/관리자 침해는 별도 위협 모델의 한계다. 이 API는 클라이언트 인증이나 발송 승인 권한을 발급하지 않는다.

## M2 메일 전용 자격 증명 slot

`MailCredentialSlot`은 ImapAppPassword/ImapOAuthAccess/ImapOAuthRefresh/SmtpAppPassword/SmtpOAuthAccess/SmtpOAuthRefresh 여섯 enum이며 `SecretKind::Mail(slot)`로 저장한다. 안정 이름은 각각 imap-app-password/imap-oauth-access/imap-oauth-refresh/smtp-app-password/smtp-oauth-access/smtp-oauth-refresh다. 같은 계정이어도 IMAP·SMTP slot은 파일과 DPAPI entropy가 다르고 변경·삭제·갱신이 서로 영향을 주지 않는다.

새 mail 파일명은 `v2-{base32hex(account_id의 원래 ASCII bytes)}--{slot-name}.secret`이다. padding 없는 base32hex alphabet 0-9/a-v를 사용하며 ID 자체를 소문자로 바꾸지 않는다. 이 방식은 Windows의 대소문자 비구분 파일시스템에서도 Acct/acct를 구분한다. ID 최대 128 bytes일 때 단일 파일명은 255 UTF-16 code units 안에 들어가야 한다. 임의 path/사용자 지정 kind 이름을 받지 않는다.

`resolve_mail_handle(account_id, slot)`은 계정 형식·root·정해진 leaf의 안전성과 존재를 확인해 opaque SecretHandle을 반환한다. 이것은 복호화·무결성 검사·접근 권한 증거가 아니며, 실제 사용 전 기존 verify/Secrets 내부 AUTH가 DPAPI 결속을 검사한다. 누락은 NotFound, 손상 파일은 resolve 후 verify에서 InvalidCiphertext다. legacy 종류를 받는 generic resolver는 추가하지 않는다. owns 검사는 parent뿐 아니라 account/kind로 다시 도출한 정확한 path와 일치해야 한다. 잘못된 account/kind/path handle은 contains=false, verify/delete=NotFound이며 원본 파일을 보존한다.

기존 OAuthAccess/OAuthRefresh/AppPassword/ModelApiKey의 안정 이름과 파일명은 보존하며 새 mail slot으로 자동 복사·공유·폴백하지 않는다. legacy raw account 파일명에는 Windows 대소문자 충돌 가능성이 있으므로 **case-safe 이전 결정 전 M3 등 새 제품 인증 경로에서 legacy 종류 사용을 금지**한다. 현재 새 메일 연결은 mail slot만 사용한다. 이 호환성 문제를 전체 비밀 저장소 완료로 숨기지 않는다.

제품의 메일 자격 증명은 [연결 결속 envelope 계약](mail-credentials.md)을 추가 적용한다. 일반 raw slot 값만으로 AUTH하지 않으며, 서버·로그인·TLS·인증·버전 변경 뒤에는 명시적 재입력이 필요하다.
