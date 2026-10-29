# ADR-013 아키텍처 의존 경계 검사

상태: **초기 구현 채택 — 2026-10-29, 연속 개발 위임에 따른 에이전트 결정; 사람 보안 검토 필요**

관련 작업: M0-03. 관련 요건: SEC-03, AI-05, CLI-06.

## 배경

mail-hantong은 CLI·MCP·GUI가 IPC와 계약만 통해 코어를 호출하고, 정책·승인·비밀·발송 경계를 우회하지 않아야 한다. Rust package 이름만 검색하는 방식은 별칭·간접 의존·개발 의존성을 놓치므로 Cargo가 생성한 dependency metadata를 기준으로 검사한다.

## 선택지

| 선택 | 결과 |
| --- | --- |
| Cargo metadata + Python 표준 라이브러리 | Cargo가 해석한 package/의존 그래프를 사용하고 추가 runtime crate가 없다. 현재 workspace와 Windows·Linux CI에서 동일한 JSON 검사를 실행한다. |
| Rust 전용 architecture 검사 crate | 검사 코드를 제품 workspace에 넣고 dependency를 추가해야 하며, 아직 확정되지 않은 crate 경계를 도구 선택과 함께 고정한다. |
| 문자열/경로 검색 | alias, transitive dependency, dev-dependency를 신뢰성 있게 구분하지 못한다. |

## 결정

`scripts/check_architecture.py`가 `cargo metadata --offline --locked --format-version 1 --no-deps`를 읽고, 명시적으로 등록된 제품 package 집합·허용 내부 edge·직접 및 transitive client 금지 edge·비정상(dev) dependency를 검사한다. Cargo dependency rename(alias)는 resolved `package` 기준으로 검사하므로 허용된 대상의 alias는 사용할 수 있지만 경계를 우회하지 못한다. 결과는 정렬된 JSON과 종료 코드로 반환하며 파일을 변경하지 않는다. 새 package나 edge는 명시적으로 검사표에 추가하기 전 실패한다.

현재 규칙은 scaffold의 빈 graph를 허용한다. 도메인과 계약은 leaf로 제한한다. `hantong-cli`가 store/secrets/adapters/ipc-server/outbox에 직접 또는 간접으로 도달하면 실패한다. adapters는 domain만, policy는 approval을 통해 실행 정책을 조합하며 approval은 contracts/domain만 참조한다. IPC server는 contracts와 application을 참조한다. core daemon은 조립 지점으로 허용된 내부 package를 참조할 수 있다. `hantong-test-support`는 제품 dependency가 아닌 dev-only 별도 package로만 허용한다. 외부 registry dependency는 이 구조 검사 대상이 아니지만, 등록되지 않은 local/path dependency는 실패한다.

이 검사는 정책·승인·비밀 저장소의 안전성을 증명하지 않는다. 해당 영역은 실행 가능한 보안 시험과 사람 검토가 필요하다.

## 재검토 조건

M1-01/02 구현 보완: `hantong-platform`은 계약만 참조하는 OS 전송 경계다. IPC client/server가 이를 공유하며 CLI는 여전히 IPC client/contracts에만 직접 의존한다. 이 추가는 저장소·비밀·어댑터 우회를 허용하지 않는다. SID·DACL·핸들 FFI의 사람 보안 검토 의무를 같은 경계에 적용한다.

M2-01 보완(2026-10-30 연속 개발 위임): Windows Store→Platform 의존을 허용하여 메일 설정 canonical hash를 기존 bounded CNG SHA-256으로 재계산한다. 암호 구현 복제와 새 암호 패키지를 피한다. Platform→Store/Secrets는 계속 금지하고 CLI의 간접 저장소 접근 검사도 유지한다. 허용 방향과 반대 방향 거부를 함께 시험한다. 이것은 일반 의존 결정이며 사람 보안 검토 완료를 뜻하지 않는다.

M2-01 자격 증명 결속 보완: Secrets→Domain을 허용하여 검증된 endpoint를 암호문 안의 결속 정보와 대조한다. Domain→Secrets/Store는 계속 금지한다. Secrets→Store 역방향 의존이나 외부 plaintext callback은 만들지 않는다. 이 변경도 허용/금지 방향 시험과 사람 보안 검토 대상이다.

package 경계, IPC/GUI 구조, 테스트 지원 package, ADR-003/008/010/019 결정이 바뀌면 허용표와 테스트를 함께 갱신한다. 사람 검토 없이 이 검사를 완화하거나 전체 보안 통과로 표시하지 않는다.
