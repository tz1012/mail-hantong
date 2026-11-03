# 다계정 공통 메일 이용 구현 계획 — 2026-11-03

사용자 지정 완료 기준: 계정을 추가해도 기존/신규 계정에서 조회·본문 읽기·작성·발송·휴지통을 사용할 수 있다. Gmail과 일반 IMAP/SMTP를 기본 CLI/TUI에서 연결한다. 기능별 명령어나 내부 요청 ID를 조립하게 하지 않는다. 지원하지 않는 서버 기능은 정확히 표시하며 영구 삭제로 대신하지 않는다. 실제 서버의 제한과 로컬 자원 한도를 무제한 지원이라고 표현하지 않는다.

## 현재 상태와 구현 순서

| 묶음 | 현재 구현 | 이어서 할 일 | 검증 |
| --- | --- | --- | --- |
| Gmail 다계정 작성·발송 | C→다계정 C 터미널 기능 선택, 128계정 범위, 계정별 단일 발송 결속 구현 | 통합 검사·패키지 검증·실제 사용자 OAuth 확인 | 실제 Store 계정 추가·epoch 변경·독립 claim, CLI 재개/취소/인증 오류 |
| 일반 메일 작성·발송 | Store SMTP endpoint·Secrets 발송·Core 보호 발송 및 터미널 확인 존재 | guided compose의 Gmail 전용 probe와 개인 preview 가정을 공통 메일 발송 경로로 연결 | 두 SMTP 계정 선택, 잘못된 계정/발신 주소 거부, 확인 전 outbox 없음, unknown 무재전송 |
| 통합 계정 메뉴 | 기본 메뉴는 Gmail만 표시·연결 | IMAP/SMTP 계정 표시, 일반 메일 연결·SMTP 연결, 공통 받은편지함·본문·작성 선택 | 순서 독립·계정 추가 후 기존 계정 보존, EOF/취소·headless 무효과 |
| Gmail 휴지통 | AI 범위에서 단일 메일 trash와 Modify·터미널 확인 구현 | 일반 CLI 기능 선택과 메뉴·메일 선택에서 같은 도구를 연결, 변경 grant 후에도 읽기/발송 유지 | Modify 전 거부, 한 메일 확인·취소·변조·오래된 epoch, 다른 계정 영향 없음 |
| 일반 IMAP 휴지통 | 미구현 | UID MOVE capability·Trash 선택·새 durable 단일 시도 원장·터미널 확인·Secrets 전송을 연결 | UIDVALIDITY 교체, 다른 UID/폴더, NO/끊김/부분 응답, 동시 클라이언트·unknown 무재시도 |
| TUI 공통 기능 | 기존 목록 중심 TUI | 위 공통 서비스의 계정/작성/휴지통 경로를 TUI 키 동작으로 노출 | 동일 Core 권한·preview·confirm 경로, 별도 창 없음 |

## 공통 경계

계정 ID를 항상 명시하고 다른 계정 권한·인증정보를 재사용하지 않는다. 작성은 초안을 만들며 발송은 불변 전체 preview를 본 뒤 별도로 확인한다. 휴지통 이동 역시 대상과 목적지를 확인한다. credential은 Secrets 안에서만 열고, Core가 현재 등록·계정 버전·epoch·내용 결속과 단일 claim을 확인한다. UI 선택은 OS 사람 존재 증거가 아니다. 실제 로그인·동의·보호 입력은 사용자 단계다.

일반 IMAP 휴지통의 기준은 [RFC 9051 MOVE](https://www.rfc-editor.org/rfc/rfc9051.html#section-6.4.8), [RFC 6851](https://www.rfc-editor.org/rfc/rfc6851.html), [SPECIAL-USE RFC 6154](https://www.rfc-editor.org/rfc/rfc6154.html)다. 계정+원본 mailbox+UIDVALIDITY+UID와 명시 Trash 목적지를 preview에 결속한다. MOVE를 지원하지 않는 서버에는 mutating 명령을 보내지 않는다. COPY/STORE/EXPUNGE로 조용히 폴백하지 않는다. 결과가 불확실하면 자동 반복하지 않고 새 확인 전 상태를 해소한다. 일반 SMTP는 휴지통 프로토콜이 아니며 짝지어진 IMAP 계정에서 이동한다.

각 묶음은 계약·실제 구현·종단 합성 시험이 연결돼야 완료다. 하나의 Gmail 회귀 수정이나 parser만으로 공통 메일 지원 완료를 표시하지 않는다. 진행 결과는 PROGRESS.md에 누적한다. 보안 코드 사람 검토 필요.
