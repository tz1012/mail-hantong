# Rust 작업공간과 보안 경계

M0-01 구현 기준. 기존 논리 모듈을 아래 Rust package로 대응시킨다. 빈 경계 생성은 기능 구현이나 보안 검토 완료를 뜻하지 않는다. 코어·CLI toolchain은 [ADR-001](../adr/ADR-001-core-runtime.md)을 따른다.

| 기존 역할 | 제품 경로 | 책임 |
| --- | --- | --- |
| 공통 계약 DTO | `crates/hantong-contracts` | 안정 식별자와 메시지 계약 |
| OS 전송 경계 | `crates/hantong-platform` | Windows SID·명시적 pipe ACL·핸들 수명 — 사람 검토 필요 |
| Core.Domain | `crates/hantong-domain` | 계정·메일 도메인, 공급자 인터페이스 |
| Core.Application | `crates/hantong-application` | 공통 도구 실행 경로 |
| Core.Policy | `crates/hantong-policy` | 권한·정책 판단 — 사람 검토 필요 |
| Core.Approval | `crates/hantong-approval` | 승인 결속·소비 — 사람 검토 필요 |
| Native approval issuer | `crates/hantong-approval-windows` | Core 전용 HWND·Windows Hello 일회 결과·공개 프로필 발행; CLI/MCP/platform 의존 금지 — 사람 검토 필요 |
| Core.Outbox | `crates/hantong-outbox` | 발송 원장·재시도 — 사람 검토 필요 |
| Store | `crates/hantong-store` | 로컬 DB·검색·캐시. `hantong-secrets`가 제공하는 키 경계와 내부 연결 콜백만 사용하며 원시 키를 반환하지 않음 |
| Secrets | `crates/hantong-secrets` | OS 비밀 저장·자격증명 사용 — 사람 검토 필요 |
| Adapters | `crates/hantong-adapters` | 공급자별 구현 |
| Ipc.Client | `crates/hantong-ipc-client` | 클라이언트 IPC |
| Ipc.Server / Auth | `crates/hantong-ipc-server` | 코어 IPC·인증 — 사람 검토 필요 |
| Core daemon | `apps/mail-hantong-core` | 코어 수명과 의존성 조립 |
| CLI | `apps/hantong-cli` | `hantong` 명령, 영어 기본·한국어 표시 |

GUI·MCP·승인 도우미는 각각의 구현 카드에서 추가한다. 승인 도우미의 별도 보안 검토 경계는 그대로 유지된다. 클라이언트는 IPC client와 계약 DTO에만 직접 의존하고, 저장소·비밀·공급자에 접근하지 않는다. Store가 Secrets의 키 경계를 호출하는 것은 허용하지만 원시 키를 반환받지 않는다. IPC 양쪽은 OS 경계 crate를 공유하며 플랫폼 crate는 계약 외 제품 계층에 의존하지 않는다. `scripts/check_architecture.py`가 이 구조를 검사한다.

`cargo build --workspace --all-targets --locked --offline`, `cargo test --workspace --all-targets --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`를 공통 검사에서 사용한다. 문서/검사 도구는 Python 표준 라이브러리를 이용하며 제품 실행 시 Python을 요구하지 않는다.

Windows MSVC를 현재 환경에서 검증한다. Linux CI와 macOS의 성공은 해당 환경에서 실제 결과를 얻기 전까지 확인한 것으로 기록하지 않는다. OS 전용 코드가 추가될 때 안전한 Rust 경계 안에 가두고 개발용 구현이 제품 빌드에 연결되지 않도록 검사한다.
