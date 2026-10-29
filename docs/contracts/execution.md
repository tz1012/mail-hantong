# 공통 도구 실행 계약

상태: 연속 개발 지시의 구현 기준, 사람 보안 검토 필요. M1-09, AI-05, AI-10, SEC-03.

## 순서와 권한

엄격한 IPC 프레임 해석 뒤 JSON-RPC method와 inner tool이 정확히 일치해야 한다. 입력 타입 검증 → 연결 소유 세션과 최신 등록 재검증 → canonical 계정 확인 및 감사 시작 commit → 모든 계정/도구 scope 검사 → 정책 검사 → 읽기 서비스 → 실제 결과 감사 commit 순서다. 한 계정이라도 scope 밖이면 전체 요청을 거부한다. 알려진 계정에 대한 scope/정책 거부는 terminal로 기록한다. 미등록·손상 입력·알 수 없는 계정은 원문 없는 안정 오류로 거부한다.

Application coordinator는 trusted core가 주입한 `ExecutionContext`를 사용한다. 이것은 wire 입력이나 플러그인 인터페이스가 아니다. 실제 adapter는 OS 연결 인증을 마친 non-cloneable AuthenticatedConnection과 Store를 소유/차용하며, 요청에서 client_id·role·session_id·approved를 만들거나 읽지 않는다. registration ID/epoch getter는 선택자이고 인증 증거가 아니다. 각 요청에 현재 등록을 다시 읽어 철회·epoch·키·범위 축소를 검사하며 실패한 세션은 닫는다.

감사 execution ID는 core CSPRNG로 생성하고 요청 ID와 분리한다. `begin_audit`는 검증된 세션 ID/epoch와 canonical 계정을 사용한다. start 실패 뒤에는 scope/정책/서비스를 호출하지 않는다. 정책/범위 거부 및 서비스 오류도 기존 안정 ErrorCode로 terminal에 저장한다. terminal 저장 실패는 INTERNAL_ERROR로 응답하며 서비스가 실행되지 않았다고 추정하거나 자동으로 재호출하지 않는다. 응답 손실도 재호출 이유가 아니다.

## 현재 제공 범위

coordinator의 유일한 서비스 포트는 계정 목록 **읽기**다. 결과 값과 선택 수치는 신뢰된 서비스가 반환하고, 감사에는 수치/오류만 들어간다. 오류 문자열이나 자유 텍스트/원문을 받는 포트는 없다. 정책 구현·읽기 서비스가 준비되지 않은 core adapter는 POLICY_BLOCKED/CAPABILITY_UNSUPPORTED로 거부한다. 시험 fixture는 제품 서비스로 연결하지 않는다.

`mail.send.execute`는 입력 검증과 scope/정책 경계만 제공한다. 정책을 통과해도 현재 coordinator는 POLICY_BLOCKED로 거부한다. send 서비스 callback이나 approval 발급 포트가 없으므로 요청 approval_id로 효과를 실행할 수 없다. 후속 구현은 승인 1회 소비·Outbox/operation 접수·감사를 같은 Store 트랜잭션으로 처리하는 전용 경로가 있어야 한다. 이 준비 전에는 APPROVAL_REQUIRED로 가짜 승인 요청을 만들거나 provider를 호출하지 않는다.

최소 시험은 method/tool 불일치, unknown 필드, 미등록/만료/철회, 다중 계정 부분 scope, 감사 start 실패, 정책 거부, send 차단, 서비스 오류 및 실제 성공 수치, terminal 실패 후 무재시도, 민감 입력 비반향이다. 실제 인증 재검증은 Windows pipe/CNG 시험으로, coordinator 순서는 합성 포트로 검사한다. bootstrap의 모든 미인증 mail.* 거부는 유지한다.

Core의 private execution_adapter는 같은 connection-owned 세션과 열린 Store만 차용한다. Store 등록을 매번 다시 읽고 CNG 공개 키 검증을 통과한 RegisteredClient로 변환한다. 등록 누락/읽기 오류는 세션을 닫고 원래 안정 오류를 유지한다. 감사 ID는 core CSPRNG에서 만들고 request ID를 사용하지 않는다. 등록/인증 wire를 연결했더라도 현재 check_policy는 항상 POLICY_BLOCKED다. read_only/rate_limits 정책·승인 서비스가 준비되기 전에는 로컬 계정 읽기까지 기본 거부하며 fake allow/seed 등록은 제품에 연결하지 않는다.
