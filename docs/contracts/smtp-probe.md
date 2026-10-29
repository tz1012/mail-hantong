# SMTP 일회성 연결 검사 v2

상태: 2026-10-30 M2-01 구현 기준, EXT-01·EXT-03·SEC-01. 연속 개발 위임으로 작성하며 사람 보안 검토는 별도다. GUI·IPC·정책 허용과 연결하기 전의 Secrets 내부 서비스다.

## 공개 경계

`SecretStore::probe_smtp(account_id, &SmtpEndpointConfig)`는 비동기 일회성 연결 검사다. 성공하면 입력 settings_version, 고정 `SmtpCapabilities`, 실제 선택되어 성공한 `SmtpAuthMechanism`을 담은 `SmtpProbeReceipt`를 반환한다. 실제 서버 응답, 주소, 자격 증명, socket, connection/session, 임의 command, callback은 반환하거나 받지 않는다. 발송·승인 권한을 만들지 않는다. MAIL/RCPT/DATA와 메시지 제출은 구현하지 않는다. IMAP은 별도 후속 서비스다.

Windows의 고정 OS 신뢰 검증기를 사용한다. 테스트 CA·임의 transport·timeout·credential loader를 caller가 주입하는 제품 API는 없다. 다른 OS는 현재 UnsupportedPlatform이다. 신뢰 UI와 공통 정책을 거치는 Core 연결은 후속이며 이 메서드의 존재만으로 CLI/MCP에서 사용할 수 없다.

## 실행 순서

1. account_id 형식을 검사한다. host/port/TLS/login/authentication/version은 검증된 Domain endpoint만 받는다.
2. 전체 30초 deadline 안에서 DNS/TCP 연결을 최대 8개 주소까지 순차 시도한다. 주소마다 TCP timeout은 최대 8초다. 연결 이후 TLS·AUTH 오류에 대한 재접속/다른 주소/인증 방식 재시도는 없다. OS DNS 작업은 취소 뒤 내부에서 종료될 수 있으며 OS resolver의 메모리/스레드 수까지 이 deadline이 보장하지는 않는다.
3. implicit_tls는 Secrets 소유 Rustls connector에서 OS 신뢰와 접속 host를 검증한다. 성공한 TLS stream만 private SMTP driver로 전달한다. STARTTLS는 greeting/EHLO → STARTTLS 필수 확인 → TLS와 hostname 검증 → EHLO 재조회다. STARTTLS 미지원·거절·TLS 실패 시 연결을 폐기한다. 라이브러리가 거절 응답 뒤 종료용 QUIT를 한 번 보낼 수 있으나 AUTH·재접속·평문 인증은 없다. 종료 시도도 전체 deadline 안에 둔다.
4. 검증된 연결의 EHLO 정보로 사용할 인증 방식을 하나 선택한다. app_password는 PLAIN 우선, 없으면 LOGIN이며 oauth2는 XOAUTH2만 사용한다. 해당 방식 미지원이면 비밀을 읽기 전에 실패한다. 선택한 한 방식이 실패해도 다른 방식으로 재시도하지 않는다.
5. 그 뒤에만 DPAPI envelope를 해제하고 account/role/version/host/port/TLS/login/authentication 전체 결속을 다시 검사한다. 현재 slot이 없거나 결속이 다르면 AUTH하지 않는다. access token 대신 refresh/legacy/다른 role을 읽는 폴백은 없다.
6. Secrets 안에서만 라이브러리 Credentials를 만들어 한 번 AUTH하고 QUIT한다. AUTH는 정확한 235, QUIT는 정확한 221 응답이어야 receipt를 반환한다. 250 같은 다른 긍정 응답을 인증 성공으로 인정하지 않는다. 종료용 QUIT는 고정 typed command로 한 번 호출하여 라이브러리 quit()의 오류 복구가 QUIT를 다시 보내지 않게 한다. 오류·취소·deadline에는 연결을 폐기하며 인증/발송을 자동 재시도하지 않는다. 이 경로는 메일을 제출하지 않으므로 SEND_UNKNOWN을 만들지 않는다.

implicit TLS를 주입받은 Lettre는 내부 stream을 plain으로 표시한다. 이 경로의 보안 근거는 broker가 먼저 완료한 Rustls handshake다. 해당 연결에서 Lettre의 TLS 상태/정책/STARTTLS/certificate API를 사용하지 않는다. STARTTLS 경로에서만 Lettre가 소유한 실제 TLS upgrade 성공을 확인한다. OS verifier나 hostname 검증을 끄는 옵션은 없다.

Lettre 0.11.23의 외부 BufReader는 STARTTLS 전환 때 비워지지 않는다. 제품은 wire budget → 8KiB 하위 BufReader → StarttlsReadGate → Lettre 순서로 감싼다. TLS와 재-EHLO가 완료되기 전까지 gate가 외부 reader에 한 번에 1 byte만 전달하여 평문 잔여물이 그 reader에 쌓이지 않게 한다. 하위 buffer의 미리 읽은 bytes는 upgrade 때 Rustls 아래에 있으므로 TLS record 검증을 거쳐야 한다. TLS 전환 뒤에만 일반 bulk read를 허용하고 수신 예산을 초기화하지 않는다. 하위 buffering으로 OS 읽기는 묶지만 handshake 중 byte 단위 전달의 CPU 비용은 남으며 실제 네트워크 성능은 후속 측정한다.

## 자원·오류·메모리

하나의 연결에서 수신 wire bytes는 TLS handshake를 포함해 최대 256KiB다. 읽기 전에 남은 budget만큼 caller buffer를 제한하고 소진 후에는 내부 read를 호출하지 않는다. 읽기마다 별도 heap buffer를 만들지 않는다. implicit TLS는 TLS 바깥에도 256KiB plaintext 제한을 둔다. STARTTLS는 upgrade 뒤에도 내부 wire counter를 유지한다. Lettre의 1,000-byte line 검사는 read_line 뒤에 수행되므로 이 계약은 16KiB 사전 line allocation 제한이나 정확한 RSS 상한을 주장하지 않는다.

전체 deadline은 연결·greeting·TLS·AUTH·QUIT를 모두 포함한다. 프로세스가 실행을 멈추거나 동기 OS 함수 자체가 정지하는 경우의 실시간 보장은 아니다. 유휴 connection pool과 재사용·배경 재시도는 없다.

오류는 `InvalidAccount`, `UnsupportedPlatform`, `Connection`, `Tls`, `MechanismUnsupported`, `Authentication`, `Protocol`, `Timeout`, `Credential(SecretError)`의 고정 내부 enum이다. 서버 원문과 하위 라이브러리 오류 문자열을 저장/노출하지 않는다. 새 CLI/공용 도구 오류 코드는 추가하지 않는다. Core가 후속 단계에서 endpoint별 관측 상태에 매핑하고 현재 settings_version과 대조한다.

DPAPI·자체 plaintext buffer는 폐기 때 지운다. Lettre Credentials와 SASL 내부 문자열 복사는 zeroize되지 않으므로 모든 복사본의 완전한 제거를 보장하지 않는다. 이 객체는 Secrets 밖으로 나가지 않고 일회성 호출 종료 때 폐기한다. Debug/로그에 credential·login·server response를 넣지 않는다.

## 검증

제품과 동일한 private 실행 함수를 in-memory Duplex + Rustls 공개 test CA로 검사한다. 테스트 전용 transport/CA seam은 private이고 `cfg(test)`에서만 호출한다. Windows DPAPI fixture로 실제 envelope를 만들며 marker는 실제 unseal 진입 시 측정한다. 정상 두 TLS 방식·인증서/hostname 실패·STARTTLS 미지원/거절·malformed TLS·무한/대형 응답·deadline·미지원 AUTH·stale envelope·AUTH 실패의 무재시도·QUIT 실패·정확한 command 순서·발송 command 부재를 확인한다. 실제 인터넷/메일 계정은 사용하지 않는다.

## SMTP 기능 관측 (M2-02/EXT-05)

receipt의 `capabilities()`와 `authenticated_mechanism()`은 [서버 기능 계약](provider-capabilities.md)의 Domain 타입을 반환한다. 기능의 원천은 TLS가 완료된 뒤의 EHLO이며 AUTH 이전 관측이다. STARTTLS 이전 EHLO는 포함하지 않는다. implicit TLS도 보호된 연결의 EHLO만 사용한다. AUTH 후 EHLO를 추가하지 않으며 AUTH235와 QUIT221까지 성공해야 receipt를 반환한다. 인증 방식은 app_password의 PLAIN 우선/LOGIN 차선 또는 oauth2의 XOAUTH2 중 실제 선택한 하나다. 기능 광고와 실제 선택 결과는 구별한다.

Secrets private 함수가 Lettre ServerInfo를 고정 enum으로 옮긴다. 서버 name/banner, 알 수 없는 extension, raw EHLO/응답은 반환·저장·Debug/로그에 포함하지 않는다. STARTTLS는 연결 수립 방식이므로 최종 기능 목록에 포함하지 않는다. Store의 IMAP accounts.capabilities에 SMTP ID를 섞지 않으며 Core/persistence 연결은 별도 계약으로 진행한다. 이 관측은 MAIL/RCPT/DATA나 발송 권한을 추가하지 않는다. 기존 모든 TLS·deadline·비밀 접근 순서·무재시도 경계를 유지한다.
