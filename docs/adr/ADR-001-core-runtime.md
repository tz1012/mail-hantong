# ADR-001 코어 런타임: Rust

상태: **확정 — 2026-10-29, 사용자 Andrew Cho의 명시적 선택**

관련 작업: M0-01 저장소 골격과 빌드, 이후 코어·CLI 구현. 관련 기준: OPS-01, CLI-16.

## 결정

mail-hantong의 **코어 데몬과 `hantong` CLI는 Rust로 구현한다.** 사용자는 두 시제품의 실측을 확인한 뒤 “시작이 좀 느리지만 메모리 관리도 중요하니 Rust로 가자”라고 결정했다. 시작 속도와 상주 메모리의 절충을 고려한 선택이다.

실행 속도는 계속 중요한 기준이며, 웜 캐시 상태에서 메일 목록을 사용할 수 있을 때까지 **1초 이내**라는 목표도 유지한다. Rust 채택을 성능 목표 달성이나 메모리 문제의 자동 해결로 해석하지 않는다. 데이터 보유량·캐시 상한·할당과 실제 처리 경로는 구현 후 검증한다.

## 선택 근거와 비교 이력

[실측 보고서](../latest/RUNTIME-BENCHMARK.md)의 Windows 시제품 비교는 동일한 합성 메타데이터 10만 건·5계정으로 40라운드씩 두 차례 수행했다. 2회차 중앙값은 다음과 같다.

| 항목 | Rust | .NET Native AOT |
| --- | ---: | ---: |
| 프로세스 생성 → PONG 응답·종료 | 190.7ms | 63.3ms |
| 프로세스 생성 → 캐시 읽기·정렬·READY | 291.5ms | 262.9ms |
| READY working set | 30.9MiB | 100.6MiB |

검색 성능은 작업별로 달랐다. 실험 직후에는 빠른 시작을 근거로 .NET AOT 후속 검증을 우선 제안했지만, 사용자는 메모리 관리도 중요하다고 판단해 Rust를 선택했다. 이 사용자 결정이 이전 추천을 대체한다. [이전 비교안](proposed/ADR-001-core-runtime.md)은 결정 경과로 보관한다.

실험은 Windows 6코어·32GB 장비, Rust 1.92.0과 .NET SDK 10.0.403을 사용했다. 실제 DB·MIME·공급자·인증 IPC·정책/승인·GUI·Mac이 포함되지 않았으므로 수치를 제품 성능이나 언어의 보편적 우열로 확대하지 않는다. 실험 실행 파일과 원시 결과는 보존한다.

## 적용 범위와 별도 결정

- 코어와 CLI의 구현 언어를 Rust로 확정한다. 비동기 실행기, 개별 crate, 라이브러리 버전, 저장 형식과 IPC 계약까지 채택한 것은 아니다.
- Rust toolchain·edition·지원 target과 최소 지원 버전은 M0-01에서 확인해 고정한다. 실험에서 사용한 1.92.0을 제품의 버전으로 자동 고정하지 않는다.
- GUI 프레임워크(ADR-002), 메일·MIME(ADR-006), MCP(ADR-009), CLI 프레임워크(ADR-010), 아키텍처 검사(ADR-013)는 Rust 코어와의 적합성을 검토해 별도로 결정한다. 기존 MailKit/MimeKit·C# SDK·System.CommandLine·NetArchTest는 제품 채택 목록이 아니다.
- .NET 비교 시제품과 로컬 SDK는 실험 재현용이다. 코어·CLI 제품 실행에 .NET을 요구하는 결정이 아니다.
- Windows 우선 출시, 이후 Mac 코어·CLI·최소 승인 도우미를 먼저 제공하고 전체 GUI를 후속 제공한다는 방향을 유지한다. OS별 비밀 저장소·IPC 인증·사용자 확인·알림·설치와 Mac 실기 검증은 별도로 필요하다.
- CLI 영어 기본·한국어 선택, GUI 영·한 및 라이트·다크·시스템 테마, 사람 승인·계정 분리·SEND_UNKNOWN 등 기존 요구를 유지한다.

## 후속 작업과 완료 경계

1. 최신 개발 지침과 활성 AGENTS의 .NET 기본 가정을 Rust 결정에 맞춘다. 제품 workspace가 없는 현재 상태에서 루트 Cargo 명령이 실행 가능하다고 안내하지 않는다.
2. M0-00의 기준 문서 채택 후 M0-01에서 제품 Cargo workspace와 실제 빌드·검사 스크립트를 만든다. [ADR-016](ADR-016-license.md)의 라이선스 문안도 2026-10-29 별도 사용자 승인으로 채택했다. 이 결정들만으로 M0-00·M0-01을 완료 처리하지 않는다.
3. C# 방식으로 표현된 모듈·보안 시험·검토 경계를 Rust crate/test 구조에 대응시킨다. 모듈 이름이 달라져도 사람 검토 의무와 정책·승인 경계를 유지한다. 작업 카드 범위·선행 조건은 임의로 바꾸지 않는다.
4. 실제 메일 캐시·검색·본문·동기화 경로에서 시작 시간, P95 응답, CPU와 상주/최대 메모리를 측정한다. 장시간 실행과 동시 작업도 검증하며 보안 검사를 성능 때문에 우회하지 않는다.

## 재검토 조건

실제 제품 경로에서 시작·응답·메모리 목표를 충족하지 못하거나 필수 Windows·Mac 연동 및 라이브러리 조건을 충족하기 어렵다면 원인·개선안·비교 근거를 제시한다. 실험 결과 일부만으로 언어를 자동 변경하지 않으며, 다른 언어로 전환하려면 별도 사용자 결정을 기록한다.

## M0-01 구현 결정 — 2026-10-29

사용자의 연속 개발 위임에 따라 제품 toolchain은 **Rust 1.95.0**, edition **2024**, Cargo resolver **3**, 최소 Rust 버전 **1.95**로 고정한다. 이 PC의 기본 stable은 1.92.0이지만 별도로 설치된 1.95.0 MSVC와 rustfmt·Clippy를 확인했다. 비교 실험의 toolchain과 소스는 바꾸지 않는다. 최신 버전이라는 주장 대신 로컬에서 검증 가능한 고정 버전을 채택한다.

루트 `rust-toolchain.toml`에는 정확한 버전·minimal profile·rustfmt·clippy를 기록한다. Cargo workspace는 제품과 비교 실험을 분리하고, 자체 라이선스는 `license-file`로 상속한다. 외부 runtime crate는 골격 단계에서 추가하지 않으며 각 package는 `publish = false`다. release 최적화는 Cargo 기본값을 사용하고 변경 시 실제 측정한다.

공식 근거: [rustup 버전 고정](https://rust-lang.github.io/rustup/overrides.html), [Cargo workspace와 상속](https://doc.rust-lang.org/cargo/reference/workspaces.html), [Rust 2024 resolver](https://doc.rust-lang.org/stable/edition-guide/rust-2024/cargo-resolver.html), [라이선스 파일](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields), [release profile](https://doc.rust-lang.org/cargo/reference/profiles.html). 정확한 crate 경계는 [Rust 구조](../latest/RUST-STRUCTURE.md)에 기록한다.
