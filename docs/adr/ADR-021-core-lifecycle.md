# ADR-021 사용자 자동 시작과 코어 수명

상태: 2026-10-30 연속 개발 위임에 따른 구현 결정. 보안·설치 경로 사람 검토 필요.

Windows 자동 시작은 HKCU의 고정 Run 값으로 동일 제품 supervisor를 시작한다. supervisor는 현재 실행 파일의 고정 `run` 자식만 관리한다. 임의 shell/명령/환경 변수 확장 기능은 제공하지 않는다. Task Scheduler COM 의존이나 Restart API는 이번 기반에 추가하지 않는다. RegisterApplicationRestart는 crash/hang 후 사용자 선택과 60초 실행 조건이 있어 무인 코어 재시작 계약에 맞지 않는다.

Run command는 `"<absolute mail-hantong-core.exe path>" --supervise` 하나다. 값 이름은 `mail-hantong`. 현재 실행 파일의 경로·파일명·UTF-16 길이를 검증하고, 현재 경로와 정확히 같은 명령만 제품 소유로 취급한다. 다른 값은 수정/삭제하지 않는다. 설치 위치 변경은 설치기의 명시적 이전 작업에서 처리한다. 개발 검사에서는 실제 Run 값을 만들지 않고 별도 임시 HKCU test key만 사용한다. 일반 코어 기동·help/version은 시작 등록을 변경하지 않는다.

재시작 정책은 같은 supervisor 수명 동안 1/2/4/8/16초 지연으로 최대 5회다. 안정 실행 5분 뒤 다음 비정상 종료는 새 예산을 사용한다. 정상 종료, 명시 중지, update maintenance, 기존 코어 충돌은 재시작하지 않는다. 실패 예산 소진은 고정 `core.restart_paused` 알림 상태로 남긴다. 실행 시간은 단조 시계의 Duration으로 측정해 벽시계 변경에 영향받지 않도록 한다. supervisor 재시작을 넘는 예산 영속화와 실제 알림 전달은 후속 연결 전 미지원으로 명시한다.

업데이트는 새 효과의 접수를 닫고 진행 중 작업의 안전 지점과 영속 기록을 확인한 뒤 종료한다. 제한 시간 내 안전 지점에 이르지 못하면 업데이트를 연기하며 강제 종료하지 않는다. 발송 복구·중복 방지는 M4 Outbox 원장이 담당한다. 기반 재시작 상태 시험만으로 INST-13/SYNC-08 완제품 수용 기준을 충족했다고 표시하지 않는다.

자동 시작 설정·registry·DB CAS의 조정은 설정 UI/설치기에 연결할 때 별도 트랜잭션 경계를 정한다. 현재 플랫폼 adapter와 순수 restart budget은 사용자의 실제 로그인 항목을 변경하는 경로에 아직 연결하지 않는다. supervisor 자식 실행·유지보수 IPC·알림도 후속 구현이다.

근거: [Microsoft Run keys](https://learn.microsoft.com/windows/win32/setupapi/run-and-runonce-registry-keys), [RegisterApplicationRestart](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-registerapplicationrestart), [Restart Manager guidance](https://learn.microsoft.com/en-au/windows/win32/rstmgr/guidelines-for-applications).
