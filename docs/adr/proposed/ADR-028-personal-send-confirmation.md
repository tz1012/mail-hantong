# ADR-028: 개인 CLI의 메일 내용 확인과 발송

상태: 2026-11-02 사용자 UX 방향 승인, 구현·보안 사람 검토 필요.

사용자는 직접 작성한 메일마다 Windows Hello가 열리는 흐름을 단순화하도록 지시했다. 개인 Gmail CLI에서는 작성 → 네이티브 내용 확인 → 보내기로 끝낸다. 계정 연결의 Google 동의와 메시지별 발송 확인은 별개다.

Core가 현재 Store에서 Active/Cli/기본 CLI label/Google OAuth 출처/writable/정확한 단일 계정/정확한 개인 발송 도구 범위/Gmail ReadSend binding을 확인한다. 요청에 human/direct/approved 플래그를 추가하지 않는다. profile epoch는 고정 숫자가 아니라 pending과 현재 등록의 일치로 검증한다. 같은 조건을 즉시 Store claim 트랜잭션에서 다시 검사한다. DB 마이그레이션은 필요 없다.

기존 OS Verified 전용 `VerifiedSend`와 별도로 `ConfirmedPersonalSend`를 둔다. 새 proof는 신뢰된 네이티브 창의 명시적 보내기 버튼에서만 발급하며 전체 불변 snapshot, 실제 표시, 만료, 단일 소비에 결속한다. CLI/MCP는 proof를 발급하지 않는다. 다른 등록은 기존 Hello 경로를 유지한다. audit, 정책, CAS/outbox, SEND_UNKNOWN 무재전송은 유지한다.

개인 창에는 보내는 주소, 수신자 전체, 제목, 본문 전체, 첨부 이름·형식·크기를 표시한다. 내부 ID와 해시는 숨기되 내부 결속에는 유지한다. 한국어·영어 및 키보드 취소를 유지한다.

CLI 등록은 실제 사람이 명령을 실행했다는 증거가 아니다. 같은 Windows 사용자 권한의 프로그램은 CLI 호출과 UI 자동화를 시도할 수 있다. 이 변경은 해당 경로의 OS 사용자 존재 확인을 제거하는 명시적 절충이며, AI 요청을 완전히 식별·차단한다는 보장을 하지 않는다. APR-04/INST-04 변경 제안은 별도 문서로 보존한다. 사용자 결정·테스트 통과는 사람 보안 검토를 대체하지 않는다.
