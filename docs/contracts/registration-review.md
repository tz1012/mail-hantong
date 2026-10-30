# CLI 등록 검토 화면과 사람 확인 v1

GUI 확장은 [GUI 등록 계약](gui-client-registration.md)을 따른다. 기존 CLI canonical v1은 유지하며 GUI 검토는 별도 domain으로 kind/profile/세 로컬 읽기 도구를 결속한다. GUI용 제목·설명·Hello 안내를 영한 리소스로 표시하고 기존 opaque proof 발급·소비 경계를 유지한다.

상태: 2026-10-30 구현 기준, CLI-07·INST-04·APR-02·OPS-07·OPS-08. 사람 보안 검토 필요. 초기 대상은 `default` CLI의 read-only `mail.accounts.list` 권한이며 일반 발송 승인을 대신하지 않는다.

## 불변 검토 자료

코어는 영속 pending 요청에서 `RegistrationReview`를 만든다. 포함 값은 request_id(32 bytes, 0 금지), request_version(1..i64::MAX), 정확한 account_ids(최대 128, 기존 RequestedScope 형식), server_public_key_hash(32 bytes, 0 금지), created_at_utc_ms와 expires_at_utc_ms(생성 이후 최대 5분)다. 종류 CLI, 선택자 default, 도구 mail.accounts.list, read_only=true, 분당 60회는 고정이다. 빈 계정은 0개 권한이며 이후 추가 계정으로 자동 확장하지 않는다.

필드는 private이고 생성 때 범위/중복/시각을 검증하며 계정은 기존 scope 규칙대로 정렬한다. 외부 표시명·HTML·명령·메일 내용은 검토 자료로 받지 않는다. Debug는 ID·hash·scope를 숨긴다. canonical bytes는 `mail-hantong/registration-review/v1\0`, request ID, big-endian u64 version, server hash, big-endian i64 생성/만료, big-endian u32 계정 개수와 각 UTF-8 ID의 u32 길이/bytes 순이다. 고정 종류/프로필/권한의 의미는 domain separator v1에 결속된다. 이 자료나 hash는 사람 확인 증거가 아니다.

## 순수 UI 상태

`hantong-approval`은 검토 자료·언어/테마·화면 상태를 다룬다. 실제 Windows Hello 결과를 받거나 승인/활성 등록을 발급하지 않는다. `ReviewState`는 AwaitingPaint, Reviewing, Verifying, Closed, Expired, Unavailable이다. 생성 직후는 AwaitingPaint이며 같은 검토 자료의 첫 표시 완료 후에만 Reviewing이 된다.

`begin_verification(now_utc_ms, elapsed)`는 Reviewing이고 UTC가 생성 이상/만료 미만이며 단조 elapsed가 5분 미만인 경우에만 Verifying으로 바꾸고 `RequestVerification` 의도를 반환한다. 이는 OS 검증을 시작하라는 UI 의도일 뿐 확인 증거가 아니다. 중복 클릭/Enter는 새 요청을 만들지 않는다. 언어/테마 선택은 snapshot, 만료, UI 세션을 바꾸지 않는다. 시계 역행/만료는 실패한다. close/cancel은 Closed이며 explicit deny도 Closed + Deny 의도만 반환한다. 외부 bool/Verified/serialized assertion을 입력받아 approved로 바꾸는 reducer API는 없다.

`Presentation`은 domain의 Language(en/ko), Theme(light/dark/system)를 사용한다. GUI 언어 미지정 시 OS 한국어 여부로 초기값을 정하고 theme 기본은 System이다. System만 OS dark 변화에 따라 바뀌며 명시적 Light/Dark는 유지한다. 고대비가 활성화되면 OS 고대비 색이 우선하고 저장된 선택은 유지한다. 첫 paint 전에 언어/테마를 결정한다. 영/한 모든 resource key가 대응해야 하며 화면 문구를 Rust에 하드코딩하지 않는다.

## Windows 발급 경계

발급은 Core만 의존할 수 있는 별도 `hantong-approval-windows`에 둔다. 일반 platform/CLI/MCP 또는 순수 approval 모델에는 발급 함수를 넣지 않는다. 이 crate는 자체 STA thread에서 HWND를 생성하고 메시지 루프를 유지한다. 한 번에 등록 창은 하나만 연다. 첫 실제 paint 뒤 검토 버튼을 활성화하며 CLI/IPC가 Verified 결과·HWND·임의 callback을 주입할 수 없다.

검토 버튼은 해당 소유 HWND로 `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`를 호출한다. OS 완료 handler는 프로세스 안의 해당 호출 전용 채널로만 결과를 전달한다. UI가 캡처한 request/version/snapshot, UI session/OS nonce, 현재 user/logon context, UTC/단조 만료를 다시 확인한 경우에만 `VerifiedRegistration`을 만든다. 이 타입의 필드와 생성자는 private이고 Clone/Copy/Serialize/bool 변환은 없다. Core는 기대 snapshot과 현재 pending Store 상태를 재검증하며 token을 한 번 소비한다. 순수 unit test의 합성 이벤트는 제품 발급 경로에 연결하지 않는다.

창 닫기·취소·Hello 실패·timeout·메시지 루프 오류·Core shutdown 때 operation을 Cancel/Close하고 발급 0건으로 종료한다. 지연 callback은 종료된 전용 채널/세션으로 격리한다. 가용성 정보만으로 승인하지 않는다. 실제 Hello 호출은 사용자가 설치한 앱의 검토 버튼을 누를 때만 실행하며 개발 자동 시험은 이를 실행하지 않는다.

Cancel은 취소 요청이며 즉시 완료가 아니다. Started operation에 곧바로 Close를 호출하지 않는다. terminal completion handler가 결과를 소비한 뒤 Close하고, 창이 이미 닫혔으면 결과를 폐기한다. handler는 raw HWND/State pointer를 보관하지 않는다. 명시적 Deny는 ApprovalDenied, 창 닫기/OS 취소/호출 future 폐기는 ApprovalRequired, 만료는 ApprovalExpired, 장치/설정/정책 미준비는 HumanStepRequired, API 오류는 InternalError로 구분한다. Core는 명시적 Deny만 영속 denied 결정으로 기록한다.

Store provisioning/프로필 원자 발행/active CAS는 별도 계약과 Core 조립 단계다. UI token 발급만으로 등록 또는 메일 발송 완료라고 표시하지 않는다. 실제 HWND/Hello·스크린리더·다른 Windows 로그온의 검증은 사람 검토 게이트로 남긴다.

OS 호출 직전 소유 review HWND가 foreground인지 확인한다. 완료 시에는 Hello 자체가 foreground일 수 있으므로 같은 조건을 강요하지 않는다. 결속 불일치는 ScopeDenied이며 explicit Deny와 구분한다. 창 크기 변경은 State mutable borrow가 끝난 뒤 자식 redraw를 요청하고, 고대비 상태가 유지된 채 색상만 바뀌어도 OS palette를 다시 읽는다.

완료 handler 등록 실패에서도 Started operation을 유지하고 Cancel 후 기존 STA가 최대 5초 동안 메시지 pump/terminal 상태 확인을 수행한다. Close는 terminal에서만, callback과 cleanup 사이 한 번만 시도한다. OS가 기한 내 완료하지 않는 경우 Started에 Close하지 않고 참조를 해제한다. 이미 등록된 callback은 뒤늦은 terminal을 정리할 수 있으나 handler 등록 자체가 실패한 경우 기한 이후 terminal Close를 보장하지 못한다. 이 OS 오류 경로의 자원 정리 한계는 실제 Hello 수명 검증 전까지 남으며 어떤 경우에도 승인 결과로 전환하지 않는다.
