# 서버 기능 snapshot 계약 v2

상태: M2-02/EXT-05의 공통 타입 선행 구현. 2026-10-30 연속 개발 위임에 따른 계약이며 사람 검토 필요. 기능 광고는 계정 권한·클라이언트 grant·승인 증거가 아니다. Gmail/Graph의 기능 매핑과 실제 대체 동작은 후속 구현이다.

이번 범위는 M2-02의 IMAP 부분 구현이며 공통 제공자 기능 전체나 M2-02 완료가 아니다. SMTP 확장 snapshot은 아래 별도 타입으로 구현하며 fallback과 Core 연결이 남아 있음을 같은 카드 상태장부에 유지한다.

현재 제품의 IMAP 기능 여섯 개를 `hantong-domain::capabilities::ImapCapabilities`에 한 번 정의한다. `ImapFeature`는 Condstore, Idle, Move, Qresync, SpecialUse, Uidplus이며 고정 ID는 각각 `imap.condstore`, `imap.idle`, `imap.move`, `imap.qresync`, `imap.special_use`, `imap.uidplus`다. 기존 Secrets 타입 경로는 같은 타입의 re-export로 유지한다. 내부 필드는 private이고 새 외부 의존성은 추가하지 않는다.

`from_features(iter)`는 알려진 typed feature를 집합으로 만들며 중복은 같은 기능이다. `from_ids(&[String])`는 저장 snapshot을 읽는 경계로 최대 6개, 엄격한 사전순, 중복/알 수 없는 ID 금지를 적용한다. 빈 목록은 알려진 추가 기능 없음이다. `to_ids()`는 위 고정 ID의 정렬된 목록을 반환한다. 기존 idle/move_messages/uidplus/condstore/qresync/special_use getter는 유지한다. 타입 생성만으로 서버 기능을 검증했다고 주장하지 않는다.

Secrets의 인증 후 CAPABILITY 해석은 그대로 유지한다. rev2만 광고한 경우 기본 IDLE/MOVE/UIDPLUS를 반영하고 rev1+rev2 동시 광고에는 명시된 기능만 반영한다. `SPECIAL-USE`는 extended LIST 지원이며 Sent/Trash 폴더의 실제 존재가 아니다. protocol token이나 모르는 속성을 이 DTO에 저장하지 않는다. `mail.read`, `mail.send` 등의 권한 이름은 IMAP 서버 기능 snapshot 입력에서 거부한다.

Core가 probe receipt를 Store의 기존 `InitialImapProbeOutcome::Ready(Vec<String>)`로 옮길 때 `to_ids()`를 사용한다. Store의 기존 ready 기록·journal 검증은 `from_ids()`로 검증하며 실패를 기존 INTERNAL_ERROR로 바꾼다. v7 스키마/저장 ID/기존 행/CLI 계정 목록 응답은 변경하지 않는다. `AccountSummary`의 다른 제공자 capability 문자열을 IMAP 전용 검증기로 전부 제한하지 않는다.

작업 순서는 기존 Secrets probe와 Store 잘못된 snapshot 회귀를 유지하고, Domain의 전체 64개 feature 조합 roundtrip·엄격 입력 거부 시험을 먼저 작성한 다음 중복 문자열 매핑 세 곳을 같은 타입으로 연결한다. 기존 TLS/AUTH 시험과 Store 전체·Core probe 분류 시험을 재실행한다. 이 범위에서 폴더 탐색·메일 변경·평문 비밀 접근·새 tool grant는 추가하지 않는다.

## SMTP 고정 기능

`hantong-domain::smtp_capabilities`에 `SmtpFeature::{EightBitMime,AuthLogin,AuthPlain,AuthXoauth2,SmtpUtf8}`, private bitset `SmtpCapabilities`, `SmtpAuthMechanism::{Plain,Login,Xoauth2}`를 정의한다. 정렬된 고정 ID는 `smtp.8bitmime`, `smtp.auth.login`, `smtp.auth.plain`, `smtp.auth.xoauth2`, `smtp.smtputf8`다. `from_features`는 typed 집합, `from_ids`는 최대 5개 정렬·중복/unknown 금지, `to_ids`는 정렬된 고정 ID만 반환한다. getter는 `eight_bit_mime`, `auth_login`, `auth_plain`, `auth_xoauth2`, `smtp_utf8`이다.

32개 조합 roundtrip·비정렬/중복/unknown/권한 ID 거부를 검사한다. Secrets는 post-TLS/pre-AUTH EHLO만 private mapping하고 실제 선택 AUTH를 별도 기록한다. 원문·서버 이름·미지 extension·STARTTLS는 snapshot에 넣지 않는다. 계정/grant/승인 권한과 무관하며 IMAP 저장 ID에 혼합하지 않는다. 기존 고정 Lettre 0.11.23 API를 사용하고 외부 의존성을 추가하지 않는다.
