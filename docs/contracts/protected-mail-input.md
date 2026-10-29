# 보호된 IMAP 앱 비밀번호 입력 v1

상태: M2-01·INST-05·ACC-03의 네이티브 설정 선행 경계. 연속 개발 위임에 따른 구현 기준, **사람 검토 필요**. 실제 계정 입력·Hello·로그인을 개발 중 자동 실행하지 않는다.

## 소유권과 수명

Core 전용 `hantong-approval-windows`가 설정 창을 소유하고 Secrets가 그 안의 표준 password EDIT를 생성·읽기·정리한다. parent HWND는 배치를 위한 OS 핸들이며 권한 증거가 아니다. HWND를 감싼 공개 생성자로 사람 확인 capability를 가장하지 않는다. CLI/MCP는 이 모듈을 의존하거나 비밀번호 인자·stdin·환경 변수를 제공하지 않는다.

수명은 `ImapPasswordCapturePlan → PasswordInput → PendingImapCredential`이다. plan/pending에는 공개 생성 필드·Clone·Serialize·Read·Deref·bytes getter가 없으며 Debug는 모두 가린다. plan과 pending은 전용 UI thread와 Core 사이를 이동할 수 있는 Send이고 Sync는 아니다. PasswordInput은 생성 UI thread에 고정되어 Send/Sync가 아니다. 입력값의 평문은 Secrets 안에서만 읽는다.

`ImapPasswordCapturePlan::new(account_id, endpoint, setup_request_id, setup_payload_hash)`는 유효한 account ID, 불변 IMAP endpoint, app_password 인증, 0이 아닌 두 32-byte 식별자를 받는다. Secrets CNG RNG가 0이 아닌 별도 32-byte capture nonce를 생성한다. ID/endpoint/request/hash/nonce의 읽기 전용 getter만 공개한다. payload hash는 [새 계정 저장 전이](account-provisioning.md)의 stage hash이며 Core가 계산한다. 입력 데이터나 hash만으로 승인되지 않는다.

plan 생성부터 capture까지 5분 단조 시계 제한, capture부터 저장까지 별도 5분 제한을 적용한다. 다음 사람 확인 review의 UTC·단조 시계 만료는 추가로 검사하며 이 입력 제한을 대체하지 않는다. 프로세스 재시작으로 plan/pending을 복구하지 않는다. 앱 비밀번호를 입력한 뒤 중단되면 다시 입력한다.

## 네이티브 EDIT

`plan.attach_to_parent(parent_hwnd: isize, control_id: u16)`는 같은 프로세스·현재 UI thread의 살아 있는 top-level root만 받는다. child style·0/중복 control ID를 거부한다. 생성 후 exact parent, root/root-owner, PID/thread를 다시 확인한다. `WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL` 단일 행 EDIT를 Secrets가 직접 만든다. 입력 제한 1,024 UTF-16 code units를 설정·조회하며 최종 UTF-8은 기존 app_password와 동일하게 1..1,024 bytes다. 사용자 값은 trim/정규화하지 않고 제어 문자·잘못된 surrogate를 거부한다.

공개 메서드는 배치, font 적용, focus, enable, child 식별 비교, cancel, 소비형 capture다. raw child HWND와 텍스트 설정/읽기 메서드는 없다. `set_font`의 HWND와 별도 HFONT는 빌린 OS 표시 리소스이며 호출자가 사용 기간 동안 유지한다. 글꼴은 비밀이나 권한이 아니다.

Secrets-owned subclass의 고정 수명 state는 `WM_NCDESTROY`에서 child가 죽었음을 기록한다. wrapper가 살아 있어도 이미 파괴된 숫자 HWND를 다시 접근/파괴하지 않는다. 모든 동작은 원래 thread·수명·class/style·parent/root·subclass identity를 재검사한다. subclass는 Copy/Cut/context menu를 억제하고 사용자의 Paste는 허용한다. 이를 같은 사용자 권한의 악성 코드 방어로 설명하지 않는다.

`capture(self)`는 새 target 인수를 받지 않는다. 원래 plan target을 유지하며 길이 한도를 검사하고 Secrets-owned UTF-16 buffer로 WM_GETTEXT를 읽는다. 길이·NUL·재조회 결과가 일치해야 한다. 일반 String이나 growable plaintext allocation을 만들지 않고 고정 크기 wiped UTF-8 buffer로 변환한다. stack encoding scratch도 지운다. input 성공/실패/cancel/Drop에서 텍스트 비우기, undo 비우기, child 파괴를 시도한다. 정리 실패에는 pending을 반환하지 않는다. 이미 NCDESTROY된 child에는 OS 호출하지 않는다.

UTF-16·UTF-8·내부 복사본은 소유 buffer Drop에서 volatile wipe한다. Windows EDIT 내부 할당, IME/clipboard 원본, CPU 임시값까지 secure erase한다고 주장하지 않는다. `ES_PASSWORD`는 표시 마스킹이며 OS의 secure desktop이 아니다. 표시·IME·접근성은 실제 Windows에서 후속 수동 검증이 필요하다.

## 저장과 확인 결속

`SecretStore::store_pending_imap_credential(pending, expected_account, expected_endpoint)`는 pending을 소비하며 exact target과 만료를 먼저 검증한다. mismatch/만료 시 기존 암호문을 변경하지 않는다. 기존 endpoint/version-bound DPAPI 저장 함수를 재사용하며 reader 기반 저장 API를 pending의 공개 Read 구현으로 연결하지 않는다. 평문은 Secrets 내부 helper로만 전달한다.

`SecretError::InvalidInputWindow`는 잘못된 native 입력 수명/소유권, `SecretError::InputExpired`는 입력 수명 만료다. 기존 InvalidAccountId/InvalidCredential/CredentialMismatch/UnsupportedPlatform/Protection/Io를 유지한다. 오류 메시지에 target·비밀번호·Windows 원문을 넣지 않는다. 외부 공통 오류는 후속 Core가 각각 INVALID_INPUT/APPROVAL_EXPIRED/기존 안정 코드로 매핑한다.

후속 네이티브 확인 결과는 pending을 직접 소유하는 `VerifiedImapSetup`이어야 한다. review는 draft/endpoint/request/payload hash/capture nonce/UTC 만료를 결속한다. Hello 시작 전에 capture가 EDIT를 비우고 파괴한다. OS Verified와 현재 로그온·표시 snapshot·단조 시계를 검증한 경로만 결과를 만들고, Core는 expected review로 한 번 소비한다. `(proof, unrelated_pending)`을 따로 조립하는 API는 만들지 않는다.

정상 제품의 Core coordinator는 신규 계정을 **사람 확인 소비 → Store stage → endpoint-bound credential 저장 → 일회 probe → Store 첫 결과 CAS** 순서로 처리한다. stage 전에는 accounts 행을 만들지 않는다. duplicate snapshot/override bool은 이번 신규 전용 경로에 없고 Store가 동일 host/login을 거부한다. 실패 재개·기존 계정 재설정은 별도 계약이다. credential 준비/입력 자체를 Core의 설정 저장이나 서버 접속 승인으로 해석하지 않는다. Secrets/Store의 데이터 primitive는 발급 API가 아니며, 이 순서는 Core 통합 시험과 CLI/MCP의 전이적 접근 금지 검사로 검증한다. Secrets가 승인 UI를 역의존하는 구조는 만들지 않는다.

## 검사와 근거

순수 target/만료/UTF 변환 시험과 Secrets 내부 합성값을 쓰는 숨겨진 Win32 fixture로 소유 thread, style/limit, teardown/NCDESTROY/handle 재사용, invalid input, DPAPI 저장 결속, cancel/drop을 검증한다. 실제 사용자 창의 값을 읽거나 실제 계정으로 접속하지 않는다. 비 Windows는 보호 입력을 제공하지 않으며 UnsupportedPlatform이다. 숨겨진 합성 fixture 검사는 사용자 시각·키보드·IME·Hello 시험을 대신하지 않는다.

기존 windows-sys 0.61.2의 UI/Controls/Shell/Threading 기능만 추가하며 새 외부 package/version은 도입하지 않는다. native 승인 UI 연결 시 approval-windows→Secrets 단방향 의존과 아키텍처 검사를 함께 갱신한다.

`GetWindowSubclass`는 Common-Controls v6 manifest가 필요하다. MSVC native UI 제품과 테스트 harness에 같은 manifest를 embed한다. 실제 숨김 fixture 실행에서 기본 v5 DLL의 export 누락으로 STATUS_ENTRYPOINT_NOT_FOUND를 재현했고 v6 manifest로 수정했다. manifest도 추적 source hash에 포함한다. `EnableWindow`/`SetFocus`의 반환값은 각각 이전 disabled/focus 상태이므로 성공 bool로 해석하지 않고 변경 후 상태를 재조회한다. [v6 요구](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-getwindowsubclass), [manifest 링크](https://learn.microsoft.com/en-us/cpp/build/reference/manifestinput-specify-manifest-input?view=msvc-170), [EnableWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enablewindow), [SetFocus](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setfocus).

공식 근거: [창 소유 thread/PID](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid), [IsWindow 수명 주의](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindow), [password EDIT](https://learn.microsoft.com/en-us/windows/win32/controls/edit-control-styles), [입력 한도](https://learn.microsoft.com/en-us/windows/win32/controls/em-setlimittext), [WM_GETTEXT](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-gettext), [undo 정리](https://learn.microsoft.com/en-us/windows/win32/controls/em-emptyundobuffer), [SetWindowSubclass](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-setwindowsubclass). 표준 컨트롤의 표시·메모리 정리 한계는 이 API 범위로 제한한다.
