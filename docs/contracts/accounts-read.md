# 계정 목록 읽기 계약

상태: M1-06/09의 로컬 읽기 서비스 구현 기준, 연속 개발 위임. 계정 추가·실제 로그인·제품 인증 wire 활성화와 별개다. 사람 보안 검토 필요.

`mail.accounts.list`는 토큰을 제외한 계정 표시 정보·상태·선택 기능 식별자를 반환한다. 요청은 기존 도구 스키마의 명시적인 1..128 계정이다. 이 숫자는 한 요청의 자원 한도이며 전체 저장 계정 수 제한이 아니다. 빈 scope·중복·잘못된 ID는 INVALID_INPUT이다. 서버가 모든 요청 계정의 현재 scope/정책을 확인한 뒤에만 이 저장소 서비스를 호출한다.

## 표시 자료

각 AccountSummary는 account_id, provider(gmail/graph/imap/archive), primary_address(nullable), display_name(nullable), color(nullable #RRGGBB), account_number(1..i64::MAX), status, 선택 capabilities를 가진다. provider_subject, granted_scopes, settings_version, credential 참조·토큰·원문은 조회하지 않는다. Debug는 모든 표시/식별 값을 숨긴다. DTO 자체는 신원·권한 증거가 아니다.

status는 ready, connecting, reauth_required, offline, disabled, error다. 캐시된 계정 상태이지 방금 외부 제공자와 연결 성공했다는 증거가 아니다. reauth_required/error 계정도 표시 행을 반환해 다른 계정 조회를 막지 않는다. archive 계정의 표시가 발송·동기화 권한을 주지 않는다. 이후 read_only·제공자 권한 모델은 해당 계정 설정 계약에서 추가한다.

주소는 최대 320 Unicode 문자/1280 UTF-8 bytes, 표시명은 128자/512 bytes다. NULL은 허용하지만 빈 문자열·공백만인 문자열·제어 문자는 거부한다. HTML로 해석하지 않는 표시 데이터다. capabilities는 요청 include_capabilities가 true일 때만 반환하며 최대 128개의 중복 없는 ASCII 식별자(1..96, 영숫자 및 . _ : -)를 정렬한다. 기능 광고는 권한/정책 검사를 대체하지 않는다.

## 저장소 조회

`Store::list_account_summaries(account_ids, include_capabilities)`는 Deferred 트랜잭션 하나에서 요청된 행만 읽어 AccountSnapshot(items, observed_at_utc_ms)을 반환한다. 숫자/enum/문자열은 길이와 형식을 확인한 뒤 복사한다. capabilities JSON은 최대 16 KiB, 문자열 배열만 허용하며 개수/중복을 검증한다. false일 때는 capabilities 데이터를 조회하지 않는다. 출력은 account_number, account_id 순이다. 관측 시각은 Store 내부 UTC다.

요청 계정이 없으면 NOT_FOUND이고 일부 결과를 성공으로 반환하지 않는다. 저장값 손상·DB 오류는 원문 없는 INTERNAL_ERROR다. 인증 실패를 나타내는 정상 상태 행과 손상된 저장 행은 구분한다. 조회로 계정/상태/설정/감사를 직접 변경하지 않는다. 감사는 공통 실행기에서 처리한다. 다른 OS의 Store는 CAPABILITY_UNSUPPORTED다.

## wire 연결 시 조건

core adapter는 신뢰된 AccountSnapshot으로 공통 response를 구성하고 실제 items 수만 감사 수치로 반환한다. observed_at은 UTC RFC3339, next_cursor는 null이다. items는 정렬된 모든 표시 행이고 account_results는 같은 순서로 각 계정의 status=complete와 items=[해당 표시 행]을 가진다. 여기의 complete는 로컬 표시 자료 조회 완료이며 계정 자체 status와 다르다. metadata는 source=local_store, cached=true다. optional capabilities만 미요청 시 생략하며 나머지 nullable 표시 필드는 null을 유지한다.

저장소의 전체 조회는 서비스 내부에만 있으며 외부에서 인증·정책을 건너뛸 수 없다. 직렬화된 tool response의 한도는 1,040,000 bytes이며 1 MiB IPC frame에서 JSON-RPC wrapper/id 공간을 남긴다. 넘으면 QUOTA_EXCEEDED로 반환하고 임의 잘림·부분 성공을 하지 않는다. 호출자는 명시적 계정 집합을 더 작은 요청으로 나눌 수 있다. 인증 wire 연결과 core adapter는 별도 검증으로 이어간다.
