# 계약 문서 모음

상태: 2026-10-29 연속 개발 위임에 따른 초기 문서화. 계약 변경은 사람 검토가 필요하다. 이 파일과 스키마가 존재한다고 해서 해당 API가 제품에서 활성화되었거나 수용 기준을 통과한 것은 아니다.

## 적용 순서

1. [오류 코드](errors.md)와 [CLI 종료 코드](cli-exit-codes.md)는 모든 클라이언트가 공유한다.
2. [IPC 경계](ipc-protocol.md)는 전송·프레이밍·등록 전 거부 규칙을 정의한다. 실제 인증 구현 전에는 보호된 호출을 허용하지 않는다.
3. [논리 데이터 모델](logical-data-model.md)은 저장소·도메인·감사·승인 객체의 의미와 경계를 정의한다.
4. `tools/*.schema.json`은 요청·응답의 JSON 구조를 정의한다. 공통 envelope를 먼저 적용하고 각 도구 스키마의 `tool` 상수와 `params` 제약을 함께 검증한다.

## 파일 상태

| 파일 | 범위 | 현재 상태 |
| --- | --- | --- |
| `common.schema.json` | 요청/응답 envelope, 계정 ID, 오류 | 초안, 활성화 전 |
| `mail.accounts.list.schema.json` | 명시된 계정 프로필 조회 | 초안, 활성화 전 |
| `mail.send.execute.schema.json` | 승인 결속 발송 의도 접수 | 초안, 활성화 전 |
| `logical-data-model.md` | 27개 논리 엔터티와 키·보안 경계 | 초안, 활성화 전 |
| `database.md`, `db-schema.sql` | 암호화 DB 키 경계·초기 스키마·백업과 마이그레이션 | 구현·검증 진행, 사람 검토 필요 |
| `query-language.md` | 검색 문법·계정 범위·기간·완전성 | 초안, 활성화 전 |
| `config.md`, `config.schema.json` | 언어·테마·시간대와 보호 설정·CAS | 도메인·암호화 저장·코어 시작 연결 검증, 인증된 설정 IPC 미연결 |
| `app-paths.md` | OS Known Folder와 고정 데이터/키 경로 | Windows 구현·시작 연결 검증 |
| `auth-transcript.md`, `cng-identity.md` | 정규 서명 입력·CNG 키 경계 | 구현·검증 중, 보호된 IPC 활성화 전 |
| `client-authentication.md` | 서버 pin·OS context·nonce와 단일 응답 | Windows 양측 서명·등록/프로필 검증, 실제 Hello·사람 검토 전 |
| `auth-wire.md` | bounded JSON handshake·한 연결 한 도구 요청·bootstrap 분기 | 실제 Windows pair 검증, Core 등록 조회/기본 거부 실행기 연결 |
| `registration-store.md`, `migrations/` | v1→v2 이전·서버 신원·클라이언트 등록 저장 | 기본 등록 전이/복구 자동 검증, 사람 검토 전 |
| `client-registration.md`, `registration-review.md`, `client-profile.md` | 기본 CLI 요청/상태·신뢰 창·공개 프로필과 중단 복구 | 구현·자동 통합 검증, 실제 Hello/사람 검토 전 |
| `audit.md` | DB v3 감사 시작/불변 결과·용량·페이지·미확정 기록 | 저장 API 대상 시험 통과, 공통 실행기/효과 연결 전 |
| `tool-validation.md` | 공통 envelope와 계정 조회/발송 의도 입력 검증 | 순수 검증 구현 중, 실제 실행·승인과 별개 |
| `execution.md` | 인증 재검증·감사·전체 범위·정책·읽기 실행 순서 | Core/wire·계정 읽기 정책 연결, 발송 차단·사람 검토 필요 |
| `accounts-read.md` | 계정 표시 DTO·범위 한정 DB 조회·엄격한 CLI 응답 검증 | 실제 인증·Store·응답 decoder 통합 시험, 사람 검토 필요 |
| `policy-read.md` | 등록별 읽기 허용·단조 시계 호출 한도·재시작 의미 | 정책/실제 연결 시험, 사람 검토 필요 |
| `mail-connection.md`, `mail-connection-store.md` | 독립 IMAP/SMTP 설정·v4 저장/CAS/중복/관측 | 모델·저장·v4 이전 검증, 설정 화면 전 |
| `mail-credentials.md` | endpoint 전체·버전에 결속한 DPAPI 자격 증명 | Windows 저장/결속 대상 시험·독립 검토 통과, TLS/AUTH 연결 전 |
| `smtp-probe.md` | Secrets 내부 일회성 TLS/AUTH 검사·수신/시간 제한 | 실제 DPAPI+in-memory TLS 대상 검증, GUI·IPC 미연결 |
| `imap-probe.md` | Secrets 내부 IMAP TLS/AUTH·tag/응답 한도·기능 snapshot | 실제 DPAPI+in-memory TLS 대상 검증, GUI·IPC/메일 수신 미연결 |
| `core-lifecycle.md` | 고정 HKCU 자동 시작 값·충돌 보존·재시작 예산 | 격리 registry/상태 시험, 설치·supervisor 연결 전 |
| `diagnostic-export.md` | 선택한 비민감 진단 항목의 미리보기/내보내기 값 | 순수 직렬화 검증, 실제 수집·UI·파일 쓰기 미연결 |

스키마의 `additionalProperties: false`는 알 수 없는 권한·승인 필드를 조용히 허용하지 않기 위한 것이다. `client_id`, `role`, `approved`, `session_id`를 요청 본문에 넣는 것으로 인증·승인이 만들어지지 않는다. 계정 범위를 생략하거나 빈 배열로 보내면 전체 계정으로 해석하지 않고 `INVALID_INPUT`으로 거부한다.

스키마는 JSON Schema Draft 2020-12를 사용한다. 실제 JSON-RPC 프레이밍·중복 키·최대 크기·인증 세션 검사는 [IPC 계약](ipc-protocol.md)의 런타임 경계가 담당한다.
