# Message body envelope metadata proposal

상태: 구현 초안, 사람 검토 필요.

`mail.messages.get`와 Gmail 메시지 목록/본문 응답에 선택적 `headers` 객체를 추가한다. 객체는
`subject`, `from`, `to`, `cc`, `date`의 선택 문자열만 포함하며, 각 값은 원문
메일에서 추출한 신뢰할 수 없는 표시 데이터다. 인증·계정 범위·승인·발송 정책에는
사용하지 않는다.

각 값은 최대 8KiB, 전체 객체는 최대 32KiB로 제한한다. 헤더가 없거나 형식이
손상된 경우 기존 본문 조회 성공을 실패로 바꾸지 않고 `headers: null`로 둔다.
기존 응답에서 `headers`가 없는 경우도 허용해 구버전 Core/클라이언트와 호환한다.
새 필드는 기존 계정 범위, UID 결속, 본문 크기 제한과 동일한 요청에서만 반환한다.
Gmail 캐시의 `message_json`에도 같은 선택 필드를 저장하며, 필드가 없는 기존
캐시는 `headers: null`로 읽는다. 저장 스키마와 메시지 식별자는 변경하지 않는다.

영향 범위: 메일 본문 표시, IMAP RFC822 header projection, `mail.messages.get`
응답 검증. 저장 스키마와 권한 범위는 변경하지 않는다. 제목·주소·날짜는
터미널에 출력될 때 별도의 표시용 sanitization/charset 처리를 거친다.
