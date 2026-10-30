# ADR-002: Windows GUI의 Native Win32 표시 계층

상태: 사용자에게 위임받은 일반 기술 선택으로 구현에 채택. 네이티브 메모리·접근성·실제 화면 검토는 별도로 수행한다. 2026-10-30.

사용자는 빠른 시작과 메모리를 우선하며 Rust, 첫 GUI부터 영·한 지원, Light/Dark/System을 요구했다. 현재 저장소는 Windows 승인 도우미에서 고정된 `windows 0.62.2`와 Win32 컨트롤·DPI·OS 테마를 이미 사용한다. GUI에 해당 표시 기술을 사용하며 기존 승인 발급 코드는 가져오지 않는다. 새 외부 UI framework나 웹 런타임은 추가하지 않는다.

WebView 기반 UI는 웹 스타일 재사용에 유리하지만 별도 웹 런타임과 보안·메모리 경계를 추가한다. 별도 Rust GUI framework는 이식성을 제공하나 지금은 추가 의존성과 플랫폼 검증이 필요하다. Win32는 Windows 시작 비용과 기본 키보드/접근성 컨트롤을 활용하는 대신 macOS의 네이티브 창 구현은 후속으로 필요하다. 코어·CLI와 순수 표시 모델은 이 선택에 묶지 않는다.

`apps/mail-hantong-gui`는 안전한 Rust 실행 진입점과 IPC readiness 조회만 가진다. `crates/hantong-desktop`은 UI 상태·영/한 리소스·의미 기반 색상·배치와 Windows 전용 네이티브 창 경계를 가진다. unsafe는 그 네이티브 경계에 제한한다. GUI와 표시 crate 모두 Store/Secrets/Adapters/Outbox/승인 발급 모듈에 접근하지 않는다. 실제 메일 조회는 GUI 고유 등록 이후 인증 IPC로 연결한다. CLI 등록 프로필은 GUI의 신원으로 사용하지 않는다.

초기 골격은 세 영역, Core 연결 상태, 영·한과 세 테마의 실행 중 선택을 구현한다. 현재 창의 표시 선택과 메일·승인 상태는 분리한다. 설정 영속 저장은 GUI 고유 등록·Core 설정 API 연결에서 제공하며 처음부터 완료했다고 표시하지 않는다. 초기 골격은 실제 메일이 없다는 뜻의 가짜 빈 목록이나 발송 버튼을 제공하지 않는다. 미연결 상태와 다음 설정 단계를 명확히 표시한다.

고대비는 명시 테마보다 우선한다. System은 실행 중 OS 변경을 반영하고 명시 테마는 보존한다. DPI 변경 시 폰트와 배치를 다시 계산하며 좁은 창은 상세 영역을 목록 아래에 배치한다. native 컨트롤의 이름·Tab 순서·포커스와 양 언어의 긴 문자열을 검증한다.

2026-10-30 화면 피드백: 기본 Win32 컨트롤을 그대로 드러낸 첫 화면과 큰 둥근 카드형 후속 시안 모두 최종 디자인으로 채택하지 않는다. Gmail·Superhuman의 밀도와 Outlook의 연속 프레임을 참고해 얇은 상단/도구 영역·조밀한 목록·넓은 읽기 영역으로 재구성한다. 실제 HWND의 키보드·접근성 의미는 보존하며 컨트롤 표시·얇은 스크롤·짧은 선택 전환을 같은 테마로 통합한다. 애니메이션은 OS 설정/고대비를 존중하고 유휴 타이머를 남기지 않는다. 공통 `windows_manifest.rs`를 GUI와 숨은 창 시험에 재사용해 Common Controls v6를 적용한다. 추가 웹 런타임이나 외부 UI 패키지를 도입하지 않는다. 레트로는 후속 히든 기능이며 현재 Light/Dark/System과 분리된 미래 범위다.

근거: [기존 디자인 가이드](../latest/DESIGN.md), [Microsoft 테마](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-windows-themes), [고대비](https://learn.microsoft.com/en-us/windows/win32/winauto/high-contrast-parameter), [DPI 변경](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged), [창 상태 수명](https://learn.microsoft.com/en-us/windows/win32/learnwin32/managing-application-state-).
