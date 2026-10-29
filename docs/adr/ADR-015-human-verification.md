# ADR-015 코어 소유 승인 창과 Windows Hello

상태: 2026-10-30 연속 개발 위임에 따른 구현 결정. **사람 보안 검토 필요.** API 조사와 설계 검토만 수행했으며 실제 Windows Hello 등록·사람 확인·승인 발급 시험을 실행하지 않았다.

## 문제와 근거

INST-04는 코어가 인증된 사용자 세션에 네이티브 창을 열고 사용자 확인을 요청하도록 한다. APR-02~04는 일반 클릭·CLI 확인·호스트 확인만으로 발급하지 않으며 최종 검토 자료·현재 요청·승인 채널·세션을 결속하도록 한다. 기준 문서의 실행 환경은 Windows 11 x64 우선이다.

Windows 데스크톱에서 `UserConsentVerifier`를 사용할 때는 자체 활성 창의 HWND를 전달하는 `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`를 사용한다. Microsoft 문서의 최소 버전은 Windows 11 build 22000이다. 반환값은 `Verified`, `Canceled` 등의 enum이며 요청 hash·nonce를 서명한 영수증이 아니다. 다른 프로세스가 `Verified`와 nonce를 보내더라도 Windows Hello를 실제 수행했다는 증거가 되지 않는다.

## 결정

1. 최소 승인 창은 코어 프로세스의 전용 UI thread가 소유한다. 초기 코어 시작 시 UI thread를 만들지 않고 보호 UI가 필요할 때 지연 생성한다. IPC용 Tokio thread에서 Win32 message loop나 사람 확인을 기다리지 않는다.
2. 승인 UI 모듈은 코어가 확정한 불변 review snapshot을 그대로 표시한다. UI가 계정·수신자·첨부·내용·hash를 별도로 재조립하지 않는다. 첫 화면부터 영/한 리소스와 Light/Dark/System·고대비 정책을 적용한다.
3. OS 사람 확인 호출과 결과 수신은 이 코어 소유 창에서 직접 수행한다. launcher/helper는 창 열기 요청만 전달할 수 있으며 nonce·hash·검증 결과를 발급하거나 `Verified`를 보고하는 IPC method를 갖지 않는다. CLI/MCP도 승인 발급 기능을 갖지 않는다.
4. 승인 coordinator가 request ID, canonical operation/review/target hash, 계정, 요청자 등록 ID와 profile epoch, 승인 전용 세션 ID, 채널, OS 난수 nonce, UTC 만료 및 프로세스 내 단조 시계 제한을 보유한다. Hello 호출 직전 표시 snapshot을 고정하고 결과를 받은 뒤 현재 저장 상태를 다시 검사한다.
5. `Verified`만 승인 발급의 필요조건이다. 현재 요청의 hash·epoch·상태·세션·nonce·만료가 모두 같을 때 pending→approved를 CAS로 전환한다. 이 결과를 승인 완료나 메일 발송 완료로 혼동하지 않는다. 발송/변경 실행 직전에 결속을 재확인하고 approval을 한 번만 소비한다.
6. OS 결과는 같은 프로세스의 닫힌 제어 흐름에서만 소비한다. `Serialize`, IPC callback, bool 또는 임의 assertion bytes로 변환해 검증된 사람 확인으로 재입력하지 않는다. 공개 `issue_approval(bool)`나 일반 도구용 발급 API를 만들지 않는다.
7. 취소·busy·미설정·장치 없음·정책 차단·재시도 소진·timeout·API 실패에서는 승인 0건이다. 일반 클릭·별도 PIN 입력창·TOTP로 자동 대체하지 않는다. P1 TOTP는 별도 검토 전 비활성이다. capability 안내와 실제 확인 결과를 분리하고, 과거 availability를 권한으로 캐시하지 않는다.

이 설계는 Microsoft의 자체 HWND interop 지침과 이 프로젝트의 요청 결속 요건을 조합한 구현 판단이다. Microsoft가 이 애플리케이션의 승인 정책을 검증했다는 뜻이 아니다.

## 선택지와 영향

| 선택 | 판단 |
| --- | --- |
| 코어 소유 HWND와 직접 Hello 결과 수신 | 채택. helper IPC에서 검증 결과를 위조하는 경로를 없애며 INST-04의 코어 소유 창과 맞는다. 코어 조립에 지연 UI thread와 Windows 모듈이 필요하다. |
| 별도 helper가 Hello enum을 전송 | 채택하지 않음. 인증된 helper 세션은 송신 세션을 식별하지만 OS 확인을 수행했다는 증거를 주지 않는다. |
| Hello KeyCredential challenge 서명 | 보류. 다른 API이며 package identity·배포·키 수명·desktop 지원의 실제 검증이 필요하다. UserConsentVerifier와 같은 증거로 취급하지 않는다. |

Stage 1 보호 승인 UI의 지원선은 Windows 11 build 22000 이상이다. 지원되지 않는 OS나 API에서는 보호 작업을 `CAPABILITY_UNSUPPORTED` 등 계약의 안정 오류로 거부하고 설정 마법사에 필요한 조치를 표시한다. 다른 OS의 CLI 컴파일 가능 여부와 승인 기능 지원 여부는 별개다. macOS 승인 경로는 별도 OS 구현과 검증을 거친다.

현재 `platform::UserVerifier`/`UnverifiedEvidence`는 외부 검증기의 미검증 증거를 나타내는 초기 인터페이스다. Windows Hello 경로에 연결하지 않는다. 실제 UI 구현 전에 같은 프로세스의 결과와 pending 요청 결속을 나타내는 계약으로 개정하고, 승인 채널·승인 전용 세션·UTC 밀리초 단위를 명시한다. GUI 모듈을 추가할 때 architecture allowlist와 의존 검사도 함께 갱신한다. UI 프레임워크·WinRT Rust projection은 별도 검증 후 필요한 최소 의존만 채택한다.

## 검증과 남은 위험

가짜 `Verified` IPC·일반 클릭/Enter·다른 요청/hash/session/epoch·만료·동시 두 창·재시작 뒤 nonce 재사용·Hello 실패는 모두 승인 0건이어야 한다. 순수 상태 시험에는 합성 OS 결과를 쓰되 제품 빌드에 가짜 확인기를 연결하지 않는다. 실제 사람 확인과 다른 Windows 사용자/로그온 환경은 별도 검토 게이트다.

코어 내부 직접 수신은 helper의 자기 주장 문제를 줄인다. 프로세스 주입, 같은 사용자 악성코드, 관리자/커널 침해, 사회공학을 완전히 차단한다는 뜻은 아니다. ADR-019의 잔여 위험을 유지한다. Hello 표시 메시지 자체는 암호학적 결속이 아니며 canonical snapshot과 서버 상태 검증을 대체하지 못한다.

## 근거

- [INST-04 및 관련 설치 요건](../requirements/inst.md), [APR-02~04](../requirements/apr.md), [기준 실행 환경](../requirements/source-content.md)
- [Microsoft: desktop UserConsentVerifier interop](https://learn.microsoft.com/en-us/windows/win32/api/userconsentverifierinterop/nf-userconsentverifierinterop-iuserconsentverifierinterop-requestverificationforwindowasync)
- [Microsoft: desktop WinRT UI 소유 창](https://learn.microsoft.com/en-us/windows/apps/develop/ui/display-ui-objects)
- [Microsoft: UserConsentVerifier](https://learn.microsoft.com/en-us/uwp/api/windows.security.credentials.ui.userconsentverifier)
- [Microsoft: UserConsentVerificationResult](https://learn.microsoft.com/en-us/uwp/api/windows.security.credentials.ui.userconsentverificationresult)
- [Microsoft: Windows Hello와 KeyCredential](https://learn.microsoft.com/en-us/windows/apps/develop/security/windows-hello)

## 2026-10-30 Rust projection 컴파일 확인

격리 `experiments/hello-bindings`에서 `windows =0.62.2`와 `windows-future =0.3.2`를 확인했다. 두 버전의 MSRV는 1.82이며 MIT OR Apache-2.0이다. 기존 windows-sys 0.61.2와 함께 사용할 수 있다. `windows::core` 재수출을 사용하며 windows-core 직접 의존/자체 COM interface 선언은 필요하지 않다.

정확한 생성 API는 `windows::Win32::System::WinRT::IUserConsentVerifierInterop::RequestVerificationForWindowAsync::<windows_future::IAsyncOperation<UserConsentVerificationResult>>(HWND, &HSTRING)`이다. `windows_future::Async` trait는 공개가 아니므로 사용하지 않는다. 공개 `AsyncOperationCompletedHandler`와 `SetCompleted`로 완료를 받고 Cancel/Close를 UI 소유 thread에서 수행한다. 호출마다 완료·오류·취소·창 닫힘을 모두 종료 상태로 연결해야 한다.

Rust 1.95 Windows MSVC에서 check/fmt/all-target Clippy를 통과했다. 실제 Hello/window/COM 초기화는 실행하지 않았으며 HWND 소유권·STA message pump·취소 경합·신뢰 review snapshot 결속은 미구현이다. 제품 의존성과 UI 구현은 해당 계약과 함께 후속 연결한다.

근거: [정확한 windows metadata](https://crates.io/api/v1/crates/windows/0.62.2), [생성 interop source](https://docs.rs/crate/windows/0.62.2/source/src/Windows/Win32/System/WinRT/mod.rs), [IAsyncOperation API](https://docs.rs/windows-future/0.3.2/windows_future/struct.IAsyncOperation.html), [Microsoft HWND consent API](https://learn.microsoft.com/en-us/windows/win32/api/userconsentverifierinterop/nf-userconsentverifierinterop-iuserconsentverifierinterop-requestverificationforwindowasync).

## 2026-10-30 제품 등록 창 연결

`hantong-approval-windows`를 Core만 의존 가능한 발급 경계로 추가했다. `windows =0.62.2`, `windows-future =0.3.2`, 기존 `windows-sys =0.61.2`의 필요한 Win32/WinRT 기능만 사용한다. 순수 review/resources는 `hantong-approval`에 있고 CLI/MCP/platform에는 native 발급 의존을 허용하지 않는다.

현재 제품 구현은 기본 read-only CLI 등록 snapshot에 한정한다. 이는 메일 발송 승인 기능의 완료가 아니다. 상세 상태·일회 결과·취소/cleanup 한계는 `registration-review.md`, 등록 저장·파일 발행·복구 순서는 `client-registration.md`와 `client-profile.md`를 따른다. 최신 부분 검사와 전체 검사 여부는 `docs/latest/PROGRESS.md`에 기록한다. 실제 Hello·접근성·시각 검증·사람 보안 검토는 남아 있다.
