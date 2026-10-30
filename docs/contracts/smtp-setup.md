# SMTP 최초 설정 계약 초안

상태: 구현 초안, 사람 보안 검토 필요. `setup.smtp.request`와 `setup.smtp.status`는 기존 로컬 사람 설정 bootstrap 경계에서만 제공한다. MCP 도구 목록이나 AI 모델의 설정 도구로 노출하지 않는다. 같은 OS 사용자라는 사실은 설정 승인이 아니다.

요청은 `protocol_version:1`, `account_id`, `host`, `port`, `tls`(`implicit_tls` 또는 `starttls_required`), `login_name`만 받는다. 비밀번호·토큰·승인값을 IPC, CLI 옵션, 환경 변수로 받지 않는다. 계정은 이미 존재해야 한다. 서버 값은 IMAP 설정에서 추측하지 않는다.

코어는 계정 버전과 기존 SMTP 부재를 확인하고 5분의 임시 요청을 만든다. 상태 조회에는 요청 ID만 받는다. 응답은 `protocol_version:1`, `request_id`(64자리 hex), `expires_at_utc_ms`, `state`, `error_code`만 포함하며 주소·비밀값을 포함하지 않는다. 상태는 `pending`, `ready`, `denied`, `expired`, `failed`, `interrupted`다. 활성 요청은 최대 32개이며 UI는 기존 승인 큐와 직렬화한다. 재시작하면 임시 요청은 사라지고 이전 ID는 `NOT_FOUND`다. 자동 재실행·자동 복구·메일 발송은 없다.

보호된 네이티브 화면이 계정, SMTP 호스트·포트·TLS·로그인을 모두 표시하고 Secrets 소유 입력창에서 앱 비밀번호를 받는다. 운영체제 사람 확인이 성공한 단회 결과는 요청 ID·계정·설정·payload hash·capture nonce·기한에 결속된다. CLI 확인이나 모델 문장은 승인으로 취급하지 않는다.

승인 직후 계정 버전과 SMTP 부재를 다시 검증한다. Secrets 안에서 계정·SMTP endpoint에 결속된 비밀번호를 먼저 저장한 뒤 Store가 expected_version=0 CAS로 설정 버전 1을 발행한다. 설정 발행 실패 시 비활성 orphan 비밀이 남을 수 있으며 재시도에는 새 네이티브 확인이 필요하다. 기존 SMTP 설정은 변경하거나 비밀을 덮어쓰지 않는다. 동일한 설정과 유효한 로컬 결속 비밀이 이미 있으면 ready, 다른 설정이면 CONFLICT다.

개발 시험은 가짜 제공자를 사용한다. 실제 계정 로그인, 실제 비밀 입력, 실제 네트워크 접속 및 발송은 개발 중 수행하지 않는다. 구현의 보호 영역은 사람 보안 검토가 필요하다.

`ready`는 보호된 로컬 설정과 credential binding의 준비를 뜻한다. 이 설정 단계에서 SMTP 서버 인증 성공이나 메일 전달을 주장하지 않으며, 연결·인증은 이후 사람이 승인한 발송 작업에서 수행한다. 현재 From 주소가 SMTP 로그인과 같으므로 login_name은 유효한 ASCII 이메일 주소여야 한다.
