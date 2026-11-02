# ADR-029: 개인 CLI에서 작성·확인·발송 완료

상태: 2026-11-02 최신 사용자 UX 정정에 따른 구현 결정. **사람 보안 검토 필요**.

사용자는 별도 창 없이 CLI/TUI에서 앱 이용을 끝내라고 명시했다. ADR-028의 개인 네이티브 내용 확인창은 이 결정으로 대체한다. 현재 명령형 CLI를 완성하며 전체 화면 TUI를 구현했다고 표시하지 않는다. 일반 개인 Gmail 연결·조회·작성·내용 확인·발송 결과는 터미널에서 이어진다. Google 로그인·동의만 공급자 브라우저를 사용한다.

Core가 불변 draft에서 전체 preview를 만들고 CLI가 안전하게 표시한 뒤 보내기·취소를 받는다. prepare는 발송이나 창을 예약하지 않는다. confirm은 account/review/hash/owner/epoch/만료와 정확한 개인 Google 출처·범위를 확인한 뒤 별도 terminal job 하나만 예약한다. claim 시 기존 동일 트랜잭션 guard와 감사·정책·outbox를 재검사한다. cancel/EOF는 전송하지 않고 pending/unknown에서는 재요청하지 않는다. JSON/no-input/리다이렉션은 발송 확인을 수행하지 않는다. 상세 wire는 [terminal-personal-send](../../contracts/terminal-personal-send.md)다.

터미널 선택은 요청 의도이며 OS가 확인한 사람 존재 증거가 아니다. 같은 사용자 권한의 AI도 기본 CLI 자격증명을 사용할 수 있고 TTY는 보안 신원 경계가 아니다. 이 잔여 위험을 숨기거나 CLI 확인을 암호학적 사람 승인이라고 표현하지 않는다. 다른 등록의 발송 권한이나 MCP 도구를 자동 확장하지 않는다. 기존 strong 경로와 비밀값 보호는 유지한다. 사용자 UX 결정·합성 시험·에이전트 리뷰는 사람 보안 검토 완료가 아니다.
