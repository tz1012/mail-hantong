# GUI 고유 등록과 로컬 메일 읽기

2026-10-30 연속 개발 구현 계약. M8-01, SEC-03, INST-04. **사람 검토 필요**. 기존 CLI 등록/프로필/승인 bytes와 scope 이력을 보존한다.

- GUI는 `ClientKind::Gui`, 고정 label `mail-hantong GUI`, 고정 profile `gui`, `profiles/gui.json`과 자신의 등록 ID에서 유도한 CNG key를 쓴다. CLI `default` 프로필을 빌리거나 fallback하지 않는다. 경로·pin·key·kind·scope는 GUI 인자로 받지 않는다.
- 최초 GUI 권한은 현재 Store 계정 ID의 명시 snapshot, 읽기 전용, 분당60회, 정확히 `mail.accounts.list`, `mail.folders.list`, `mail.messages.search` 세 도구다. 빈 계정은 접근0개다. 폴더 갱신·동기화·설정 쓰기·발송·승인 발급은 포함하지 않는다. 로컬 조회는 기존 공통 인증→scope→policy→audit→Store 경로를 사용한다.
- 등록 전 메서드 `registration.gui.request`는 `{"protocol_version":1}`, `registration.gui.status`는 여기에 기존32-byte canonical `request_id`만 추가한다. `RegistrationCommand::{GuiRequest,GuiStatus}`로 서버가 kind를 선택한다. 성공/오류/ACK/5초 예산은 기존 등록 계약과 같다. GUI status는 CLI 요청 ID를 조회하지 않으며 기존 CLI status도 GUI ID를 거부한다. 누락/중복/추가/타입 오류는 기존 엄격 parser로 거부한다.
- 요청은 사람 승인이나 인증 증거가 아니다. GUI의 명시 연결 동작만 request한다. startup/readiness/상태 조회는 자동 요청·Hello·권한 갱신을 시작하지 않는다. 같은 kind의 pending 요청은 원래 ID/TTL을 재사용한다. 다른 종류 작업이 승인 UI slot을 점유하면 RateLimited다. CLI와 GUI 등록은 각각 하나이며 다른 kind를 Conflict로 오인하지 않는다.
- 기존 native 승인 도우미에 GUI 종류·프로필·정확한 계정과 세 읽기 도구를 표시한다. `RegistrationReview::new_gui`는 별도 canonical domain으로 kind/profile/정확한 scope까지 결속한다. 기존 `new`의 CLI canonical bytes는 바꾸지 않는다. 발급 주체는 계속 Core가 호출하는 native 승인 도우미다. GUI·CLI·IPC에 승인 발급 API를 넣지 않는다.
- 승인 소비→현재 request/version/계정/pin 재검증→key 생성→provisioning 저장→고정 GUI 프로필 발행→active CAS 순서를 따른다. stage/deny/activate는 kind와 label과 정확한 scope를 함께 검증한다. 기존 CLI 메서드는 GUI 데이터를 거부한다. 공유 내부 구현을 사용할 수 있지만 공개 경계는 분리한다. 새 DB 표/마이그레이션은 필요하지 않다.
- 중단 복구는 저장된 kind에 맞춰 같은 프로필/selector만 재발행·활성화한다. 새 key 생성·다른 프로필 덮어쓰기·이력 변환은 하지 않는다. orphan 정리는 CLI/GUI 예약 모두를 검증하고 어느 kind든 연결된 key는 보존한다. label/scope 불일치·중복·손상은 실패로 보존한다.
- GUI profile codec은 정확한 세 도구만 허용한다. default codec은 과거 허용 집합을 유지한다. trusted loader는 각 고정 파일을 읽은 후 예상 profile selector가 일치하는지도 검사한다. 서로의 bytes를 잘못된 파일에 놓아도 인증하지 않는다. publisher의 기존 ACL/reparse/ancestor handle/원자 no-replace 검사를 GUI에도 적용한다.
- 검증은 격리 DB/파일/키 fixture와 fake native effects만 사용한다. 실제 사용자 등록·Hello·계정·제공자·메일은 조작하지 않는다. profile 혼동/재시작/provisioning/서명·scope 거부/CLI 회귀를 구현 후 통합 확인한다.
