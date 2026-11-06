# 대화형 Mac 격리 시험 준비 계획

범위: 읽기 전용 사전검사를 유지하고 별도 격리 실행 컨트롤러와 fake 안전성 시험을 구현한다. 이번에는 실제 Mac·Keychain·Secrets 실행, OS 승인, 권한 변경·배포를 하지 않는다. 기존 CI 진입점과 제품 계약은 변경하지 않는다. Ponytail의 기존 코드 재사용·최소 변경 원칙을 적용한다.

1. 별도 `scripts/prepare_macos_interactive_compat.py`를 만든다. OS가 확인한 전용 비관리자 사용자·GUI 콘솔 소유자·실제 home·TTY·아키텍처를 확인한다. 환경 변수로 사용자나 CI 신분을 만들지 않는다.
2. 전용 home 아래 고정 격리 경로, 단일 합성 Keychain의 기본·검색 목록, private 소유·모드·symlink/hardlink/ACL 거부, 패키지 해시·source commit/clean 상태와 기존 제품 데이터·runtime 부재를 확인한다. baseline/prepared/restore-check의 읽기 전용 의미는 바꾸지 않는다.
3. 별도 빌드 helper는 폐기 가능한 clone에만 기존 합성 fixture를 주입한다. old/candidate의 정확한 시험 실행 파일을 빌드한 뒤 원본 소스 바이트·clean 상태를 복구하고 실행 파일의 해시·identity·아키텍처를 결속한다. 복구 실패 시 실행하지 않는다.
4. 별도 controller의 start/cancel/finalize와 private 상태·잠금·취소 표식을 구현한다. 공식 패키지의 검증한 Core를 foreground로 old1회·candidate2회 실행해 상태·암호화 DB 헤더·종료를 확인한다. 이후 prebuilt fixture의 seed/upgrade/reopen을 실행한다. 설치 스크립트·PATH 변경·daemon 자동 시작·임의 PID 종료·자동 재시도는 없다.
5. 모든 종료 경로에서 자신이 생성한 프로세스만 회수하고 시험 자료를 보존한다. 외부에서 사람이 정상 복원한 baseline을 읽어 finalize가 정리 적격 여부만 기록하며 실제 삭제하지 않는다. 같은 run-id를 재개하지 않는다.
6. fake 단위시험으로 순서·단일 사용·이미지/해시/아키텍처 거부·소스 복구·취소/시간 초과·종료 실패·원상 복구 거부·비밀 비노출·삭제 없음과 기존 회귀를 검증한다. 실제 Mac 실행 결과로 표시하지 않는다.
7. 독립 코드 검토 후 정확한 전체 커밋·파일·OS 요청 대상·최소 사람 조작과 시작/중단/finalize 명령을 보고한다. 별도 Secrets 사람 검토와 실제 Intel·ARM GUI 시험을 충족하기 전 실행·배포를 보류한다.
