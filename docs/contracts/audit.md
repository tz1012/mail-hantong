# 영속 감사 기록 v1

상태: M1-08 구현 기준, 연속 개발 위임. 사람 보안 검토 필요. 감사 저장은 인증·승인·발송 성공의 증거를 발급하지 않는다. 보호된 제품 도구는 공통 정책 실행 경로가 완성되기 전까지 계속 비활성이다.

## 범위와 타입

초기 도구는 `mail.accounts.list`, `mail.send.execute` 두 enum 값이다. 임의 도구 문자열·자유 텍스트 details·원문 요청·주소·제목·본문·첨부·provider 응답·비밀을 받지 않는다. 요청자 이름은 승인된 등록의 표시명만 저장하며 최대 128자/512 bytes, 제어 문자·공백만인 값은 금지한다. 모든 감사 타입의 Debug는 execution/registration/account ID와 표시명을 숨긴다. 도구별 target·실제 approval·Outbox 연결은 해당 원장이 구현될 때 추가하며 입력 approval_id를 그대로 감사에 복사하지 않는다.

`AuditBegin`은 코어 CSPRNG가 만든 execution ID(32 bytes), 인증된 연결에서 얻은 registration ID(16 bytes), profile epoch(1..i64::MAX), 도구 enum, 1..128개의 중복 없는 canonical account ID를 가진다. 계정은 기존 ASCII 형식과 길이 제한을 따르며 정렬 보관한다. 호출자는 신뢰 저장소/실행기에서 확인한 ID만 전달하고 원문 params를 전달하지 않는다. 이 타입은 인증 principal이 아니며 wire에서 역직렬화하지 않는다.

`AuditTerminal`은 실제 서비스 반환값으로 만든 returned_ok(선택 result_count 0..i64::MAX) 또는 returned_error(기존 ErrorCode)다. 문자열 성공 주장·클라이언트 보고·AI 문장을 받지 않는다. SEND_UNKNOWN은 명시적 returned_error이고 terminal이 없는 Unresolved와 구분한다. 어느 상태도 자동 재실행을 지시하지 않는다.

## DB v3

v2 전체 DDL을 보존한 후 v1→v2→v3 순서로 검증·암호화 백업·단일 단계 트랜잭션을 수행한다. 각 단계는 자기 버전의 원본과 새 DDL을 확인한다. 2→3 실패 시 이미 성공한 v2까지는 보존할 수 있다. 다음 실행은 저장된 버전을 검증하지만 같은 버전의 백업이 이미 있으면 CONFLICT로 중단한다. 명시적 복구 전에는 자동 재시도·백업 삭제·backup-before-v1/v2 덮어쓰기를 하지 않는다.

- `audit_capacity`: singleton=1, retained_executions, retained_account_refs. 초기 0이며 start 삽입과 같은 트랜잭션에서 증가한다.
- `audit_starts`: 증가 sequence, 고유 execution ID, 등록 ID FK RESTRICT, profile epoch, 시작 당시 client kind/표시명, tool enum, Store UTC 시작 시각.
- `audit_start_accounts`: execution ID FK RESTRICT + account ID의 복합 PK. 계정 삭제 뒤에도 이력을 남기므로 accounts FK는 두지 않는다.
- `audit_terminals`: execution ID PK/FK RESTRICT, Store UTC 완료 시각, 결과 종류, 안정 오류 코드 또는 NULL, 선택 결과 수. 성공이면 오류 NULL, 실패이면 오류 존재 및 결과 수 NULL이다.

Store는 시작 트랜잭션에서 검증한 등록부의 종류·표시명을 감사 행에 복사한다. 요청자가 AuditBegin으로 이 값을 제공하지 않는다. AuditRecord의 AuditActor는 그 스냅샷을 반환하고, 과거 조회는 현재 등록 종류·표시명을 조인해 대체하지 않는다. 등록 철회·메타데이터 손상 뒤에도 당시 요청자를 유지한다. 이름/종류 변경에는 새 등록 ID를 쓰며, 등록 철회 tombstone은 감사가 참조하는 동안 삭제하지 않는다.

## 저장 API와 경계

`begin_audit`는 Immediate 트랜잭션에서 중복 execution ID, 등록 존재·active 상태·현재 epoch, 용량을 검사하고 start/계정 참조를 삽입한다. commit 성공 뒤에만 반환한다. 중복은 CONFLICT, 미등록은 CLIENT_NOT_REGISTERED, 비활성은 CLIENT_SUSPENDED, epoch 불일치는 AUTH_REQUIRED다. 시간은 Store 내부 UTC이며 요청자가 주입하지 않는다.

현재 존재하는 canonical 계정만 시작 기록에 넣고, 없는 계정은 SCOPE_DENIED로 기록 변경 없이 거부한다. 이 확인은 등록의 허용 scope/정책 검사를 대신하지 않는다. 저장 API는 `begin_audit(&AuditBegin) -> sequence`, `finish_audit(execution_id, AuditTerminal)`이며 sequence와 execution ID는 레코드 선택자다. 권한을 나타내는 bearer handle을 만들지 않는다.

보존 예산은 실행 100,000건·계정 참조 1,000,000건이다. 둘 중 하나라도 초과하면 QUOTA_EXCEEDED로 아무 변경 없이 실패한다. terminal 한 건의 논리 공간은 start 때 예약한 것으로 취급해 terminal 추가에 별도 개수 제한을 적용하지 않는다. 실제 디스크 오류는 INTERNAL_ERROR이며 성공으로 감추지 않는다. 물리 공간을 미리 할당한다는 뜻은 아니다.

`finish_audit(execution_id, terminal)`은 시작 행이 있어야 하며 terminal은 INSERT만 한다. 없는 시작은 NOT_FOUND, 중복 결과는 CONFLICT다. UTC가 시작보다 역행하면 오류로 남기고 기존 start를 보존한다. 과거 결과를 UPDATE하거나 자동으로 성공/실패로 채우는 API는 없다.

`load_audit`는 ID로 한 기록, `list_audit_page(before_sequence, limit)`는 sequence 내림차순으로 최대 100개를 반환한다. before는 배타적이며 limit 1..100·sequence 양수 범위를 검증한다. 각 조회는 start·계정·terminal을 같은 snapshot에서 읽고 길이·개수·enum·시각·FK를 검증한다. 계정 참조는 정렬한다. terminal이 없으면 결과를 Unresolved로 표현하며 이를 실패나 성공으로 추정하지 않는다. Store 결과 자체는 사용자에게 공개된 API가 아니며 후속 인증/정책 검사를 거친 조회 화면에서만 제공한다.

## 실행기와 후속 원장

일반 읽기 경로는 엄격한 입력→연결 인증/최신 등록→canonical 대상 확인→감사 시작 commit→scope/정책 검사→서비스→서버 반환값 감사 결과 순서다. 무인증·손상 입력은 원문을 보관하지 않고 기존 제한된 진단 이벤트만 사용한다. 감사 시작 실패 시 서비스를 호출하지 않는다. 결과 저장 실패를 서비스 미실행으로 오인해 자동 재시도하지 않는다.

발송에는 이 두 API를 독립 호출하는 것으로 충분하지 않다. 후속 전용 Store 입장 트랜잭션에서 approval 1회 소비·Outbox/operation 접수·감사 start를 같이 처리하고 결과 갱신도 원장과 감사 terminal을 같이 처리한다. 중간 크래시의 재개 판정은 Outbox가 담당한다. 정책·승인·원장 전에는 발송 효과를 연결하지 않는다.

자동 pruning은 제공하지 않는다. M9-14 보존 정책은 terminal이 존재하는 오래된 실행만 명시적 정책으로 제한된 batch에서 정리하며 unresolved·활성 원장 참조는 보존한다. 요청 중 용량 확보를 위해 기록을 자동 삭제하지 않는다. 쓰기 connection은 journal_mode=DELETE·synchronous=EXTRA를 설정하고 값을 검증한다. 전원 장애 내구성은 VFS·OS·저장장치의 flush 보장에 의존하며 설정 시험만으로 절대적인 전원 장애 안전을 주장하지 않는다.

v3 시작 검증은 capacity singleton과 카운터 범위를 확인하고 실제 start/계정 참조 행 수와 대조한다. 누락·불일치 시 자동으로 카운터를 재계산하거나 기록을 삭제하지 않고 시작을 거부한다. 정상 API는 한 트랜잭션에서 두 카운터와 실제 행 수를 함께 유지한다.


## 읽기 전용 폴더·UID 작업 확장

v10은 `mail.folders.list`와 `mail.folders.refresh`, v12는 `mail.sync.page`를 enum allow-list에 추가한다. UID 도구의 cursor/status도 같은 enum으로 감사하며 request만 원래 audit 소유권을 영속 작업 원장에 넘긴다. 공개 JSON의 action과 mailbox/intent는 각 검증된 계약으로 처리하고 감사에는 원문을 복사하지 않는다. 범위/정책 거부는 이번 audit에만 terminal을 기록한다. 성공 effect와 원래 audit terminal은 해당 Store transaction으로 원자 저장한다. 미완료 audit를 startup이나 status 조회로 재실행하지 않는다. 상세는 [폴더 도구](imap-folder-tools.md)·[UID 영속 작업](imap-uid-jobs.md)을 따른다.
