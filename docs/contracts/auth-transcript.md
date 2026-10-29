# IPC 인증 서명 입력 v1

상태: M1-02 구현 기준, 사람 보안 검토 필요. 이 계약의 타입을 생성하는 것은 인증·등록·승인이 아니다. 제품의 보호된 메서드는 등록 UI와 영속 등록부 및 연결 세션이 연결될 때까지 계속 거부한다.

서명은 아래 canonical bytes의 SHA-256에 대한 Windows CNG ECDSA P-256이다. 공개 키는 72바이트 CNG `ECS1` public blob, 서명은 64바이트 CNG 출력이다. 개인 키는 내보내지 않는다. JSON 직렬화 문자열을 서명하지 않는다.

순서와 형식은 다음과 같다. 모든 정수는 little endian이며 문자열은 ASCII bytes의 u16 길이 + bytes로 쓴다. 문자열 뒤 NUL·padding은 없다. 총 입력은 32,768바이트 이하이다.

| 순서 | 값 | 길이/형식 |
| --- | --- | --- |
| 1 | domain separator | `mail-hantong/ipc-auth/v1` + NUL, 25 bytes |
| 2 | signer role | u8: 서버 1, 클라이언트 2 |
| 3 | protocol_version | u16, 1 |
| 4 | registration_id | 16 bytes |
| 5 | profile_epoch | u64, 1 이상 |
| 6 | issued_at / expires_at UTC ms | i64 각각, 0 이상, `issued < expires <= issued + 30000` |
| 7 | server_instance_id | 32 bytes, 매 코어 시작마다 새 OS 난수 |
| 8 | server_nonce / client_nonce | 각각 32 bytes, 연결마다 새 OS 난수 |
| 9 | server / client public blob SHA-256 | 각각 32 bytes, 등록 시 고정된 키 |
| 10 | server user SID / logon SID | 각각 길이 + canonical SID 문자열 |
| 11 | client user SID / logon SID | 각각 길이 + canonical SID 문자열 |
| 12 | 요청 account 목록 | u16 개수 + 각 account_id 길이/bytes |
| 13 | 요청 tool 목록 | u16 개수 + 각 tool ID 길이/bytes |

SID는 `S-1-` 뒤 decimal identifier authority(48비트 이내)와 1..15개 decimal subauthority(각 u32)로 표현한다. 불필요한 선행 0·공백·부호·Unicode 숫자를 거부한다. logon SID는 `S-1-5-5-X-Y` 형식이다. 서버가 읽고 impersonation으로 확인한 client OS context와 서버 자신의 context를 결속한다. 클라이언트는 자체 OS context와 기대하는 동일 로그인 서버 context를 대조하고 등록 때 고정한 서버 키의 서명을 확인한다. 요청에 넣은 SID·PID·이름 자체는 인증 증거가 아니다.

목록은 각각 최대 128개, 중복 없이 ASCII byte 오름차순이다. account ID는 기존 common schema의 `[A-Za-z0-9_-]{1,128}`, tool ID는 `[a-z][a-z0-9.]{0,95}`다. canonical builder는 순서를 정렬하고 중복은 거부한다. 빈 account 목록은 계정 접근 권한이 0개라는 뜻이다. 권한은 등록 당시 범위와 요청 범위의 교집합이며 도구 권한이 있어도 계정 권한을 추가하지 않는다. 전역 설정 도구는 계정 도구와 별도로 검증한다.

서버 서명을 먼저 확인한 클라이언트만 role=2 서명을 반환한다. 역할만 바꾼 동일 입력도 서명이 다르므로 반사 공격을 거부한다. 서버는 연결에 보관한 challenge를 성공·실패와 관계없이 한 번 소비하며 새 연결·다른 instance·만료·clock rollback·다른 epoch·서명 변조·범위 변경을 거부한다. UTC 만료와 별도로 단조 시계 30초 기한을 적용한다. 등록 철회·epoch와 현재 계정 범위는 각 도구 요청 직전에 재검사한다.

이 문서는 binary transcript를 정의한다. JSON handshake envelope, 영속 등록부와 승인 도우미 등록 화면은 후속 연결 범위이며 현재 공개 bootstrap 응답을 변경하지 않는다.

## Canonical decoder

클라이언트가 서버 challenge를 받을 때 `AuthTranscript::decode(bytes, expected_role)`로 같은 binary 형식을 읽는다. 입력 전체 32,768 bytes 상한, domain/version/역할, 각 문자열의 개별 길이·ASCII·SID 규칙, 목록 개수 128, 중복/정렬, 전체 소비를 확인한다. 임의 trailing bytes·잘린 필드·다른 역할은 거부한다. 재인코딩한 bytes가 입력과 정확히 같아야 하므로 비정규 순서를 정렬해 유효한 서명 입력으로 고쳐 주지 않는다. 길이와 개수를 확인한 뒤에만 메모리를 할당하고, offset 연산은 범위를 검사한다. 디코딩 성공은 자료의 형식 검증일 뿐이다. 고정 서버 공개 키 서명·nonce·계정 scope·OS 문맥·시각은 PendingClientAuthentication이 이어서 검증해야 한다.

`issued + 30000` 자체가 i64 범위를 넘는 비정상 시각도 거부한다. 현재 서버 구현의 pending 상태는 검증된 OS 연결을 직접 소유하며 복제·직렬화할 수 없다. 성공/실패 때 상태를 소비하고, 활성 세션에서 등록 제거·철회·epoch/키 변경·기존 권한 축소가 관찰되면 연결을 닫는다. 일반 요청의 범위 거부는 승인이나 발송 허가를 생성하지 않는다. live 등록부와의 연결은 아직 남아 있다.
