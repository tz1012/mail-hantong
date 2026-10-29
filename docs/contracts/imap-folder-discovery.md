# IMAP 폴더 탐색 v1

상태: M2-03/EXT-06의 읽기 전용 선행 구현. 2026-10-30 연속 개발 위임에 따른 계약이며 **사람 검토 필요**. 기존 [일회 연결 검사](imap-probe.md)는 유지하고 별도 API로 폴더를 탐색한다. 이 문서로 메일 읽기·변경 권한을 발급하거나 실제 계정에 접속하지 않는다.

## 경계와 순서

`SecretStore::discover_imap_mailboxes(account_id, &ImapEndpointConfig)`는 Windows에서 기존 TLS·인증 경계를 거친 뒤 인증 후 CAPABILITY → 고정 `LIST "" "*"` → LOGOUT을 한 번 수행한다. 다른 OS에서는 UnsupportedPlatform이다. TLS/hostname 검증 전에 비밀을 해제하지 않고 Secrets 밖으로 socket/session/credential callback/raw command를 내보내지 않는다. 임의 패턴·명령·CA/transport를 공개 인자로 받지 않는다. STATUS/SELECT/FETCH/STORE/APPEND/MOVE/EXPUNGE는 없다. 실패·취소 후 자동 재시도하지 않는다.

완료 결과는 Domain의 `ImapMailboxSnapshot`이며 account_id와 endpoint settings_version, 인증 후 `ImapCapabilities`, 폴더 descriptor 목록을 결속한다. 고정 tag OK와 LOGOUT BYE+OK까지 모두 확인한 뒤만 반환한다. 부분 성공·잘린 목록·잘린 이름을 반환하지 않는다. Store 영속화·Core 정책/도구 연결은 후속이며 현재 결과를 기존 account ready나 승인 증거로 사용하지 않는다.

호출자는 같은 호출에서 보유한 불변 endpoint 객체와만 결과를 소비한다. snapshot 자체는 endpoint 전체의 신원이나 영속 적용 권한이 아니며 단독 저장·재사용하지 않는다. 후속 Store 계약에서는 읽기 시작 시점의 정확한 endpoint/hash와 현재 account/settings version을 원자적으로 대조해야 한다. 현재 구현에는 snapshot을 Store에 적용하는 경로가 없다.

기본 LIST만 사용한다. 현재 고정 codec/types의 공개 builder는 extended LIST RETURN을 제공하지 않으므로 문자열 명령으로 우회하지 않는다. SPECIAL-USE가 광고되어도 역할 목록이 완전하다고 주장하지 않는다. 기본 LIST에 서버가 보낸 특수용도 속성은 capability 광고 여부와 별개로 보존할 수 있다.

위 `LIST "" "*"`는 빈 reference와 전체 wildcard의 의미다. 고정 `CommandBody::list("", "*")` builder의 실제 송신 bytes는 `h6 LIST "" *\r\n`이며 wildcard를 따옴표 없이 encoding한다. 시험은 이 canonical bytes를 확인한다.

## 응답 제한

LIST 중 untagged OK는 전체 frame 한도 안에서 허용한다. untagged NO/BAD는 이후 tagged OK가 와도 전체 탐색 실패로 처리한다. BYE·다른 DATA·다른 tag도 거부하며 이때 snapshot이나 부분 목록을 반환하지 않는다. 이는 불완전한 관측을 성공 목록으로 저장하지 않기 위한 제품 정책이다.

LIST 전용 frame reader는 CRLF·tag·단계·응답 종류를 검사한 뒤 고정 `ResponseCodec`으로 해석한다. 정상 LIST의 마지막 mailbox 이름에서만 literal을 허용한다. 선언 길이를 읽은 즉시 1,024-byte 한도와 남은 총량을 검사하고, 초과하면 body를 읽거나 drain하지 않고 연결을 폐기한다. 이름 literal에는 CRLF/NUL/제어 문자 허용 예외가 없다. 다른 DATA/literal·중첩 extension 구조·순서/문법 오류는 Protocol이다.

raw frame reader가 codec보다 먼저 literal marker의 위치·길이·남은 frame 예산을 검사한다. 불허된 선언 뒤 application parser가 body를 소비하거나 선언 길이에 비례한 buffer를 할당하지 않는다. OS/TLS/고정 크기 BufReader의 제한된 선행 수신은 이 소비 금지와 구분하며, 네트워크에서 body bytes가 전혀 도착하지 않는다고 주장하지 않는다. codec 자동 drain fallback은 없다.

응답 frame 전체는 16KiB, 한 이름의 wire bytes는 1,024, 속성은 최대 32개/atom 최대 128 bytes다. 알 수 없는 정상 속성은 bounded parse 후 버리며 명령·역할·권한으로 쓰지 않는다. LIST 결과는 최대 1,024개, LIST exchange의 전체 frame은 최대 1,056개다. 한 세션의 TLS 아래 wire budget은 4MiB, TLS 뒤 plaintext budget은 2MiB다. 출력은 기존 command당 32KiB/전체64KiB이며 전체 DNS/TCP/TLS/AUTH/LIST/LOGOUT deadline은 30초다. 단계 전환으로 수신 예산을 초기화하지 않는다. 기존 probe의 더 작은 한도는 바꾸지 않는다. 한도는 제품 결정이며 RFC 자체의 상한이 아니다.

이름의 quoted/literal/atom 전송 형식 차이는 동일한 decoded wire identifier로 합친다. 같은 canonical mailbox identity가 두 번 나오면 전체 탐색을 실패시킨다. INBOX만 ASCII 대소문자를 무시하여 `INBOX`로 정규화하고 다른 이름에는 case-fold/NFC/trim/대체문자 삽입을 하지 않는다.

identity는 atom/quoted/literal framing과 quoted escape만 제거한 bytes이며 modified UTF-7을 표시 문자열로 decode하기 **전**의 값이다. INBOX 이외에는 이 bytes를 정확히 보존한다. modified UTF-7의 encoded printable ASCII와 잘못된 padding은 별칭으로 합치지 않고 거부한다. 한 세션의 encoding 모드를 목록 전체에 적용하며 display 문자열로 중복 검사를 하지 않는다. 역할 속성 이름은 ASCII 대소문자를 무시한다. 위 출력 32KiB/64KiB는 **송신 command** 예산이며 수신 frame·wire·plaintext·snapshot 이름 총량 한도와 별개다.

## 순수 Domain 모델

`hantong-domain::mailboxes`는 다음 private-field 타입을 제공한다. Debug는 계정·폴더 이름을 가린다. 새 외부 의존성은 없다.

- `MailboxEncoding::{ModifiedUtf7,Utf8}`: post-auth CAPABILITY가 rev2만 광고하면 Utf8, rev1을 함께 광고하거나 rev1만이면 ModifiedUtf7다. ENABLE 확장은 아직 사용하지 않는다.
- `ImapMailboxName::from_wire(bytes, encoding)`: 1..1,024 bytes, rev1은 printable ASCII와 엄격한 modified UTF-7, rev2는 유효한 UTF-8을 요구한다. modified UTF-7은 `&-`, 쉼표로 바꾼 base64 alphabet, padding-bit/UTF-16 surrogate를 손실 없이 검증한다. 잘못된 encoding·직접/decoded 제어 문자·encoded printable ASCII를 거부한다. 표시 문자열은 최대 4,096 UTF-8 bytes다. wire identifier와 표시명을 별도 getter로 제공하며 둘 다 untrusted data다. Net-Unicode NFC 정규화의 완전한 검증이나 모든 rev2 서버와의 호환성을 주장하지 않는다.
- `SpecialUseRole::{All,Archive,Drafts,Flagged,Junk,Sent,Trash}`: 서버가 명시한 역할만 보존한다. 현지화된 이름으로 Sent/Trash 등을 추정하지 않는다. INBOX는 이름으로 구분하며 특수용도 role에 섞지 않는다.
- `MailboxChildren::{Unknown,HasChildren,HasNoChildren}`: 양쪽 속성이 함께 오거나 둘 다 없으면 Unknown이다.
- `ImapMailboxDescriptor::new(name,delimiter,no_select,non_existent,children,roles)`: delimiter는 없거나 printable ASCII 한 문자다. 역할은 정렬·중복 제거한다. no_select 또는 non_existent면 selectable=false다. 다른 이름을 계층 구분자로 분해해 새 서버 경로를 만들지 않는다.
- `ImapMailboxSnapshot::new(account_id,settings_version,capabilities,mailboxes)`: 계정 ASCII 영숫자/밑줄/하이픈 1..128 bytes, version 1..i64::MAX, 목록 최대1,024개, wire 이름 총량 최대1MiB, canonical identity 중복 없음. wire identifier 순으로 정렬한다. 빈 목록과 delimiter 없음도 표현한다. 결과의 역할 완전성은 기본 LIST에 한정되어 false다. UIDVALIDITY/UID를 추정하거나 포함하지 않는다.

Domain 오류는 고정 `MailboxError` 분류이며 원문을 넣지 않는다. Secrets는 잘못된 서버 자료를 기존 MailProbeError::Protocol로 변환한다. 공개 CLI 종료 코드나 공통 도구 오류는 추가하지 않는다.

## 특수 폴더의 순수 매핑

`hantong-adapters`는 Domain만 의존하며 네트워크·Store·Secrets를 호출하지 않는다. `resolve_imap_role(snapshot, role)`은 `ImapFolderRole::{Inbox,Special(SpecialUseRole)}`에 대해 `Missing`, `Unavailable`, `Ambiguous`, `Unique` 후보를 반환한다. Inbox는 canonical INBOX만, 다른 역할은 명시적 속성만 후보가 된다. 후보가 여러 개면 Ambiguous, 한 개지만 선택 불가면 Unavailable이다. Unique는 서버 후보이지 삭제 권한·사용자 매핑 승인·실제 폴더 존재의 영구 보증이 아니다. Trash가 불명확하면 삭제를 허용하는 bool이나 대체 EXPUNGE를 만들지 않는다. Gmail 다중 라벨이나 로컬 태그를 IMAP 폴더 이동으로 해석하지 않는다.

## 검증과 근거

순수 모델 시험을 먼저 작성하고 RFC 예시의 modified UTF-7/한글/emoji/ampersand·손상 surrogate·잘못된 padding·duplicate INBOX·이름 보존·한도·모호한 역할을 검사한다. Secrets의 private in-memory TLS/DPAPI seam에서 정상 quoted/literal 이름, 단계/tag 오류, literal 길이 선검사, 속성/목록/frame/총량 초과, TLS 실패 전 비밀 접근0, 인증 실패 전 LIST0, LIST/LOGOUT 실패 시 receipt0, 계정/version 결속을 검사한다. 실제 계정·외부 네트워크·Hello는 쓰지 않는다.

규격 근거: [RFC 3501 LIST·modified UTF-7](https://www.rfc-editor.org/rfc/rfc3501.html), [RFC 9051 LIST·rev2 mailbox](https://www.rfc-editor.org/rfc/rfc9051.html), [RFC 6154 특수용도](https://www.rfc-editor.org/rfc/rfc6154.html), [RFC 5258 확장 LIST](https://www.rfc-editor.org/rfc/rfc5258.html). 고정 로컬 배포 소스 `io-imap 0.7.1`, `imap-codec 2.0.0-alpha.9`, `imap-types 2.0.0-alpha.7`에서 `CommandBody::list`, `Data::List`, `ResponseDecodeError::LiteralFound` API를 확인했다. API 존재 확인은 실제 상호 운용성 검증이 아니다.
