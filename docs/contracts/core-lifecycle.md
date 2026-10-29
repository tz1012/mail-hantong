# 자동 시작과 수명 계약

상태: M1-11 구현 기준, 연속 개발 위임. 사람 보안 검토 필요.

## AutoStart

`AutoStart::state()`는 `Result<AutoStartState, PlatformError>`다. Disabled/Enabled/OwnershipConflict를 구분하며 OS 읽기 오류를 Disabled로 바꾸지 않는다. 미지원 OS는 Unsupported다. `set_enabled`는 고정 제품 값만 생성/삭제한다. 다른 타입·데이터·경로/인자가 있으면 Denied를 반환하고 값을 보존한다. 이미 원하는 상태면 아무 변경 없이 성공한다.

Windows value는 HKCU `Software\Microsoft\Windows\CurrentVersion\Run`의 `mail-hantong` REG_SZ다. 명령은 `"<current executable absolute path>" --supervise`이며 basename은 대소문자 무관 mail-hantong-core.exe다. 경로는 current_exe에서 구하며 config/wire가 주입하지 않는다. UNC·상대·NUL·제어 문자·따옴표·환경 확장 경로를 거부한다. command는 최대 260 UTF-16 code units(종료 NUL 제외)다. 파일을 shell로 실행하지 않는다. 경로 검증과 설치 provenance는 별개이며 installer/settings 외 경로에는 adapter를 연결하지 않는다.

읽기는 타입과 최대 522 bytes(260 UTF-16 + NUL)를 검사하고 실제 저장 데이터가 예상 REG_SZ와 정확히 같아야 Enabled다. 잘못된 UTF-16·내부 NUL·누락된 종료 NUL·초과 길이는 OwnershipConflict다. disable은 자신의 정확한 값을 재확인한 뒤 그 값 하나만 삭제하며 Run key나 다른 앱 값은 삭제하지 않는다. write 후 readback으로 확인한다. Windows registry 자체의 외부 프로세스 변경까지 원자 CAS를 제공한다고 주장하지 않는다. 같은 사용자 악성코드는 기존 위협 모델의 잔여 위험이다.

native 시험은 제품 Run key 대신 임시 HKCU `Software\mail-hantong\tests\autostart-<unique>`를 사용하는 private cfg(test) seam에서 수행한다. test key와 값은 그 시험이 만든 것만 정리한다. 제품 생성자는 key/value 이름을 외부에서 받지 않는다.

## RestartBudget

입력은 자식 실행 시간 Duration과 종료 이유 Clean, RequestedStop, Maintenance, AlreadyRunning, Abnormal이다. 출력은 Stop, RestartAfter(Duration), PauseAndNotify다. Abnormal만 예산을 소비한다. 처음 5회의 비정상 종료에 각각 1/2/4/8/16초 지연하고 여섯 번째에 PauseAndNotify로 잠근다. 최소 300초 연속 실행 뒤 비정상 종료는 예산을 초기화하고 1초부터 시작한다. 중지/정상/유지보수/충돌 또는 소진 후 같은 객체는 재시작하지 않는다. 생성 시 실제 OS·시계·sleep·프로세스 실행은 하지 않는다. 호출자가 단조 시계로 관측한 실행 시간을 전달한다.

이 상태는 supervisor 수명 안에서만 보장된다. 예산 영속화, 사용자 알림 전달, child 실행 및 유지보수 안전 종료의 제품 연결 전에는 카드 전체 완료로 표시하지 않는다. 반복 실패를 자동 메일 재발송의 근거로 사용하지 않는다.
