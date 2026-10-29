# IMAP 일회성 연결 검사 v1

상태: 2026-10-30 M2-01 구현 기준, EXT-01·EXT-03·SEC-01. M2-02 기능 탐지의 입력을 제공한다. 연속 개발 위임에 따른 구현 계약이며 사람 보안 검토는 별도다.

## 공개 경계

`SecretStore::probe_imap(account_id, &ImapEndpointConfig)`는 Windows에서 한 번 연결·TLS·인증·기능 조회·LOGOUT을 수행한다. 성공 receipt는 settings_version과 알려진 IMAP 기능의 boolean만 반환한다. 원문 응답·알 수 없는 capability·주소·socket/session·임의 command·credential callback을 노출하지 않는다. 비밀 해제와 인증 코드는 Secrets 안에만 둔다. 다른 OS에서는 UnsupportedPlatform이다. CLI/MCP·공통 정책/UI 연결은 후속이며 이 API는 등록/발송/쓰기 권한을 발급하지 않는다.

## 실행 순서

1. 검증된 Domain endpoint와 유효한 account_id를 받는다. 전체 30초 안에서 DNS 및 최대 8개 주소의 TCP 연결을 순서대로 시도한다. 각 TCP 시도는 최대 8초다. 연결 후 TLS/인증 오류에는 재접속·다른 방식 재시도가 없다. OS resolver 작업 자체의 즉시 취소까지 보장하지 않는다.
2. implicit TLS는 OS 신뢰·hostname 검증 후 OK greeting을 읽는다. STARTTLS는 OK greeting → 명시적 CAPABILITY → STARTTLS 광고 확인 → 고정 tag STARTTLS → 정확한 동일 tag OK → OS 신뢰·hostname 검증 순이다. PREAUTH는 자격 증명 확인을 생략하므로 거부한다. STARTTLS NO/BAD/잘못된 tag/미지원/실패에는 평문 폴백 없이 닫는다. 미리 읽은 bytes는 TLS 아래의 buffer에 남겨 TLS record로 검증한다.
3. TLS 뒤 CAPABILITY를 새로 요청한다. IMAP4REV1 또는 IMAP4REV2와 사용할 인증 방식이 확인된 뒤에만 전체 endpoint/account/role/version에 결속된 DPAPI envelope를 해제한다. app_password는 AUTH=PLAIN 우선, 없으면 AUTH=LOGIN이다. oauth2는 AUTH=XOAUTH2만 사용하며 refresh/legacy/다른 role로 폴백하지 않는다.
4. 고정 tag AUTHENTICATE를 한 번 수행한다. PLAIN/XOAUTH2는 SASL-IR 광고 시 initial response를 사용한다. 없으면 빈 challenge 한 번에만 응답한다. LOGIN은 challenge 두 번에 사용자 이름과 비밀번호를 순서대로 응답한다. challenge는 최대 512 bytes이며 요청 횟수 초과·조기 성공·잘못된 tag·NO/BAD는 실패한다. 서버가 보낸 challenge를 명령이나 설정으로 해석하지 않는다.
5. 인증 뒤 CAPABILITY를 다시 조회하고 그 결과만 기능 snapshot에 반영한다. greeting·인증 과정의 capability는 snapshot으로 신뢰하지 않는다. 고정 tag LOGOUT 뒤 BYE와 동일 tag OK를 모두 확인해야 receipt를 반환한다. 실패·취소·deadline에는 연결을 닫고 자동 재시도하지 않는다. SELECT/FETCH/STORE/APPEND/MOVE/EXPUNGE/메일 발송은 이 경로에 없다.

## 응답과 자원 한도

고수준 io-imap send/session/STARTTLS coroutine을 쓰지 않고 검증된 codec/types만 사용한다. command tag는 고정되고 응답 tag를 정확히 비교한다. response gate는 CRLF, 최대 16KiB frame, literal 부재, 해당 단계에서 허용된 비재귀 응답 종류를 typed decode 이전에 검사한다. CAPABILITY는 최대 256개, atom당 최대 128 bytes이며 알 수 없는 값은 외부로 반환하지 않는다. 문법 오류 뒤 drain/recovery 없이 연결을 폐기한다.

한 exchange는 status/continuation/tagged를 합해 최대 32개 frame이다. greeting을 제외한 각 명령마다 새 예산을 쓰되 인증 continuation마다 초기화하지 않는다. 총 wire 수신은 TLS handshake 포함 256KiB이며 STARTTLS 때 초기화하지 않는다. TLS 후 plaintext에도 256KiB 한도를 둔다. 출력은 command당 32KiB, 세션 전체 64KiB로 제한한다. endpoint/credential 입력 한도를 먼저 검사하고 제한된 크기만 serialize한다. 이 수치는 전체 프로세스 RSS나 OS resolver 내부 메모리 상한을 뜻하지 않는다.

기능 snapshot에서 IDLE/MOVE/UIDPLUS는 명시적 광고 또는 IMAP4REV2만 광고하는 서버의 기본 기능을 반영한다. IMAP4REV1과 IMAP4REV2를 함께 광고하면 rev1 모드로 해석하고 명시적 기능만 반영한다. CONDSTORE/QRESYNC/SPECIAL-USE는 명시적 광고를 요구한다. special_use 값은 extended LIST 선택/RETURN 지원이며 특정 Sent/Drafts 폴더의 존재를 뜻하지 않는다. 일반 LIST의 특수 폴더 속성은 이 값 없이도 존재할 수 있다.

현재 codec은 ASCII response text에 한정되므로 모든 rev2 응답과의 상호 운용성을 주장하지 않는다. `+ CRLF` 빈 challenge만 허용하며 공백을 생략한 `+CRLF` 비표준 변형은 거부한다. AUTH 중 status/CAPABILITY는 합산 frame 한도 내에서 소비하고 무시한다. 선택되지 않은 mailbox DATA 등 이 좁은 probe에서 필요 없는 다른 종류는 거부한다. 호환성 확대는 별도 제한/회귀 시험을 거쳐야 한다.

비밀 payload와 자체 최종 encoding buffer는 폐기 시 지운다. codec 내부의 임시 base64/encoding 복사본까지 완전한 zeroization을 보장하지 않는다. 인증 객체·원문 frame을 Debug/로그에 출력하지 않는다. Secrets가 log facade의 `max_level_off`를 활성화하며 단독 crate 시험에서도 정적 Off를 검사한다.

오류는 기존 `MailProbeError` 고정 enum을 사용한다. TLS 거절은 Tls, 선택 가능한 인증 방식 부재는 MechanismUnsupported, AUTH NO/BAD는 Authentication, 잘못된 tag/문법/순서/한도/CAPABILITY·LOGOUT 실패는 Protocol이다. 서버 원문과 하위 오류 문자열은 노출하지 않는다. credential 오류는 기존 고정 SecretError로 제한한다. 새 CLI 종료 코드나 공용 도구 오류 코드를 만들지 않는다.

## 검증

실제 DPAPI fixture와 in-memory Duplex/Rustls 공개 테스트 CA를 사용한다. 외부 네트워크/실제 계정은 사용하지 않는다. 정상 TLS·인증 방식·SASL-IR, TLS 실패 시 비밀 접근 0, STARTTLS 평문 주입, 잘못된 tag/상태/순서, stale credential, literal·대형 frame·status 폭주, timeout/cancel, post-auth 기능 갱신, LOGOUT 실패를 검사한다. 제품 caller가 transport/CA/credential loader를 주입할 수 없으며 private test seam만 사용한다. 실제 서버 상호 운용성·공통 정책/UI·수신 기능은 별도 검증 대상이다.
