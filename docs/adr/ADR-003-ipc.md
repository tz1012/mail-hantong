# ADR-003 로컬 IPC

상태: 2026-10-29 연속 개발 위임에 따른 구현 결정. **사람 보안 검토 필요**.

Windows는 명시적 사용자 SID ACL을 적용한 로컬 named pipe와 길이 접두 JSON-RPC 2.0을 사용한다. Linux 개발용 소켓은 테스트 경로로 분리하고 Mac 제품은 별도 OS 인증 구현 후 제공한다. HTTP 서버는 1단계 기본 전송에 추가하지 않는다.

gRPC는 IDL과 streaming 도구가 풍부하지만 HTTP/2·protobuf 런타임 및 추가 생성 경로가 필요하다. 명명된 파이프는 OS 사용자를 확인하고 외부 포트를 열지 않으며, JSON 도구 계약과 직접 연결할 수 있다. 대신 ACL, 이름 선점, 프레임 크기·timeout·세션 검증을 제품이 직접 시험해야 한다.

JSON은 `serde_json 1.0.154`를 채택하며 문자열 결합이나 자체 JSON parser로 확장하지 않는다. 실제 사용 시 lockfile과 제3자 고지를 함께 갱신한다. 비동기 전송은 Tokio 최소 기능만 켜며 초기 연결 상한과 bounded queue를 계약에 따른다. 실행기 도입은 실제 IPC 구현 카드에서 검증한다.

[IPC 계약](../contracts/ipc-protocol.md), [호출 주체 인증](ADR-019-client-auth.md)을 함께 적용한다. 보호된 호출은 인증·권한 구현과 시험을 마칠 때까지 거부한다. 이 문서 작성이 보호 IPC의 활성화나 보안 검토 완료는 아니다.

근거: [Microsoft named pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights), [serde_json](https://docs.rs/serde_json/1.0.154/serde_json/).

기동 구현은 Tokio 1.54.1의 `rt`, `net`, `io-util`, `time`, `signal`, `macros` 중 실제 사용 기능만 켠다. Windows 실측에서 같은 handle의 disconnect/connect 재사용은 Mio에 남은 이전 EOF를 다음 연결에서 읽었다. 연결마다 새 pipe handle을 만들고 별도 Global named mutex의 객체 수명으로 단일 코어를 유지하도록 수정했다. 대기·mutex 소유권에 의존하지 않아 async 스레드 이동과 소유권 해제가 섞이지 않는다. 반복 연결 시험에 이 회귀를 고정한다.

Tokio ClientOptions의 GENERIC_WRITE가 pipe-instance 생성 비트까지 요구하므로 사용하지 않고 명시적 파일 접근권으로 열어 Tokio에 소유권을 넘긴다. `FILE_READ_ATTRIBUTES`도 CreateFile이 요구하므로 DACL/client mask에 추가한다. 이 Windows FFI와 SID·ACL 처리는 `hantong-platform`에만 둔다. 이 crate는 계약 외 제품 계층을 참조하지 않고 IPC 양쪽이 공유한다. 근거: [Tokio 1.54.1 named pipes](https://docs.rs/tokio/1.54.1/tokio/net/windows/named_pipe/index.html), [Microsoft .NET pipe access rights](https://source.dot.net/System.IO.Pipes/System/IO/Pipes/PipeAccessRights.cs.html).
