# 초기 계정 읽기 정책 v1

상태: M1-09/CLI-15/AI-05/SEC-03 구현 기준. 사용자 연속 개발 위임에 따른 일반 구현 선택이며 **사람 보안 검토 필요**다. 전체 정책·발송 승인을 구현했다는 의미가 아니다.

## 허용 범위와 입력

현재 정책이 허용할 수 있는 도구는 `mail.accounts.list` 하나다. 등록의 read_only 값이 true 또는 false여도 이 읽기만 허용한다. `mail.send.execute`는 두 값 모두 금지한다. 새 도구는 명시적 정책 추가 전 기본 거부한다. 계정 추가·등록/권한 변경·외부 제공자 호출·승인을 이 정책으로 허용하지 않는다.

Core는 연결 소유 인증 세션과 최신 Store 등록을 재검증하고 전체 계정/도구 scope를 확인한다. 정책에 전달하는 등록 ID·read_only·requests_per_minute는 Store에서 읽으며 wire 필드를 사용하지 않는다. 정책 입력값이나 성공 반환은 다른 경로에서 재사용할 수 있는 인증/승인 증거가 아니다. 0 등록 ID·1..600을 벗어난 저장 한도는 INTERNAL_ERROR로 거부한다.

순서는 `execution.md`대로 입력 → 인증/등록 재검증 → canonical 계정 확인과 감사 시작 commit → scope → 정책/호출 한도 → 읽기 → 감사 terminal commit이다. 감사 시작 뒤 등록 scope에서 거부되면 한도를 소비하지 않고 감사 terminal에 기록한다. canonical 계정 자체가 없거나 감사 시작이 실패하면 감사 start/terminal을 만들지 않으며 정책과 서비스도 실행하지 않는다. 정책에서 RATE_LIMITED/POLICY_BLOCKED가 발생하면 감사 terminal에 같은 오류를 기록한다. 발송은 정책 이후에도 application coordinator에서 별도로 차단한다.

오류 우선순위는 모든 도구 공통이다. scope 밖 발송은 SCOPE_DENIED, scope 안이고 한도가 남은 발송은 1회 소비 후 POLICY_BLOCKED, scope 안이고 한도가 찬 발송은 무소비 RATE_LIMITED다. approval_id 값이 이 순서나 발송 차단을 바꾸지 않는다.

## 프로세스 내 호출 한도

`hantong-policy`의 장기 생존 `ReadPolicy`를 Core가 프로세스 시작 때 하나 만든다. 연결/요청마다 새 객체를 만들지 않는다. 현재 단일 thread Core가 독점 mutable borrow로 접근하며 병렬 정책 호출을 허용하지 않는다. 등록 ID별 단조 시계 `Instant`의 이동 60초 구간 `(now - 60초, now]`을 사용한다. 정확히 60초 전 기록은 만료된다. UTC·클라이언트 시각·계정 ID·profile epoch는 window key가 아니다. 서로 다른 등록은 독립이며 같은 등록의 여러 계정/연결/도구는 같은 한도를 소비한다.

정상 범위·최신 등록을 통과해 정책에 도달한 시도는 도구 허용 판정 전에 1회를 소비한다. 이후 POLICY_BLOCKED나 서비스/감사 실패가 나도 되돌리지 않는다. 현재 window의 개수가 최신 한도 이상이면 RATE_LIMITED로 거부하고 timestamp를 추가하거나 window를 연장하지 않는다. profile epoch·한도 변경으로 기존 사용량을 초기화하지 않으며 감소된 한도는 즉시 적용한다. rate의 기본값 60과 저장 범위 1..600은 등록 계약을 따른다.

최대 1,024개의 비어 있지 않은 등록 bucket과 bucket당 최대 600개 timestamp만 보관한다. 매 검사에서 만료된 기록/빈 bucket을 제거할 수 있다. 만료되지 않은 bucket을 용량 확보를 위해 버리지 않는다. 가득 찬 상태에서 새 등록을 받으면 QUOTA_EXCEEDED이고 기존 등록의 검사에는 계속 같은 규칙을 적용한다. 마지막 관측 Instant보다 시간이 역행하면 INTERNAL_ERROR로 거부하고 사용량을 변경하지 않는다. 정책에는 DB·네트워크·시계 직접 조회·비밀·파일 I/O가 없다.

상태는 메모리에만 있으며 Core 재시작은 한도를 초기화한다. 재시작을 가로지르는 한도나 자원 고갈 전체 방어를 보장하지 않는다. 거부된 호출도 감사 용량을 사용한다. 감사 기록을 한도 계산으로 재사용하거나 자동 삭제하지 않는다. 향후 영속 한도·승인 요청 한도(APR-07)·감사 보존은 별도 계약이다.

## 검증

최초 허용·정확한 60초 경계·등록 간 격리·여러 연결의 공유·도구 거부 시 소비·한도 초과 시 무증가·한도 변경·시간 역행·bucket 예산·만료 정리·재시작 초기화를 순수 시험으로 확인한다. Core 시험은 실제 Windows 인증 경로와 격리 Store를 사용해 성공 응답/실제 행 수 감사, scope/철회/epoch 거부, RATE_LIMITED와 발송 차단을 확인한다. 실제 사용자 프로필·외부 메일 계정은 사용하지 않는다.
