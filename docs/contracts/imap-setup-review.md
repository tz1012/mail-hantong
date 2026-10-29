# 새 IMAP 계정 설정 확인 v1

상태: 보호 입력·네이티브 설정 UI 연결 구현 기준, **사람 검토 필요**. 전체 설정 마법사·기존 계정 재연결·실제 로그인 검증 완료가 아니다.

`hantong-approval::AccountSetupReview`는 데이터다. 공개 생성자는 사람 확인 증거를 만들지 않는다. 불변 `NewImapAccountDraft`, `ImapEndpointConfig`, 32-byte request ID, stage payload hash, credential capture nonce, UTC created/expires 값을 받는다. 세 byte 배열은 각각 0이 아니어야 하며 endpoint는 version=1, app_password만 허용한다. created>0, 1..300,000ms lifetime을 checked subtraction으로 검사한다. 모든 필드는 private, read-only getter와 Clone/Eq만 제공하고 Debug는 redacted다. Secrets·Store·Windows를 의존하지 않는다.

request ID는 새 설정 의도의 임시 식별자다. DB에 존재하지 않는 setup request/version이나 duplicate snapshot을 있다고 가장하지 않는다. stage hash는 Core가 [계정 생성 계약](account-provisioning.md)의 함수로 계산하고, native binding은 review와 [capture plan](protected-mail-input.md)의 request/account/endpoint/hash/nonce가 정확히 같은지 검사한다. hash 자체를 권한 증거로 취급하지 않는다.

설정 확인의 표시값은 이름, 색상, 서버, 포트, TLS 방식, 로그인 ID, 앱 비밀번호 방식이다. 비밀번호는 summary·hash·review에 넣지 않는다. 표시명과 login에는 Unicode 방향 제어/줄 구분 문자, 역슬래시/따옴표를 명시적 escape하여 외부 값이 항목 경계를 바꾸지 못하게 한다. 일반 한국어/Unicode는 보존한다. 고정 라벨과 안내는 별도 영/한 리소스이며 누락·중복·빈 값·두 언어의 키 불일치를 빌드와 시험에서 거부한다. 테마는 기존 Presentation을 사용한다.

후속 native binding은 현재 OS user/logon, 임의 session nonce, 첫 UI capture UTC/Instant, 전체 review를 보유한다. 실제 화면 paint와 예상 summary를 확인하고 사용자가 선택한 뒤 PasswordInput을 소비한다. capture가 native EDIT를 비우고 파괴한 뒤 Hello를 시작한다. 시작 직전 owned root가 foreground인지 검사한다. 완료 때는 Hello 창이 foreground일 수 있으므로 root와 foreground의 재일치를 요구하지 않고 표시·대상·로그온·시각을 다시 검사한다. 해당 OS Verified 결과만 `VerifiedImapSetup`을 만들며 pending을 그 안에 함께 소유한다. CLI assertion, 클릭 상태, 조립 가능한 bool로 결과를 발급하지 않는다.

`consume(expected_review)`는 exact review·현재 logon·nonce·UTC/단조 만료를 다시 검사하고 원래 PendingImapCredential을 한 번 이동시킨다. 검토 취소/만료/불일치/OS 실패/비동기 future drop에는 pending을 폐기하며 Store·DPAPI를 바꾸지 않는다. 유효한 결과를 받은 Core만 stage와 비밀 저장/일회 probe를 연결한다. 기존 등록 UI와 동시 창 gate를 공유하며 OS Hello cleanup 한계는 [등록 확인 계약](registration-review.md)을 따른다.

검증은 순수 입력·동등성·표시·리소스 시험, nonce/대상/세션/시각 불일치, 취소/비정상 결과 폐기를 구분한다. 실제 사람의 Hello·접근성·보이는 창 수동 시험은 자동 검증과 별도로 남긴다.
