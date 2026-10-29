# 공통 도구 요청 검증 v1

상태: M1-09의 입력 경계 구현 기준, 사람 보안 검토 필요. 검증된 요청은 인증·정책·사람 승인 증거가 아니다. 제품의 보호된 호출은 등록과 인증 wire가 연결되기 전까지 계속 거부한다.

## 범위

`hantong-application`은 `tools/common.schema.json`과 도구별 schema를 만족하는 입력을 타입으로 변환한다. 현재 대상은 `mail.accounts.list`와 `mail.send.execute` 두 가지다. 미지원 도구와 알 수 없는 필드는 `INVALID_INPUT`이다. send 입력을 파싱하더라도 발송·승인 발급 함수를 호출하지 않는다.

입력은 IPC의 길이·UTF-8·중복 key·깊이·노드 수 검사를 통과한 JSON Value다. 이 타입 변환기를 raw JSON parser 대신 사용하지 않는다. 네트워크 경계는 항상 엄격한 프레임 decoder를 먼저 사용한다. 타입에는 인증된 client_id, 역할, 세션, approved 필드가 없다. Debug는 요청 ID·계정·초안·작업·승인·해시 값을 출력하지 않는다.

## 필드 검증

- envelope는 `schema_version`, `request_id`, `tool`, `params` 네 필드만 허용한다. 버전은 정수 1이다. 요청 ID는 1..64 ASCII 문자 `[A-Za-z0-9._:-]`다.
- 계정 ID는 1..128 ASCII 문자 `[A-Za-z0-9_-]`다. accounts.list는 `account_id` 하나 또는 `account_ids` 1..128개 중 정확히 하나를 받는다. 중복·빈 배열·null·두 필드 동시 지정은 거부한다. 검증 후 계정 목록은 정렬해 보관하며 입력 순서를 권한 의미로 쓰지 않는다. 선택 `include_capabilities`는 boolean만 허용하고 기본 false다.
- send.execute는 account_id, draft_id, operation_id, expected_version, payload_hash, approval_id, idempotency_key를 모두 요구한다. ID들은 1..128 ASCII 문자 `[A-Za-z0-9._:-]`이며, expected_version은 1..9223372036854775807 정수다. payload_hash는 정확히 64자리 ASCII 16진수를 32바이트로 변환한다. 대소문자에 따른 다른 hash를 만들지 않는다.
- JSON 숫자를 문자열·boolean·소수에서 정수로 강제 변환하지 않는다. 잘못된 입력과 오류에 원문을 반사하지 않는다.

## 후속 실행기 경계

후속 IPC 조립 지점은 바깥 JSON-RPC method와 envelope tool이 같은지도 확인한다. 인증된 연결의 주체·최신 등록부·요청 권한 교집합을 검사하고, 감사·정책·승인 검사를 거친 뒤에만 도메인 서비스를 호출한다. 요청 타입만으로 서비스를 실행하는 우회 API를 만들지 않는다. 현재 검증기는 순수 변환이며 네트워크·저장소·공급자 의존성이 없다.
