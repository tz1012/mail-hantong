# 대화형 Mac 격리 시험 준비 계획

범위: 읽기 전용 사전검사와 재현·중단·복구 절차를 구현한다. 실제 Keychain 생성·선택·잠금 해제, Secrets 시험 실행, OS 승인, ACL·trusted-app·partition-list·서명 변경은 이번 실행에 포함하지 않는다. 기존 CI 진입점과 제품 계약은 변경하지 않는다. Ponytail의 기존 코드 재사용·최소 변경 원칙을 적용한다.

1. 별도 `scripts/prepare_macos_interactive_compat.py`를 만든다. OS가 확인한 전용 비관리자 사용자·GUI 콘솔 소유자·실제 home·TTY·아키텍처를 확인한다. 환경 변수로 사용자나 CI 신분을 만들지 않는다.
2. 전용 home 아래 고정 격리 경로, 단일 합성 Keychain의 기본·검색 목록, private 소유·모드·symlink/hardlink/ACL 거부, 패키지 해시·source commit/clean 상태와 기존 제품 데이터·socket 부재를 읽기 전용으로 확인한다. 생성·수정·삭제·실행 선택지는 제공하지 않는다.
3. 결과는 준비 계획이며 실행 허가가 아니다. 원래 공식 바이너리 DB 키 시험과 별도 합성 IMAP/SMTP source 시험, Intel·ARM 각각의 정상·취소·실패·복구 절차를 문서화한다. 실제 단계 실행에는 새 대화형 controller의 별도 검토·구현이 필요함을 명시한다. CI 환경 변수 위조나 기존 CI main의 우회를 정상 경로로 제시하지 않는다.
4. OS·파일·Keychain 명령은 mock/fake로 격리한 단위시험에서 수락과 거부, race 감지, 비밀 없는 오류, 실행·권한 변경이 없음을 검사한다. 기존 CI 하네스 회귀와 문서 검사를 함께 실행한다.
5. 독립 코드 검토를 받고 사람이 검토해야 할 고정 커밋·파일·항목과 실제 두 Mac GUI 환경에서 남은 검증을 보고한다. 배포 보류를 유지한다.
