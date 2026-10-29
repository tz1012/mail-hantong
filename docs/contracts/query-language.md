# 통합 검색 문법 계약 v1

상태: M0-11 초안, **사람 검토 필요**, API 활성화 전. 관련 요건은 `MAIL-03`(검색과 범위 명시)와 도구 추가분이다. 이 문서는 파서·검색 엔진의 기준을 제안한다. 구현·제공자 SDK·로컬 색인 형식은 ADR-005와 M2-08에서 확정한다.

## 검색 요청의 바깥 범위

검색 문자열은 계정 범위를 정하지 않는다. 호출자는 공통 도구 envelope에서 다음 중 하나를 반드시 보낸다.

- `account_id`: 한 계정
- `account_ids`: 하나 이상의 계정. 배열은 비어 있을 수 없다.

인증된 세션의 허용 계정과 요청 범위의 교집합만 검색한다. 요청 계정 범위가 비어 있거나 허용 계정과 교집합이 없으면 `INVALID_INPUT` 또는 `SCOPE_DENIED`를 반환한다. 검색 문자열에 `account:`, `all:`, `scope:` 같은 계정 선택자를 허용하지 않는다. 따라서 메일 본문이나 검색 결과에 들어 있는 문장이 계정 범위를 넓힐 수 없다.

요청에는 `schema_version: 1`, 사용자 표시용 `timezone_id`(IANA 시간대), 그리고 선택적인 불투명 `cursor`가 포함된다. `timezone_id`가 없거나 유효하지 않으면 기간 표현을 해석하지 않고 `INVALID_INPUT`을 반환한다. 시간대는 서버·CLI·GUI가 임의로 바꾸지 않는다.

## 어휘와 문법

입력은 유효한 UTF-8 문자열이며 길이는 16,384바이트 이하로 제한한다. AST 깊이는 16단계, AST 노드는 512개, 토큰 수는 512개를 넘을 수 없다. NUL(U+0000)과 ASCII 공백·탭을 제외한 모든 제어 문자는 거부한다. ASCII 공백(U+0020)과 탭(U+0009)은 토큰 사이의 공백으로만 허용하며 줄바꿈·기타 제어 문자는 허용하지 않는다. Unicode 정규화는 검색 전에 수행하지 않는다. 저장된 색인과 제공자 검색의 정규화 방식이 다르면 결과의 `warnings`와 `completeness`에 표시한다.

아래 표기에서 `WS`는 ASCII 공백 또는 탭 하나이고 `WS1`은 하나 이상의 `WS`다. `field`는 아래 허용 필드 이름 중 하나이고, `word_char`는 공백·괄호·따옴표·콜론·백슬래시를 제외한 Unicode 문자다. `safe_char`는 따옴표와 백슬래시를 제외한 Unicode 문자다. 키워드 `AND`, `OR`, `NOT`, 필드명, 열거형 값만 ASCII 대소문자를 구분하지 않는다. 주소·제목·본문·라벨·태그 같은 텍스트 값은 입력 문자를 보존하고 제공자·로컬 색인의 정해진 비교 규칙을 따른다. 특히 주소의 local-part를 임의로 소문자화하거나 Unicode 정규화하지 않는다.

```ebnf
query       = or_expr ;
or_expr     = and_expr , { WS1 , "OR" , WS1 , and_expr } ;
and_expr    = not_expr , { WS1 , [ "AND" , WS1 ] , not_expr } ;
not_expr    = ( "-" , primary ) | ( "NOT" , WS1 , primary ) | primary ;
primary     = term | "(" , { WS } , or_expr , { WS } , ")" ;
term        = phrase | word | field_term ;
field_term  = field , ":" , field_value ;
phrase      = '"' , ( escaped_char | safe_char ) , { escaped_char | safe_char } , '"' ;
word        = ( escaped_char | word_char ) , { escaped_char | word_char } ;
field_value = phrase | word ;
escaped_char= "\\" , ( "\\" | '"' | ":" | "(" | ")" | "-" ) ;
field       = "from" | "to" | "cc" | "bcc" | "subject" | "body"
            | "attachment" | "label" | "tag" | "folder" | "status"
            | "is" | "has" | "classified" | "after" | "before" | "on" ;
```

`AND`가 `OR`보다 우선하고, `NOT`이 가장 우선한다. 괄호가 없는 인접한 두 항은 `AND`다. `-term`과 `NOT term`은 같은 뜻이다. 빈 괄호, 단독 연산자, 닫히지 않은 따옴표, 이스케이프되지 않은 문법 문자는 거부한다. 알려지지 않은 필드·연산자와 값이 없는 필드도 `INVALID_INPUT`이다. UTF-8·길이·AST 깊이·노드 수·토큰 수·NUL/제어문자 제한을 어긴 입력도 `INVALID_INPUT`이며 부분 AST를 실행하지 않는다. 따옴표 밖에서 단독으로 쓰인 `AND`, `OR`, `NOT`은 예약된 연산자이며 일반 텍스트로 검색하려면 `"AND"`, `"OR"`, `"NOT"`처럼 인용한다. 오류에는 입력 원문이나 메일 원문을 넣지 않는다.

어휘 분석기는 따옴표 밖의 단독 `AND`, `OR`, `NOT`을 일반 단어보다 먼저 예약어로 인식한다. 검색어 자체로 쓰려면 반드시 따옴표로 감싼다. 따옴표 안의 `:`·괄호·`-`는 값으로 취급한다. 예를 들어 `subject:"A:B"`와 `from:"홍길동"`은 유효하다. 일반 단어의 `*`, 정규식, SQL 조각, 링크 방문·첨부 해석은 v1 문법에 없다. 일반 단어는 아래의 기본 텍스트 집합에서 토큰 또는 구문으로 검색한다.

## 필드와 값

필드의 값은 한 개의 단어 또는 따옴표 구문이다. 필드별 값의 의미는 다음과 같다.

| 표현 | 의미와 값 |
| --- | --- |
| `from:`, `to:`, `cc:`, `bcc:` | 주소·표시 이름. 주소 비교는 제공자 주소 식별자와 Unicode 검색 색인을 함께 사용하며, 결과에는 일치한 필드를 표시한다. |
| `subject:` | 제목 검색 |
| `body:` | 일반 텍스트 본문 검색. HTML 태그 자체는 검색어가 아니다. |
| `attachment:` | 첨부 파일명·MIME 유형·첨부 메타데이터 검색. 첨부 전문은 기본 검색 대상이 아니다. |
| `label:` | 서버 라벨 이름. 계정 안에서만 해석하며 로컬 태그와 섞지 않는다. |
| `tag:` | 로컬 태그 이름. 서버 라벨과 섞지 않는다. |
| `folder:` | 계정의 폴더·보관함 이름. 제공자의 특수 폴더 매핑을 사용한다. |
| `status:` | `draft`, `scheduled`, `spam`, `trash` 중 하나. 임시보관함·예약메일함·스팸함·휴지통을 명시적으로 고른다. |
| `is:` | `read`, `unread`, `favorite`, `attachment` 중 하나. `is:favorite`는 별표/즐겨찾기 상태를 뜻한다. |
| `has:` | 현재 v1에서는 `attachment`만 허용하며 `is:attachment`와 같은 AST를 만든다. |
| `classified:` | 로컬 분류. v1 값은 `advertising` 하나이며 자동 삭제·구독 해지 의미가 없다. |
| `after:`, `before:` | `YYYY-MM-DD` 또는 따옴표로 감싼 RFC 3339 시각. 아래 기간 규칙을 따른다. |
| `on:` | `YYYY-MM-DD` 민간 날짜만 허용한다. RFC 3339 시각은 `on:`에 사용할 수 없다. |

필드가 없는 `word`·`phrase`는 제목·주소·일반 텍스트 본문·첨부 파일명·서버 라벨·로컬 태그의 검색 가능한 색인에 대해 OR로 평가한다. `body:`처럼 명시한 필드는 다른 데이터로 확장하지 않는다. 검색 결과에서 `account_id`와 내부 메시지 링크를 항상 함께 반환한다.

`status:`와 `is:`는 같은 의미를 중복해서 표현하지 않는다. 예를 들어 `status:draft`는 `is:unread`와 결합할 수 있지만 `status:unknown`, `is:sent`는 거부한다. `classified:advertising`은 분류가 없는 메일을 포함하지 않는다.

## 기간과 시간대

날짜 값은 사용자의 `timezone_id`에 있는 해당 민간 날짜의 시작을 포함하고 다음 날짜의 시작을 제외한다. 따라서 `on:2026-10-29`는 `[2026-10-29T00:00, 2026-10-30T00:00)`이다. `on:`은 민간 날짜 전체를 뜻하고 timestamp를 뜻하지 않는다.

- `after:VALUE`는 `VALUE` 이상이다.
- `before:VALUE`는 `VALUE` 미만이다.
- `on:DATE`는 해당 민간 날짜의 반열린 구간이다.
- 두 표현을 함께 쓰면 교집합이다. `after:2026-10-01 before:2026-11-01`은 10월 전체다.

날짜만 쓰면 `timezone_id`로 해석한다. 시각을 포함한 값은 RFC 3339 offset을 포함해 따옴표로 감싼다(예: `after:"2026-10-29T09:00:00+09:00"`). 콜론·더하기 기호가 있는 timestamp를 일반 unquoted `word`로 해석하지 않는다. offset이 없는 지역 시각은 DST 겹침·존재하지 않는 시각을 숨길 수 있으므로 거부한다. 날짜 경계를 계산할 때 현지 자정이 존재하지 않거나 두 번 존재하면 `INVALID_INPUT`으로 거부한다. 시간대 규칙상 민간 날짜 전체가 건너뛰어진 날짜도 `INVALID_INPUT`이다. 그 밖의 날짜 안 DST 전환은 시간대 규칙에 따라 반열린 구간으로 계산한다. 임의의 한 시간을 선택하거나 누락 날짜를 빈 결과로 바꾸지 않는다. 범위의 시작이 끝보다 늦거나 초 단위 정밀도가 지원되지 않으면 `INVALID_INPUT`이다. 반환 결과에는 해석한 `timezone_id`와 기간 경계를 메타데이터로 기록한다.

## 제공자 처리와 완전성

파서는 먼저 제공자와 무관한 AST로 변환한다. 각 계정에 대해 `ProviderCapabilities.search`와 색인 상태를 확인한 후 다음 규칙을 적용한다.

1. 제공자가 해당 AST를 정확히 표현할 수 있으면 서버 검색을 사용한다. 서버가 검색 범위를 보장하지 않으면 완전한 결과로 표시하지 않는다.
2. 제공자가 지원하지 않는 필드·구문은 동기화된 로컬 색인에서 동일한 AST를 평가한다. 로컬 색인이 없거나 오래되어 범위를 증명할 수 없으면 `account_results.status: unavailable` 또는 `partial`로 반환한다.
3. 서버와 로컬 결과를 합칠 때 내부 메시지 위치 키로 중복 제거한다. 다른 계정의 같은 제목·Message-ID·thread ID를 합치지 않는다.
4. 일부 계정 오류, 미동기화 기간, 제공자 결과 제한, 로컬 색인 누락이 있으면 응답 `completeness`는 `complete`가 아니다. `warnings`와 계정별 상태에 원인을 고정된 코드로 기록한다.
5. 서버가 지원하지 않는 `NOT`, `OR`, 기간 경계 또는 필드를 근사해서 넓은 결과를 반환하지 않는다. 정확한 로컬 평가가 불가능하면 결과를 `unavailable`로 두고 누락·근사를 성공으로 포장하지 않는다.

`complete`는 요청 범위의 모든 계정과 요청 기간에 대해 서버·로컬 색인의 검색 범위를 확인했고 오류·미동기화·제공자 제한이 없다는 뜻이다. 검색 결과가 0건이라는 뜻이 아니다. `partial`은 일부 범위만 검사했다는 뜻이며, `unavailable`은 안전한 결과를 계산하지 못했다는 뜻이다.

## 커서와 안정성

`next_cursor`는 불투명한 값이며 다음 정보를 서버가 내부적으로 결속한다: 정규화된 AST의 해시, 명시적 계정 집합의 정렬된 해시, `timezone_id`, 각 계정의 색인/동기화 버전, 제공자 cursor, 만료 시각. 클라이언트는 cursor 내용을 해석하거나 수정하지 않는다.

같은 요청 범위·시간대·문법 해시가 아니거나 색인 버전이 바뀌면 cursor를 재사용하지 않는다. 이 경우 `CURSOR_EXPIRED`를 반환하고 새 검색을 요구한다. 결과 정렬·페이지 크기·계정별 상태도 cursor에 결속한다. 커서가 없으면 첫 페이지이며, 결과가 더 없으면 `next_cursor: null`이다.

## 예시

```text
from:alice@example.com after:2026-10-01 before:2026-11-01
subject:"회의 일정" AND (is:unread OR tag:중요)
status:draft OR status:scheduled
NOT classified:advertising has:attachment
after:"2026-10-29T09:00:00+09:00"
```

`has:attachment`은 `is:attachment`의 읽기 쉬운 별칭으로 문서·파서에서 같은 AST를 만든다. 별칭을 추가하는 경우에도 결과 의미를 넓히지 않는다.

다음은 거부해야 한다.

```text
account:other-account subject:비밀       # 계정 범위 선택자
status:unknown                          # 열거형 밖의 값
after:2026-10-29T01:30:00               # quote와 offset이 모두 없음
subject:(회의                          # 닫히지 않은 괄호
foo:                                    # 값 없는 알 수 없는 필드
```

## 이후 시험 매트릭스

M2-08 구현 시 최소한 다음을 요건 ID가 붙은 시험으로 고정한다.

| 영역 | 시험 |
| --- | --- |
| 파서 | UTF-8·한글 구문, 따옴표/escape, 연산자 우선순위, 암시적 AND, 괄호, 빈 입력·중복 연산자·미완성 구문 거부 |
| 필드 | 각 허용 필드와 열거형, 알 수 없는 필드·계정 선택자·값 없는 필드 거부, 로컬 태그와 서버 라벨 분리 |
| 시간 | 날짜 반열린 구간, RFC 3339 offset, DST 겹침·존재하지 않는 시각, 잘못된 범위와 잘못된 IANA 시간대 |
| 범위 | 명시적 계정 교집합, 빈 목록 거부, 다른 계정 제목·Message-ID·thread ID 혼합 금지 |
| 제공자 | capability별 정확한 변환, unsupported의 로컬 fallback, 색인 누락·일부 오류에서 `partial/unavailable`, 근사 결과를 `complete`로 표시하지 않음 |
| 커서 | AST·계정·시간대·색인 버전·정렬 결속, 변경 후 `CURSOR_EXPIRED`, 불투명 값 변조 거부 |
| 결과 | 모든 항목의 `account_id`·메시지 링크, 경고·계정별 상태, 0건과 `complete`의 구분 |

이 계약은 검색만 정의한다. 결과를 이동·삭제·태그 적용·발송에 사용하는 변경은 별도의 preview·approval·operation 계약을 거쳐야 하며 검색 문자열만으로 권한이나 승인을 만들 수 없다.
