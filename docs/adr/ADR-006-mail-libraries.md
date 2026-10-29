# ADR-006 메일 연결 모델과 전송 라이브러리

상태: 2026-10-30 순수 연결 설정 모델·TLS 원칙 채택. 분리 실험의 Rust 1.95 호환성과 in-memory TLS/AUTH 시험을 바탕으로 SMTP/직접 TLS 의존성을 Windows Secrets에 통합한다. IMAP/MIME 제품 의존성은 후속이다. 사람 보안 검토 필요.

M2-01의 설정은 네트워크·OS에 의존하지 않는 `hantong-domain`에 두고 기존 serde/serde_json만 사용한다. 연결 설정의 JSON 경계·크기·TLS enum은 [연결 계약](../contracts/mail-connection.md)을 따른다. host/login/authentication은 IMAP·SMTP별로 독립 저장하며 TLS를 끄는 값은 제공하지 않는다. 설정 객체는 SecretHandle·토큰·비밀번호를 포함하지 않는다.

IMAP은 io-imap 0.7.1의 IO-free 상태 처리·스트리밍을 선택한다. async-imap 0.12.0의 Tokio 통합도 검토했으나, 연결과 자격 증명 접근을 Secrets 내부에서 소유하고 literal 크기를 소비 전에 제한하는 구조에는 IO-free 엔진이 맞는다. SMTP는 아래 Secrets 소유의 implicit TLS/필수 STARTTLS 흐름을 사용하며 opportunistic 모드는 허용하지 않는다. 실제 OS 신뢰 저장소와 Windows·macOS 인터넷 연결은 후속 실행 시험이 필요하다. 메모리·속도 우열은 아직 측정하지 않았다.

확인한 공식 manifest: [io-imap](https://github.com/pimalaya/io-imap/blob/v0.7.1/Cargo.toml)은 0.7.1/Rust 1.88, [lettre](https://github.com/lettre/lettre/blob/v0.11.23/Cargo.toml)는 0.11.23/Rust 1.85다. [mail-parser main](https://github.com/stalwartlabs/mail-parser/blob/main/Cargo.toml)은 Rust 1.98을 요구하므로 현재 toolchain에 그대로 채택하지 않는다. crates.io의 버전별 metadata와 분리 lockfile을 함께 확인했다.

| 라이브러리 | 고정 버전과 feature | 범위 |
| --- | --- | --- |
| io-imap | =0.7.1, defaults off, client + scram | IMAP 상태 엔진 |
| lettre | =0.11.23, defaults off, smtp-transport + tokio1 + tokio1-rustls + rustls-platform-verifier + ring | SMTP 연결·AUTH probe, 발송은 후속 승인/Outbox 경로에서만 |
| mail-parser | =0.11.9, full_encoding | MIME·한국어 포함 다중 문자셋 해석 |
| mail-builder | =1.0.0, defaults off | 후속 MIME 생성 |
| nom | =7.1.3, defaults off, alloc | 아래 공개 의존 feature 문제의 제한적 보완 |

분리된 [실험](../../experiments/mail-stack/README.md)에서 Rust 1.95.0 Windows/MSVC, Tokio 1.54.1로 locked/offline test 1개가 통과했다. 한국어 제목·본문 MIME 생성/해석 및 public API 컴파일만 확인한다. 실제 서버·TLS handshake·인증서·AUTH·성능 시험은 아니다. 로그는 `target/mail-stack-compatibility-check.log`다.

최초 컴파일은 imap-codec 2.0.0-alpha.9의 published manifest가 nom 기본 feature를 끄고 alloc을 켜지 않아 실패했다. nom 7.1.3의 공개 alloc feature를 직접 통합하여 해결했으며 source patch는 없다. 제품 도입 때도 이 보완을 명시하고 lockfile과 검사를 유지한다. upstream 수정 후 제거할 수 있는지 검토한다. 실험 밖 제품 의존성을 자동으로 바꾸지는 않는다.

io-imap 0.7.1에는 미배포 master의 max_literal_len/sizes 옵션이 없다. [배포 소스](https://github.com/pimalaya/io-imap/blob/v0.7.1/src/rfc3501/fetch_stream_batch.rs)의 WantsStream { len } 결과에서 소비 전에 자체 상한을 검사하고 초과하면 연결을 폐기해야 한다. 무제한 버퍼나 drain을 통한 우회는 허용하지 않는다.

AUTH 코드와 라이브러리의 자격 증명 객체는 Secrets 경계 안에 둔다. 단순 with_secret callback이나 외부 임의 stream에 비밀을 쓰는 기능은 금지한다. Secrets 소유의 구체적 broker가 endpoint 검증·TLS·비밀 해제·AUTH를 함께 수행한다. 인증된 IMAP 세션은 Clone/Serialize/Deref/into_inner/raw command/socket을 노출하지 않고 읽기 동작만 제공한다. STORE/APPEND/MOVE/EXPUNGE는 없다. SMTP는 이 단계에서 AUTH probe의 상태와 QUIT만 제공하며 제출·발송 session을 반환하지 않는다. 이후 M4 승인/Outbox capability 없이 발송을 추가할 수 없다.

자격 증명은 [Secrets 계약](../contracts/secrets.md)의 IMAP/SMTP별 여섯 slot을 사용하며 legacy 공용 비밀번호로 폴백하지 않는다. 라이브러리의 문자열 복사 수명도 후속 검토 대상이다. 자체 DPAPI buffer의 지움만으로 라이브러리 내부 복사본의 완전한 zeroization을 주장하지 않는다. 의존 edge·스풀·구체적 상태/크기 한도는 broker 계약에서 추가한다. 모델 검증 완료를 실제 TLS 연결·로그인·메일 수신 완료로 기록하지 않는다.

## 배포 버전 API 확인과 후속 통합 조건

io-imap 0.7.1의 현재 실험 feature(client/scram)는 내장 TLS connect helper를 켜지 않는다. `ImapSessionOpen` 등의 IO-free 엔진과 별도 TLS driver를 결합하거나 feature를 명시적으로 보완해야 한다. 표준 client의 100MiB fragmentizer 기본값도 제품 한도로 사용할 수 없다. [0.7.1 client](https://github.com/pimalaya/io-imap/blob/v0.7.1/src/client.rs), [session engine](https://github.com/pimalaya/io-imap/blob/v0.7.1/src/session.rs).

lettre 0.11.23 `test_connection()`은 builder에 credential을 먼저 넣어야 하므로 TLS 검증 뒤 비밀을 해제하는 제품 순서에 사용하지 않는다. private `AsyncSmtpConnection`을 credential 없이 연결·TLS 검증한 뒤 AUTH해야 한다. 이 API의 응답 제한은 read_line 이후 검사하므로 newline 없는 악성 입력의 선행 메모리 상한이 아니다. 제품에서는 내부 transport에 총 수신 byte budget과 전체 deadline을 강제하고, 설정한 상한을 입증하는 in-memory 시험 후에만 통합한다. [connection](https://github.com/lettre/lettre/blob/v0.11.23/src/transport/smtp/client/async_connection.rs), [stream seam](https://github.com/lettre/lettre/blob/v0.11.23/src/transport/smtp/client/async_net.rs).

이 점검은 실험의 compile/MIME smoke 성공을 취소하지 않지만 실제 TLS/AUTH 동작이 아직 검증되지 않았음을 구체화한다. 직접 rustls/Tokio TLS driver와 테스트 CA 의존은 버전·feature·상한 계약을 정한 뒤 추가한다. TLS 검증 무효화, 외부 공개 stream injection, SMTP send API는 추가하지 않는다.

## 2026-10-30 SMTP broker 통합

Windows Secrets에 lettre 0.11.23, rustls 0.23.46(ring/std/tls12), tokio-rustls 0.26.6(ring/tls12), rustls-platform-verifier 0.7.1, Tokio 1.54.1을 고정해 추가했다. 모두 공개 버전의 API를 사용하며 source patch는 없다. IMAP/MIME 후보는 아직 분리 실험에만 있다. 구체적 범위는 [SMTP probe 계약](../contracts/smtp-probe.md)을 따른다.

implicit TLS는 자체 Rustls connector/OS verifier/hostname 검증 후 private Lettre driver에 넣는다. Lettre의 injected stream은 내부적으로 plain으로 표시되므로 해당 경로의 TLS 상태 판정·STARTTLS·certificate API를 쓰지 않는다. TLS 완료는 private VerifiedSmtpConnection 생성 경로가 보증한다. STARTTLS는 실제 upgrade 뒤에만 이 타입을 만든다. upstream [Tokio TLS client](https://github.com/rustls/tokio-rustls/blob/v0.26.6/src/client.rs), [OS verifier](https://github.com/rustls/rustls-platform-verifier/blob/v0.7.1/src/lib.rs)를 확인했다.

독립 검토와 실패 회귀 시험에서 Lettre BufReader의 STARTTLS 이전 평문 잔여 응답이 TLS 이후 응답으로 읽히는 현상을 재현했다. 제품은 하위 bounded buffer와 전환 전 1-byte 전달 gate를 적용해 이를 차단한다. 무검증 TLS나 라이브러리 source fork로 해결하지 않았다. 기존 실험의 단순 starttls 호출은 제품의 완성된 안전 경계가 아니다. 정상 연결뿐 아니라 220 뒤에 가짜 EHLO/AUTH/QUIT 응답을 함께 넣어도 TLS 오류·unseal 0·AUTH 0인지 검증한다.

AUTH 235와 QUIT 221을 직접 검사하며 다른 positive reply를 성공으로 인정하지 않는다. 고정 typed Quit command를 사용해 quit() 자체의 재시도 부작용을 피한다. 라이브러리 STARTTLS 거절 처리의 종료용 QUIT는 허용하지만 인증·평문 폴백은 없다. TLS·DPAPI·가짜 서버 시험을 실제 제공자 연결이나 OS 신뢰 저장소의 현장 검증으로 기록하지 않는다.

## 2026-10-30 IMAP 경계 재검토

0.7.1의 `ImapSend`는 첫 tagged response의 tag를 보낸 command tag와 비교하지 않는다. `ImapStartTls`는 tag를 buffer의 임의 위치에서 찾고 응답이 OK인지 검사하지 않는다. 이 고수준 send/session/STARTTLS 경로를 제품 broker에 직접 사용하지 않는다. 공개 `codec`/`types`를 이용한 좁은 연결 검사에서 frame 크기·CRLF·literal 부재·허용 응답 종류·tag 일치·OK/NO/BAD를 명시적으로 검사한다. 이는 다른 서버의 응답이나 명령문으로 인증 상태가 바뀌지 않게 하기 위한 결정이다. IO-free FETCH 등 후속 기능도 별도 경계 검토 없이 채택하지 않는다.

또한 `imap-types::Secret`의 Debug는 개발 빌드에서 값을 보일 수 있고 io-imap/io-sasl의 trace가 원문 bytes를 기록한다. 제품에 이 의존성을 추가하면 `log =0.4.34`의 `max_level_off`를 Secrets 의존에서 활성화하고 정적 수준 시험을 둔다. Core에만 넣으면 Secrets 단독 시험 graph에는 적용되지 않으므로 충분하지 않다. 자체 구조화 Core 로그는 log facade를 쓰지 않는다. 새 의존성이 다른 log 버전/상충 max_level feature를 추가할 때 이 경계를 다시 검사한다.

근거: [io-imap 0.7.1 send](https://github.com/pimalaya/io-imap/blob/v0.7.1/src/send.rs), [STARTTLS](https://github.com/pimalaya/io-imap/blob/v0.7.1/src/rfc3501/starttls.rs), [log compile-time filters](https://docs.rs/log/0.4.34/log/#compile-time-filters), [RFC 3501 STARTTLS](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.2.1). 현재 제품에는 io-imap을 아직 추가하지 않았고 분리된 hostile-response/TLS 시험을 진행한다.

## 2026-10-30 codec 전용 IMAP broker 통합

위 분리 시험 이후 Windows Secrets에 io-imap 0.7.1(client/scram), nom 7.1.3(alloc), log 0.4.34(max_level_off)를 고정해 추가했다. 고수준 coroutine 대신 codec/types만 사용한다. 실제 DPAPI envelope + in-memory TLS에서 제품 gate/driver 시험 26개가 통과했으며 SMTP/DB 회귀를 포함한 Secrets 시험 89개와 Clippy도 통과했다. 세부 경계는 [IMAP probe 계약](../contracts/imap-probe.md)에 있다. MIME 의존성은 아직 제품에 추가하지 않았다.

기능 snapshot은 인증 뒤 새 CAPABILITY만 사용한다. IMAP4REV2만 광고하면 기본 IDLE/MOVE/UIDPLUS를 반영하고, rev1과 rev2를 함께 광고하면 명시적 기능만 반영한다. SPECIAL-USE는 extended LIST 지원을 의미하며 실제 특수 폴더의 존재와 구분한다. AUTH 중 bounded status/CAPABILITY를 허용하되 기능·인증 방식은 바꾸지 않는다. SASL-IR은 명시적으로 광고된 경우만 사용하고 그 외에는 한정된 continuation 교환을 한다. 근거: [RFC 9051 CAPABILITY](https://www.rfc-editor.org/rfc/rfc9051.html#section-6.1.1), [Appendix E](https://www.rfc-editor.org/rfc/rfc9051.html#appendix-E), [AUTHENTICATE](https://www.rfc-editor.org/rfc/rfc9051.html#section-6.2.2), [RFC 4959](https://www.rfc-editor.org/rfc/rfc4959.html), [RFC 6154](https://www.rfc-editor.org/rfc/rfc6154.html#section-2).

codec의 ASCII response text 및 좁은 응답 whitelist 때문에 모든 rev2/비표준 서버와 호환된다고 주장하지 않는다. 제품 호출자는 CA/transport/비밀 callback을 주입할 수 없으며 실제 OS 신뢰와 제공자 상호 운용성은 후속 검증이다. 독립 코드 검토에서 중대한 결함은 발견되지 않았으나 이는 사람 보안 검토가 아니다.
