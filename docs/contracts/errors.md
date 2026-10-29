# 오류 계약 v1

상태: 2026-10-29 연속 개발 위임에 따른 구현 기준. 사람 보안 검토 필요. 원문 §14.3의 코드 의미를 보존한다.

기계 코드와 CLI 종료 코드는 언어에 따라 바꾸지 않는다. 예외·OS 오류·사용자 입력·메일 본문을 오류 메시지에 그대로 넣지 않는다. 사용자 안내는 영어·한국어 리소스에서 선택한다. 아래 번호는 처음 정의한 고정 값이며 이후 추가만 허용한다.

| 코드 | 종료 코드 | 의미 | 자동 재시도 |
| --- | ---: | --- | --- |
| SUCCESS | 0 | 성공 | 해당 없음 |
| INVALID_INPUT | 2 | 잘못된 형식·필수 입력 누락 | 아니오 |
| AUTH_REQUIRED | 10 | 계정 로그인·재인증 필요 | 아니오 |
| SCOPE_DENIED | 11 | 계정 OAuth 범위 밖 | 아니오 |
| POLICY_BLOCKED | 12 | 앱·조직 정책으로 거부 | 아니오 |
| CONFLICT | 13 | 버전 충돌 | 아니오 |
| RATE_LIMITED | 14 | 요청 한도 | 서버 안내와 요청의 멱등성 확인 후 |
| OFFLINE | 15 | 오프라인 | 요청의 멱등성 확인 후 |
| NOT_FOUND | 16 | 허용된 범위에 대상 없음 | 아니오 |
| SEND_UNKNOWN | 17 | 발송 접수 여부 불명 | **절대 자동 재발송 안 함** |
| APPROVAL_REQUIRED | 18 | 신뢰 채널의 사람 승인 필요 | 아니오 |
| APPROVAL_EXPIRED | 19 | 승인 만료 | 아니오 |
| APPROVAL_DENIED | 20 | 승인 거절 | 아니오 |
| APPROVAL_RATE_LIMITED | 21 | 승인 요청 한도 | 아니오 |
| CLIENT_NOT_REGISTERED | 22 | 인증된 활성 클라이언트 등록 없음 | 아니오 |
| CLIENT_SUSPENDED | 23 | 클라이언트 정지 | 아니오 |
| HUMAN_STEP_REQUIRED | 24 | 사람이 설정을 완료해야 함 | 아니오 |
| CORE_UNAVAILABLE | 25 | 코어가 준비되지 않음 | 조회/멱등 요청만 |
| VERSION_MISMATCH | 26 | 프로토콜·스키마 호환 불가 | 아니오 |
| CURSOR_EXPIRED | 27 | 커서 보존 범위 벗어남 | 상태 요약 후 새 조회 |
| CAPABILITY_UNSUPPORTED | 28 | 계정의 기능 미지원 | 아니오 |
| QUOTA_EXCEEDED | 29 | 제공자 용량·할당량 초과 | 원인 해결 후 |
| INTERNAL_ERROR | 70 | 공개할 수 없는 내부 실패 | 아니오 |

`INTERNAL_ERROR`는 OS·패닉 등 계약 외 실패를 원문 없이 알리기 위해 추가했다. 발송 도중 내부 오류로 결과가 불명확하면 이 코드로 감추지 않고 `SEND_UNKNOWN`을 사용한다. JSON의 `retryable`은 현재 요청이 안전하게 다시 실행될 수 있음이 확인된 경우에만 true다. 오류 코드 자체가 자동 재시도 권한을 주지 않는다.
