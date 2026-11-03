# SMTP 인증 사용자명과 발신 주소 계약 변경안

상태: 구현 제안, [사람 검토 필요]

SMTP 인증 사용자명과 메시지 발신 주소는 서로 다른 값일 수 있다. SMTP endpoint는 선택적 `sender_address`를 지원한다. 값이 없으면 기존 설정과 동일하게 `login_name`을 발신 주소로 사용한다. 값이 있으면 유효한 단일 RFC 메일 주소여야 하며, MIME `From`과 SMTP `MAIL FROM`에 이 값을 사용하고 인증에는 `login_name`을 사용한다.

`sender_address`는 SMTP 설정의 payload hash, 설정 버전, 보호된 credential binding, 터미널 미리보기에 포함한다. 비밀번호·토큰은 계속 Secrets 밖으로 내보내지 않는다. 기존 JSON에 필드가 없을 때의 직렬화와 해시는 기존 계약을 유지한다. 저장소에는 기존 설정을 깨뜨리지 않도록 별도 SMTP sender 레코드를 사용한다.
