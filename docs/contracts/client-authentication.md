# 클라이언트 IPC 인증 v1

상태: M1-02 구현 기준, 사람 보안 검토 필요. 이 문서는 연결별 CNG challenge 응답 경계만 정의하며 등록 UI·영속 등록부·보호된 메서드 활성화를 의미하지 않는다.

클라이언트는 trusted profile store가 제공한 기존 CNG 키 이름과 서버 공개키, registration id, profile epoch, 요청 범위만 사용한다. `PendingClientAuthentication::begin`은 `NCryptOpenKey` 경로를 통해 키를 재개방하며 키를 생성하거나 등록하지 않는다. 연결 nonce와 현재 Windows user/logon SID 문맥은 내부에서 생성·조회하며 호출자가 주입할 수 없다. 개인 키 바이트·bearer token·승인 ID·직렬화 세션을 반환하지 않는다.

응답 객체는 연결별 nonce, 생성 시각, 현재 Windows peer context를 보관한다. 요청을 만들 때 nonce와 범위만 읽기 전용으로 확인할 수 있다. 서버 challenge의 registration id·epoch·client nonce·요청 범위·양쪽 peer context·server/client public-key hash·UTC 발급/만료 시각을 모두 비교한다. 발급 시각은 요청 시작 이후이고 현재 시각보다 미래일 수 없으며, 만료는 현재 시각 이후·발급 후 30초 이내여야 한다. 단조 시계도 30초 안에 있어야 한다.

검증 순서는 role=server canonical transcript 서명 확인 후 role=client canonical transcript 서명 생성이다. 서버 공개키가 다르거나 challenge가 변조·재생·만료·반사되면 `AUTH_REQUIRED`로 거부하고 pending 객체를 소비한다. 성공 결과는 64바이트 CNG ECDSA 서명 하나뿐이며, 전송·JSON handshake와 세션 권한 부여는 후속 계층의 책임이다.
