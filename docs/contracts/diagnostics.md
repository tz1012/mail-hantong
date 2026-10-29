# 구조화 진단 로그 계약 v1

M1-07 진단 기록은 한 줄 JSON이며 `schema_version`, 고정 `event`, `severity`, 호출자가 주입한 0 이상 UTC epoch milliseconds를 가진다. 선택 필드는 안정 오류 코드와 수치형 `count`·`duration_ms`뿐이다. 자유 텍스트·경로·계정·주소·제목·본문·토큰은 구조체에 들어갈 수 없다.

`redact_sensitive`는 신뢰할 수 없는 문자열을 전체 `<redacted>`로 바꾼다. 헤더·주소·토큰 일부를 보존하지 않으며 줄바꿈과 JSON 주입도 제거한다. 원인 설명은 안정 오류 코드와 사용자 문구로 제공한다. 감사(`M1-08`)의 요청자·도구·승인 연결은 별도 계약이다.
