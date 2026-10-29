# CLI 초기 경계 계약 v1

상태: 연속 개발 위임에 따른 구현 기준. M0-08 초기 거부 경계와 CLI-16 표시 기반. 인증 IPC·등록·메일 도구가 연결되기 전까지 기능 완료로 표시하지 않는다.

- `hantong --help`, `hantong --version`은 계정 정보를 포함하지 않고 종료 코드 0을 반환한다.
- `--lang en|ko`가 언어를 선택하며 생략 시 영어다. 첫 실행은 언어를 묻지 않는다. 저장된 `cli.language` 연결은 설정 카드에서 수행한다.
- `hantong accounts list`, `hantong send execute`, `hantong approvals get`는 보호된 명령이다. 초기 인증 미구현 상태에서는 `CLIENT_NOT_REGISTERED`로 거부한다. CLI가 승인 발급·토큰 입력·자동 등록을 제공하지 않는다.
- `--json`, `--no-input`, `--client <profile>`는 인식하되, profile/TTY/환경 변수/메일 내용은 인증이나 승인 증거가 아니다. 알 수 없는 옵션과 부족한 값은 `INVALID_INPUT`으로 거부한다. 비대화형 모드는 stdin을 기다리지 않는다.
- JSON 오류 응답은 `schema_version`, `ok`, `error: {code, message, retryable}`, `items`, `next_cursor`, `account_results`, `completeness`, `warnings`, `observed_at`을 포함한다. 오류 때 items/account_results는 빈 배열, next_cursor/observed_at은 null, completeness는 `unavailable`, retryable은 false다. JSON 데이터는 stdout에만 쓰고 사람용 오류는 stderr에 쓴다.
- 오류 메시지는 고정된 번역 리소스에서 가져오며 전달된 옵션값·프로필 이름·원문을 인용하지 않는다. 불필요한 데이터를 반환하지 않는다.
- `--help`는 현재 동작 가능한 초기 명령과 미구현 상태를 정확히 설명한다. 가짜 성공·가짜 계정·데모 메일을 실제 데이터처럼 반환하지 않는다.

이 초기 계약은 실제 IPC의 JSON-RPC 프레이밍·주체 인증을 정의하지 않는다. 향후 IPC 계약은 별도로 작성하고 보호 경로를 연결할 때 이 초기 거부 동작의 시험을 유지한다.

2026-10-30 확장: `--client`는 `client-profile.md`의 `default` 선택자만 허용한다. 다른 선택자·파일 경로는 INVALID_INPUT이다. `client-registration.md`에 따른 clients register/status/verify는 요청 제출·상태 조회·기존 프로필 상호 인증만 수행한다. 승인 발급 기능이 아니며 메일 실행 정책을 우회하지 않는다.
