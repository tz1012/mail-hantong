# ADR-032: Mac 시험 전용 Keychain 읽기 진단

상태: 2026-11-06 사용자 승인 / 구현·검증 중 / 배포 보류.

사용자 확인 원문: “승인할게”. 직전 응답에서 명시한 이 ADR의 Mac 시험 전용 직접 의존성 추가와 별도 비대화 읽기 진단 범위에 대한 승인이다. 새 제품 권한, ACL 변경, 자동 OS 확인, 실제 사용자 비밀 사용 또는 새 변경의 사람 보안 검토 완료로 확대하지 않는다.

후속 사용자 설명으로 승인 근거를 재확인했다. 이미 lockfile에 있는 `security-framework 3.7.0`을 Mac 시험 전용 직접 의존성으로 추가해 격리 합성 자격증명의 오류 상태만 확인하는 범위다. 실제 사용자 Keychain, ACL/trusted-app/partition-list 변경, 바이너리 재서명 및 OS 확인 자동화는 승인되지 않았다. 읽기 거부를 호환성 성공으로 처리하지 않고 기존 읽기·값 보존·갱신·재시작 조건을 유지한다. 의존성 추가 승인과 별도 Secrets 사람 검토는 구분한다. 진행 중인 두 Mac 시험은 중복 시작하지 않는다.

## 2026-11-07 대화형 시험 준비 범위

사용자는 “이번에는 준비·구현과 권한을 바꾸지 않는 시험만 진행”하고 실제 OS 승인·권한 변경·자동 확인·ACL 완화를 하지 말라고 명시했다. 이에 따라 CI와 별개인 **읽기 전용** baseline/prepared/restore-check 도구와 mock 단위시험, [대화형 재현 절차](../../latest/MAC-INTERACTIVE-COMPAT-RUNBOOK-20261107.md)를 추가한다. 제품 계약·Rust·기존 CI 진입점·비밀 selector는 변경하지 않는다. 새 의존성도 없다.

이 준비 도구는 현재 OS의 전용 표준 사용자·home·console/GUI/TTY·경로·금고 선택·패키지/소스를 조회하고 실행 계획만 만든다. 전환 전 baseline을 누락하면 prepared를 거부한다. synthetic-only provenance에는 별도 운영자 선언도 필요하며 파일명·사용자명만으로 합성 내용을 증명했다고 표시하지 않는다. restore-check는 선택 목록 비교일 뿐 자식 프로세스 종료 확인이나 cleanup 허가가 아니다.

후속 실제 executor가 Keychain 생성·unlock·default/search 전환·복원·삭제 및 제품/시험 실행을 하려면 **운용 범위 변경안과 별도 사람 검토·실행 승인**이 필요하다. 현재는 그러한 선택지나 우회 호출을 구현·실행하지 않는다. 실제 사용자 금고·계정·메일은 사용하지 않는다. 이 준비 승인도 기존 Secrets 사람 검토, 실제 두 Mac 호환성 시험 또는 배포 조건을 대체하지 않는다.

## 문제

공식 구버전→후보 초기 메뉴와 합성 자격증명 교차 버전 시험이 실패했다. 단계 마커와 process sample은 Keychain/Security 읽기 대기를 좁히지만 정확한 OSStatus나 UI 필요 여부를 확정하지 못한다. 현재 선택된 legacy backend는 이 API를 노출하지 않는다. 실패를 잠금·비밀번호·ACL 중 하나로 임의 분류하지 않는다.

## 선택지

| 선택지 | 영향 | 판단 |
| --- | --- | --- |
| 기존 마커·sample만 유지 | 새 API 없음. OSStatus·대화 필요 여부가 계속 미확정 | 현재 적용 범위 |
| Mac 시험 전용 직접 `security-framework = "=3.7.0"` 및 별도 읽기 probe | 이미 잠긴 전이 버전을 안전한 Rust API로 호출. manifest와 시험 보안 경계 변경 | 승인 요청안 |
| 시험 모듈에서 Security C API 직접 호출 | 새 Cargo 의존성은 없지만 새 unsafe FFI와 수명·복원 검증 필요 | 우선 적용하지 않음 |

protected store 전환, ACL/trusted-app/partition-list 변경, 기존 바이너리 재서명, 자동 OS 확인은 선택지에 포함하지 않는다.

## 제안 범위와 승인 조건

직접 의존성은 Mac 시험에만 추가한다. probe는 새 프로세스에서 UI를 허용하지 않는 guard와 기존 격리 합성 항목의 읽기만 수행하고, OSStatus를 고정 범주로만 기록한다. 원래 호환성 시험의 읽기·값 보존·갱신·재시작 조건 및 실패 판정은 유지한다. 이 probe가 실패를 성공으로 대체하지 않는다. 실제 사용자 데이터·Keychain·항목 ACL·제품 저장/인증 인터페이스는 변경하지 않는다.

세부 안전 조건과 근거: [Mac 비대화 진단 제안](../../latest/MAC-KEYCHAIN-NOUI-DIAGNOSTIC-PROPOSAL-20261106.md).

AGENTS.md §7의 라이브러리 선택 및 새 의존성 추가 승인을 위 원문으로 기록한다. §6의 Secrets 사람 검토는 배포 전 별도 유지한다. 대화형 OS 확인이 필요하면 격리 Mac의 사람이 수행해야 하며, hosted CI에 현재 그 화면을 관찰·응답할 경로는 없다. 정상 승인·접근 제어를 우회하지 않는다.
