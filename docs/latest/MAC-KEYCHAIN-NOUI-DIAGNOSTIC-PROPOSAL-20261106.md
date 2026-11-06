# Mac Keychain 무대화 진단 변경 — 승인·구현 중

2026-11-06 사용자 원문 “승인할게”로 ADR-032의 시험 전용 범위를 승인했다. 이 기록은 새 변경의 사람 보안 검토나 호환성 시험 성공을 대신하지 않는다.

이 제안은 호환성 실패를 성공으로 바꾸거나 접근 권한을 부여하는 방안이 아니다. 현재 단계·sample 진단으로도 원인을 구분하지 못할 경우의 다음 읽기 전용 진단 범위를 정한다. 기존 실패 gate는 그대로 실패로 남긴다.

## 현재 한계

선택된 `apple-native-keyring-store = 1.0.2`의 legacy Keychain backend에는 UI 대기 금지 guard와 안정적인 OSStatus 접근 API가 없다. `protected` backend의 UI 옵션은 저장 방식·권한 조건이 다르므로 전환하지 않는다. `security-framework = 3.7.0`은 lockfile에 전이 의존성으로 존재하지만 Secrets에서 직접 호출할 수 있는 승인된 직접 의존성은 아니다. 임의 backend 오류 문자열을 출력하지 않는다.

일반 전체 검사와 새 합성 항목의 읽기 성공은 기존 항목을 새 실행 이미지가 읽을 수 있다는 증거가 아니다. 공식 beta29 ARM64 아카이브의 실행 파일은 offline 구조 검사에서 ad-hoc CodeDirectory만 있었고, Intel에는 LC_CODE_SIGNATURE가 없었다. 이는 실제 macOS codesign 검증이나 현재 후보의 designated requirement 검증을 대신하지 않으며 timeout 원인을 확정하지 않는다.

## 제안 범위

- Mac 시험 전용 직접 의존성 `security-framework = "=3.7.0"` 추가. 제품 실행 경로·Unix 인증·Keychain 저장 형식은 변경하지 않는다.
- 별도의 새 Secrets 시험 프로세스에서만 legacy `SecKeychainSetUserInteractionAllowed(false)`의 RAII guard와 격리된 합성 항목 읽기를 수행한다. 별도 probe fixture는 후보에만 주입하고 기존 구버전 fixture·공식 구버전 바이너리를 수정하지 않는다.
- OSStatus는 고정된 진단 범주로만 매핑한다. `interaction_not_allowed`는 UI가 필요하다는 분류이며, 잠금 또는 항목 신뢰 규칙 중 어느 것인지 단독으로 확정하지 않는다. 경로·selector·계정·값·OS 오류 문자열은 출력하지 않는다.
- exact CI Keychain default/search baseline과 합성 root를 전후 확인한다. 값은 Secrets 경계 안에서만 읽고 비교하며 기록하지 않는다. 새 프로세스 종료 시 interaction 상태가 다른 프로세스에 남지 않아야 한다.
- 항목 ACL·소유자·trusted-app·partition-list·서명·기존 저장 값을 변경하지 않는다. set/delete/update 또는 자동 확인 API를 probe에 포함하지 않는다.
- 기존45초/900초 검증, 실제 값 보존 assertion, 실패 판정과 배포 보류 조건을 유지한다. probe 결과만으로 호환성 gate를 통과 처리하지 않는다.

## 승인과 검증

AGENTS.md의 새 의존성 승인 규칙 및 §6 Secrets 사람 검토 범위에 해당한다. 위 승인에 따라 시험 전용 manifest·진단을 구현하고 고정 분류·비밀 비노출·비대화 종료·정확한 합성 항목 결속 회귀를 Mac Intel/ARM의 격리 환경에서 검증한다. Windows 저장소·키·인증 형식은 변경하지 않는다.

실제 OS 확인이 필요하다고 판명되면, 대화형 격리 Mac에서 비관리자 시험 사용자와 합성 Keychain만으로 관찰해야 한다. GitHub hosted runner에는 현재 그 화면을 관찰·응답할 경로가 없다. 사람의 일반 Keychain 허용 선택을 자동 실행하거나 Always Allow·자동 ACL 수정을 대체 수단으로 사용하지 않는다. 안정적인 공식 서명 identity를 도입하려면 별도 배포·서명 설계와 정당한 서명 환경이 필요하며 계정 생성·인증서 발급은 수행하지 않는다.

## 근거

- [Apple TN3137: On Mac keychains](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains)
- [Apple Keychain access control lists](https://developer.apple.com/documentation/security/access-control-lists)
- [Apple Code Signing Guide](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/AboutCS/AboutCS.html)
- [Apple Keychains API](https://developer.apple.com/documentation/security/keychains)
