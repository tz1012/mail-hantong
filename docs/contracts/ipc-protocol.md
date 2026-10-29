# IPC 계약 v1 — 초기 연결 경계

상태: 연속 개발 위임에 따른 구현 기준, **사람 보안 검토 필요**. 관련 SEC-03, CLI-06·15, MCP-01·03. 전송과 등록이 미구현인 현재 보호된 제품 명령은 거부한다.

## 전송과 자원 한도

- Windows named pipe만 제품 기본 전송으로 사용한다. 현재 로그온 SID에 필요한 read/write 권한을 명시하고 원격 클라이언트를 거부한다. DACL의 client 권한에 불필요한 pipe instance 생성 권한을 포함하지 않는다.
- 첫 server instance 생성 플래그로 이름 선점을 감지하고 실패하면 시작을 중단한다. 다른 이름으로 몰래 우회하지 않는다. 연결 후 OS impersonation token의 SID를 확인한다. SID 일치는 등록 인증을 대신하지 않는다.
- 프레임은 unsigned 32-bit little-endian 바이트 길이 + UTF-8 JSON이다. 길이는 1..1,048,576바이트다. BOM·잘못된 UTF-8·압축·뒤따르는 JSON/쓰레기·중복 object key를 거부한다. JSON nesting 한도 64, 한 요청에 하나의 JSON-RPC object만 허용한다.
- 프레임 수신 5초, 인증 challenge 30초, 연결당 동시 요청 1개, 프로세스당 연결 16개, 응답 queue 32개를 초기 상한으로 둔다. 큰 결과는 cursor로 나눈다. 상한을 넘으면 안정 오류/연결 종료를 사용하고 무제한 버퍼를 만들지 않는다.
- JSON value node는 프레임당 최대 16,384개다. 짧은 숫자/빈 배열을 매우 많이 넣어 바이트 상한보다 훨씬 큰 객체 메모리를 만들려는 입력도 거부한다.
- JSON-RPC `jsonrpc`는 `2.0`, `id`는 1..64자 문자열이다. notification·batch는 초기 범위에서 거부한다. 알 수 없는 envelope 필드와 method를 거부한다. 변경 요청의 멱등성 키는 JSON-RPC id와 별개다.

## 등록 전 응답

공개 method는 버전·민감 정보 없는 readiness·등록 요청 제출/대기 상태 확인뿐이다. readiness에 계정 수·메일 수·주소·오류 원문을 넣지 않는다. 인증 handshake는 이 최소 연결 절차의 일부다. 등록 요청은 승인을 발급하지 않고 opaque pending ID만 반환한다.

`mail.accounts.list`를 포함한 모든 `mail.*` 호출은 서버가 만든 활성 세션이 없으면 `CLIENT_NOT_REGISTERED`로 거부한다. 요청 본문의 `approved`, `role`, `client_id`, `session_id`, `--client`, TTY 주장으로 세션이나 권한을 생성하지 않는다.

## 인증과 세션 결속

서버는 연결별 CSPRNG 32바이트 nonce와 instance ID·유효기간을 발급하고 한 번만 소비한다. 클라이언트와 서버 모두 등록 시 고정한 상대 공개 키를 확인한다. 서명 대상은 JSON 문자열이 아니라 [auth-transcript.md](auth-transcript.md)의 고정 binary transcript다. [CNG 키 계약](cng-identity.md)의 키·서명 구현과 연결별 상태 시험은 별도 검증한다. JSON handshake wire envelope와 영속 등록부·승인 도우미가 연결되기 전에는 제품 인증을 활성화하지 않는다.

transcript에는 protocol version·server/client nonce·server instance·registration ID·profile epoch·만료·양측 OS peer context·요청 범위를 결속한다. 연결을 바꾸거나 항목이 하나라도 다르면 거부한다. 권한은 등록된 계정/도구 집합과 요청 범위의 교집합이며 빈 계정 목록은 전체가 아니다. 인증된 세션은 직렬화하지 않고 해당 transport 객체에만 연결한다. 철회·epoch 변경은 다음 요청 전 검사하고 활성 연결을 종료한다.

## 일반 도구 envelope

요청은 명시적 account_id 또는 비어 있지 않은 account_ids, request_id, schema_version, tool ID를 가진다. 변경에는 operation_id·expected_version·payload_hash가 필요하다. 외부 발송에는 신뢰 채널에서 발급한 approval_id가 필요하며 CLI/MCP에는 발급 API가 없다.

읽기 결과는 items·next_cursor·account_results·completeness·warnings·observed_at을 포함한다. 메일 원문은 untrusted_content 아래에 두고 정책/실행 메타데이터와 구분한다. 오류는 [errors.md](errors.md)를 따른다. raw exception·비밀·원문을 오류에 넣지 않는다. 필드 단위 JSON Schema는 M0-05 도구 schema 작업에서 작성하고 검증한 method부터 활성화한다.

## 구현/검토 경계

### 코어 기동과 최소 응답 구현 범위

OS peer 검사 전에 전송 계층이 고정 preface byte `0x01`을 읽는다. Windows impersonation은 마지막으로 읽은 메시지의 보안 문맥을 쓰므로 연결만 한 상태에서는 peer 검사를 수행하지 않는다. preface 수신은 2초 안에 끝나야 하며 틀린 값·끊김·timeout은 연결을 폐기한다. 정상 JSON 응답을 전부 검증한 클라이언트는 receipt byte `0x06`을 쓰고 닫는다. 서버는 이 receipt까지 5초 요청 제한 안에서 기다린다. 수신 확인은 전송 종료를 위한 값이며 사람 승인·인증·발송 성공의 증거가 아니다. [Windows disconnect 문서](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-disconnectnamedpipe)의 미수신 데이터 폐기를 피하기 위한 처리다.

초기 전송은 사용자 SID로 정한 고정 파이프 이름 `\\.\pipe\mail-hantong-v1-<user SID>` 하나를 사용한다. DACL은 현재 로그온 SID에만 read/write data·read attributes·synchronize (`0x100083`)를 허용하며 `FILE_CREATE_PIPE_INSTANCE`는 제외한다. `CreateFile`의 파이프 접근에 read attributes가 필요한 점을 반영한다. 클라이언트는 `SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT`로 연결한다. 같은 사용자 다른 로그온 세션도 허용하지 않는다. 연결 후 impersonation으로 사용자·로그온 SID를 대조하고 성공·실패·unwind 모두에서 원래 스레드 토큰으로 복귀한다. 복귀 자체가 실패하면 프로세스를 중단한다. 이 OS 경계는 등록된 클라이언트의 신원 인증을 대신하지 않는다.

동시 처리 수는 초기 1개다(앞의 16은 상한). 이전 연결의 비동기 read-ahead/EOF가 다음 연결로 넘어가지 않도록 연결 종료 때 pipe handle을 폐기하고 매번 `FIRST_PIPE_INSTANCE`로 새로 만든다. 별도 `Global\mail-hantong-v1-<user SID>-instance` named mutex의 **객체 존재**를 프로세스 수명 동안 유지한다. 이 객체도 명시적인 로그온 SID DACL만 가지며 초기 mutex 소유권·wait는 사용하지 않는다. 이미 존재하거나 접근이 거부되면 `CONFLICT`로 종료한다. pipe를 갱신하는 사이에도 다른 코어를 시작하지 못한다. 이전 I/O 취소가 완료되기까지 최대 2초 재시도하되 새 pipe 생성은 항상 첫 인스턴스 플래그를 유지한다. 다른 프로세스가 pipe 이름을 선점하면 다른 이름으로 우회하지 않고 실패한다. 이 간격의 이름 선점은 서비스 거부 잔여 위험이며 보호된 메일 경로 활성화에는 별도의 서버 키 인증이 필요하다.

명령 `mail-hantong-core run`은 foreground로 실행하며 Ctrl+C/Ctrl+Break 때 요청을 중단하고 파이프를 닫는다. 도움말·버전은 OS 초기화 없이 실행한다. 현재 Windows 외 플랫폼은 실행 시 `CAPABILITY_UNSUPPORTED`로 거부한다. 초기화·종료 상태는 비밀값 없는 고정 이벤트만 기록한다.

초기 허용 method는 `system.version`과 `system.readiness`뿐이다. 둘 다 params는 `{\"protocol_version\":1}`이며 다른 필드는 거부한다. 버전이 다르면 `VERSION_MISMATCH`, 잘못된 형식은 `INVALID_INPUT`이다. `system.version`은 제품명·제품 버전·protocol_version을, `system.readiness`는 `{\"transport_ready\":true,\"mail_ready\":false,\"authenticated\":false,\"protocol_version\":1}`만 반환한다. 이 응답은 아직 서버 등록 키로 인증되지 않았으므로 메일 작업 가능이나 신뢰된 서버 인증을 의미하지 않는다. `mail.*`는 전부 `CLIENT_NOT_REGISTERED`, 나머지 method는 `INVALID_INPUT`으로 거부한다. 이 bootstrap 경로는 등록 제출·계정 정보·메일 동작을 활성화하지 않는다. 현재 코어는 별도 [인증 wire](auth-wire.md)의 auth.begin을 같은 파이프에서 분기한다. Store의 현재 등록·서버 신원과 상호 서명 검증을 통과해야 보호 요청 단계로 이동하며, 등록 생성 UI/프로필 배포·정책 허용은 아직 제공하지 않는다. 첫 frame 상한은 bootstrap/auth 공통 131072 bytes, 인증 뒤 도구 frame은 기존 1MiB다.

응답은 같은 길이 프레임 안의 JSON-RPC `{jsonrpc,id,result}` 또는 `{jsonrpc,id,error:{code,message}}`다. code는 오류 계약의 문자열이고 message는 고정 영어 오류 코드다. 정상 파싱된 요청의 id만 응답에 반영한다. 프레임/파싱 실패는 응답 없이 연결을 닫아 유효하지 않은 id를 반사하지 않는다. 한 접속은 요청/응답 한 쌍 후 닫으며 수신·처리·응답을 포함해 5초 timeout을 적용한다. CLI의 `hantong core status`는 이 최소 응답만 읽는다. timeout·손상 응답·접속 불가는 원문 없이 `CORE_UNAVAILABLE`로 보고한다. 보호된 메일 명령은 계속 거부한다.

현재 CLI bootstrap의 자체 거부 시험은 OS transport나 등록 프로토콜의 검증이 아니다. M1-02에서 ACL·CNG·프레임·철회·부정 호출을 실제 Windows 환경에서 시험한다. 테스트용 등록 fixture는 제품 생성자나 feature 우회로 남기지 않는다.

## 2026-10-30 등록 제출·상태 확장

위 bootstrap 초기 구현에 `client-registration.md`의 두 메서드를 추가한다. 요청은 고정 default CLI 등록만 제출하며 계정/도구/승인/키/pin 필드를 받지 않는다. 반환하는 ID/진행 상태는 인증 증거가 아니다. 발급은 코어 소유 신뢰 창, 파일 발행은 Core 전용 경계에만 있고 CLI/MCP에는 승인 발급 API가 없다.
