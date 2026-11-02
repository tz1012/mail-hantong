# CLI 발송 계약

상태: 구현 초안, **사람 보안 검토 필요**. APR-01, AI-09, SYNC-07.
상세 원장·정책 계약은 [native-send-ledger.md](native-send-ledger.md)를 따른다.

`hantong draft create`는 계정, 수신자, 제목, UTF-8 본문 파일을 검증하고
암호화 저장소에 버전 1의 불변 초안을 만든다. 본문 최대 크기는 1MiB다.
응답의 draft ID, version, canonical compose payload hash를 `hantong send`에
지정한다. 이 명령은 `mail.send.request`로 보호된 사람 검토를 요청한다.
상태 조회는 `hantong send status --account ACCOUNT --review-id REVIEW_HEX`다.

CLI·MCP는 승인 문자열이나 토큰을 발급·소비하지 않는다. native 창이 계정,
SMTP 발신자·서버, 수신자 전부, 제목, 전체 본문을 표시하고 보호 등록에서는 OS 사람 확인을
요구한다. 개인 Google CLI 예외는 아래 규칙을 따른다. 확인은 정확한 draft/version/hash, profile epoch, 계정·SMTP 설정
버전, operation/idempotency와 결속된다. 일반 RPC `mail.send.execute`는
차단되며 신뢰된 Core coordinator만 opaque proof를 소비한다.

Core는 현재 권한과 정책을 재검사한 후 원장에서 단 한 번의 `sending`
claim을 커밋하고 SMTP를 시작한다. 명확한 성공은 `sent`, 명확한 거부는
`failed`, 응답 불확실·전송 도중 권한 변경·시작 시 남은 sending은
`unknown`과 `SEND_UNKNOWN`이다. 자동 재시도나 재발송은 없다.
동일 operation/idempotency의 재요청은 기존 결과만 반환한다.

canonical hash는 사람이 검토하는 계정·수신자 순서·제목·본문을 결속한다.
전송 모듈은 이를 MIME 인코딩하고 Date/Message-ID를 생성한다.
원시 SMTP 응답·인증값·수신자·제목·본문은 상태 오류에 반사하지 않는다.
개발 검증은 가짜 제공자와 메모리 TLS 스트림만 사용한다.

## 2026-11-02 개인 Google CLI 예외

위 OS 본인 확인 조건은 native-send-ledger의 개인 발송 예외에서만 대체된다. Core가 저장된 정확한 개인 Google CLI 범위를 판정하면 별도의 불투명 네이티브 내용 확인 proof를 소비한다. 사용자는 내용을 보고 보내기를 누르며 Hello는 없다. CLI yes나 새 플래그로 발급할 수 없고 같은 guard를 claim 트랜잭션에서 재검사한다. 다른 등록은 기존 OS Verified 경로를 유지한다. source identity는 사람 실행 증거가 아니며 사람 보안 검토가 필요하다.
