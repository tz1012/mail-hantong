# 터미널 개인 발송 요건 변경 제안

최신 사용자는 별도 창 없이 CLI/TUI에서 앱 이용을 마치라고 명시했다. 개인 Google 발송에 한해 APR-02~04/INST-04의 네이티브·OS 사용자 존재 확인 조건을 Core의 불변 전체 preview와 명시적 터미널 선택으로 대체한다. 기준 requirements 원문은 수정하지 않는다. 이전 PERSONAL-SEND-CONFIRMATION 제안의 네이티브 개인 창도 대체한다.

prepare는 창/발송 없음, confirm은 owner/epoch/account/review/hash/범위/만료 결속, cancel/EOF는 confirm 없음, unknown 무재전송, 정책·감사·outbox 단일 claim을 수용 기준으로 제안한다. TTY는 사람/AI를 구별하는 보안 경계가 아니며 같은 사용자 권한의 자동화에 대한 보장은 약해진다. 사용자 UX 결정은 사람 보안 검토를 대신하지 않는다. ADR-029 및 terminal-personal-send 계약을 따른다.
