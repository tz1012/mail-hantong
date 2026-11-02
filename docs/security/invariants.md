# 보안 불변 조건 — 구현 현황

2026-11-02 최신 정정: 개인 Google CLI는 [terminal-personal-send](../contracts/terminal-personal-send.md)의 터미널 전체 preview와 명시적 보내기 선택을 사용한다. 아래 개인 네이티브 확인창 지침은 대체됐다. prepare는 창/발송을 예약하지 않고, confirm은 불변 account/review/hash 및 현재 owner/epoch/계정/권한을 검증한 뒤 단일 claim만 진행한다. 이 선택은 OS 사람 존재 증명이 아니며 같은 사용자 프로그램의 CLI 키 사용을 구별하지 못한다. 일반 RPC/JSON에서 자동 확인하거나 다른 등록으로 권한을 확장하지 않는다. **사람 보안 검토 필요**.

상태: 구현 경계별 증거, **사람 검토 필요**. 이 목록은 완성된 보안 증명이 아니다. 최신 전체 실행 결과는 `docs/latest/PROGRESS.md`에서 확인한다.

2026-11-02 사용자 결정: 일반 개인 CLI 계정 연결은 추가 한통/Hello 승인 대신 검증된 Google OAuth 완료에 결속한다. [개인 CLI 계약](../contracts/google-personal-cli.md)의 관찰된 OS 호출자·계정·모드·만료·일회 소비와 고정 기능 제한을 따른다. 기능 선택·기존 계정·클라이언트 요청은 권한 증거가 아니다. 발송/대량 변경의 사람 승인과 AI/MCP 별도 활성화는 유지한다. 이 변경은 **사람 보안 검토 필요**이며 사용자 UX 결정으로 보안 검토를 대체하지 않는다.

| 불변 조건 | 현재 증거 | 후속 검증 |
| --- | --- | --- |
| 이름·TTY·본문으로 권한을 얻지 못함 | CLI bootstrap의 미등록 accounts/send 거부, `cli15_client_name_cannot_authenticate_and_unknown_options_are_invalid` | 실제 IPC ACL·등록 키·세션 철회 |
| CLI/MCP에서 승인 발급 불가 | CLI 명령에 발급 경로 없음, send 거부 시험 | GUI·도우미 승인 결속·소비·재사용 방지 |
| 원문/비밀을 오류로 반사하지 않음 | `cli04_no_input_does_not_wait_or_echo_untrusted_input`, 정적 현지화 오류 | 실제 저장소·공급자·로그 경로 전체 누출 검사 |
| 계정 격리 | Fake 공급자의 계정 불일치 거부 시험 | 실제 도메인·DB·캐시·동기화·승인 통합 |
| SEND_UNKNOWN 자동 재발송 금지 | 가짜 공급자의 접수 후 응답 유실·접수 횟수 관찰 | 영속 outbox와 재시작·응답 유실 장애 시험 |
| 메일을 불신 데이터로 취급 | 20개 합성 주입 메일과 EUC-KR/MBOX 샘플 | 실제 도구 실행·렌더링·가져오기 경계 시험 |
| 비밀은 보호 저장소 안에서만 사용 | Windows DPAPI 12개 시험: 합성 값 왕복·변조·계정 결속·파일 원자성·경로 교체·입력 실패·Debug 비노출 | 실제 다른 Windows 사용자·제공자 통합·사람 보안 검토 |
| IPC는 OS 주체를 확인하고 보호된 호출을 거부 | 실제 Windows pipe의 명시적 DACL·peer SID 불일치·중복 인스턴스·취소/재연결 8개 시험, bounded 프레임·최소 응답·mail.* 거부 14개 시험 | CNG 상호 인증·세션 철회·다른 실제 사용자/로그온 세션·원격 접속 시험 |
| TLS 실패 시 평문 폴백 없음 | IMAP/SMTP probe의 TLS·인증·STARTTLS 평문 주입/오류 시험, 인증 전 비밀 해제 0회 | 실제 제공자·OAuth·메일 수신/발송 경로·사람 검토 |
| 정책·승인 우회 경로 없음 | Cargo dependency 검사와 테스트 지원 crate의 제품 의존 금지. 계정 읽기는 실제 인증·최신 등록·감사·scope·한도/정책을 거침. 발송은 차단 | 전체 도구·outbox·승인 1회 소비·사람 검토 |
| 외부 AI 전송 기본 꺼짐 | 외부 모델 호출 코드 없음 | 계정별 정책·노출 범위·명시적 활성화 시험 |
| DB 키와 원문을 평문 파일에 남기지 않음 | 실제 SQLCipher 암호화 왕복·키 누락/변조 거부·journal/temp sentinel·암호화 백업·migration rollback 시험. 코어는 저장소 검증 후 준비 상태로 전환 | 자동 크래시 복구·복원 UI·실제 메일 캐시/색인·다른 사용자·사람 보안 검토 |
| 일반 설정으로 보호 정책을 켜지 못함 | 암호화 설정 CAS·중복/알 수 없는 필드·잘못된 시간대·보호 값 변경 거부, 외부 AI 기본 false | 인증된 설정 IPC와 보호 UI 경로 연결 |
| 연결마다 등록 키를 증명하고 범위를 재검사 | 실제 CNG non-exportable P-256·역할 분리 transcript·OS pipe 연결 소유·재사용/변조/만료/철회 거부, 기본 등록/프로필·중단 복구·인증된 계정 읽기 시험 | 실제 Hello·다른 사용자/로그온·프로필 권한 변경·사람 보안 검토 |

SEC-02 로컬 데이터 보호, SEC-04 파일/네트워크 권한, SEC-05 보존/삭제, SEC-06 AI 제공자와 데이터 이용, SEC-07 업데이트/공급망, SEC-08 보안 UX/민감 정보 가리기의 미구현 수용 기준은 추적표에서 미구현으로 유지한다. 새 경계를 구현할 때 실제 거부 시험을 추가한다. 0개 시험·skip·placeholder를 제품 보안 통과로 기록하지 않는다.

## 2026-11-02 개인 발송 확인의 명시적 보장 변경

ADR-028의 정확한 Google 개인 CLI 발송만 OS 사용자 존재 확인 대신 네이티브 전체 내용 표시와 보내기 버튼에 결속한다. CLI/MCP에는 발급 API가 없고 immutable snapshot·만료·단일 소비·정책·감사·outbox CAS·SEND_UNKNOWN 무재전송은 유지한다. 현재 등록·계정·범위·endpoint가 달라지면 개인 claim 트랜잭션에서 거부한다. 다른 보호 경로는 기존 OS Verified 조건을 유지한다. 같은 사용자 권한의 AI가 기본 CLI나 UI 자동화를 시도하는 것을 이 분류로 완전히 막을 수 없다. 사용자 UX 결정에 따른 보장 약화이며 **사람 보안 검토 필요**다.
