# IMAP 단일 메일 휴지통 이동 — 구현 계약

2026-11-03 사용자 지정 공통 메일 기능. 구현·통합 시험 진행 중이며 사람 보안 검토 필요.

## 요청과 검토

`mail.messages.trash.request`의 IMAP 요청은 `kind=imap_trash`, account_id,
mailbox_wire_hex, mailbox_encoding, expected_catalog_generation, uidvalidity,
uid, operation_id, idempotency_key, payload_hash를 가진다. 숫자 범위는 기존
IMAP UID/catalog와 같으며 빈 계정·UID 0·시퀀스 번호·와일드카드·UID 집합은 거부한다.
기존 Gmail kind=trash의 의미와 원장은 바꾸지 않는다. 원문 Message-ID는 식별자가 아니다.

요청은 pending만 저장하고 provider job을 만들지 않는다. Core가 현재
account/settings/endpoint/catalog 결속과 MOVE capability를 확인하고 현재
catalog에서 selectable한 `\Trash` 역할을 정확히 하나 선택한다. 없음·모호함·
선택 불가·MOVE 미지원은 변경 명령 전에 CAPABILITY_UNSUPPORTED다. 클라이언트나
메일 내용이 목적지를 정하지 않는다. 이미 Trash인 메일을 영구 삭제로 바꾸지 않는다.

불변 검토 snapshot에는 owner registration·epoch·account version·endpoint
hash/version·catalog generation·원본 mailbox/encoding·UIDVALIDITY/UID·목적지
mailbox/encoding·intent hash·review ID·만료가 결속된다. 사람은 같은 터미널에서
정확한 계정·메일·원본/목적지를 보고 이동 또는 취소를 고른다. 이 선택은 OS 사람
존재 증거가 아니다. AI/MCP의 응답은 확인을 발급하지 않는다.

확인 시 현재 등록·선택 계정의 trash 권한·scope epoch·연결·catalog·원본 UID를
다시 검사하고, audit start와 이동 claim을 같은 transaction에 저장한다. 재사용·
취소·만료·다른 계정/내용/목적지·동시 요청은 provider 효과를 만들지 않는다.

## 실행과 복구

Secrets만 해당 계정·endpoint에 결속된 credential을 연다. TLS 실패에 평문 폴백은
없다. 인증 후 capability를 재확인하고 정확한 원본을 SELECT해 READ-WRITE와
preview의 UIDVALIDITY를 확인한 다음 `UID MOVE <한 UID> <정확한 Trash>`를 한 번
보낸다. mailbox는 검증된 IMAP encoder로 직렬화하며 원문을 명령에 이어 붙이지 않는다.
catalog 이후 capability나 UIDVALIDITY가 바뀌면 MOVE 없이 거부한다.

tagged OK만 성공으로 확정한다. 명령 전 확정 실패는 rejected다. MOVE 전송 이후
연결 단절·timeout·응답 손실/모순·부분 결과는 unknown으로 두며 자동 재시도하지
않는다. tagged NO도 부분 이동 가능성을 이유로 원복됐다고 단정하지 않는다.
COPYUID가 있으면 정확한 단일 source/destination UID mapping만 받아 결속한다.
같은 대상의 unresolved unknown은 새 operation ID로도 다시 실행하지 않는다.
moving 원장의 시작 복구는 네트워크 호출 없이 unknown으로 끝낸다.

COPY+STORE, \Deleted, bare EXPUNGE, UID EXPUNGE, CLOSE, 자동 Trash 생성은 이
기능의 폴백이 아니다. 영구 삭제·대량 삭제는 제공하지 않는다. 이동 결과가 불확실한
오류는 기존 Gmail mutation과 같은 SEND_UNKNOWN 코드/무재시도 의미를 유지하고,
사람용 UI는 휴지통 이동 상태 미확인으로 설명한다.

## 저장·호환

schema v26에 별도 imap_trash_reviews/imap_trash_outbox를 추가한다. Gmail mutation
history와 외래 키를 재해석하지 않는다. 기존 v25를 동결 검증하고 암호화 백업·원자적
migration·rollback·foreign_key_check·sequence high-water를 보존한다. migration은
계정·credential·grant·요청·실행을 만들지 않는다. 새 CLI manage toolset은 기존 C를
바꾸지 않고 trash.request/execute를 추가한 상위 범위다. 기존 scope hash는 동결하고
새 버전의 명시적 terminal_cli 전이만 이 범위를 발급한다. Google은 Modify 동의를
별도로 충족해야 하며 SMTP만으로 IMAP 휴지통 권한을 추론하지 않는다.

## 근거와 수용 시험

[RFC 9051 MOVE](https://www.rfc-editor.org/rfc/rfc9051.html#section-6.4.8),
[RFC 6851](https://www.rfc-editor.org/rfc/rfc6851.html),
[RFC 6154 SPECIAL-USE](https://www.rfc-editor.org/rfc/rfc6154.html).

ACC-02/03, CLI-15, APR-01, AI-09, EXT-03/04, SYNC-07, SEC-01/03/08:
합성 IMAP 서버에서 두 계정 분리·단일 MOVE 성공·취소·만료·UIDVALIDITY 변경·
catalog/endpoint 변경·capability/Trash 누락·명령 주입·응답 단절·재시작 무재시도·
동시 claim을 검증한다. 모든 wire 기록에서 EXPUNGE/COPY/STORE/CLOSE가 없어야 한다.
실제 사용자 계정/네트워크는 자동 시험에 사용하지 않는다.
