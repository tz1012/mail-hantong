# IPC 인증 wire v1

상태: M1-02 후속 구현 기준, 연속 개발 위임. 사람 보안 검토 필요. trusted client profile/등록 UI를 우회하거나 기본 active client를 만드는 계약이 아니다.

## 단일 연결 순서

OS preface/impersonation → auth.begin → 서버 challenge → auth.finish → authenticated 확인 → 보호 RPC 한 건 → receipt 0x06 → close. 기존 system.version/readiness bootstrap은 요청 한 건/응답/receipt 동작을 유지하며 별도 dispatcher를 사용한다. 인증된 경로에서 auth.begin 재호출·다른 등록으로 전환·system 메서드 재사용은 거부한다.

모든 패킷은 기존 little-endian 길이+UTF-8 JSON-RPC frame이다. 요청 envelope는 기존 strict decoder를 통과해야 한다. 중복/미지 필드·BOM·trailing JSON·잘못된 자료형을 허용하지 않는다. auth params/응답은 아래 필드만 허용한다. 이진 값은 소문자 0-9a-f hex이며 길이 확인 후 디코딩한다. 문자열은 키·토큰의 경로가 아니다.

`auth.begin` params: protocol_version=1, registration_id(32 hex), profile_epoch(1..i64::MAX integer), client_nonce(64 hex), scope={accounts:[ASCII IDs],tools:[ASCII tool IDs]}. 목록은 각각 최대 128, 중복 없이 canonical 오름차순이다. 빈 계정은 0개 권한이며 전체 계정이 아니다. role/public_key/client_kind/label/verified 필드는 받지 않는다.

서버는 registration_id로 현재 Store 등록 snapshot을 읽고, wire epoch 일치를 확인한다. transcript의 공개 키/epoch/상태/허용 범위는 Store만 결정한다. 미등록·비활성·epoch 불일치는 각각 CLIENT_NOT_REGISTERED/CLIENT_SUSPENDED/AUTH_REQUIRED, DB 손상/IO는 INTERNAL_ERROR이며 숨겨서 다른 오류로 바꾸지 않는다. 이 오류는 서버 경계의 결과 코드다. 실패한 handshake의 transport는 폐기하며 현재 wire는 실패 사유 응답을 보내지 않는다. 클라이언트는 실패/EOF를 안정 AUTH_REQUIRED로 표시하고 원문·등록 정보를 반사하지 않는다.

서버 challenge의 result는 protocol_version=1, transcript_hex(서버 역할 canonical transcript, decoded 최대 32768 bytes), server_signature_hex(128 hex)다. 서버 key와 instance nonce는 코어 startup에서 유지한 값, challenge nonce/시각은 서버 내부 값이다. 클라이언트는 bounded hex→AuthTranscript::decode(Server)→pinned server key 및 모든 의미 결속 검증 후에만 서명한다. 서버가 반환한 키를 pin으로 자동 채택하지 않는다.

`auth.finish` params는 signature_hex(128 hex) 하나다. 같은 connection의 PendingAuthentication만 소비한다. 서버는 finish 직전 Store 등록을 다시 읽고 현재 key/epoch/state/scope로 검증한다. 실패한 pending은 재사용할 수 없다. 성공 result는 protocol_version=1, authenticated=true뿐이다. challenge/finish 응답에는 receipt를 보내지 않고 다음 phase의 request를 보낸다. 마지막 보호 RPC 응답에만 receipt 0x06을 보낸다.

## 시간과 종료

현재 코어 dispatcher는 첫 frame(bootstrap 또는 auth.begin)에도 131072 bytes 상한을 적용한다. auth.begin이 아니면 기존 최소 bootstrap 응답만 사용하고 인증 callback/Store 등록 조회를 호출하지 않는다. auth.begin은 현재 Store 등록과 상호 서명 검증을 거친 뒤에만 보호 요청 단계로 이동한다. 등록 생성·승인 UI·client profile 배포는 아직 연결하지 않았으며 Core 정책은 기본 거부다. 등록 fixture의 wire 성공을 제품 계정 접근 허용으로 해석하지 않는다.

예외적으로 첫 frame의 auth.finish/다른 auth.*는 순서 위반이므로 bootstrap 응답 없이 닫는다. Store/CNG/현재 실행 어댑터의 INTERNAL_ERROR는 일반 연결 오류로 삼키지 않고 core까지 전파해 서비스 루프를 종료한다. 사용자 입력 형식·서명·범위·연결 오류는 해당 연결만 닫고 다음 접속을 받는다. 향후 외부 공급자 오류는 공급자별 안정 코드로 분리하고 내부 상태 오류를 정상 실패로 숨기지 않는다.

OS preface 2초, begin frame/응답 5초, challenge 발급부터 finish 검증/성공 응답까지 30초 단조 기한, 보호 RPC 수신/처리/응답/receipt 5초다. 실제 I/O timeout을 적용하며 형식 오류·timeout·서명 실패·등록 재검증 실패는 연결을 폐기한다. 보호 RPC는 connection-owned 세션으로 공통 실행기를 한 번만 통과한다. 응답 손실 뒤 도구 자동 재실행은 없다.

처리 code는 서버 내부 안정 오류로 남기고 wire에서 외부 자료/예외를 출력하지 않는다. begin/finish JSON-RPC id는 각 응답과 일치해야 하며 인증 권한이 아니다. 각 auth frame 최대 131072 bytes, 성공 challenge의 transcript_hex 최대 65536자다. 전체 IPC frame 상한보다 작은 별도 한도다.

## CLI 프로필과 제품 연결

인증된 클라이언트 wrapper는 연결을 소비하는 exchange 한 번만 공개하며 raw transport를 내보내지 않는다. method-specific decoder 전의 응답 bytes는 신뢰하지 않는 자료다. receipt는 bytes를 읽었다는 표시이며 응답 내용의 신뢰/권한 승인이 아니다. 서버도 인증된 연결을 소비하는 execute_once에서 보호 요청 하나만 받는다. trusted core callback은 공통 실행기와 같은 연결의 ExecutionContext를 구성한다. callback을 요청 데이터/플러그인으로부터 받지 않는다. 현재 동기 Store 작업은 강제 중단하지 않으며, 5초 경과를 작업 전후 확인하고 기한 초과 응답/재실행 없이 연결을 닫는다. I/O 대기는 실제 timeout으로 취소한다.

client helper는 신뢰된 프로필 resolver에서 registration ID/epoch, 기존 CNG key selector, pinned server key, 요청 scope를 받는다. CLI 옵션으로 raw ID/키 이름/공개 키/epoch를 덮어쓰지 않는다. --client는 프로필 선택자이고 프로필 누락 시 자동 등록/TOFU/fake 성공을 하지 않는다. resolver·등록 UI가 준비되기 전 CLI의 보호 호출은 계속 거부한다. wire parser/fixture 시험을 제품 등록 완료로 표시하지 않는다.
