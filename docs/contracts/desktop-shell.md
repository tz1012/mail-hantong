# 데스크톱 표시와 로컬 메일 읽기 경계

M8-01/OPS-07/OPS-08/OPS-02, SEC-03 구현 계약. **사람 검토 필요**. 본문·첨부·메일 쓰기·동기화·승인 발급은 포함하지 않는다.

- `hantong_desktop::run(options, probe)`는 readiness만 표시한다. `run_with_callbacks(options, probe, ShellCallbacks)`는 아래 명시적 사용자 동작을 연결한다. Desktop crate는 Contracts에만 의존하며 IPC·등록·승인·비밀 저장소를 호출하지 않는다.
- `ShellOptions { language: Option<Language>, theme: Theme }`; EN/KO, light/dark/system을 실행 중 선택한다. 초기 언어는 명시값 우선, 없으면 OS Korean만 ko이며 나머지는 en이다. 고대비는 OS 색을 사용하며 해제 시 원래 theme로 돌아간다. 선택은 현재 창에만 적용하며 저장했다고 안내하지 않는다.
- `CoreStatus::{Checking, Available { mail_ready }, Unavailable(ErrorCode)}`는 bootstrap readiness이며 GUI 등록·계정 접근 성공의 증거가 아니다. 최초 paint는 readiness probe 한 번만 예약한다. 프로필·등록 요청·메일 조회는 자동 시작하지 않는다.
- Connect는 `MailAction::Connect`→`RegistrationCommand::GuiRequest`만 실행한다. Check registration은 이 창이 Connect로 받은 request ID만 `GuiStatus`로 조회하며 ID가 없으면 비활성이다. 두 결과는 `RegistrationProgress` 값으로 반환하고 ENKO 리소스로 렌더링한다. Ready라도 계정 읽기를 자동 시작하지 않는다.
- Refresh는 현재 목록·선택·커서를 먼저 지운 뒤 readiness를 확인하고 성공하면 `MailAction::Accounts`를 순차 실행한다. 자체 `gui.json`을 사용하는 `list_gui_accounts(false)`만 호출하며 등록 요청·CLI 프로필 fallback·Windows Hello·권한 확대를 하지 않는다. 등록된 이전 실행의 GUI 프로필도 Connect 없이 Refresh로 읽을 수 있다.
- 명시적 계정 선택→`Folders { account_id, cursor }`, 명시적 폴더 선택→`Messages { account_id, folder_wire_hex, encoding, cursor }`로 이어진다. 처음 계정·폴더·메시지를 자동 선택하지 않는다. 폴더 페이지는 최대32개, 메시지 페이지는 기본16/최대32개, 계정은 최대128개다. 폴더 다음 페이지와 메시지 다음 페이지는 각각 직전 응답 커서가 있을 때만 활성화한다. 빈 메시지 페이지에도 다음 커서가 있으면 이동할 수 있다.
- 폴더는 raw wire hex와 정확한 encoding을 함께 보존한다. 미등록 계정/폴더·다른 인코딩·이전 커서는 IPC 전에 거부한다. `no_select`/`non_existent` 폴더는 선택 목록에서 제외하며 페이지 커서는 유지한다. 응답 적용 시에도 원래 요청과 현재 계정·폴더·인코딩·커서를 비교하고, 다른 scope의 행·중복 ID·페이지 상한 초과·잘못된 cursor를 거부한다.
- 네이티브 화면은 계정/폴더 combo, 실제 cached message listbox, 선택 행의 sender/subject/UTC date/read/favorite 메타데이터, 다음 페이지 버튼을 제공한다. 상세는 현재 선택 행만 표시하고 페이지 변경 시 비운다. 작은 창에서도 스크롤 가능한 읽기 전용 상세 영역을 제공한다. 본문·첨부는 불러오지 않았음을 안내한다. 탐색 항목은 화면 골격이며 실제 메일 범위는 계정/폴더 선택으로만 정한다.
- 캐시가 불완전하거나 오래되었을 수 있음을 ENKO로 항상 표시한다. `observed_at=null`은 별도의 미관측 안내이며 빈 실제 메일함으로 표현하지 않는다. 오류는 안정 ErrorCode와 현지화 안내만 표시한다. 조회 실패 또는 잘못된 응답은 이전 행·선택·커서를 지우며 갱신된 데이터처럼 남기지 않는다.
- 공급자에서 온 표시문자열은 제어문자·bidi/default-ignorable 문자를 제거하고 UTF-8 경계의512byte 이내로 표시하고 잘린 표시값 끝에 명시적 생략기호(…)를 붙인다. 긴 헤더 때문에 유효한 페이지를 거부하거나 행을 제거하지 않는다. 상세에도 같은 축약값을 표시한다. 계정ID/msg_SHA256 ID/raw mailbox identity는 별도 strict 검증한다. 원문 문자열로 권한·작업 종류·등록 상태를 결정하지 않는다.
- readiness와 mail action은 창 전체에서 직렬화하여 동시에 하나만 실행한다. 각 IPC wrapper의5초 예산을 유지하고 Refresh의 두 호출도 순차 실행한다. 각 worker의 `sync_channel(1)` 응답에는 원래 action을 결속한다. 진행 중 입력은 추가 thread/요청을 만들지 않으며 언어/theme 변경은 보존한다. 닫힌 창의 늦은 응답은 버린다.
- Native 경계는 `native.rs` 하위로 제한한다. `CreateWindowExW` 동안 살아 있는 `&Rc<Context>`를 전달하고 NCCREATE에서 clone한 Rc를 `into_raw`로 userdata에 보관한다. NCDESTROY에서 userdata 제거 후 정확히 한 번 `from_raw`로 해제한다. DestroyWindow 실패 시 HWND 소유 참조를 유지하여 dangling pointer를 만들지 않는다. synchronous SendMessage 중 RefCell borrow를 유지하지 않는다. font/brush/COM/DPI는 RAII로 관리하고 FFI panic·시스템 오류는 안정 ErrorCode로 변환한다.
- 초기 실패는 언어별 일반 안내와 안정 ErrorCode만 MessageBox로 표시한다. 숨은 창 시험은 이 wrapper를 호출하지 않아 modal을 띄우지 않는다. non-Windows는 CapabilityUnsupported이며 권한 fallback을 추가하지 않는다.
- `--help`, `--version`, 잘못된 인자는 창·IPC·프로필을 시작하지 않는다. 검증은 가짜 callback/backend와 숨은 native controls를 이용하며 실제 사용자 프로필·Hello·계정·네트워크는 사용하지 않는다.
