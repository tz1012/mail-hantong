# `mail.messages.get` 본문 읽기 계약 초안

상태: 구현 초안, 사람 보안 검토 필요. 이 계약은 로컬 메타데이터 검색과 별개로
정확히 하나의 메시지 본문을 읽는다. 본문은 신뢰할 수 없는 데이터이며 CLI는
`hantong_contracts::message_body::render_terminal_text`를 거친 불활성 텍스트만
출력한다.

## 요청

공통 JSON-RPC envelope의 `tool`은 `mail.messages.get`이고 `params`는 다음의
정확한 필드만 받는다.

| 필드 | 규칙 |
|---|---|
| `mode` | 정확히 `imap_body` |
| `account_id` | 계정 범위와 일치하는 영숫자/`-`/`_`, 1–128 bytes |
| `mailbox_wire_hex` | 현재 mailbox의 wire name을 hex로 표현 |
| `mailbox_encoding` | `modified_utf7` 또는 `utf8` |
| `uidvalidity` | 1 이상의 현재 UIDVALIDITY |
| `uid` | 1 이상의 UID |
| `max_bytes` | 1–1,048,576; 서버 요청 상한도 이 값을 넘지 않음 |

키는 `account_id + mailbox_wire_hex + mailbox_encoding + uidvalidity + uid`의
전체 조합이다. UIDVALIDITY가 바뀌면 캐시를 사용하지 않고 `CONFLICT`를 반환한다.
먼저 read-only `EXAMINE`으로 UIDVALIDITY를 확인한 뒤 IMAP 요청은 정확히
`UID FETCH <uid> (UID BODY.PEEK[])`이고 `STORE`, `EXPUNGE`,
`BODY[]`, `RFC822`는 금지한다. literal이 상한을 넘거나 UID가 일치하지 않으면
실패하며 자동 재요청하지 않는다.

## 캐시·출력

본문 캐시는 기존 SQLCipher 데이터베이스 안에 전체 키를 포함해 저장하고,
계정·mailbox·UIDVALIDITY·UID 중 하나라도 다르면 조회하지 않는다. 평문 본문을
로그, 오류, 감사 결과, IPC 응답의 진단 필드에 넣지 않는다. 캐시가 없는 경우에만
Secrets 소유 TLS 경로로 한 번 읽는다. HTML은 렌더링하지 않고 태그·ANSI escape·제어
문자를 제거한 텍스트로 출력한다.

## 권한

`mail.messages.get`는 계정별 명시 scope가 필요하다. 이 scope는 기존 metadata
scope에 묵시적으로 추가되지 않는다. 본문 읽기는 발송·승인 발급·서버 변경 권한을
부여하지 않는다.

## 오류

잘못된 입력은 `INVALID_INPUT`, scope 불일치는 `SCOPE_DENIED`, 현재 UID namespace와
불일치는 `CONFLICT`, 없는 메시지는 `NOT_FOUND`, bounded literal 위반은
`QUOTA_EXCEEDED`, TLS/provider 실패는 `OFFLINE` 또는 `INTERNAL_ERROR`로 매핑한다.

## 내부 메시지 ID 해석과 비동기 응답

`mode: locator` 요청은 `account_id`, 검색 결과의 `internal_message_id`(`msg_`와 64자리 소문자 hex), `max_bytes`만 받는다. 코어가 저장된 위치의 전체 IMAP 식별자를 재구성하고 위치 hash와 현재 namespace를 확인한다. 동일 ID에 서로 다른 위치가 있으면 INTERNAL_ERROR다. 응답은 schema_version=1, ok=true, account_id, internal_message_id, mailbox_wire_hex, mailbox_encoding, uidvalidity, uid만 포함한다. 위치 해석에도 동일 본문 scope·정책·감사 경로를 적용한다.

본문 응답은 schema_version=1, ok=true, state, 전체 IMAP 식별자, text, truncated만 포함한다. 캐시가 없으면 state=pending, text=null, truncated=false를 반환하고 코어의 기존 계정별 IMAP 슬롯에 읽기 작업 하나를 넣는다. 동일 요청의 폴링은 중복 fetch를 만들지 않는다. 결과 캐시는 scope/profile epoch/endpoint version/UID namespace를 다시 확인한 뒤 저장한다. 권한 철회 시 취소한다. 비영속 읽기 작업은 재시작 시 재개하지 않는다.

CLI는 최대 30초 동안 초당 한 번 상태를 확인한다. ready text는 최대 65,536 UTF-8 bytes이며 잘린 경우 truncated=true다. 원문 수신 상한은 별도 max_bytes(최대 1MiB)다. SQLCipher 캐시의 원문 hash, endpoint 버전과 namespace를 확인한다. 캐시 손상은 INTERNAL_ERROR다.

MIME은 제한된 깊이/part 수에서 text/plain을 우선 선택하고 text/html을 불활성 텍스트로 처리한다. base64와 quoted-printable을 해독하며 UTF-8/ASCII/ISO-8859-1을 지원한다. 지원하지 않는 charset·binary part·잘못된 전송 인코딩은 CAPABILITY_UNSUPPORTED다. 첨부·원격 이미지·링크·HTML 스크립트는 실행하지 않는다. 모델에는 본문을 자동 전달하지 않는다.
