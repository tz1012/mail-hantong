# ADR-001 코어 런타임

상태: **과거 비교 제안 보관 — [Rust 채택 결정](../ADR-001-core-runtime.md)으로 대체됨**

2026-10-29 사용자가 시작 속도와 메모리 사용의 절충을 검토해 코어·CLI에 Rust를 선택했다. 아래의 미확정 상태와 .NET 우선 추천은 결정 전 이력이다. 현재 구현 기준은 위 정식 ADR을 따른다.

작성일·공식 자료 확인일: 2026-10-29

관련 작업: M0-01 저장소 골격과 빌드, 이후 코어·CLI 구현

사용자 추가 조건(2026-10-29): “언어 선택의 최우선은 빠른 속도”. 앞서 C# + .NET을 추천한 개발 편의 중심 판단을 수정한다. 이후 Windows 우선 출시와 macOS 정식 후속 지원을 승인했으며, Mac에서는 코어·CLI를 먼저 사용할 방향을 확인했다. Mac 출시 단계 번호·일정과 런타임은 아직 미확정이다.

## 배경

mail-hantong은 1단계에서 Windows에 상시 실행 코어, `hantong` CLI, MCP 클라이언트 경로와 선택형 GUI를 제공한다. 향후 Linux 코어 및 Mac 코어·CLI 우선 지원을 위해 플랫폼 의존 부분을 분리한다. [개발 지침](../../latest/DEVELOPMENT_GUIDE.md)의 ADR-001 후보는 .NET, Rust, Go이며 아직 채택된 런타임은 없다.

## 선택지 비교

아래 구현 편의와 검증 부담은 프로젝트 요구를 기준으로 한 설계 판단이다. 성능 우열을 실측한 결과는 아니다.

| 후보 | 이 프로젝트에서의 장점 | 고려할 점 |
| --- | --- | --- |
| **Rust — 비교 후보** | GC 없는 네이티브 실행과 세밀한 자원 제어를 통해 시작 시간·메모리·지연을 관리하는 후보로 검토한다. Windows와 macOS 빌드 대상을 지원한다. | .NET보다 항상 빠르다는 뜻은 아니다. GUI·메일·Windows 연동의 라이브러리 조합과 기존 C# 중심 제안을 재검토해야 한다. |
| **C# + .NET 10 LTS / Native AOT — 후속 검증 우선 제안** | Windows 코어·CLI와 향후 데스크톱 UI를 같은 언어와 도구로 구성하기 쉽다. Native AOT는 시작 시간과 메모리 사용을 줄일 수 있다. | AOT 호환성과 실제 성능을 확인해야 한다. 동적 로딩·런타임 코드 생성 등 제약이 있으며 GUI 전체에 AOT를 적용할 수 있다고 가정하지 않는다. |
| Go | 데몬·CLI를 단순하게 구성하는 대안이다. | Windows GUI와 승인 도우미까지 포함한 플랫폼 통합을 별도로 설계해야 한다. 기존 기술 제안을 재검토한다. |

.NET 10은 현재 최신 LTS이며 2028-11-14까지 지원된다. 현재 PC에 설치된 .NET 8·9의 존재를 제품 런타임 선택 근거로 삼지 않는다. SDK의 정확한 패치 버전은 채택 후 M0-01에서 당시 지원되는 버전을 확인해 고정한다. [.NET 지원 정책](https://dotnet.microsoft.com/en-us/platform/support/policy), [.NET 릴리스와 지원](https://learn.microsoft.com/dotnet/core/releases-and-support)

Rust는 안정 버전의 하위 호환과 에디션 전환 정책을 제공하며, Go는 두 개의 후속 주요 버전이 나올 때까지 해당 주요 버전을 지원하는 정책을 쓴다. 각 후보의 유지보수 정책도 선택에 포함한다. [Rust 에디션 안내](https://doc.rust-lang.org/edition-guide/editions/), [Go 릴리스 정책](https://go.dev/doc/devel/release)

## 제안하는 결정

초기에는 Rust를 우선 검증 후보, .NET 10 Native AOT를 비교 후보로 두었다. 사용자가 동일 기능의 작은 시제품 비교에 동의해 2026-10-29 실험을 수행했다. [실측 보고서](../../latest/RUNTIME-BENCHMARK.md)에서는 .NET AOT의 프로세스 시작이 빨랐고 Rust의 메모리가 적었으며 검색 성능은 작업별로 달랐다. 현재는 **.NET 10 Native AOT를 다음 실제 제품 경로 검증의 우선 후보**로 제안하되 제품 런타임 채택은 미확정으로 유지한다. 언어 이름만으로 제품 속도를 보장하지 않는다. 플랫폼 독립 로직과 Windows 구현을 분리하고, 현재 1단계 제품 지원 범위는 유지한다.

Rust의 실행 모델과 지원 대상, .NET AOT의 이점과 제약은 공식 자료로 확인했다. 이번 실측은 표준 라이브러리와 합성 데이터만 사용하는 초기 신호다. 실제 DB·MIME·IPC·정책/승인·GUI·Mac 성능을 아직 비교하지 않았으므로 제품의 1초 목표 달성이나 보편적인 언어 우열로 확대하지 않는다. [Rust 공식 백서](https://rust-lang.org/pdfs/Rust-Tilde-Whitepaper.pdf), [Rust 플랫폼 지원](https://doc.rust-lang.org/rustc/platform-support.html), [Native AOT](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/)

이 결정은 GUI 프레임워크(ADR-002), 프로젝트 라이선스(ADR-016), IPC·비밀 저장소·메일 라이브러리·설치 패키지를 함께 승인하는 결정이 아니다. 해당 항목은 각 비교안과 계약 검토에서 정한다.

## 실행 속도 확인 방법

- 코어가 꺼진 상태의 전체 시작, 코어가 켜진 상태의 CLI 첫 응답, GUI의 사용 가능한 기본 화면 표시를 따로 측정한다.
- 같은 데이터·동작·하드웨어·Release 설정에서 캐시 목록·본문 열기·로컬 검색·MIME 처리의 지연과 처리량을 비교한다. 평균과 느린 구간(P95), CPU·메모리도 기록한다.
- 원격 메일 서버의 지연·속도 제한과 로컬 처리 성능을 분리한다. 보안·승인 검사를 생략한 시험으로 빠르다고 판정하지 않는다.
- 실제 사용 라이브러리가 연결된 경로의 결과로 판단한다. 간단한 반복문만 비교한 결과를 앱 전체 성능으로 확대하지 않는다.
- 원문 §13.1의 기준은 4코어급 CPU·16GB RAM·SSD·5계정·메타데이터 10만 건이다. 사용자 승인(2026-10-29)으로 웜 캐시 기본 메일 목록 사용 가능 시점의 목표를 **3초에서 1초 이내로 강화**했다. 현재 기준과 측정 조건은 [기능 범위의 성능 보완](../../latest/FEATURES.md)에 기록한다. 캐시 목록 100개 500ms, 캐시 본문 1초, 로컬 검색 P95 2초는 유지한다. 모두 측정할 목표이며 달성 결과가 아니다.

## macOS 포팅 범위

.NET과 Rust 모두 macOS용 코어·CLI를 만들 수 있는 기반이다. 공통 메일 로직을 유지하려면 비밀 저장소·IPC·알림·사용자 확인·자동 실행·파일 경로 같은 OS 연동을 분리해야 한다. Mac용 보안 동작과 패키지·배포 시험은 별도 작업이다. [.NET macOS 지원](https://learn.microsoft.com/en-us/dotnet/core/install/macos), [Rust 플랫폼 지원](https://doc.rust-lang.org/rustc/platform-support.html)

WinUI 3와 WPF는 Windows용이므로 기존 제안의 GUI를 그대로 Mac에서 실행할 수는 없다. 사용자 승인으로 Mac 후속 목표가 확정됐으므로 ADR-002에서 크로스플랫폼 GUI 후보, OS별 구현 범위와 Rust 코어의 연결 방식을 함께 비교한다. Mac 코어·CLI 출시가 전체 GUI 완성에 묶이지 않도록 구성하고, 등록·비밀 입력·발송 승인에 필요한 최소 도우미를 별도 제공한다. 여기서 특정 GUI를 채택하지 않는다. [WinUI](https://learn.microsoft.com/en-us/windows/apps/winui), [WPF](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/overview/), [Avalonia 지원 플랫폼](https://docs.avaloniaui.net/docs/supported-platforms), [.NET MAUI 지원 플랫폼](https://learn.microsoft.com/en-us/dotnet/maui/supported-platforms)

## 채택 후 할 일

1. 사용자 선택을 이 ADR과 단일 진행 기록에 남긴다.
2. M0-00 및 ADR-016 선행 조건을 확인한 뒤 M0-01에서 정확한 SDK 버전과 빌드 구조를 고정한다.
3. Windows 전용 구현을 분리하고 가짜 제공자 시험부터 연결한다. 사용자가 승인한 비교 실험에 한해 `.tools/`에 .NET 10 SDK를 설치하고 `experiments/runtime/`에 두 시제품을 작성했다. 제품 프로젝트는 아직 생성하지 않았다.

## 재검토 조건

후속 후보가 필수 Windows·Mac 연동이나 배포 조건을 충족하지 못하거나, 실제 라이브러리 연결 후 속도·메모리 문제가 확인되면 근거를 갖춰 비교안을 다시 작성한다. Native AOT 호환성을 가정해 라이브러리를 선정하지 않는다.
