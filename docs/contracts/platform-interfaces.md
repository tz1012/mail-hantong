# 플랫폼 인터페이스 v1

상태: M0-10 구현 기준, 연속 개발 위임. **사람 검토 필요**.

공통 인터페이스는 OS를 참조하지 않는 contracts module에 둔다. 구현은 Windows 제품과 테스트 지원 package로 분리한다. Linux 개발용 구현은 `hantong-test-support` 안의 메모리 기반 가짜이며 제품 dependency로 연결할 수 없다. 제품에서 지원되지 않는 OS 보호 기능을 가짜 성공이나 평문 파일로 대신하지 않는다.

- `SecretStore`: 입력 reader를 비밀 module 안에서 읽고 불투명 handle을 반환한다. 존재 확인·내부 검증·삭제만 공개한다. 평문 반환/비밀 Debug/Serialize는 없다. 실제 Windows 저장소는 별도 Secrets 구현을 사용한다.
- `IpcTransport`: 길이 제한된 frame 전송/수신 인터페이스. 인증된 세션 타입을 만들거나 인자로 받지 않는다. transport 성공은 인증 성공이 아니며 frame/auth/policy 검증은 코어가 수행한다.
- `Notifier`: static message key와 opaque object reference만 전달한다. 메일 원문·주소·토큰을 기본 알림 문자열로 넘기지 않는다.
- `UserVerifier`: 요청 ID·계정·작업 해시·nonce·만료에 결속할 확인 자료를 받는다. 반환은 **검증되지 않은 evidence**이며 서버가 신뢰 채널·서명·재사용을 검증하기 전 승인 권한이 아니다. bool 하나를 반환해 발송을 허용하는 인터페이스를 금지한다.
- `AutoStart`: 설치된 코어의 시작 등록/해제/상태. 상태 조회는 Result로 오류와 Disabled를 구분하며 기존 외부 값은 OwnershipConflict로 보존한다. [수명 계약](core-lifecycle.md)을 따른다. 임의 셸 문자열·추가 인자를 실행하는 일반 도구를 노출하지 않는다.

테스트 구현은 시나리오·호출 기록·거부 결과를 제공하되 실제 OS/메일/외부 서비스에 접근하지 않는다. 제품 manifest의 normal/build dependency에 test-support가 섞이면 빌드 검사가 실패한다. OS 보호 경로의 사람 검토 상태는 이 가짜 시험과 별도로 관리한다.
