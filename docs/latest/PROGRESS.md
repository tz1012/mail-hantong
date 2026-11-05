# mail-hantong 개발 진행 기록

## 2026-10-31 추가 생성 사전 점검: 두 개 제한으로 중단

사용자는 새 비밀값 1개 생성과 지정된 안전 저장·Google 인증 전송을 다시 승인했지만 비활성화된 값 삭제는 승인하지 않았다. 실제 생성 전에 읽기 전용으로 기존 client 목록의 Creation date·Status와 Add client secret 버튼 활성 여부만 확인했다. 비밀값이나 비밀값을 포함한 접근성 라벨은 읽거나 출력하지 않았다.

- 원본 (2026-10-30 20:18:52 GMT+9): Enabled.
- 노출 후 비활성화한 두 번째 값 (2026-10-31 10:10:51 GMT+9): Disabled.
- 항목 수: 2개. Add client secret 버튼: 비활성화.

Google의 최대 두 개 제한 때문에 Disabled 항목도 남아 있는 현재 상태에서는 추가할 수 없다. 새 생성을 진행하려면 Disabled 두 번째 항목을 먼저 삭제하는 별도 사용자 결정이 필요하다. 기존 원본 값은 유지해야 한다. 삭제 버튼은 누르지 않았고 새 값 생성·rotation/reset·OAuth·한통 보호 입력·저장 시험도 실행하지 않았다. 기존 코어는 변경하지 않았다.

안전한 공식 UI copy/paste 경로의 합성값 검증은 용량 제한을 먼저 확인해 아직 실행하지 않았다. 경로가 검증됐거나 불가능하다고 주장하지 않는다. 생성이 허용되는 상태가 된 후에도 실제 생성 전에 합성값 검증이 필요하다. 이는 도구 승인 거부나 불확실한 실행이 아니라 제품 제한과 미승인 삭제 범위 때문에 중단한 결과다. 근거는 secret-capacity-preflight.json이다. 코드·전체 검사 1,621개 통과 기록은 유지한다. **[사람 검토 필요]**.

## 2026-10-31 노출된 두 번째 비밀값 비활성화 완료

사용자가 01:18:43 UTC에 원본 유지·이번에 추가한 두 번째 값만 비활성화하는 것에 명시적으로 동의했다. 생성 직후 값 표시 패널을 먼저 닫고 페이지를 새로고침한 뒤, Creation date와 Status만 읽어 대상을 구분했다. 비밀값이나 비밀값을 포함한 복사 버튼 접근성 라벨은 읽거나 출력하지 않았다.

- 원본 생성 시각: 2026-10-30 20:18:52 GMT+9 (11:18:52 UTC). 최종 **Enabled**.
- 이번에 추가한 두 번째 값 생성 시각: 2026-10-31 10:10:51 GMT+9 (01:10:51 UTC). 최종 **Disabled**.

새 생성 시각에 해당하는 고유 행의 Disable client secret 버튼만 선택하고 확인 대화상자의 Disable을 확정했다. Disabling 중간 상태에서는 추가 클릭하지 않았으며 이후 Disabled로 바뀐 것을 확인했다. 원본은 Enabled로 유지됐다. 이번 조치는 성공이 확정됐으며 거부·대기·불확실 상태가 아니다. 삭제·새 생성·rotation/reset·OAuth·보호 저장 시험은 하지 않았다. 상태 근거는 secret-disable-result.json이다.

앞선 도구 출력 노출 사건은 기록으로 보존한다. 그 값을 도구 출력에서 제거했다고 주장하지 않는다. 노출된 자격 증명은 이제 비활성화됐다. Gmail 연결·읽기는 여전히 미완료이며, 추가 자격 증명 작업은 이번 승인에 포함하지 않는다. 코드 구현과 1,621개 전체 검사 통과 결과는 변경되지 않았다. **[사람 검토 필요]**.

## 2026-10-31 승인된 새 비밀값 추가와 도구 출력 노출

사용자는 기존 Desktop 클라이언트에 새 비밀값 1개를 추가하고 기존 값은 유지하며, 지정된 보호 저장·Google 인증/갱신 전송에 사용하는 것을 승인했다. 생성 전에 검증된 수정 코어의 보호 입력창을 열었고 공식 CUA로 창 읽기·입력란 마우스 및 키보드 접근을 확인했다. 현재 세션의 WinHome 브라우저 연결 번호가 달라진 것은 공식 목록의 기존 탭 ID와 창 제목으로 구분했다. 잘못된 연결의 ERR_FAILED 탭은 닫았고 WinHome 기존 설정 탭으로 접근했다.

기존 클라이언트에서 Add client secret을 한 번 눌렀다. 생성 전 Enabled 항목 1개에서 생성 후 2개로 바뀌었고 Add 버튼이 비활성화돼 새 값 1개 추가를 확인했다. 기존 값은 유지했다. 생성 중복, 새 client 생성, scope 확대, 기존 값 비활성화·삭제는 없었다.

그러나 이어 복사 컨트롤의 DOM 메타데이터를 확인하면서 버튼 aria-label을 출력했고, Google이 그 라벨에 전체 비밀값을 포함하고 있어 새 값이 이 대화의 도구 실행 출력에 노출됐다. 이는 사용자의 무노출 요구를 지키지 못한 에이전트의 실수다. 보고서에는 값을 다시 쓰지 않는다. 공개 웹에 게시됐다는 증거는 없지만 도구 출력 노출 자체로 비밀 취급 조건을 위반했으므로 이 값은 사용하지 않기로 했다.

실제 복사·한통 입력·보호 저장·Google 토큰 교환은 실행하지 않았다. 로컬 입력창은 Escape로 닫았다. 공식 요청 결과는 interrupted / APPROVAL_REQUIRED, login_started=false, account_created=false였다. 이 코드는 사용자의 승인 부족 판정이 아니라 이번에 요청을 취소한 결과다. 수정 코어를 종료하고 기존 preview.4를 복구했으며 공식 상태는 transport_ready=true, authenticated=false, mail_ready=false다.

현재 새 값과 기존 값은 모두 Enabled다. 비활성화·삭제·rotation/reset은 명시적으로 금지된 범위여서 임의 실행하지 않았다. 남은 조치는 이번에 추가된 두 번째 값만 비활성화·폐기하는 것에 대한 사용자 결정이다. 기존 값은 유지해야 한다. 이 사건은 도구 승인 거부가 아니라 실제 생성 이후 출력 처리 실패다. 실제 Gmail 목록·읽기는 미실행이며, 이 턴에 새 OAuth 요청을 시작하지 않았다.

비밀값 원문을 일반 파일·로그·CLI 인자·스크린샷·보고서·패치에 추가하지 않았다. 도구 출력에 이미 나타난 값은 제거했다고 주장하지 않는다. 코드 구현은 변경하지 않았고 직전 최종 전체 검사 1,621개 통과 기록은 유지한다. Secrets/승인 경계는 **[사람 검토 필요]**다.

## 2026-10-31 최종 전체 재검사 완료 및 비밀값 복구 선택지 확인

중단됐던 회귀 검사를 최종 코드로 다시 실행해 check_all exit 0을 확인했다. 문서 29 + 빌드 도구 89 + Rust 1,503 = 1,621개 통과, Rust 30개 suite 실패·무시 0건이다. 로그는 target/client-secret-complete-check-all.log다. 기존 코어만 경로 검증 후 잠시 종료하고 전체 검사 뒤 원본 preview.4를 복구했다. 공식 상태는 transport_ready=true, authenticated=false, mail_ready=false다.

사용자가 기존 client_secret을 보관하지 않았다고 확인했다. 기존 Desktop client UI에는 Enabled 비밀값 1개와 활성 Add client secret 버튼이 있으며, 연결된 Google 공식 문서는 기존 값이 수동 비활성화 전까지 유효한 추가 방식을 설명한다. 최대 2개 제한이다. 새 값 생성·기존 값 비활성화·삭제는 실행하지 않았다. 새 credential 생성의 별도 사용자 결정이 남아 있으며, 실제 수정 후 OAuth·Gmail 읽기와 secret 입력·저장·전송은 미실행이다. 만료된 입력 요청을 반복 생성하지 않았다. 최신 변경 기록과 검증 요약은 상위 oauth-deliverables에 있으며 Secrets/승인 경계는 **[사람 검토 필요]**다.

## 2026-10-31 client_secret_missing 원인 수정 — 보호 입력창 표시 확인, 기존 비밀값 확보 필요 / 사람 검토 필요

실제 Google HTTP 400 invalid_request/client_secret_missing을 근거로 사용자가 승인한 최소 지원을 구현했다. 이전 진단 전용 patch는 상위 oauth-deliverables/diagnostic-only에 보존했다. 실제 시크릿 설정·보관·Google 전송의 별도 사용자 승인도 받았으나, 이 기록 시점에는 합성값 검사만 실행했다.

기존 native 보호 입력·paint 검증·Windows 본인 확인·단일 창/cancel gate를 재사용하는 GoogleClientReview를 추가했다. 정확한 Desktop client ID와 고정 토큰 목적지를 표시한다. 입력값은 Secrets 소유의 opaque GoogleClientSecret으로만 이동하고 CLI 인자·일반 IPC·설정·로그에는 노출하지 않는다. Google/IMAP/SMTP 입력 혼용과 다른 client ID 사용을 거부한다. 브라우저 시작 전 입력 확인과 기존 최종 계정 확인을 모두 유지한다.

Secrets가 code 교환 및 refresh 요청에만 client_secret을 인코딩하고, 최종 계정 확인 후 account/subject/client/version에 결속된 DPAPI bundle format 4에 함께 저장한다. refresh token 회전 시 시크릿을 보존한다. 기존 시크릿 없는 format 3은 계속 읽고 저장한다. format 4 계정은 구 preview.4에서 갱신할 수 없으므로, 새 연결 이후 바이너리 롤백 시 이 제한을 안내한다. DB 마이그레이션이나 scope 확대는 없다.

변경 영역: Secrets google/client_secret·oauth·refresh/storage·typed protected capture 및 합성 테스트, Approval GoogleClientReview와 양언어 리소스, Approval-Windows binding/dialog/window, Core setup_google, Google 계약/ADR/사용 안내. 전체 컴파일 통과. 보안 관련 테스트는 Approval 19, Approval-Windows 59, Secrets 209개 통과(새 6개 포함). 최초 새 교환 테스트는 기존 fake의 no-client-secret 가정과 충돌했으며, 기존 검사는 보존하고 새 합성 transport에서 필드 인코딩을 별도로 검증하도록 수정했다. 기능 수정의 전체 check_all exit0: 문서29+빌드도구89+Rust1503=1621개 통과. 최초 Clippy 항목 배치 오류 2건은 정리 후 전체 재검증했다.

실제 보호 입력창 시험은 처음 INVALID_INPUT으로 실패했다. 숨김 코어 시작 옵션이 첫 ShowWindow를 숨김 처리하는 것을 임시 단계 로그로 확인했고, 첫 호출 뒤 숨겨져 있을 때만 다시 표시하도록 수정했다. 보호 입력의 소유·focus 검증은 유지한다. 수정 후 공식 CUA 창 목록에 “mail-hantong — Google 클라이언트 자격 증명”이 나타났다. 창 읽기 도구가 승인 대기 시간초과됐고 그 사이 5분 입력 제한으로 공식 상태가 APPROVAL_EXPIRED가 됐다. 실제 secret 입력·저장·전송과 외부 OAuth는 실행되지 않았다. 현재 유효한 입력 요청은 없다. 임시 native 로그는 제거했다. 최종 창 표시 수정까지 포함한 재검사 결과는 상위 oauth-deliverables/verification-summary.json과 target/client-secret-final-check-all.log를 따른다.

Google Cloud의 ERR_FAILED는 기존 공식 Chrome 탭으로 접근해 복구했다. 기존 프로젝트의 Desktop 클라이언트 → Information and summary → Client secrets 페이지는 조회에 성공했다. 실제 남은 blocker는 Google Cloud가 기존 비밀값의 보기·다운로드를 더 이상 제공하지 않는다는 제품 제한이다. 새 생성·rotation은 승인 범위 밖이므로 실행하지 않았다. 사용자는 지정된 기존 비밀값 저장·Google 전송을 이미 승인했다. 기존 값을 안전하게 보관 중인지 확인하고, 보관 중이면 새 보호 입력창에 직접 입력한 뒤 Windows 본인 확인이 필요하다. 비밀값을 채팅·일반 파일·CLI 인자로 받지 않는다. 수정 후 OAuth/Gmail 읽기는 미실행이다. **[사람 검토 필요]**.


## 2026-10-31 OAuth 실패 단계 진단 작업 사본 — 자동 검증 통과, client_secret 누락 원인 확인

사용자 승인에 따라 원본 릴리스와 데이터·백업을 보존한 사본에서 Secrets 내부 진단을 추가했다. 변경 파일은 `google/diagnostics.rs`(신규), `google.rs`, `google/oauth.rs`, `google/tests.rs`, 이 진행 기록과 자동 생성 `docs/traceability.md`다. 콜백 대기·시간초과·검증, 토큰 전송·응답 해석, refresh credential 존재, identity 확인을 구분한다. 시간·단계·고정 오류 분류만 기록하며 URL/code/state/verifier/token/secret/주소/본문/공급자 원문을 로그에 넣지 않는다. 기존 권한·CLI 계약·PKCE·5분 제한·Windows 본인 확인·저장 경로는 유지한다.

Windows 네이티브 빌드·fmt·Clippy·요건·아키텍처·추적표 검사가 통과했다. 문서 29개, 빌드 도구 89개, Rust 전체 1,497개가 통과했다(합계 1,615; Secrets 205개와 신규 3개는 Rust 수량에 포함). 첫 check_all은 실행 중인 원본 코어의 제품 named pipe 점유로 IPC 테스트가 Conflict/exit101이었다. 정확한 코어를 잠시 종료하고 동일 소스의 전체 Rust 테스트를 다시 실행해 0 failed/0 ignored를 확인했다. 전체 check_all 자체의 최종 exit0으로 표시하지 않는다. 로그는 target/oauth-check-all.log, oauth-workspace-tests.log, oauth-secrets-tests.log다.

첫 진단 요청의 계정 선택은 도구 검토에서 거부됐다. 사용자가 정확한 계정 선택을 다시 명시적으로 승인한 후 새 PKCE 요청을 한 번 생성했다. 이번에는 callback_validation과 callback_wait가 성공했다. 2026-10-30 23:04:59 UTC, token_exchange HTTP 400 / invalid_request / client_secret_missing, token_response AUTH_REQUIRED를 확인했다. 약 72.86초 후 실패했으며 Windows 본인 확인·Gmail 목록/본문에는 도달하지 않았다. 코드·기존 계약은 client_secret을 전송하지 않으므로 실제 요구와 불일치한다. 과거 요청의 원인을 소급 확정하지 않는다.

새 비밀 설정·보관·Google 전송은 현재 승인 범위의 중지 조건이므로 실행하지 않고 최소 지원 제안을 상위 oauth-deliverables/client-secret-support-proposal.md에 작성했다. 기존 계약·ADR 변경과 보호 입력·Secrets 보관·교환/갱신 경로의 검토가 필요하다. 진단 코드를 추가 변경하거나 로그인 반복을 하지 않았다. 진단 코어 PID 77632 종료 후 원본 preview.4를 복구했으며 최신 PID/상태는 runtime-restoration-after-retry.json 및 restored-after-retry-core-status.json에 기록한다. 원본·데이터·백업은 보존했다.

변경 기록·검토 patch·해시 manifest·민감정보 없는 실제 단계 근거는 상위 oauth-deliverables 폴더에 있다. 다음은 새 비밀 지원 범위 승인 후 최소 원인 수정과 실제 Gmail 읽기 시험이다. Secrets 경계 변경은 **[사람 검토 필요]**다. 커밋·푸시·배포·메일 변경은 하지 않았다.

최종 갱신: 2026-10-30 (KST)

작업 방식 변경: 사용자 요청에 따라 기능별 병렬 구현을 먼저 완료하고 통합 점검·수정을 수행한다. 반복 중간 리뷰와 RED/GREEN·전체 검사·상세 보고를 줄인다. GUI 진행 파일을 보존하고 CLI 전용 패키지·설치·실행 흐름을 우선 검증한다.

연속 개발 지시 재확인: 사용자 정정에 따라 개별 묶음의 검증·커밋에서 턴을 종료하지 않고 다음 승인된 개발을 이어간다. GUI 기준선 `80ed442`도 전체 제품 완료가 아니다.

이 문서는 개발 진행 상황을 확인하는 단일 진입점이다. 작업을 시작할 때 현재 상태를 확인하고, 작업을 마칠 때 변경 파일·실행한 검증·남은 일·다음 작업을 같은 문서에 누적한다. 요건과 설계의 상세 내용은 연결된 문서에서 관리한다.

## 현재 상태

Google 다중 계정 로그인·Gmail 읽기 구현(2026-10-30): 브라우저 OAuth/PKCE, 계정별 DPAPI 갱신, v19 캐시·동기화, native 확인과 CLI·터미널·공통 도구 연결을 구현했다. 최종 전체 검사 **1,612개 통과** 후 preview.4 패키지의 설치·업데이트·제거 검증을 진행 중이다. 실제 Google 프로젝트 등록·로그인·메일 호출은 수행하지 않았다. 보안·계약 변경은 **[사람 검토 필요]**다.

2026-10-30 CLI preview.3 기능 묶음: 본문 읽기·암호화 캐시, 초안 작성, 개별 native 승인 발송 원장·SMTP, 최초 SMTP 보호 설정, 독립 MCP 등록·도구 조회/호출, Codex/Claude Console AI 대화, 공급자/모델/추론/fast/agent 세션 제어와 한 줄 설치를 연결했다. `/provider`와 `/provide`를 같은 명령으로 받고, 값 없는 공급자·모델·모드 명령은 상하키 선택을 제공한다. 최종 전체 검사 **1,565개 통과** 후 소스 `065366b`와 [개발 미리보기 v0.1.0-preview.3](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-preview.3)을 공개했다. 공개 주소의 최종 ZIP으로 실제 임시 설치·실행·제거·데이터 보존을 재검증했다. 아래 preview.2의 1,450개는 직전 기준선으로 보존한다. 보안·계약 변경은 **[사람 검토 필요]**다.

**우선순위 변경(사용자 지시, 2026-10-30): GUI보다 CLI 출시를 먼저 진행한다.** GUI 진행 파일은 보존하고 추가 디자인 작업을 중단했다. 상단 폴더 드롭다운 제거·왼쪽 실제 폴더 통합·스크롤 끝 자동 추가 로딩은 후속에 남긴다. 코어는 별도 프로세스이고 계정 비밀번호 입력·등록/권한 승인은 보호된 Windows 창을 유지한다.

직전 검증 산출물은 `artifacts/cli/0.1.0-preview.2/`에 보존한다. 새 label은 `0.1.0-preview.3`이다. 미리보기는 완성된 정식 제품을 뜻하지 않는다. 첨부·예약·주소록·규칙 자동화·실제 제공자 상호 운용성과 사람 보안 검토가 남아 있다.

| 항목 | 상태와 근거 |
| --- | --- |
| 기준 자료 | 사용자 승인으로 채택한 [Markdown 기준 요건](../requirements/README.md): 요건 170개·1단계 P0 144개·문장별 AC ID 341개. 작업 카드 정의 151개는 보존한다. |
| 현재 카드 | 사용자 우선 지시에 따른 **CLI 개발 미리보기의 로컬 패키지·설치 검증**을 마쳤다. M9-10/11/15/16의 전체 설치·새 OS·서명·출시 게이트 완료가 아니다. M8 GUI 진행 파일은 보존했다. 보안·계약은 **사람 검토 필요**다. |
| 최신 검증 | **1,612개 통과**(문서 29·빌드 도구 89·Rust 1,494). Windows check-all의 fmt/Clippy/build/test·문서/의존/추적 검사 통과. Linux 전체 target cross-Clippy 통과. 5계정 병렬·토큰/계정 격리·실제 서명 IPC·v19 이전/롤백을 검증했다. preview.4 설치·업데이트·제거 검증은 진행 중이며 실제 Google·새 OS·Mac 검증은 남아 있다. |
| 제품 구현 | Rust workspace·영/한 CLI·Windows 코어/인증 IPC·DPAPI·SQLCipher·정책/감사·보호 설정/등록, 폴더·UID 수집/대조/복구·검색·본문·초안·native 개별 승인 발송, managed AI·MCP·한 줄 설치를 연결했다. 첨부·예약·주소록·규칙·전체 GUI·실제 제공자 검증은 후속이다. |
| GitHub | 공개 저장소 [tz1012/mail-hantong](https://github.com/tz1012/mail-hantong), main remote 연결, 개발 미리보기 v0.1.0-preview.3과 ZIP/SHA256SUMS/BUILD-METADATA 게시. 실행 데이터·에이전트 상태는 소스에 포함하지 않는다. WinGet/MSI 등록과 서명은 아직 없다. |
| 새 사용자 요청 | 진행 상황을 이 문서에 누적하고, GitHub에서 명령어로 설치 및 개발을 시작할 수 있는 경로를 마련한다. 상세 범위는 [공개와 설치 계획](DISTRIBUTION.md)에 기록한다. |
| 문서 관리 방식 | Markdown을 개발 기준으로 갱신하고 기존 Word는 보관한다. 최신 사용자 지시 “제작 완료까지 일단 묻지말고 진행해”에 따라 일반 구현 판단은 기록하며 연속 진행한다. |
| 실행 성능 기준 | 빠른 실행을 추구하면서 상주 메모리 사용도 함께 고려한다. 사용자가 이 절충을 근거로 Rust를 선택했다. 웜 캐시 상태의 기본 메일 목록 사용 가능 목표 **1초 이내**는 유지한다. 현재 기준은 [FEATURES.md](FEATURES.md)의 성능 보완이다. |
| 런타임 결정 | **코어·CLI Rust 확정** — 2026-10-29 사용자 선택, [ADR-001](../adr/ADR-001-core-runtime.md). 비교 실험에서 .NET은 프로세스 시작, Rust는 메모리에 유리했고 검색은 혼재했다. [실측 보고서](RUNTIME-BENCHMARK.md)는 보존하며 제품의 1초 목표는 아직 검증 전이다. |
| Mac 지원 방향 | 사용자 승인으로 정식 후속 목표에 포함했다. Windows 우선 출시 후 Mac 코어·CLI·최소 승인 도우미부터 제공하고 전체 GUI를 후속 제공한다. 구체적인 단계·일정·패키지는 미정이며 현재 제품은 미구현이다. |
| 공개 사용 정책 | **[LICENSE Version 1.0](../../LICENSE) 정식 채택** — 개인·회사 내부 업무용 사용, 수정본 비공개 및 원본·수정본 무료 재배포 허용. 프로그램 판매·유료 제공 금지. 고지 유지·수정 표시·보증과 책임 조항 포함. 저작권자는 **Andrew Cho**, 웹사이트 **https://tzt.kr**, 연락처 account@example.invalid**이다. [한국어 안내](LICENSING.md), [ADR-016](../adr/ADR-016-license.md). |
| 다음 제품 작업 | CLI 배포 기준선을 보존하고 첨부·초안 편집·예약·주소록·규칙 등 남은 P0와 사람 보안 검토·실제 제공자 검증을 이어간다. GUI 디자인·자동 추가 로딩·상단 폴더 선택 제거는 후속 범위다. |

## CLI preview.4 — Google 다중 계정 브라우저 로그인·Gmail 읽기 (2026-10-30)

- **범위**: ACC-01/02/03/04/05, SYNC-02. Google 고유 subject의 SHA256으로 계정을 식별하며 이메일이 같아도 다른 subject는 분리한다. 같은 계정 재연결은 CAS로 기존 ID·메일을 유지한다. 계정 수 자체를 제한하지 않으며 네트워크 동시 작업은 8개로 제한한다.
- **인증**: 시스템 브라우저·PKCE S256·256bit state·5분 단일 loopback callback·실제 반환 scope·검증된 UserInfo를 사용한다. 토큰 교환·조회·갱신과 Gmail HTTPS는 Secrets 안에서만 수행한다. 고정 Google 호스트·TLS·리다이렉트 금지·응답/시간 한도·계정별 refresh 잠금·세대별 DPAPI 저장을 적용했다.
- **보호 연결**: Google 확인 결과를 불투명 pending credential과 함께 native 검토에 결속하고, 정확한 계정·클라이언트·권한·기존 연결 교체 여부와 Windows 본인 확인 후 저장한다. 기존 GUI 언어·테마를 따른다. CLI·MCP에는 승인 발급 기능이 없다.
- **저장·정책**: v19에 Google 연결/라벨/메일/동기화 상태/실행 원장을 추가했다. 계정·현재 등록/epoch·권한·MCP AI 동의·operation/idempotency/version·페이지 토큰/라벨을 결속하고 감사와 결과를 원자적으로 확정한다. 재시작은 중단을 기록하고 네트워크 요청을 자동 재실행하지 않는다. 인증 만료 계정의 로컬 메일은 보존한다.
- **사용 경로**: `hantong login google`, `setup google request/status`, `gmail labels/sync/status/list/read`. 첫 로그인 뒤 별도 등록/범위 확인과 INBOX 32개 가져오기를 연결했다. 공통 메일 도구에는 명시적 `gmail_*` 모드를 추가했다. 기존 터미널 화면도 같은 CLI를 사용하며 로그인·명시적 새로고침을 연결했다. 본문은 텍스트 64KiB 한도이며 첨부·외부 콘텐츠를 열거나 Gmail을 수정/발송하지 않는다.
- **검증**: Windows Secrets/Contracts 261개, Store 451개 집중 검사 통과. 5계정 병렬/인증 실패 격리·잘못된 subject/epoch·중복 요청·v19 이전/롤백·실제 서명 IPC 통합 시험을 추가했다. 최종 `check-all` **1,612개 통과**(29+89+1,494), 로그 `target/google-preview4-check-all.log`, SHA256 `bdda4a0dd7d4be2139367b4ff41281af4994267b935fa6ed23031a4380a00372`. 소스 해시 `8d9f712e43c2a356b7c789f3c379e5319ff5767d6d00935058322ecbfb1b40de`; staged 소스 576개와 작업 파일 바이트 일치를 확인했다. 최신 Windows 전체 Clippy와 Linux 전체 target cross-Clippy도 통과했다.
- **패키지**: `artifacts/cli/0.1.0-preview.4/` 생성. ZIP 21,657,672바이트, SHA256 `fac65f8bb44232846f60ffbd971cf67f33e860197cb33ba433a1a9b659d648ad`. 실제 빌드 소스 해시·모든 manifest 파일·체크섬·4개 실행 파일의 Windows 시스템 DLL 의존과 외부 VC++ 런타임 불필요를 확인했다. Google 사용 안내를 패키지에 포함했다. 설치·업데이트·제거 검증 결과는 완료 후 갱신한다.
- **배포·한계**: 배포자 Google Desktop OAuth client ID는 아직 등록되어 있지 않다. 최초 실행은 사용자가 준비한 공개 client ID·Gmail API·동의/테스트 사용자 설정이 필요하다. 실제 Google 검증과 제한 범위 공개 앱 인증은 외부 출시 게이트다. 기존 full9 MCP 등록에 새 계정을 추가하는 별도 갱신 흐름은 아직 없다. 이 변경으로 기존 계정 권한이나 외부 AI 동의를 자동 확대하지 않는다. macOS/Linux 제품 메일은 여전히 미지원이다.
- **문서/변경**: [Google 설정·명령 안내](GOOGLE-MAIL.md), [계약](../contracts/google-mail.md), [ADR-018](../adr/ADR-018-google-desktop-oauth.md). Contracts·Secrets·Store·Application·Approval/Windows·IPC·Core·CLI·터미널·패키징 및 해당 테스트를 변경했다. 보안·계약은 **[사람 검토 필요]**이며 요건 AC나 정식 출시 게이트를 통과로 올리지 않는다.

## CLI preview.3 — 본문·승인 발송·AI·MCP·명령 설치 (2026-10-30)

- 본문은 인증 IPC·정책·감사를 거쳐 읽기 전용 IMAP EXAMINE/BODY.PEEK[]로 수집하고 SQLCipher에 저장한다. 계정·정확한 mailbox·UIDVALIDITY·UID·endpoint 버전을 결속한다. 제한된 MIME plain/HTML text 해독, 영한 문자열, 제어문자 제거, 최대 64KiB 표시를 연결했다. 첨부·외부 이미지 실행은 후속이다.
- 초안은 계정·수신자·제목·전체 UTF-8 본문 hash를 검증해 불변 버전1로 저장한다. native 창이 전체 메시지와 SMTP 대상·발신자를 표시하고 OS 사람 확인을 요구한다. opaque proof 소비·현재 권한/epoch/계정/endpoint 재검사·정책/감사를 통과해야 단 한 번의 Sending claim을 커밋한다. 응답 불확실·도중 권한 변경·시작 시 미완료 sending은 SEND_UNKNOWN이며 자동 재전송하지 않는다. 최초 SMTP 설정도 보호 입력·계정 버전 CAS를 사용한다.
- MCP는 CLI와 다른 등록·CNG 키·mcp.json·native v2 proof를 사용한다. 새 full9 등록은 직접 실행 RPC를 제외한8개 도구만 광고한다. legacy read3는 명시 재검토 후 새 주체로 교체하고 원자적 폐기·lineage·보호 프로필 CAS를 기록한다. 계정별 외부 AI 동의는 확인된 활성화와 같은 트랜잭션에만 생성하며 업데이트·시작으로 부여하지 않는다.
- AI는 공식 Codex app-server 관리 인증 및 Claude Console/API 과금 연결을 사용한다. 실제 공급자 로그인·추론은 수행하지 않았고 가짜 프로세스/모델 목록으로 검증했다. Claude 구독 OAuth는 제공하지 않는다. 기본 mail context·범용 shell/filesystem/web 도구는 꺼져 있으며 `/agent on`은 별도 MCP 등록과 계정 동의가 필요하다. 공급자/모델 전환·agent off는 이전 메일 문맥을 재사용하지 않는다.
- `/provider`와 `/provide`, `/model`, `/mode`의 값 없는 입력은 ↑↓·Enter·Esc를 지원한다. 비대화형/JSON/no-input은 메뉴를 열지 않는다. mode는 모델의 실제 low/medium/high/xhigh/max 지원 범위로 제한한다. `/fast on|off`는 실제 서비스 설정만 요청하며 미지원은 거부한다. `/agent`·`/fast` 단독은 상태를 표시한다.
- 루트 `install.ps1` 한 줄 부트스트랩은 GitHub의 호환 ZIP·체크섬·크기·압축 경로를 검증한다. 사용자 PATH에 소유한 항목만 추가하고 제거하며 다른 PATH와 데이터는 보존한다. Windows PowerShell5.1 Restricted 파일 정책에서도 시스템/사용자 정책 변경 없이 검증된 스크립트 내용을 실행하는 시험5개가 통과했다.
- 전체 `target/cli-preview3-final-check.log` exit0: **1,565개**(문서29·빌드 도구88·Rust1,448), fmt·Clippy·build·아키텍처·요건 검사 통과. 로그 SHA-256 `9533798ed72bc6f250d1ec21fb9f97ce0f2433a38b7528d2098ff9fb4c0e2052`. 동결 후 Windows Clippy/fmt 및 Linux 전체 target 교차 Clippy도 통과했다. 최종 추적·패키지 소스 hash는 `0c39d71f42e61d5aa2b92cd5acae08f49e42ce019da54e813a4859c7b31a7c7e`다. 추적표는 개별 AC의 사람 검토·공급자 증거가 없는 항목을 통과로 승격하지 않는다.
- 실제 ZIP으로 preview.2 설치→preview.3 업데이트·반복→설치된 영/한 도움말·JSON 종료 코드→제거를 시험했다. 공백/한글 경로와 알 수 없는 파일·데이터를 보존했다. 로그 `target/cli-preview3-package-upgrade-smoke.log` exit0, SHA-256 `b608c337a8d8b8375e1d9ade7d36cd11f83f07da21dbba64dba3b6b70b886022`. 최종 mode·줄바꿈 정리 후 CLI/Core와 build metadata/manifest가 재생성됐다. 설치 스크립트는 시험한 바이트와 같고 최종 네 실행 파일의 help/version·PE·각 manifest checksum을 다시 검증했다. 최종 source 입력547개는 Git index 바이트와 같음을 확인해 새 clone의 줄바꿈/추적 불일치를 방지했다.
- 공개 릴리스의 ZIP·SHA256SUMS를 실제 HTTPS로 받아 체크섬 확인→임시 설치→설치된 CLI/Core version·도움말→제거→합성 데이터 보존을 검증했다. 사용자 PATH는 변경하지 않았고 Core run·계정·메일은 사용하지 않았다. `target/cli-preview3-public-download-smoke.log` exit0, SHA-256 `01d483267252339522c4adbe2971383e8d49e2ef6f7474cefb76a900a8b9c80b`. GitHub 자산 digest도 로컬 ZIP SHA와 일치한다. 릴리스는 `065366ba639a71aec550d0915b78e0b1945f2d12`에 연결된다.
- GitHub [소스 CI 실행](https://github.com/tz1012/mail-hantong/actions/runs/41023688795)은 Ubuntu 전체 검사를 통과했다. 2026-10-30 17:05 KST 확인 당시 Windows Server2025는 native 의존성의 첫 빌드/검사를 진행 중이다. 완료되지 않은 원격 결과를 통과로 표시하지 않는다. 작업 PC의 Windows 전체1,565개 결과와 구분한다.
- 최종 ZIP `artifacts/cli/0.1.0-preview.3/mail-hantong-cli-0.1.0-preview.3-windows-x86_64.zip`: **20,731,868 bytes**, SHA-256 `c77c85cc9077086022659a37cda19f502b76f900a81919a18177fdaaa29ab146`. 정적 MSVC CRT, Windows 시스템 DLL만 사용함을 확인했다. 의존성 고지134개 포함; 법적 검토·SBOM 완전성을 주장하지 않는다. 실토큰 패턴 검사0건. 앞선 후보들은 별도 rejected 디렉터리에 보존했다.
- 통합 수정: SMTP recipient550를 lettre 예외와 구분해 명확한 거부로 기록, post-DATA 연결 유실은 Uncertain 유지, SMTP 입력의 IMAP-caption/저장 경로 혼용 차단, 취소/만료/rate/정책 실패 receipt와 strict decoder 정합성, 역사적35개 테이블 보존·새6개 무동의·rollback·backup collision, 새 패키지 그래프 시험/JSON schema ID·공통 envelope, Windows TEMP 8.3 fixture 경로, Clippy 첫 native 빌드 timeout을 보완했다. 시험을 삭제·건너뛰기로 바꾸지 않았다.
- **[사람 검토 필요]**: 승인·비밀·정책·발송·IPC·계약 변경. 실제 계정/서비스/Hello/외부 추론은 사용하지 않았다. 서명·깨끗한 Windows·실제 IMAP/SMTP 및 AI 공급자 상호 운용성, 첨부·예약·주소록·규칙, Mac 제품은 남아 있다. 이번 배포는 완성된 정식 제품이나 전체 출시 게이트의 통과가 아니다.

## CLI preview.2 — 진단 권한 연결 수정 (2026-10-30)

- preview.1 검증 뒤 개발을 이어가며 `clients refresh`의 fresh 목표가 기존 v5 계약의 A가 아닌 M으로 남은 누락을 수정했다. `apps/mail-hantong-core/src/scope_refresh.rs`의 helper 선택만 A로 연결했다. 새 CLI 옵션·RPC·DB 스키마·의존성은 없다. 기존 명시 요청·native 전체 범위 확인·opaque proof·프로필 발행·활성화 경로를 유지한다.
- 기존 L/F/U/M pending/staged 목표와 프로필 bytes를 보존한다. 새 요청 자체는 권한을 부여하지 않는다. 같은 계정의 M→A는 별도 확인 뒤에만 활성화하고, 이미 같은 계정의 A이면 `CONFLICT`로 상태·이력을 보존한다. 실제 요청 큐가 저장한 목표도 A인지 검사했다. 신규 회귀2개를 포함한 관련13개 통과, 다른 담당자의 보안 경계 검토 CLEAR. **[사람 검토 필요]**는 유지한다.
- 현재 fresh 갱신 승인 후 `hantong diagnostics snapshot --sections sync,recent_work,errors --hours 24 --limit 100 --json`과 저장된 CLI 언어 읽기를 사용할 수 있다. 과거 중단 요청을 재개하면 과거 권한을 유지하므로, 그 작업이 끝난 뒤 별도 새 갱신이 필요할 수 있다. `CLI-LAUNCH.md`에 해당 절차를 반영했다.
- 최신 ZIP은 `artifacts/cli/0.1.0-preview.2/mail-hantong-cli-0.1.0-preview.2-windows-x86_64.zip`(17,152,461 bytes), SHA-256 `5a4da25a3576609922fbf21336f4ef8f4ec2a6f06ae6feeb30bdf23e1e8608ee`다. 이전 검증 산출물 preview.1은 보존하고 기본 패키지 label을2로 올렸다. 두 실행 파일의 Cargo 버전은 계속0.1.0이며 label과 구분한다.
- Windows PowerShell 5.1에서 실제 preview.1 설치→preview.2 업데이트→재설치→영/한 도움말·JSON 오류/종료 코드→설치본 제거를 확인했다. 공백/한글 경로·기존 데이터 보존·양쪽 버전 실행 파일 제거가 통과했다. 실제 Core `run`·계정·Hello·메일은 실행하지 않았다. 로그 `target/cli-preview2-package-upgrade-smoke.log` exit0, SHA-256 `3ba36306d216708e5ce743ae9b0fd2182c52180d358a67842415dcb4b683f129`.
- 최종 `target/cli-preview2-full-check.log` exit0, **1,450개 통과**, SHA-256 `ae6238a839451f6a97602b65e5d75e24419cb54579a7906c7238b9f391dae5db`. 패키지와 검사 소스 hash는 `338c891aa5d39e219f148dae15d680f00254d694129c7fcea7fe1973064c1e4e`로 같다. 새 Core의 정적 CRT 의존성 검증은 `target/cli-preview2-core-dependencies.log`에 기록했다. 공개·서명·깨끗한 Windows·실제 제공자/Mac/성능과 사람 보안 검토, 본문·첨부·작성·발송 등 제품 기능은 여전히 남아 있다.
- 이전 미리보기 준비는 `5cc881a`, 이번 수정은 후속 `Connect explicit CLI scope refresh to app read permissions` 커밋으로 구분한다. 전체 제품이나 M1-10/M9 카드의 완료를 주장하지 않는다.

## CLI preview.1 패키지와 통합 검증 (2026-10-30)

- 산출물: `artifacts/cli/0.1.0-preview.1/`의 ZIP(17,151,556 bytes), `SHA256SUMS`, `BUILD-METADATA.json`. ZIP SHA-256은 `eca10406b1c669cfd08a4331a2bedaceaa7813b3669d13cf78a6b15a739b722c`이다. GUI 실행 파일 없이 `hantong.exe`·`mail-hantong-core.exe`만 포함하며, 패키지 표시는 `0.1.0-preview.1`, 실행 파일 버전은 `0.1.0`이다.
- 구현: `scripts/package_cli.py`, `packaging/windows/` 설치·제거·공통 경로 검사, 잠금 파일에 결속한 의존성 고지, `CLI-LAUNCH.md` 영한 안내, CLI 도움말을 추가했다. 새 의존성은 없다. 134개 외부 crate의 보수적인 normal/build closure와 245개 고지 파일을 수집했다. 라이선스 원문이 빠진 두 IMAP crate는 정확한 upstream commit·Git blob·registry checksum·SHA-256으로 결속한 사본을 사용한다. 법률 검토나 완전한 SBOM 인증을 주장하지 않는다.
- 설치기는 경로 이탈·연결 폴더·변조 파일·동시 작업을 거부한다. 검증 후 버전 포인터를 마지막에 교체하고 실패 시 이전 활성 버전을 보존한다. 삭제는 소유 파일에 한정한다. 상시 PATH·자동 시작·실제 계정 설정은 변경하지 않는다. 설치·업데이트·제거 회귀 10개와 패키징 시험 7개가 빌드 도구 81개 검사에 포함된다.
- 실제 ZIP 검증: Windows PowerShell 5.1 `-File`에서 기본 `$PSScriptRoot`가 매개변수 기본값에 반영되지 않는 결함을 재현하고 본문에서 경로를 설정하도록 수정했다. 초기 후보는 `artifacts/cli-rejected/`에 보존했다. 수정 ZIP을 공백·한글이 있는 격리 경로에 두 번 설치하고 영/한 도움말·버전·잘못된 명령의 JSON/종료 코드2·설치본 제거·별도 데이터 보존을 확인했다. 코어 `run`, 실제 계정·Hello·메일은 실행하지 않았다. 로그 `target/cli-package-smoke-verified.log` exit0, SHA-256 `bec7d2eda4bc536a6c2de123813547055317d743388ed9bbb67f0a7a41460fc5`.
- 배포 빌드는 MSVC CRT를 정적 링크한다. 두 실행 파일의 `dumpbin /DEPENDENTS` 결과에 별도 Visual C++ 런타임 DLL이 없으며 Windows 시스템 DLL은 필요하다. 근거는 `target/cli-release-{cli,core}-dependencies.log`다. 깨끗한 Windows PC/VM 시험을 대신하지 않는다.
- 최종 전체 검사: `target/cli-launch-full-check-verified.log` exit0, **1,448개 통과**, SHA-256 `c345b187634588cd69c58a009f19255778171b1297aa1430313756f9f05ec1a3`. 소스 hash는 패키지 빌드 기록과 같은 `0d02d9995b46665e9baef64e8d78a201750caa2ad9f5fc94508921989bd7e911`이다. CLI 대상 Linux/macOS 정적 검사는 `target/cli-launch-{linux,macos}-final.log` exit0이다. 이전 실패 로그도 보존한다.
- 보존한 GUI 변경은 `bf7ca31`에 기록했고 전체 회귀에 포함했다. 스크롤바의 화면 밖 native 시험은 실제 메모리 DC에 그린 뒤 좌표를 검사하도록 보완했으며, 포인터·캡처·이동 assertion을 유지했다. 데스크톱 시험 36개가 통과했다. 새 디자인의 최종 사용자 승인이나 스크롤 끝 자동 로딩 구현으로 표시하지 않는다. CLI 배포 준비는 후속 `Prepare Windows CLI developer preview packaging` 커밋으로 분리한다.
- 남은 범위: 현재 설정 흐름에는 별도 app-read 권한을 부여하는 CLI 경로가 없어 `diagnostics snapshot`은 이 미리보기의 사용 가능 기능에서 제외했다. 본문/첨부·작성/발송·예약·주소록·자동화, 실제 제공자·Mac·성능, 깨끗한 Windows·서명·GitHub 공개와 사람 보안 검토는 후속이다. **[사람 검토 필요]**, 제품 전체 수용 기준 완료0개 상태를 유지한다.

## GUI 등록·로컬 메일 표시 구현 후 통합 점검 (2026-10-30)

- Native Win32 GUI, 영어/한국어, Light/Dark/System·고대비·DPI, 좁은 창에서 상세를 아래로 배치하는 화면을 구현했다. 계정→폴더→저장된 메타데이터 목록/상세·다음 페이지를 명시 선택으로 연결했다. 데이터가 일부이거나 오래될 수 있음을 표시한다.
- GUI 고유 요청/승인 결속/등록·고정 gui.json·별도 CNG 인증을 연결했다. 권한은 계정/폴더/메일 목록 읽기 3개로 고정하고 CLI 프로필과 분리했다. 등록 요청·등록 상태·일반 새로고침은 별개이며 최초 실행은 readiness만 읽는다. 실제 서명 pipe 시험은 계정 격리·5개 비허용 동작·GUI 감사·제공자 호출 0회를 검증했다.
- 구현 후 보호 파일 시험의 핸들 수명 가정, 종류 변조 fixture의 기대 오류, Win32 상수/타입·Clippy 경고·창 제목 초기화를 수정했다. Rc 창 소유권·stale 응답·등록 경계의 최종 독립 검토는 CLEAR다.
- **1,410개 통과**(Docs29 + Build64 + Rust1,317). check-all 전체 build/test/fmt/Clippy·의존·문서·추적 검사 통과. Linux x86_64·macOS arm64 교차 Clippy도 동일 소스에서 통과했다. 소스 hash `823c51821d8214e419acba4714807380bb7e146fb3f38ec0bd26d057f0ceae55`.
- 최종 로그 `target/gui-full-check-9.log` exit0, SHA256 `4b327025502c2a5f3af5c04ff8a69ac423d147535ddc2ac542968481962c9333`. 교차 로그 `target/gui-linux-9.log`, `target/gui-macos-9.log` exit0. 통합 중 실패 로그도 보존했다.
- 실제 개발 GUI에서 영/한·세 테마·컴팩트 배치·새로고침·안정 오류·수정된 제목·닫기를 확인했다. 메일 동작은 합성 backend와 숨은 native 컨트롤로 검증했다. 실제 사용자 계정·등록 승인·Hello·메일 발송은 실행하지 않았다.
- 계약·보안은 **사람 검토 필요**다. 설정 영속 저장·추가 계정의 GUI 범위 갱신·본문·첨부·발송·전체 GUI와 설치, 실제 Mac/제공자/성능 시험은 남아 있다. 전체 수용 기준 완료로 표시하지 않는다.

## CLI 전환 전 GUI 변경 기록

전체/읽지 않음/즐겨찾기 필터와 현재 폴더 첫 페이지 갱신, Subject RFC2047 표시 디코더를 연결했다. 구현 후 검사에서 발견한 디코딩 경계/생략 표시와 시험의 UID 내림차순 가정을 수정했다. 집중 GUI/Core 시험과 이번 CLI 준비의 전체 회귀 검사에 포함해 검증했다. 검사기는 빠른 fmt/Clippy를 긴 빌드/회귀 시험보다 먼저 실행하도록 순서만 조정해 불필요한 반복 비용을 줄인다.

사용자는 기존 GUI의 클래식한 모양을 지적하고 참조 링크를 상기시켰다. 현대적인 기본 UI로 탐색·버튼·목록·빈 상태·서체·색상과 배치를 병렬 수정한다. Software Company의 실제 배포 화면과 나머지 저장소/공개 소스를 재확인했다. 3D 참조의 배포 도메인은 DNS 오류로 열리지 않아 공개 소스로 확인했다. 레트로 통신 테마는 사용자가 지정한 **나중의 히든 기능**이며 현재 테마 메뉴에는 추가하지 않는다. 디자인 기준은 `DESIGN.md`에 갱신했다.

추가 지시: Superhuman 소개 글과 Gmail 화면도 참조하되 우리 기능에 맞게 독창적으로 구성한다. Gmail 이미지를 직접 확인했고 Superhuman의 공개 공식 키보드 사용 가이드를 보완 근거로 확인했다. 다중 계정 범위·분류·예약/승인/자동화 작업 상태·AI 제안과 실행의 구분을 고유 설계 기준에 추가했다. 이 후속 기능들을 현재 모두 구현했다고 표시하지 않는다.

첫 재설계에서 실제 HWND 문자열을 보존한 64DIP 행·아이콘·카드형 상세/빈 상태·영한 토큰을 구현했다. 기능 경계와 native 수명/GDI 독립 검토에서는 HIGH/MEDIUM 지적이 없었으며 실제 영한·Light/Dark·좁은 창을 확인했다. 후속 Windows 전체 검사는 스크롤 보완의 EM_* 상수 import 누락으로 compile 단계에서 실패했다(`target/gui-modern-full-check.log`). 이 시점의 Linux/macOS 교차 Clippy는 통과했지만 현재 변경 묶음의 전체 통과로 기록하지 않는다.

사용자는 Outlook 캡처를 추가하고 프레이밍·전환·스크롤 디자인을 지적한 뒤 “지메일 인터페이스나 슈퍼휴먼 인터페이스를 따라가도 되고. 아무튼 지금은 아니야”라고 정정했다. **이 카드형 시안은 폐기한다.** 디자인 기준을 연속 평면의 얇은 상단/도구 영역·조밀한 목록·넓은 읽기 영역으로 갱신하고, 네이티브 의미를 유지한 얇은 테마 스크롤 및 OS 모션 설정을 존중하는 짧은 선택 전환을 병렬 구현한다. 현재 GUI 코드는 이 재구성 중이며 최종 디자인·전체 기능 완료를 뜻하지 않는다. 기존 조회·등록·필터·권한 계약과 레트로 후속 히든 범위는 유지한다.

## v17 최근 메일 미리보기 구현 후 통합 점검 (2026-10-30)

- `sync initial-start/status/plan/resume/pause`를 영·한 CLI→엄격 IPC→공통 권한/정책/감사→Core worker→Store로 연결했다. start는 EXAMINE만 하고 resume마다 최근 한 페이지를 수집한다. 원래 receipt와 현재 계획 조회를 구분하며 조회는 자동 재시작하지 않는다.
- 최근 UID 계획·요청·페이지·메시지 4개 표를 추가했다(전체35개). 기존 base cursor/epoch/identity를 보존한다. 별도 preview와 base 목록은 동일 locator ID를 사용하고, base가 덮는 범위의 gap/remote_absent를 되살리지 않는다. 새 m2 커서는 계획·범위·조건·페이지에 결속되며 기존 m1 형식은 보존한다.
- 구현 후 검토에서 발견한 Core 중단 전 start 고착과 이전 instance pause 고착을 수정했다. 새 명시 start의 같은 소유자·다른 instance 정리와 명시 resume takeover를 원자 처리하고, 같은 instance 실행은 추정 재시작하지 않는다. 두 발견 사항의 집중 재검토는 CLEAR다.
- SQLCipher 격리 fixture, fake provider, 실제 서명 IPC로 pause/commit, 재시작, 원장 충돌, 데이터 보존, m2 만료·중복·손상·한 페이지 한도와 ABORT/IGNORE rollback을 검증했다. 실제 사용자 DB·계정·메일·외부 제공자와 Hello를 사용하지 않았다.
- **1,353개 통과**(Docs29 + Build63 + Rust1,261). `check-all` build/test/fmt/Clippy·아키텍처·문서·추적 검사 통과. Linux x86_64·macOS arm64 교차 Clippy 통과. 소스 hash `902e73700496c6a2e1bf92e8cb474080a666eca82d6a38dba48742e912291b23`.
- 최종 로그 `target/initial-preview-full-check.log` exit0, SHA256 `e9ddb5b85a21292c10b94ae422ba23a522cad08cea5f45f9f72d735bd6a0cb49`. 교차 로그 `target/initial-preview-linux.log`, `target/initial-preview-macos.log` exit0. 첫 통합에서 정수형 DB 변환, 기존 migration fixture의 새 표 구분, Clippy 조건·미사용 코드를 수정했다. 실패 로그는 보존한다.
- 보안·계약은 **사람 검토 필요**다. 최근 선택 범위 완료는 전체 mailbox 동기화 완료가 아니다. GUI·본문·첨부·발송·설치, 실제 macOS/제공자/성능 검증은 남아 있으며 전체 수용 기준 완료로 표시하지 않는다.

## v16 구현 후 통합 점검 (2026-10-30)

- CLI 저장 언어: 명시 `--lang` 우선, 유효한 일반 메일/진단 명령에서만 선택 조회1회·비동기250ms 예산·실패시영어. JSON/help/version/invalid/등록·설정 명령은 추가조회0회.
- `hantong diagnostics snapshot --sections sync,recent_work,errors`: 자기 등록의 현재 계정 범위에 속한 실행만 단일 DB snapshot에서 집계. 최대31일/1,000실행, 식별자·주소·메일 원문을 포함하지 않음. 이번 조회의 audit 제외, 오류 중복 계수 방지, v2 canonical bytes와 기존 v1 보존.
- 공통 앱 권한: scope A는 보호 확인으로만 부여. 과거 L/F/U/M·v1~v4·pending/staged bytes 보존. typed app scope·CLI-only 정책·공유 rate·계정참조0 감사·엄격 IPC/JSON을 연결. DB v16은31개 표를 유지하며 기존 행·ID·FK·audit sequence와 실패 rollback을 검증.
- 최초 동기화: Domain의 고정 최근 UID 범위·CAS·일시정지/명시재개 모델과10개 시험 구현. 최근 page의 영속 저장·목록 연결은 다음 작업이며, 모델만으로 실제 동기화 완료를 표시하지 않음.
- 구현 담당자와 다른 담당자의 공통 경계·CLI 연결 검토에서 HIGH/MEDIUM 없음. 실제 사용자 DB·계정·Hello·메일 발송은 사용하지 않음. CNG 잔존 이름 집계1개 및 hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`가 기준선과 같음.
- 최종 `target/app-read-full-check.log` exit0, SHA256 `fcfce0675145f2d9cbab42d546856e9a09df4ac57d68adadcc2be715e577a1b1`. 소스 hash `3a49d9ecdbb8f7806b4a0d73b4854baef153e2ebb6fb1fdf8856fe00372855df`. 통합 중 리소스 빈 줄과 미지원 `prepare_cached`, Clippy Default 구현을 수정했으며 실패 로그는 보존함.
- 남은 범위: 실제 Mac/제공자/제품 성능·전체 GUI·본문/첨부·발송·설치 배포, 진단 파일 내보내기 UI와 사람 보안/화면 검토. 전체 수용 기준 완료로 승격하지 않음.

## 기능별 현재 상태

검사 개수는 구현한 경계의 회귀 검증량이며, 전체 기능 완성률이 아니다. 제품 전체 수용 기준 완료는 아직 0개다.

| 기능 범위 | 구현 상태 | 검증 상태 / 남은 연결 |
| --- | --- | --- |
| Rust workspace·영/한 기본 CLI·Windows Core/IPC | 기반 구현 | 자동 검사 통과, 제품 전체 시작 속도·메모리 목표는 미검증 |
| 암호화 DB·비밀 저장·등록·정책·감사·native 확인 | 기반 구현 | 격리 자동 검사 통과, 실제 Hello·사람 보안/화면 검토 필요 |
| IMAP 계정 설정·재개·계정 조회·CLI 범위 갱신 | 구현 및 연결 | 가짜 제공자/실제 로컬 pipe 자동 검사 통과, 실제 사용자 계정 시험 전 |
| IMAP/SMTP 연결 진단 | 내부 전송 경계 구현 | in-memory TLS/DPAPI 시험 통과, 제공자 상호 운용성 시험 전 |
| 폴더 탐색·특수 폴더 후보·저장 | CLI 목록/갱신·권한·Core worker·v10 원장 연결 | 실제 IPC/가짜 제공자 자동 검사 통과, GUI·사용자 역할 지정·실제 제공자 검증 후속 |
| 실제 메일 동기화·목록·본문·첨부·검색 | 단일 UID 수집·FLAGS/부재 대조·UIDVALIDITY 복구·로컬 헤더 목록/필터의 명시 CLI·정책/감사·원자 저장 연결 | 격리 실제 pipe 시험 통과; 본문/첨부·FTS/서버 검색·전체 최초 동기화·실제 제공자/성능 검증은 후속 |
| 작성·발송·예약 발송·임시보관·주소록 | 후속 구현 | 안전 승인/Outbox와 제품 종단 흐름 미완료 |
| 태그·즐겨찾기·스팸·광고필터·자동화 | 저장된 IMAP Flagged 기준 즐겨찾기 목록 필터 연결; 변경 기능은 후속 | 1단계 필수 범위 유지, 태그/즐겨찾기 변경·필터/자동화 제품 흐름 미완료 |
| 전체 영/한 GUI·라이트/다크/시스템 테마 | 첫 메일 GUI·자체 등록/인증·로컬 목록/상세·언어/테마 구현 | 자동/실제 표시 시험 통과; 전체 기능·설정 영속 저장·접근성 종단 후속 |
| GitHub 공개·명령어 설치·macOS 제품 | CLI 개발 미리보기 ZIP·명령형 설치/제거 로컬 검증; 공개·Mac 제품은 후속 | 실제 ZIP·PowerShell 5.1 시험 통과, 공개 릴리스·서명·새 OS·실제 Mac 실행 전 |

### 병렬 개발 운영

2026-10-30 사용자가 기능 담당 구현 → 다른 담당자의 검증 → 연결 테스트 방식을 제안했다. 공통 계약을 먼저 고정한 뒤 독립적인 기능 단위의 병렬 개발을 확대한다. 구현 담당자와 검토 담당자를 분리하고, root가 공통 파일 변경·통합 시험·전체 회귀 결과를 책임진다. 같은 DB migration·인증/권한 계약을 여러 담당자가 동시에 수정하지 않는다. 공통 기반 때문에 의존하는 부분만 순차 처리하며, 기능별 구현·클라이언트 연결·시험 준비는 독립 소유권으로 나눈다. 자동 검사 통과를 사람 검토나 제품 완성으로 기록하지 않는다.

### 적극적 병렬 실행 계획 — 2026-10-30 사용자 승인

사용자가 “적극적 병렬 작업 계획을 세우고 이를 실행하자”고 지시했다. 현재 동시 실행 한도는 root 포함 4개다. 아래 기능 묶음을 동시에 구현하고, 완료한 구현 담당자를 다른 묶음의 검토 담당자로 전환한다. 별도 진행 문서는 만들지 않는다.

| 순서 / 담당 | 기능 단위와 소유 범위 | 완료 판단과 연결 |
| --- | --- | --- |
| 기준선 / root | 진행 중 v8 catalog의 전체 검사·추적·커밋 | 실패 수정 후 검증한 소스 기준선 고정 |
| 병렬 A / accounts_client | SMTP 기능 snapshot: Domain SMTP 타입, Secrets SMTP receipt/driver/fixture | post-TLS 정보만 반환·비밀/서버 원문 미노출·실제 선택 AUTH·전체 기존 TLS 회귀 |
| 병렬 B / scope 담당 | 폴더 읽기/갱신 권한 업그레이드: Store scope 이력·등록 provenance, 승인 화면·프로필 발행 | 기존 v1 이력 보존·명시 native 확인·도구만 추가하는 갱신·구 epoch 차단·중단 복구 |
| 병렬 C / folder client 담당 | 폴더 목록/갱신 요청·페이지 모델·영/한 CLI·strict 응답 해석 | 계정 명시·세대 결속 페이지·프레임 상한·잘못된 응답 거부·단위/계약 검사 |
| 통합 / root | 공통 계약·v9 migration·정책/감사 admission·Core 폴더 worker와 원자 완료 | queue 대기 중 IPC 응답·철회/설정 변경 차단·catalog+감사 원자 commit·종료 시 자동 재접속 없음 |
| 교차 검증 / 다른 담당자 | 자신이 구현하지 않은 묶음의 코드·계약·보안·실패 경로 검토 | 결함마다 재현 시험과 수정, 미해결 지적 기록 |
| 연결 검증 / root | 가짜 제공자 → Secrets → Store → 정책/IPC → CLI 흐름과 전체 회귀 | 격리 fixture만 사용, 실제 사용자 계정/Hello/메일 및 공개 배포는 수행하지 않음 |

공통 파일은 한 담당자만 쓴다. 계약을 먼저 고정하고 타입/API·필드 의미·파일 소유권을 각 담당자에게 전달한다. 동일 파일 충돌이 생기면 root가 통합하고 다른 담당자는 해당 편집을 중단한다. Cargo는 native build cache를 공유하므로 target 디렉터리를 담당자별로 지정하거나 순차 실행한다. 전체 회귀 동안 소스 변경을 잠시 동결해 결과와 소스 hash를 결속한다. 특정 기능의 선행 작업이 늦어지면 다른 기능의 구현·검토를 먼저 계속한다.

현재 묶음 이후에는 실제 메일 동기화/조회, 작성·임시보관·승인 Outbox, 주소록·분류 기능을 각 계약과 선행 카드에 맞춰 같은 방식으로 나눈다. GUI는 영/한·라이트/다크/시스템 테마를 공통 기반부터 적용한다. 기능 구현 완료, 교차 검토 완료, 연결 검사 완료, 실제 운용/사람 검토 대기를 별도로 기록하며 테스트 개수로 제품 완성률을 추정하지 않는다.

### 다음 병렬 묶음 — 기존 UID 상태 대조

기준선 `94fd693`의 전체 검사 1,016개 통과 후 M2-04의 기존 메일 관측 대조를 진행한다. [고정 계약](../contracts/imap-uid-reconcile.md)을 먼저 작성하고 독립 설계 검토·RFC 근거 확인을 마쳤다. 전체 카드 완료나 실제 사용자 메일 검증으로 표시하지 않는다.

| 담당 | 독립 소유권 | 연결·검증 |
| --- | --- | --- |
| reconcile_protocol | Domain 고정 UID 범위 DTO·Secrets 읽기 driver·in-memory TLS 시험 | UIDNEXT 증가에도 범위 유지, 기존 codec 불변, 비정상 응답 시 무반영 |
| reconcile_store | observation overlay·pass head·영속 job·SQLCipher 시험 | 원본 보존, CAS/replay, 원장 간 충돌, 원자 rollback, 손상 거부 |
| reconcile_public | 공통 DTO·Application·IPC client·영/한 CLI·tool schema | 기존 action 보존, 별도 intent/receipt, strict 8KiB parsing |
| root | v13 migration·공유 Core worker·인증 pipe 통합·전체 검사 | 세 묶음 연결, 실행 중 변경 취소, 작성자와 다른 검토자 교차 검증 |

성공한 한 페이지 관측에서 기존 UID가 보이지 않으면 원격 부재 관측만 별도로 기록한다. 원본 메일 삭제·발송·자동 reset은 하지 않는다. pass 완료는 접수 때 고정한 기존 페이지 묶음의 완료이며 전체 mailbox 최신 상태를 뜻하지 않는다. 이 묶음은 아래 v13 snapshot의 전체 검증을 마쳤고 `5f27187`로 보존했다.

### 현재 병렬 묶음 — UIDVALIDITY 복구

v13 기준선 `5f27187` 다음에는 같은 세 담당자가 [복구 계약](../contracts/imap-uid-recovery.md)의 독립 영역을 구현한다. 통신 담당은 새 namespace 첫 페이지와 TLS 실패 처리를, 저장소 담당은 epoch·과거 기록 보존·복구 원장을, 공개 경계 담당은 CLI/IPC와 인증 pipe 시나리오를 맡는다. root는 migration·공유 worker·전체 회귀를 통합한다. 구현을 끝낸 통신 담당은 공개 경계를 교차 검토하고, 별도 설계 담당자는 Store의 과거 기록·원자 전환·충돌 경계를 검토한다.

전체 검사는 검토 지적 수정과 소스 동결 후 실행한다. native Cargo cache는 담당자 간 순차 인계하며, 그동안 다른 담당자는 코드 작성과 검토를 계속한다. 현재 단계별 증거는 이 문서 하단 v14 기록에 누적한다.

## 기록 규칙

1. 작업별 새 진행 보고서를 만들지 않고 이 문서에 날짜와 카드 ID로 기록을 추가한다. 최근 상태는 위 표를 갱신한다.
2. 기록에는 변경한 파일, 실행 명령, 실제 결과, 미실행 검사와 이유를 남긴다. 이전 기록의 오류를 정정할 때는 정정 이유를 남긴다.
3. 카드별 상태·증거의 기계 판독용 자료는 [card-status.csv](card-status.csv)다. 이 문서와 함께 갱신하며 서로 다른 완료 상태를 남기지 않는다.
4. `review_required`는 산출물을 사람이 검토해야 하는 상태다. 문서 생성·자동 검증 통과를 제품 완성이나 사람 승인으로 기록하지 않는다.
5. 현재 Word v2.3과 과거 개정본은 보존한다. 이번 진행 기록과 공개 계획은 2026-10-29 보완 문서다. 정식 요건 변경 시에는 원문·색인·계획·변경 내역을 같은 개정에 맞추고, Git 사용 이후에는 커밋·PR·태그로 이력을 연결한다.
6. 코드와 문서 변경을 커밋하게 되면 해당 기록에 커밋 또는 PR을 연결한다. 아직 없는 주소·태그·승인자·시험 결과를 채워 넣지 않는다.

## 2026-10-29 첫 개발 작업과 공개 목표 반영

**요청:** `docs/latest`를 읽고 개발 시작. 작업 상황을 한 문서에 정리. GitHub에서 누구나 명령어로 설치하고 개발할 수 있는 목표 포함.

**확인한 현재 조건:** 제품 코드와 Git 메타데이터가 없고, M0-00이 첫 선행 없는 카드다. 설치된 .NET SDK는 8.0.427과 9.0.308이지만, 설치 여부가 제품 런타임 채택을 뜻하지 않는다. 승인된 ADR·계약은 최신 모음에 없다.

**작업:**

- [문서 분할 도구](../../scripts/prepare_requirements.py)와 [시험](../../tests/Docs/test_prepare_requirements.py)을 구현했다. Python 3.10 이상 표준 라이브러리만 사용하며 제품 런타임을 결정하지 않는다.
- [검토 패키지](../proposals/M0-00/README.md)에 170개 요건을 17개 영역 파일로 분할하고 문장 기준 AC 후보 341개를 부여했다. [원문 대응표](../proposals/M0-00/traceability.csv)에 Word 표·행·앵커와 원문 단계·우선순위를 연결했다.
- 본문·표의 텍스트, 이미지 2개, 하이퍼링크 대상 4개를 [원문 동반본](../proposals/M0-00/source-content.md)에 보존했다. Word와 색인의 여섯 필드 및 ID 집합을 대조하고, AC를 다시 합치면 원문과 일치하는지 확인한다.
- AC 후보는 독립 시험 수가 아니다. 설명·조건·예외·단계 적용 검토가 남았으며, MAIL-15의 상시 호스트 조건과 APR-02의 휴대폰 경로 등 8개 대표 항목에 검토 메모를 달았다. 요건의 단계 값을 AC의 확정 적용 단계로 복제하지 않는다.
- [공개와 설치 계획](DISTRIBUTION.md)에 GitHub 소스 공개, 일반 사용자 설치, 기여자 환경 구성, 공개 운영의 조건을 기록했다. 루트 README에는 현재 실제 실행 가능한 문서 검사 명령을 추가했다.
- 루트 `AGENTS.md`와 최신 개발 지침에 단일 진행 기록 갱신 규칙을 연결했다. `.gitignore`에는 Python 캐시를 제외하고, `.gitattributes`에는 생성 문서의 LF 줄바꿈과 이미지 바이트 보존을 지정했다. 기존 카드 범위·선행·완료 조건, 요건 ID·우선순위·단계는 바꾸지 않았다.

**검증 결과:**

| 실행 또는 확인 | 결과 |
| --- | --- |
| `python -X utf8 -m unittest discover -s tests/Docs -v` | **21개 통과**. ID 누락·중복·바꿔치기, 설명/수용기준/우선순위/단계 불일치, 빈 기준, 출력 보호, 반복 생성, 문서 변조·누락·추가 파일, ASCII 출력과 다른 문서 덮어쓰기 방지 검사 포함 |
| `python -X utf8 scripts/prepare_requirements.py` | 검토 패키지 생성 성공. 170개 요건, 1단계 P0 144개, AC 후보 341개 |
| `python -X utf8 scripts/prepare_requirements.py --check` | 누락·추가·변경 파일 0개. 검사 모드는 기존 패키지를 수정하지 않음 |
| 원본 대조 | DOCX/CSV 내용 불일치 0개. 이미지 원본 바이트 유지, AC 문장 재결합 일치, 생성 문서 LF 줄바꿈 및 PC별 절대 경로 배제 확인 |
| 문서 연결·상태 정합성 | Markdown 32개에서 로컬 링크·원문 앵커 252개 확인, 오류 0개. 상태장부 151개 중 사람 검토 필요 1개·미착수 150개, 승인자·시각은 미기입 |
| Python 정적 구문 검사 | 도구와 시험 파일 2개 AST 파싱 통과. 별도 린터·타입 검사 설정은 아직 없음 |
| `dotnet build/test`, 제품 보안·설치 검사, `scripts/check-all` | **미실행**. 아직 제품 프로젝트와 전체 검사 스크립트가 없다. 문서 도구 검사로 대체 통과 처리하지 않음 |

회귀 사례를 먼저 추가했을 때 18개 중 10개가 실패했고, 그 원인인 입력·출력 검증 누락과 문장·문단 처리 오류를 수정해 통과시켰다. 이후 Windows 줄바꿈 차이, ASCII 파이프 출력 실패, 다른 문서의 README를 덮어쓰는 문제를 각각 시험으로 재현하고 수정해 최종 21개를 통과했다. 출력 폴더가 비어 있지 않으면 기존 생성 매니페스트와 소유 파일 목록을 확인한다. 시험은 실제 메일 계정이나 네트워크를 사용하지 않았다.

**보존:** Word v2.3 SHA-256은 `c9a28a856672d1d2241e8afea5262c2036e7d46099da4eea5386e2212ae9c8c1`로 유지했다. 과거 개정 폴더는 수정하지 않았다. 최신 모음의 안내·상태장부와 루트 작업 규칙은 이번 요청에 맞춰 갱신했으므로, 과거 시점에 기록한 이 파일들의 체크섬은 현재 파일과 다를 수 있다.

**카드 상태:** `review_required`. 자동 검증은 분할의 보존성과 재현성을 확인하며 기준 문서 채택을 승인하지 않는다. 루트 [AGENTS.md](../../AGENTS.md) §7과 M0-00의 완료 조건에 따라 `docs/requirements/` 전환과 카드 완료는 사람 검토 후 진행한다.

**남은 결정:** M0-00의 원문 대응과 수용기준 분할 검토, ADR-001 코어 런타임, ADR-016 프로젝트 라이선스. GitHub 소유자·저장소 주소·패키지 배포 방식은 공개 실행 전에 확정한다.

**다음 작업:** M0-00 채택 검토를 마치고 런타임·라이선스의 비교안을 확정한 뒤 M0-01 저장소 골격과 빌드를 진행한다. 아직 공개 저장소 생성·코드 업로드·릴리스 발행은 실행하지 않았다.

## 2026-10-29 사용자 검토 1 문서 관리 방식

**제안:** 앞으로 분야별 Markdown을 개발 기준으로 갱신하고, 기존 Word는 원본으로 보관한다.

**사용자 답변:** “네”. 같은 대화에서 확인했다.

**확정 범위:** Markdown 중심의 문서 관리 방식. 이 답변을 런타임·라이선스·미결 계약·보안 구현 승인으로 확대하지 않는다. 세부 검토 질문은 한 번에 하나씩 제시한다.

**다음 검토:** [ADR-001 코어 런타임 비교안](../adr/proposed/ADR-001-core-runtime.md). 공식 지원 정책을 확인하고 C# + .NET 10 LTS를 추천안, Rust·Go를 대안으로 정리했다. 아직 채택·SDK 설치·제품 골격 생성을 수행하지 않았다.

## 2026-10-29 사용자 검토 2 실행 성능과 Mac 이식성

**사용자 조건:** “언어 선택의 최우선은 빠른 속도”. 앞서 빠른 실행과 향후 Mac 포팅 가능성을 질문했다.

**반영:** ADR-001의 개발 편의 중심 C# 추천을 재검토 상태로 바꿨다. Rust를 성능 검증 우선 후보, .NET 10 Native AOT를 비교 후보로 두고 시작 시간·화면/CLI 반응·로컬 검색·처리량·메모리를 동일 조건에서 확인하는 기준을 기록했다. 아직 두 구현의 벤치마크를 실행하거나 성능 우열을 확정하지 않았다.

**이식성 검토:** 두 런타임 모두 Mac용 코어·CLI의 기반이 될 수 있다. Windows GUI·비밀 저장소·승인·알림·설치 경로는 Mac용 구현과 검증이 필요하다. Mac 출시는 아직 확정된 단계 요건이 아니며, 공통 GUI 선택도 미정이다.

**현재 결정 상태:** 문서 관리 방식은 확정, 성능 최우선 선택 기준은 확정, 코어 언어·GUI·Mac 출시 범위는 미확정. 원문 성능 목표(웜 캐시 기본 화면 3초 등)는 유지하며 더 엄격한 목표의 여부를 다음 질문에서 확인한다.

## 2026-10-29 사용자 검토 3 앱 시작 목표

**제안:** 캐시가 준비된 상태에서 기본 메일 목록을 사용할 수 있을 때까지의 목표를 3초에서 1초 이내로 강화한다.

**사용자 답변:** “네”. 같은 대화에서 확인했다.

**반영:** FEATURES.md와 ADR-001에 1초 목표와 원문 기준 환경을 기록하고, 최신 README에서 보관 Word의 3초 목표를 대체한다는 점을 명시했다. 실행 요청부터 목록 사용 가능까지 측정하며 스플래시 표시만으로 달성 처리하지 않는다.

**검증:** 관련 Markdown의 변경과 로컬 링크 36개를 확인했고, 기존 Word의 SHA-256이 동일함을 확인했다. 성능 실측은 아직 수행하지 않았으며 런타임과 GUI는 계속 미확정이다.

**다음 질문:** 향후 macOS 버전을 정식 후속 목표로 포함할지 확인한다. 이 질문에 답하기 전에는 Mac 출시 범위나 일정을 확정하지 않는다.

## 2026-10-29 사용자 검토 4 Mac 후속 지원과 CLI 우선

**질문과 답변:** Windows를 먼저 출시하되 macOS도 정식 후속 목표에 포함할지 물었고 사용자가 “넵”이라고 답했다. 이어 “맥에서도 클리 버전은 우선 사용 가능하지?”라고 확인했다.

**반영:** Windows 우선 출시를 유지하고 Mac에서는 로컬 코어·CLI·최소 승인 도우미를 먼저 제공한 뒤 전체 GUI를 추가하는 방향을 FEATURES.md, DISTRIBUTION.md, 런타임 비교안과 문서 진입점에 반영했다. 루트 작업 규칙에도 후속 방향을 연결했다.

**CLI 범위:** 전체 메일 GUI 없이 동기화·조회·검색·초안·일반 설정을 수행한다. 계정 연결은 시스템 브라우저나 보호된 설정 흐름으로, 클라이언트 등록 승인·비밀 입력·보안 설정·발송 승인은 신뢰할 수 있는 최소 도우미로 처리한다. CLI 확인 입력으로 승인 발급을 대체하지 않는다. 이후 GUI를 추가해도 코어·계정·데이터를 공유한다.

**완료 조건과 제약:** Mac용 비밀 저장소·IPC 인증·사용자 확인·알림·자동 실행·설치와 실제 Mac 검증이 필요하다. Windows Hello의 보호를 Mac에서 충족할 방식은 별도 ADR·계약 검토 대상이다. 현재 사용 가능한 CLI나 설치 패키지가 있다는 뜻은 아니다. Mac 지원 OS·CPU·출시 시점, 런타임과 GUI는 아직 미확정이다.

**검증:** 기존 INST-02·04·05, APR-02~04, CLI-05·06·15와의 승인 경계를 독립 검토했다. 로컬 문서 링크 59개와 원본 Word SHA-256 보존을 확인했다. 이번 변경은 문서이며 제품 실행·Mac 시험을 수행하지 않았다.

## 2026-10-29 사용자 검토 5 공개 라이선스 선택 준비

**준비:** [ADR-016 라이선스 비교안](../adr/proposed/ADR-016-license.md)에 MIT·Apache-2.0·GPLv3의 허용 범위와 의무를 정리했다. 공식 라이선스 원문을 확인하고 요약의 정확성을 별도 검토했다.

**다음 질문:** 다른 사람의 상업 이용·수정·재배포와 수정본 비공개도 허용하는 MIT로 공개할지 묻는다. MIT를 추천안으로 제시하며, 사용자 답변 전에는 확정하지 않는다.

**현재 상태:** 라이선스 미확정. 루트 LICENSE 생성이나 외부 공개는 실행하지 않았다. 선택 후 저작권자 표기와 제3자 의존성 조건을 확인한다.

## 2026-10-29 사용자 검토 5 답변 비상업 조건

**사용자 답변:** “상업적 이용은 금지 수정본 비공개 허용”.

**반영:** 상업적 이용을 허용하는 MIT 추천을 철회했다. ADR-016·개발 지침·공개 계획·진입점에 비상업적 소스 공개 방향과 수정본 소스 공개 의무를 두지 않는 조건을 기록했다. 실제 라이선스는 아직 선정하지 않았으며 LICENSE나 공개 릴리스를 생성하지 않았다.

**다음 질문:** 회사에서 직원이 업무 메일을 관리하는 내부 업무용 사용도 금지하는지, 프로그램 재판매·유료 서비스 제공 등 수익화만 금지하는지 확인한다. 답변 전에는 이 경계를 임의로 정하지 않는다.

**검증 근거:** OSI 정의 §6과 기존 후보들의 공식 원문을 확인하고, 비상업 조건 및 수정본 비공개 조건에 대한 요약을 독립 검토했다. 제3자 라이선스 의무가 자동으로 사라지는 것으로 해석하지 않는다.

## 2026-10-29 사용자 검토 6 업무용 사용 범위

**선택:** 사용자가 2번인 “업무용 사용은 허용하되, 프로그램 재판매·유료 서비스 제공 등 수익화는 금지”를 선택했다.

**확정 범위:** 회사 직원이 업무 메일을 관리하는 내부 사용은 허용한다. 프로그램 자체나 수정본을 판매하거나 유료 서비스로 제공하는 것은 금지한다. 수정본 소스 공개를 강제하지 않는 조건은 유지한다.

**반영:** ADR-016·개발 지침·공개 계획·진입점에서 내부 업무용 사용까지 막는 것으로 읽힐 수 있는 포괄적인 비상업 표현을 정정했다. ‘수익화 제한이 있는 소스 공개’로 안내하고, 이전 기록은 결정 경과로 보존한다. 사용 정책은 확정했으며 실제 라이선스 문구는 검토 전이다.

**다음 질문:** 라이선스에 저작권자로 표시할 이름을 확인한다. 아직 이름을 추정하거나 LICENSE를 생성하지 않았다.

**검증:** 허용·금지 사례와 문서 링크 44개를 확인했다. 이번 변경은 사용 정책 문서이며 제품 코드·시험에는 변경이 없다.

## 2026-10-29 사용자 검토 7 저작권자와 무료 재배포

**사용자 답변:** “Andrew Cho, tzt.kr, account@example.invalid”, 이어 “무료 재배포 허용”.

**확정 범위:** 저작권자 Andrew Cho와 웹사이트·연락처를 사용한다. 원본·수정본의 무료 재배포를 허용하고 수정본의 소스 공개 의무를 두지 않는다. 개인·회사 내부 업무용 사용 허용과 판매·유료 서비스 제공 금지는 유지한다.

**반영:** ADR-016과 최신 README·공개 계획·개발 지침의 현재 상태를 갱신했다. [라이선스 초안](LICENSING-DRAFT.md)에 영문 문안과 한국어 요약을 작성했다. 고지 보존·수정 표시·보증과 책임 조항은 검토 문안이며 사용자가 별도로 확정한 조건으로 기록하지 않는다.

**남은 일:** 라이선스 문안 검토와 채택, 배포 구성 요소의 라이선스 확인. 루트 LICENSE 작성이나 GitHub 게시·릴리스는 아직 수행하지 않았다. 제품 개발의 다음 결정은 실행 성능을 기준으로 한 런타임 비교·채택이다.

**검증:** 7개 문서의 로컬 링크 52개, 저작권자·연락처·무료 재배포 조건 반영, 루트 LICENSE 부재를 확인했다. 독립 문안 검토에서 사용자 결정보다 넓게 해석될 수 있다고 지적한 유료 번들 문구는 제거했다. 제품 코드는 바꾸지 않았으므로 제품 빌드·시험을 실행하지 않았다.

## 2026-10-29 사용자 검토 8 런타임 비교 실험

**승인:** Rust와 .NET Native AOT로 같은 기능의 작은 시제품을 만들어 속도를 비교한 뒤 선택하자는 질문에 사용자가 “네”라고 답했다. 이번 작업은 ADR-001 결정을 위한 별도 승인 실험이며 M0-01 제품 골격 카드의 선행 조건을 완료 처리하지 않는다.

**변경:** `experiments/runtime/`에 공통 실험 명세, Rust와 C# 시제품, Python 기준 결과·테스트·측정기, PowerShell 재현 빌드, 원시 결과를 추가했다. .NET SDK 10.0.403은 Microsoft 파일의 SHA-512 확인 후 `.tools/`에 설치했다. SDK·NuGet 캐시·생성 데이터·빌드 산출물은 `.gitignore`로 제외했다. 시스템 SDK·PATH·백신·전원 설정은 변경하지 않았다. 현재 카드 M0-00은 계속 `review_required`다.

**시험 우선·수정:** 먼저 시제품 미구현 상태의 공통 검사 6개와 측정기 검사 3개가 실패함을 확인한 뒤 구현했다. 독립 검토에 따라 sender/unread를 모두 보관하고 .NET 계정 파서를 일치시켰으며 요약 필드의 양성 검색을 측정에 추가했다. 상위 100개 제한 검사도 추가했다. 이 검사에서 Windows 기본 CRLF 출력 때문에 LF 명세와 충돌한 fixture를 발견해 LF 바이트 생성으로 바로잡았다.

**실측:** 합성 10만 건·5계정·128 KiB 본문, 준비 3회 후 40라운드를 두 번 실행했다. 데이터·바이너리·소스 해시·빌드 설정이 동일함을 확인했다. 모든 측정 응답을 Python 기준 결과와 대조했다. 프로세스 시작 중앙값은 Rust 약 191~193ms, .NET AOT 약 63~64ms, READY working set은 각각 약 31MiB·101MiB였다. 캐시 로드 중앙값은 .NET이 낮았으나 2회차 P95는 Rust가 낮았고, 검색은 작업별로 달랐다. 상세 표와 원시 결과는 [보고서](RUNTIME-BENCHMARK.md)에 연결했다.

**검증:** 공통 동작 7개와 측정기·데이터·통계 3개, 합계 10개 검사를 통과했다. Rust Release 빌드·format·Clippy, .NET Native AOT 게시·format 검사가 통과했다. .NET 네이티브 컴파일의 속도 최적화 플래그도 확인했다. 제품 `check-all`과 실제 GUI·DB·MIME·공급자·승인·Mac 시험은 구현 전이라 실행하지 않았다. 실험의 OPS-01 검사명을 제품 수용 기준 통과로 추적표에 추가하지 않는다.

**최종 회귀 확인:** 기존 문서 도구 검사 21개도 통과했고 `prepare_requirements.py --check`는 누락·변경·추가 파일 없이 유효했다. 관련 문서 로컬 링크 61개, 두 실측 결과의 모든 중앙값/P95·표본 수·현재 소스 및 바이너리 해시를 재검산했다. 보관 Word SHA-256은 기존 값과 동일하다. 새 실험 10개와 기존 검사 21개, 합계 31개 검사 통과를 확인했다.

**다음 결정:** 빠른 프로세스 실행 요구를 근거로 .NET 10 Native AOT를 실제 제품 경로 검증의 우선 후보로 제안한다. 제품 런타임을 최종 채택하거나 1초 목표를 달성했다고 기록하지 않는다. 라이브러리 AOT 호환성과 실제 경로 성능을 확인할 조건을 함께 결정해야 한다.

## 2026-10-29 사용자 검토 9 Rust 채택

**사용자 결정:** “시작이 좀 느리지만 메모리 관리도 중요하니 Rust로 가자”.

**확정 범위:** 코어 데몬과 `hantong` CLI의 구현 언어는 Rust다. 시작 속도와 메모리 사용의 절충을 받아들인 선택으로, 기존 .NET 우선 추천을 대체한다. 웜 캐시 목록 사용까지 1초라는 목표는 유지한다. GUI·개별 crate·비동기 실행기·제품 toolchain 버전·계약은 이번 결정에 포함하지 않는다.

**반영:** 정식 `docs/adr/ADR-001-core-runtime.md`를 작성하고 기존 proposed 비교안은 과거 제안 이력으로 표시했다. 루트 AGENTS의 .NET 기본 검사 명령을 향후 Rust workspace 기준으로 바꾸되 아직 실행 가능한 제품 명령으로 표시하지 않았다. 최신 개발 지침의 C# 전용 라이브러리·검사 제안은 별도 재검토로 표시하고 진입점·기능·배포·검토 문서·실측 보고서의 현재 상태를 맞췄다. 진행 기록의 과거 항목은 보존했다.

**구현 상태:** 비교 시제품·원시 결과·소스 해시·기존 Word·요건/작업 카드 정의는 유지했다. Rust 채택만으로 M0-00을 승인하거나 M0-01 제품 골격을 생성하지 않았다. 모듈 이름이 Rust crate로 바뀌어도 기존 정책·승인·비밀 저장소 등의 사람 검토 경계는 유지한다.

**남은 일:** M0-00 분할본의 기준 문서 채택과 ADR-016 라이선스 문안 검토. 그 뒤 M0-01에서 Rust 제품 골격·고정 버전·실제 검사 명령을 마련한다.

**검증:** 변경한 문서 14개의 로컬 링크 89개를 확인했다. 두 비교 실행의 기록된 소스 해시와 현재 실험 소스가 일치하고, 보관 Word의 SHA-256도 동일하다. 독립 검토에서 Rust 확정 범위와 미확정 항목의 구분, 기존 보안 검토 경계 유지가 일치함을 확인했다. 이번 변경은 결정·안내 문서이며 제품·실험 코드를 바꾸지 않았으므로 빌드·성능 시험은 반복하지 않았다.

## 2026-10-29 사용자 검토 10 라이선스 Version 1.0 채택

**사용자 승인:** 기존 사용 정책에 저작권 고지 유지·수정본 표시·보증 없음 조항을 담은 초안을 정식 라이선스로 채택하는 질문에 “네”라고 답했다.

**반영:** 루트 `LICENSE`를 만들고 `docs/adr/ADR-016-license.md`를 정식 결정으로 작성했다. 승인된 초안의 영문 여섯 조항은 그대로 옮기고 제목의 Draft 0.1만 Version 1.0으로 변경했다. 최신 모음에 `LICENSING.md` 한국어 안내를 추가하고 README·공개 계획·개발 지침·현재 검토 상태를 갱신했다. 기존 초안과 proposed ADR은 보관 이력으로 표시했다.

**확정 범위:** 개인·회사 내부 업무용 사용, 수정본 비공개, 원본·수정본 무료 재배포를 허용한다. 프로그램 판매·대여·접근/사용 대가 청구·유료 호스팅/관리 제공은 금지한다. 재배포 시 고지를 유지하고 수정본임을 표시하며 공식 배포본으로 사칭하지 않는다. 제3자 라이선스의 권리·의무는 그대로 존중하고 보증·책임 조항은 적용 법률이 허용하는 범위로 한정한다.

**다음 작업과 경계:** M0-01의 ADR-001·016 결정 선행 조건은 충족했다. M0-00 분할본의 기준 문서 채택은 별도 사람 검토로 남아 있다. 제3자 구성 요소 배포 조건·고지 확인은 계속 필요하다. 이번 승인으로 GitHub 게시·릴리스·설치 패키지 배포를 실행하거나 M0-00·M0-01 카드를 완료 처리하지 않았다.

**검증:** LICENSE의 여섯 조항이 승인된 초안과 정확히 일치함을 확인했고 독립 검토도 같은 결과였다. 관련 Markdown 13개·로컬 링크 100개를 확인했다. 기존 Word와 실험 소스 해시가 보존되었고 `prepare_requirements.py --check`는 누락·추가·변경 파일 없이 유효했다. 제품·실험 코드 변경이 없어 빌드·성능 시험은 반복하지 않았다.

## 2026-10-29 사용자 검토 11 M0-00 기준 문서 채택

**사용자 승인:** “원문 170개 요건을 보존한 Markdown 분할본을 개발 기준 문서로 채택할까요?”라는 질문에 “네”라고 답했다. 이전에 승인한 Markdown 관리 방식과 구분해 실제 분할본의 채택 승인으로 기록한다.

**반영:** `docs/requirements/`에 17개 영역 문서와 원문 동반본·이미지 2개·AC 원문 대응표를 반영했다. 요건 ID·제목·설명·수용기준 문장·우선순위·단계는 유지하고 채택 상태와 현재 보완 문서 링크만 추가했다. `adoption.json`에 승인 근거·반영 시각·파일 해시·원문/색인 해시를 남겼다. 시각은 승인 반영 시각이며 대화 메시지의 원래 송신 시각을 추정하지 않았다.

**해석 경계:** 341개 AC ID는 추적을 위한 문장별 번호다. 문서 채택을 독립 시험 341개 정의·제품 구현·보안/호환성 시험 통과로 기록하지 않는다. 원문의 조건·예외·채널·단계와 검토 메모를 함께 적용한다. 대응표의 개별 AC 검토 상태는 보존하고 승인된 1초 목표·Mac 후속 방향·Rust·라이선스 결정은 기준 README에서 연결한다.

**검증 준비:** 기준 문서 검사기 `scripts/check_requirements.py`와 회귀 검사를 추가했다. 기존 `prepare_requirements.py`는 계속 검토본만 생성하고 기준 경로로 출력하지 못한다. `docs/traceability.md`는 문서 채택과 제품 시험을 구분한 초기 요약이며 실제 요건↔제품 시험 자동 연결은 M0-04 범위로 남긴다.

**범위:** 원본 Word·생성된 제안 패키지·기존 작업 카드 정의·비교 시제품과 원시 결과를 보존한다. 이번 카드는 M0-00 채택이며 제품 M0-01 코드는 아직 생성하지 않았다. 현재 계획 정의·상태장부는 계속 `docs/latest/`에서 관리한다.

**최종 검증 및 완료:**

| 실행 또는 확인 | 결과 |
| --- | --- |
| `python -X utf8 -m unittest discover -s tests/Docs -q` | **29개 통과**. 기존 생성 도구 21개와 채택 검사기 8개. 내용 변경·누락·중복·잘못된 링크·경로 이탈·중첩 추가 파일 검출 포함 |
| `python -X utf8 scripts/check_requirements.py` | `valid: true`, 요건 170개·AC 341개·영역 파일 17개, 오류 0개 |
| `python -X utf8 scripts/prepare_requirements.py --check` | 보관 제안본의 누락·추가·변경 파일 0개 |
| 원문과 변경 범위 대조 | 제안본 23개 파일·작업 카드 정의 해시 유지. 영역 문서는 채택 안내만 변경. 원문 동반본·AC 대응표·이미지 2개는 제안본과 바이트 일치 |
| 문서 연결과 상태 | 현재 안내·기준 문서 29개의 로컬 링크 320개 정상. M0-00만 `complete`, 나머지 150개 카드는 `not_started` 유지 |
| 독립 검토 | 기준 요건 원문 보존, 보완 결정 연결, 제품 완료와의 구분을 확인했고 채택 차단 사항 없음 |

상태장부의 승인자는 Andrew Cho이며 `approved_at_utc=2026-10-29T12:34:51Z`는 채택 기록과 같은 **승인 반영 시각**이다. 사용자 메시지의 실제 송신 시각을 뜻하지 않는다. 승인 근거는 이 절에 기록한 실제 질문과 답변이다.

**카드: M0-00 / 상태: 완료.** 제품용 `check-all`·빌드·보안 검사는 아직 제품 작업공간과 명령이 없어 실행하지 않았다. 이 카드의 문서 검증을 해당 검사들의 통과로 대신 기록하지 않는다. **다음 추천 카드: M0-01 저장소 골격과 빌드** — M0-00·ADR-001·ADR-016 선행 조건 충족.


## 2026-10-29 연속 개발 — 기반부터 제품까지

**사용자 지시:** “제작 완료까지 일단 묻지말고 진행해”. 기존 세션당 한 카드와 일상적 선택마다 확인하는 절차를 이번 연속 개발에는 적용하지 않는다. 기존 제품 요건·라이선스·사람 승인 원칙은 유지한다. 자동 판단을 사람의 코드 검토 완료로 기록하지 않는다.

**실행 계획:** Rust workspace로 논리 경계를 고정하고, 실행 가능한 검사·계약·가짜 공급자를 먼저 만든다. 이후 인증과 로컬 저장, CLI, 계정/메일 읽기, 쓰기와 승인, 자동화, GUI 및 패키징 순서로 기존 카드의 선행 관계를 따른다. 수정한 코드의 실패 사례를 먼저 시험하고 구현 후 전체 검사를 수행한다. 공식 문서로 확인한 기술 선택은 ADR에 기록한다.

- [x] M0-01: `Cargo.toml`, `rust-toolchain.toml`, `crates/`, `apps/` 생성. 외부 런타임 의존성 없이 컴파일 경계와 `hantong` 실행 파일 이름을 검증했다.
- [x] M0-01: `scripts/check_all.py`, `scripts/check-all.ps1`, `scripts/check-all`에서 문서 검사와 `cargo build/test/fmt/clippy`를 실행하고 실패를 종료 코드로 전달한다. 첫 단계에는 제품 보안 테스트가 없다는 사실을 출력한다.
- [x] M0-02: `.github/workflows/check.yml`에서 Ubuntu·Windows matrix, pinned official actions, read-only permissions, dependency fetch, offline common checks, always-run aggregate를 구성했다. [CI 기준](CI.md)과 구조 테스트를 추가했다. 실제 GitHub 실행과 branch protection 등록은 아직 외부 상태다.
- [ ] M0 기반 후속: 의존 규칙, 추적표, 가짜 공급자/공격 말뭉치, 계약과 보안 경계의 실제 거부 시험을 독립 작업으로 구현·검증한다.
- [ ] M1 이후: 코어·인증·저장소·CLI의 실행 경로를 연결한 뒤 공급자와 메일 기능을 추가한다. 미구현 보호 경로는 거부한다.
- [ ] UI와 배포: 한국어/영어·테마를 첫 화면부터 적용하고 설치/개발 명령과 패키지를 검증한다. 실제 계정 확인과 사람 보안 검토, GitHub 게시의 실행 여부는 따로 기록한다.

**시작 조건:** 제품 소스는 아직 없고 Rust 1.92.0 MSVC가 설치돼 있다. 문서 요건과 원본 비교 실험은 보존한다. 이번 구현에서 공개 저장소 주소나 실제 계정 정보를 임의로 만들지 않는다.


### 기반 구현 중간 검증

- M0-01: Rust 1.95.0/edition 2024 작업공간, 공통 빌드·서식·Clippy·문서 검사 완료. 골격 단계 공통 검사는 실제 Windows에서 통과했다. 현재 runtime 비교 실험과 제품은 분리돼 있다.
- M0-02/03: Windows·Ubuntu CI 구성과 Cargo 의존 경계 검사를 추가했다. 실제 GitHub 실행·병합 필수 검사 설정은 아직 수행하지 않았다. 보안 경계의 사람 검토도 에이전트 검토로 대신하지 않는다.
- M0-04: 341개 AC 추적표 생성과 `--check`를 추가했다. 단계 적용을 원문 숫자로 자동 확정하지 않고, 단순 태그/텍스트 파일을 시험 통과로 세지 않는다. 자기 기입한 승인자만으로 비적용 처리하는 경로를 차단했다. 현재 전체 AC를 통과로 기록한 것은 0개다.
- M0-06/09: 가짜 Gmail·Graph·SMTP·OAuth의 계정/상태/재생·응답 유실 시나리오와 합성 주입 메일 20개·EUC-KR·손상 MBOX를 추가했다. 이는 시험 도구이며 실제 제공자 호환성 통과가 아니다.
- 초기 CLI: 영어 기본·한국어 리소스가 실행 화면에 연결됐다. `--help`, `--version`, JSON 오류, `--no-input`을 시험하며 미등록 조회/발송은 거부한다. CLI/계정 전체 기능은 미완료다.
- 독립 검토 보완: 가짜 OAuth를 계정/저장 state에 결속하고, 추적표 증거 검증·CLI 실제 stdin 비노출 시험·검사 timeout을 강화했다. Windows Secrets의 실패 시 메모리 정리·임시 파일·경로 검사를 추가 보완 중이다.
- 외부 환경 제약: Docker가 설치되어 있지 않아 GreenMail 개발용 구성을 실제 기동하지 못했다. 제품 실행에 Docker/WSL을 요구하지 않는다. 이 제약으로 독립 구현을 중단하지 않는다.

### M0/M1 기반과 Windows 코어 연결 검증

**변경:** `hantong-platform`에 Windows pipe ACL·사용자/로그온 SID 검사를 분리하고, 코어 foreground 실행과 `hantong core status`를 연결했다. 초기 IPC는 길이/깊이/중복 키/노드 수를 제한하고 버전·최소 readiness만 반환한다. `mail.*`는 서버에서도 거부한다. CLI의 영/한 문구·JSON 공통 envelope·잘못된 Unicode 인자 비노출을 검증했다. core 로그는 외부 문자열을 받지 않는 고정 이벤트다.

**시험에서 찾고 수정한 문제:** Secrets의 디렉터리 handle에 `FILE_LIST_DIRECTORY`를 추가해 저장 루트 교체를 막았고, 기존 값을 보존하는 파일 교체와 실패 시 버퍼 정리를 시험했다. IPC의 최소 DACL에는 Windows CreateFile이 요구하는 `FILE_READ_ATTRIBUTES`를 추가했다. 같은 Tokio/Mio pipe handle을 재사용하면 이전 EOF가 다음 연결에 남는 회귀가 나타나, 연결마다 handle을 새로 만들고 별도 Global named mutex의 객체 수명으로 단일 코어를 유지하도록 고쳤다. 처음 생성 플래그·명시적 ACL은 모든 재생성에 적용한다. peer 토큰 검사 전 0x01 preface를 읽고 impersonation은 RAII로 복귀한다. 응답 후 0x06 수신 확인을 받아 미수신 데이터를 버리지 않게 했다. 이 바이트는 승인·인증 증거가 아니다.

**계약 보완:** 도구 응답의 성공/오류 분기를 나누고 발송 의도에 `draft_id`와 `idempotency_key`를 추가했다. 논리 모델의 원문 필드를 대조하고 계정 연결 `ConnectionSession`과 IPC 인증 세션을 구분했다. JSON Schema는 아직 API 활성화 전 초안이며 현재 검사는 구조·로컬 참조 검사다. Draft 2020-12 전체 validator 실행은 남아 있다.

| 실행/확인 | 실제 결과 |
| --- | --- |
| `python scripts/check_all.py` | Windows **종료 코드 0**, 문서 29개·빌드 도구 36개·Rust 66개 시험, 총 **131개 통과**. build·fmt·Clippy·기준/제안 무결성·의존 경계·추적표 대조 통과. 로그 `target/foundation-core-check.log` |
| Windows OS 전송 | 8개 시험. 명시적 DACL·generic-write 거부·user/logon 불일치 거부·스레드 토큰 복귀·잘못된 preface·accept 취소·연속 20회 왕복·인스턴스 잠금 확인 |
| Windows 비밀 저장소 | 12개 시험. 실제 DPAPI 왕복·변조·계정/종류 결속·과대 입력·읽기 실패·원자 교체·junction/경로 교체·디버그 비노출 확인 |
| 실제 두 프로세스 smoke | 코어 시작 → 영문 JSON/한국어 상태 응답 → 두 번째 코어 `CONFLICT(13)` → 미등록 계정 `CLIENT_NOT_REGISTERED(22)` → Ctrl+C에서 stopping/stopped 이벤트 → 종료 뒤 `CORE_UNAVAILABLE(25)` 확인. PTY shell의 Ctrl+C 종료 값은 코어의 단독 exit 값으로 간주하지 않음 |
| Linux x86_64 / macOS arm64 대상 | `cargo clippy --workspace --all-targets --target <target> --locked --offline -- -D warnings` 양쪽 통과. **교차 컴파일 검사이며 해당 OS에서의 실행·제품 지원 완료가 아님** |
| 독립 검토 | 현재 Secrets와 Windows IPC 구현에서 추가 critical/high 차단 사항 없음. 에이전트 검토이며 사람 보안 검토를 대체하지 않음 |

**남은 일:** CNG 상호 인증·등록/철회·공통 정책 실행기, 암호화 DB, 메일 읽기/쓰기·승인·자동화·GUI·패키징은 계속 구현해야 한다. 실제 다른 Windows 사용자/로그온 세션·원격 접속·실제 제공자·깨끗한 설치와 GitHub CI는 아직 확인하지 않았다. 개별 AC 전체를 통과로 표시한 수는 여전히 0이며, 현재 단위/경계 시험을 완제품 요건 통과로 부풀리지 않는다. M0/M1 전체 완료나 출시를 선언하지 않는다.

### M1-04 암호화 저장소와 빌드 환경 준비 중

- `ADR-004`, `contracts/database.md`, `db-schema.sql`에 Secrets 소유 키·초기 3개 테이블·암호화 온라인 백업·원자 마이그레이션 경계를 정의했다. M0-11 검색 문법과 M1-05 설정의 계약 초안도 작성했다. 원본 요건과 151개 카드 정의는 유지한다.
- Windows Secrets에 별도 DB 키 범위, CNG 난수·DPAPI, raw-key 67바이트 전달·메모리 정리, 데이터/키 잠금과 파일 ID 비교, 같은 키의 검증된 백업을 구현 중이다. 기존 키 누락·손상·미확인 sidecar는 파일을 변경하지 않고 거부한다. 자동 크래시 복구는 아직 연결하지 않는다.
- Store는 설정 CAS와 schema v1 초기화·검증을 담당한다. 마이그레이션은 백업 검증 후 트랜잭션으로 수행하며 실패 시 변경을 롤백한다. 스키마 전체 DDL을 대조해 변형된 컬럼·제약·인덱스·추가 트리거를 거부하도록 보강했다.
- 공식 NASM Windows 바이너리는 이 환경에서 `-v`의 리디렉션 출력이 비어 OpenSSL 구성이 실패했다. 다른 버전에서도 재현했다. 공식 NASM 3.02의 변경 없는 소스를 MSVC로 빌드하자 Perl backtick 버전 검사와 Win64 fixture 조립이 통과했다. 원인은 PATH 누락으로 기록하지 않는다. 소스 tag object는 `8f1fb545a582c55c69607f457b4d1e71c19b2ecf`, 실제 commit은 `4a56d66ed9626d5a3ded5414c9d8b7f1a48ce065`다. 도구는 무시된 `.tools/` 내부에만 두고 시스템 PATH·레지스트리는 변경하지 않았다.
- `scripts/cargo_native.py`와 공통 검사기에 검증된 로컬 도구를 자식 프로세스 환경으로만 전달하는 경로를 추가했다. CI에는 Windows 도구 준비 단계를 추가했다. 실제 SQLCipher vendored 빌드를 진행 중이며, 이 시점에는 Windows 암호화 DB 시험 통과를 주장하지 않는다.
- M1-07 진단 경계에 `DiagnosticEvent`를 추가했다. 이벤트·심각도·UTC 시각·안정 오류 코드·수치 지표만 JSON 한 줄로 직렬화하고, 자유 텍스트와 민감 문자열은 API에서 받지 않도록 했다. 전체 마스킹 헬퍼와 JSON key allowlist·줄바꿈 주입·음수 시각·지표 오버플로 시험 4개가 애플리케이션 crate에서 통과했다. Cargo.lock의 애플리케이션 의존성도 갱신했다.
- 추적표의 시험 증거 소스 해시에 빌드 도구·native manifest·계약과 SQL 스키마를 포함했다. 스키마를 바꾼 뒤 이전 테스트 증거를 재사용하는 회귀를 실패 시험으로 확인하고 수정했다. 현재 추적 도구 시험 10개, 설정 계약 구조 시험 6개가 통과했다. 전체 AC 완료 수는 여전히 0이다.

**상태:** 구현·검증 진행 중, 사람 보안 검토 필요. 실제 DB 암호화·백업·rollback·전체 workspace 검사는 네이티브 빌드 완료 후 이어서 수행한다.

### M1-04/05/07 저장소·설정·로그 검증

**실행 결과:** 소스에서 빌드한 NASM으로 SQLCipher **4.19.0 community**의 실제 링크·실행을 확인했다. 최초 OpenSSL 포함 빌드는 16분 35초였다. 고정 GitHub source archive에서 NASM을 다시 빌드하는 설치 스크립트도 실제 실행했고, 생성된 도구의 버전·Win64 COFF·파일 해시·완료 marker 확인을 통과했다. Windows에서 child PATH만 지정하면 `nmake` 탐색에 실패하는 문제를 발견해, 캡처한 MSVC PATH에서 실행 파일을 먼저 절대 경로로 찾도록 수정했다. 시스템 환경은 바꾸지 않았다.

| 검증 | 결과 |
| --- | --- |
| `python scripts/check_all.py` | **종료 코드 0**. 문서 29개·빌드 도구 58개·Rust 107개, 총 **194개 통과**. 전체 build·fmt·Clippy·의존 경계·기준/제안 무결성·추적표 대조 통과. `target/storage-full-check.log` |
| Windows 암호화 저장소 | 기존 DPAPI 12개 + DB 내부 2개 + DB 통합 12개. 실제 암호화 왕복·키 없는 복사본·키 누락/변조·손상·journal/temp 원문 비노출·잠금·junction·경로 분리·암호화 백업 확인 |
| Store | 11개. schema v1·metadata/버전·DDL 전체·계정 제약·CAS·백업 실패 시 migration 미실행·실제 DDL/DML 후 rollback·버전 원자 갱신 확인 |
| 설정 | 도메인 9개·암호화 저장 3개. 알 수 없는/중복 필드·명시 null·입력 크기·IANA 시간대·DST gap/fold/날짜 건너뜀·오류 비반향·CAS·보호 설정 변경 거부·재개방 확인 |
| 진단 | 공통 타입 4개 + 코어 로그 경계 시험. 자유 텍스트 없는 JSON 이벤트와 전체 마스킹. 코어 시작/종료 경로에 연결. SQLCipher의 직접 stderr 진단은 끄고 안정 오류만 반환 |

검증 snapshot source hash: `9d1a8cdf5130e26fd75fa61467f7504739f7d64c6c0b88e87cb45084a400a9b3`. 이후 코어 경로 연결은 별도 변경으로 다시 검증한다.

**수정한 결함:** SQLite의 u64 SQL 변환 지원 차이를 실제 Windows 컴파일에서 확인해 범위 검사 후 i64로 바꿨다. 스키마 이름 필터는 `_`가 와일드카드인 LIKE 대신 GLOB을 사용해 숨은 추가 객체를 놓치지 않게 했다. JSON 설정의 선택 언어는 `null`을 허용하지 않고 미설정 필드를 생략하도록 스키마와 맞췄다. 잘못된 날짜가 panic을 일으키던 상수 생성자를 오류를 반환하는 생성자로 바꿨다.

**남은 경계:** Store·설정은 아직 정상 코어 startup/인증된 IPC에 연결되지 않았다. 자동 크래시 복구·백업 복원 UI·키 회전·등록/철회·메일·승인·GUI는 미완료다. 실제 다른 사용자/로그온 세션과 사람 보안 검토도 남아 있다. M1-04는 **사람 검토 필요**, M1-05/07은 후속 연결 진행 상태다. 194개 시험을 전체 P0 AC 완료 수로 환산하지 않는다.


### 2026-10-30 — M1-01/02/05/10 코어 저장소와 인증 기반

**코어 연결:** Windows Known Folder API로 데이터·비밀의 고정 형제 경로를 구한다. 환경 변수 덮어쓰기를 자식 프로세스 시험으로 거부했고, API 반환 메모리는 실패해도 해제한다. 코어는 암호화 DB·schema·저장 설정을 검증한 뒤 IPC를 열며 종료까지 Store 잠금을 유지한다. 최초 시간대는 UTC이고 기존 설정은 유지한다. 두 startup 시험에서 저장 설정 보존·동시 열기 거부·키 누락 시 DB 보존을 확인했다.

**실제 실행:** 새 코어 바이너리의 정상 사용자 데이터 경로에서 시작 이벤트를 확인했다. 별도 CLI 프로세스의 영어 JSON·한국어 상태 조회, 두 번째 코어의 CONFLICT, 미등록 계정 조회 거부, Ctrl+C stopping/stopped, 종료 뒤 CORE_UNAVAILABLE를 확인했다. 메일 계정·실제 서비스는 사용하지 않았으며 실행한 코어는 종료했다. Windows 개발 실행 명령은 네이티브 도구 환경을 주입하는 cargo_native.py 경로로 README에 반영했다.

**인증 기반:** canonical binary transcript의 역할·epoch·시각·양측 nonce·키 해시·OS SID·요청 범위를 계약과 정확한 바이트 배치 시험으로 정의했다. Windows CNG의 현재 사용자 signing-only/non-exportable P-256 키를 실제 생성·재개방·서명·검증한다. 개인 키 export policy를 읽어 검증하고 private blob export가 실패하는지 확인한다. 이 Windows KSP는 정책 0의 private export에 NTE_NOT_SUPPORTED를 반환하므로 NTE_PERM 하나만 기대하던 시험을 공식 문서와 실제 결과에 맞게 고쳤다. 공개 blob의 비정렬 슬라이스 접근·키 이름 Unicode 검증·native handle 해제 순서를 보강했다.

서버 인증 상태는 OS로 확인한 연결을 소유하고 성공·실패 모두 한 번 소비한다. 실제 pipe와 CNG로 정상·재사용·역할 반사·변조·UTC 만료·시계 역행·단조 시계 만료·epoch 변경·철회·등록 제거·키 변경·빈 계정 범위 등 12개 시나리오를 한 통합 시험에서 확인했다. 권한은 요청/등록 범위의 교집합이며 축소·철회 뒤 연결을 닫는다. transcript 데이터 객체 자체는 인증 증거가 아니며 서버가 키 해시와 관찰한 OS context를 계산한다. 클라이언트 쪽 검증도 별도 구현·강화 중이다.

**진단 기반:** M1-10의 고정 섹션 선택·미리보기와 내보내기의 동일 JSON·범위 제한 시험 3개가 통과했다. 아직 없는 동기화/최근 작업 정보는 unavailable이다. 원문·주소·토큰·경로·자유 텍스트를 받는 필드는 없다. 실제 상태 수집·확인 화면·파일 내보내기는 후속 연결이다.

**현재 검증 범위:** 플랫폼 16개, IPC 서버 15개, contracts 4개, application 7개 대상 시험이 통과했다. macOS arm64 대상 workspace Clippy도 통과했으며 실제 Mac 실행을 뜻하지 않는다. 이 기록 이후 클라이언트 인증 검증을 통합하고 전체 검사를 다시 실행한다.

**남은 일:** 인증 wire envelope·영속 등록부·서버 키 pin 배포·승인 도우미와 공통 정책 실행기가 연결되지 않아 제품 bootstrap의 mail_ready/authenticated는 계속 false다. 위 CNG/pipe 시험의 전송 fixture를 제품 handshake 완료로 표시하지 않는다. 실제 다른 사용자·로그온 세션·원격 파이프와 사람 보안 검토는 별도 미검증이며 전체 P0 완료/출시를 선언하지 않는다.


**양측 인증 통합 후 전체 검사:** `python scripts/check_all.py`가 종료 코드 0으로 끝났다. 문서 29개·빌드 도구 58개·Rust 127개, 총 **214개 통과**. 전체 build·fmt·Clippy·요건/제안 무결성·의존 경계·추적표 대조 통과. 로그는 `target/auth-storage-full-check.log`, 검증 source hash는 `87cbac4c6bbf1ed2338bd35e0608cfefa3a7b7df5f5b42befe98e886c6b9d23c`다. 클라이언트는 OS nonce/context를 내부에서 만들고 등록 ID·epoch·범위·양측 키 해시·시각을 대조한 뒤 고정 서버 키의 서명을 확인한다. 의미 필드를 바꾼 서버가 다시 서명해도 거부하는 시험을 포함했다. 서버 instance/nonce는 서버의 연결 상태와 서명에 결속한다.

**시험 정리 정정:** 초기 CNG 구현 시험이 남긴 이 프로젝트 이름 공간의 테스트 키 5개를 확인해 제거했다. 모든 제품 인증 경로는 아직 비활성이고 해당 키는 이 세션 시험에서만 생성됐다. 최종 시험은 소유한 키의 cleanup guard를 사용한다. 잔여 키 0개를 확인한 뒤 CNG 시험 5개를 다시 실행했고, 다시 잔여 0개를 확인했다. 테스트 성공만으로 OS 자원 정리를 완료했다고 간주하지 않는다.

**다음 범위:** 서버 역할 전용 CNG 키 이름/타입, 복구 가능한 서버 identity, DB v2 등록부와 마이그레이션을 구현한다. 전체 GUI 전에도 최소 승인 도우미는 필요하며, 클라이언트 등록의 사람 확인·서버 pin 전달·인증 wire 연결 이전에는 보호된 제품 명령을 활성화하지 않는다. 같은 사용자 악성코드의 KSP 키 사용을 software CNG/ACL만으로 차단했다고 주장하지 않는다.


### 2026-10-30 — DB v2·서버 신원 영속화 진행

`registration-store.md`와 v1 보존 DDL·v1→v2 고정 migration·전체 v2 DDL을 추가했다. 기존 설정/계정 테이블은 보존하며 서버 신원과 등록 요청·등록/계정/도구 범위용 5개 빈 테이블을 더한다. migration은 검증된 v1과 암호화 백업 후 한 트랜잭션에서 실행하고 metadata/user_version·전체 새 DDL 검사까지 통과해야 commit한다. 신규 DB에는 active 등록을 넣지 않는다. production migration과 rollback 시험이 같은 내부 함수를 사용하도록 정리했다.

CNG 서버 키를 `ServerSigningKey`와 별도 server namespace로 분리했다. client 키 API로 서버 키를 열 수 없으며, 동일 ID 생성 충돌 뒤 기존 공개 키가 유지되는지 확인한다. client/server의 native 생성·열기·서명·공개 키·삭제 처리는 공통 내부 구현을 사용한다. 두 역할 모두 개인 키 export policy 0과 signing-only 속성을 검증한다.

Store는 creating 서버 identity를 먼저 저장하고 공개 키/버전을 CAS로 active 전환한다. 코어 시작 coordinator는 생성 중 중단되면 같은 key_id로 이어가고, active 키가 없거나 공개 키가 바뀌면 DB와 기존 키를 보존하고 실패한다. 새 신원·재개방·creating 키 있음/없음 복구·잘못된 CAS 후 복구·active 키 누락/불일치 시험을 실제 암호화 DB와 CNG로 수행했다. 코어 대상 시험 8개와 도움말/오류 통합 시험 2개가 통과했다.

현재 pending 요청의 형식·수량·만료 제한과 등록 read model을 추가 구현 중이다. 승인 도우미가 없는 상태에서 active 클라이언트 생성 API는 열지 않는다. 이 v2 변경은 앞선 214개 검사 이후의 변경이므로 전체 검사·교차 검사·추적표를 다시 갱신해야 한다. 아직 정상 사용자 프로필을 v2/server identity로 재기동한 smoke는 실행하지 않았다.


**추가 수정/검증 중:** 등록 read model의 초기 구현이 요청 scope를 실제 grant로 사용하고 SELECT 컬럼 순서를 잘못 읽는 결함을 발견해 교체했다. 이제 정규화된 grant 테이블만 권한 목록으로 반환하고, 하나의 읽기 트랜잭션에서 원본 요청·승인 채널/시각·상태와 실제 계정 참조를 검증한다. DB 문자열/blob은 길이를 확인한 후 복사하며 129번째 grant로 과대 집합을 거부한다. 만료는 Store UTC와 CAS를 사용하고 DB 오류를 Conflict로 감추지 않는다. 독립 코드 검토에서 추가 지적 없음. 합성 등록 회귀 시험은 Windows native 빌드 완료 후 실행한다.

CNG OpenKey의 모든 오류를 NotFound로 바꾸던 경로를 수정했다. 문서상 실제 키 누락인 NTE_BAD_KEYSET만 NotFound이며 권한/provider 등 다른 오류에서는 생성 복구로 진행하지 않는다. Windows 플랫폼 시험 20개가 통과했고, 정상 사용자 코어를 v2로 실행하기 전 CNG 제품 namespace 잔여 테스트 키 0개를 확인했다.

**M1-09 입력 경계:** `tool-validation.md`와 두 도구 스키마의 계정 수/버전 상한에 맞춰 순수 타입 검증기를 구현했다. 계정 조회와 발송 의도의 필수 필드·unknown/null·계정/ID 형식·128개 범위·중복·i64 버전·hash 대소문자와 Debug 비노출을 검사한다. Application 시험 총 19개(새 도구 입력 시험 12개)가 통과했다. 이 타입 검증기는 인증·승인 증거가 아니며 제품 메일 dispatcher는 계속 비활성이다. 감사·정책 공통 실행기는 후속 구현이다.


**후속 승인 설계:** [ADR-015](../adr/ADR-015-human-verification.md)에 코어 소유의 지연 승인 창과 직접 Windows Hello 결과 수신을 기록했다. UserConsentVerifier는 서명 영수증을 반환하지 않으므로 별도 helper의 Verified 주장이나 nonce 복사만으로 사람 확인을 인정하지 않는다. INST-04의 코어 소유 창과 맞추고 canonical snapshot·세션·hash·epoch·만료·CAS를 코어 내부에서 검증한다. Windows 11 build 22000 이상을 보호 승인 지원선으로 정했다. 현재는 공식 API 조사와 설계 검토이며 UI 코드·Hello 설정/실제 사람 확인·승인 발급은 아직 구현/실행하지 않았다. 사람 보안 검토는 남아 있다.


**Windows 실제 검증:** 네이티브 라이브러리 재빌드 후 코어·CLI build가 종료 코드 0으로 끝났다(19분 43초). Store 시험은 테스트 fixture의 Rust 값 소유권 오류 한 곳을 수정한 뒤 단위 29개·설정 통합 3개, 총 **32개 통과**했다. 잘못된 SELECT 컬럼·승인 metadata·요청/실제 grant 분리·64/1024 한도·만료/CAS·최대 범위 JSON과 v1 migration 회귀를 포함한다. `target/registration-store-check.log`에 결과를 저장했다.

정상 사용자 데이터의 v1 DB로 코어를 시작해 `backup-before-v1.db`(32,768 bytes), v2 DB(94,208 bytes), 새 서버 CNG 키 1개를 확인했다. 영어 JSON/한국어 core status는 0, 미등록 accounts list는 CLIENT_NOT_REGISTERED(22), 두 번째 코어는 CONFLICT(13), 정상 종료 뒤 status는 CORE_UNAVAILABLE(25)였다. 재시작 후 동일 서버 키 선택자와 DB byte hash 유지, 다시 정상 종료를 확인했다. 현재 코어는 실행 중이지 않으며 정상 앱 서버 키 1개는 다음 실행을 위해 **보존**한다. 이후 테스트 cleanup은 자신이 생성한 키만 제거해야 한다. 실제 계정·메일은 사용하지 않았다.

전체 workspace build/test/fmt/Clippy와 문서·의존·추적 검사를 `target/registration-v2-full-check.log`에 실행 중이다. 완료 전에는 이번 v2 변경 전체 통과를 주장하지 않는다.


**v2 통합 검증 완료:** `python -X utf8 scripts/check_all.py` 종료 코드 **0**. 문서 29개·빌드 도구 58개·Rust 166개, 총 **253개 통과**. workspace build·fmt·Clippy·구조 의존 검사·기준/제안 문서 무결성·추적표 대조 통과. 로그 `target/registration-v2-full-check.log`, 로그 SHA-256 `1844692bc941c6baee235ca0ba69b9419beab129350e5854cd72b58f8de9d07c`, source hash `d7e0a98c55ed45e21331501e48278cc6688bdc731aa948eb362f94ca108bd196`. 검사 후 정상 서버 CNG 키 1개가 그대로 유지되고 잔여 테스트 키 0개임을 확인했다.

전체 AC 341개 중 완제품 범위를 통과로 표시한 수는 여전히 0이다. 구현된 기반의 시험과 전체 기능 완료를 구분한다. 다음은 M1-08 영속 감사 시작/결과 기록과 M1-09 공통 실행 경로다. 감사 기록만으로 인증·승인·발송 권한이 생기지 않도록 유지하며, UI와 실제 메일 연동을 계속 구현한다.


### 2026-10-30 — M1-08 영속 감사 기반 구현 중

`audit.md`에 개인정보를 담지 않는 start/terminal 기록, 실제 서버 반환 결과, 정규 계정 참조, 100,000 실행/1,000,000 참조 한도, 최대 100개 조회를 정의했다. 감사는 인증·승인 증거가 아니며 terminal 없는 상태는 Unresolved로 유지한다. 용량/저장 실패 때 기록을 자동 삭제하거나 서비스를 실행하지 않는다. 실제 발송에는 추후 approval/Outbox/감사를 함께 갱신하는 전용 트랜잭션이 필요하다.

DB v2 DDL을 보존하고 audit 4개 테이블을 추가한 v3 및 v1→v2→v3 순차 migration을 구현했다. 각 단계는 개별 원본 검증·암호화 백업·commit 전 새 DDL 검증을 수행한다. v3 startup은 감사 카운터와 실제 행 수도 대조하며, 불일치를 자동 복구하지 않는다. v1 실패 시험이 잘못된 v3 기대 스키마 때문에 실패하지 않도록 v2 기대값으로 바로잡았다.

순수 audit 타입은 private 필드와 범위 검증을 사용하며 임의 문자열·원문·승인 주장을 받지 않는다. Store 감사 API를 테스트 먼저 추가한 뒤 구현했다. Windows 감사 대상 시험 **12개 통과**: 시작/재개방·성공/오류 결과·중복 거부·등록/epoch·용량·페이지·철회 뒤 이력·손상·트리거 오류에 대한 원자 rollback. 로그 `target/audit-store-check.log`. 새 기능은 앞선 253개 전체 검사 이후 변경이므로 전체 검사는 다시 필요하다. 쓰기 connection의 DELETE/EXTRA 설정·검증과 독립 검토를 이어서 진행한다.

**v3 검증 완료:** 문서 29개·빌드 도구 58개·Rust 191개, 총 **278개 통과**, 전체 `check_all.py` 종료 코드 0. build/fmt/Clippy·의존 경계·기준/제안 무결성·추적표 검증 통과. 로그 `target/audit-v3-full-check.log`, SHA-256 `C9A599A5ED93F44ECB6B842EE2BE5C9F487009B409B2247553FD7F824BCBA666`, source hash `edee169d4a1a19f081e5bed14778b30b420d8098e6935c0f7cf1c2e24b254320`. Linux x86_64·macOS arm64 workspace all-target Clippy도 통과했다. 실제 Mac 실행 증거는 아니다.

독립 검토에서 당시 클라이언트 종류·이름을 감사에 보존하지 않는 누락을 발견했다. 실패 회귀 시험 후 v3 시작 행에 검증된 등록부의 스냅샷을 넣고, 등록 메타데이터 변경/손상 뒤에도 당시 요청자를 조회하도록 수정했다. 공통 ClientKind/표시명 검증을 재사용하며 Debug는 표시명을 숨긴다. Store 48개 단위+설정 통합 3개, contracts 10개 대상 검증도 통과했다. 쓰기 connection의 DELETE+EXTRA 명시 설정과 재조회 검증을 반영했다. 실제 전원 차단 안전성을 증명한 것으로 표시하지 않는다.

정상 사용자 데이터에서 v2→v3 기동·영어 JSON/한국어 상태 조회·정상 종료·재시작을 확인했다. `backup-before-v2.db` 94,208 bytes와 v3 DB 126,976 bytes가 생성됐다. 종료 후/재시작 후 DB SHA-256은 `CC171D6DABC8D9C00FA07234EB1C11B1C98CB1527E34C29DB95452C2E92736FD`로 같았다. 정상 서버 CNG 키 1개도 유지되고 잔여 테스트 키는 없다. 실행 중 파일 잠금으로 hash 조회가 거부되어 종료 후 읽었으며, 실제 계정/메일은 사용하지 않았다. 코어는 종료한 상태다.

**남은 범위:** M1-08의 대상/승인/복구 연결과 M1-09 실행기, 인증 wire·등록 UI는 후속 작업이다. 사람 보안 검토와 전체 AC 완료 판정은 별도이며 완료로 표시하지 않는다. 이후 공통 실행 경로 변경은 이 278개 snapshot 이후 변경으로 다시 검증한다.

### 2026-10-30 — M1-09 공통 실행기·M1-11 자동 시작 기반

공통 실행기는 outer JSON-RPC method/inner tool 일치, 엄격한 입력 검증, 연결 재검증, 감사 시작, 전체 계정 scope, 정책, 읽기 서비스, 실제 결과 감사 순서를 고정한다. 시작 실패는 서비스 미호출, 결과 저장 실패는 INTERNAL_ERROR·무재시도다. send callback은 없으며 입력 approval_id만으로 발송할 수 없다. 실제 계정 읽기·core adapter·인증 wire는 후속 연결이다. Windows 인증 세션에는 현재 등록 재검증과 전체 계정 범위 검사 및 handshake ID/epoch getter를 추가했다. getter는 권한 증거가 아니다.

Application 대상 시험 **34개 통과**(공통 실행·입력 경계 포함), IPC 서버 기존 실제 pipe/CNG 시나리오 **15개 통과**. 감사 결과의 execution ID/오류/수치까지 검사하도록 테스트를 보강했다. 가짜 포트의 통과를 제품 인증 성공으로 기록하지 않는다.

ADR-021/수명 계약에 고정 HKCU Run command와 재시작 예산을 정의했다. WindowsAutoStart는 현재 core 실행 파일만 사용하고 임의 경로/인자를 받지 않는다. own value만 등록/해제하고 foreign/손상 값을 보존하며 조회 오류를 Disabled로 바꾸지 않는다. 초기 구현의 enable 미동작·빈 값 상태 오판을 검토에서 찾아 수정했다. 고정 522-byte 조회, 최소 registry 권한, exact REG_SZ/readback, native handle RAII를 적용했다. 제품 Run key를 변경하지 않고 독립 HKCU test key에서 검증했다.

Windows 플랫폼 **28개**(자동 시작 8개 포함), test-support **15개**, 총 **43개 통과** 및 Clippy 통과. 로그 `target/autostart-native-check.log`. 검사 후 해당 테스트 key 잔여 0개다. Windows가 일부 잘못된 REG_SZ 시험 데이터의 종단을 보정하는 현상을 확인해, 보존 검사는 입력 추측값 대신 호출 직전 실제 저장 bytes와 비교한다. 순수 RestartBudget 시험 4개는 1/2/4/8/16초·예산 소진·안정 실행 뒤 초기화·정상/유지보수/명시 중지 후 재시도 금지를 확인했다.

**남은 일:** 실제 supervisor child 실행, 사용자 알림, 설치/설정 반영, 유지보수 종료/Outbox 안전 지점은 미연결이다. M1-11 전체 완료나 SYNC-08/INST-13 전체 AC 통과로 표시하지 않는다. 이 변경 이후 전체 workspace/교차 검사를 다시 수행한다.

**공통 실행/수명 통합 snapshot:** `check_all.py` 종료 코드 0, 문서 29개·빌드 도구 58개·Rust 214개, 총 **301개 통과**. 로그 `target/execution-lifecycle-full-check.log`, SHA-256 `F24284770BA00AF98FB3A7D823B59CE70B638E25EAFA35F823DAEC930DF37C0B`, source hash `9db29b40d530a7086ba929f1b355d2f527d0e2f8de3b5425d1e45562f7ca31ec`. Linux x86_64/macOS arm64 workspace Clippy 통과.

이후 독립 검토에서 이미 원하는 자동 시작 상태인데도 쓰기 권한을 요구하는 문제를 찾았다. 격리 test key에서 실제 KEY_SET_VALUE 거부 DACL로 실패를 재현한 뒤 query-only 상태를 먼저 확인하도록 수정했다. 변경할 때는 write handle에서 소유 값을 다시 확인한다. 자동 시작 대상 시험은 **9개 통과**, Clippy 통과. 알 수 없는 canonical 계정의 감사 시작 거부 뒤 scope/정책/서비스 호출이 없는 시험도 추가했다. 이 두 수정은 위 301개 snapshot 이후이므로 다음 통합 검사에 포함한다.

### 2026-10-30 — M1-06/09 계정 표시 읽기

`accounts-read.md`에 공개 표시 필드·상태·capability 형식·길이/개수·정렬·누락 계정 처리·응답 한도를 정의했다. Domain DTO는 provider/status enum과 표시값 경계 및 UTC RFC3339를 검증하며 Debug에서 숨긴다. Store는 명시적 최대 128개 계정만 하나의 읽기 트랜잭션으로 조회하고, 요청되지 않은 계정/토큰/공급자 주체/승인 scope를 읽어 반환하지 않는다. 한 계정의 재인증 상태도 표시 자료로 반환한다. capabilities 미요청 시 해당 DB 열을 선택하지 않는다.

읽기 시험 **7개 통과**: 부분 범위/안정 정렬·최대 계정/capability·누락/빈/중복/잘못된 ID·손상 표시/JSON·미요청 capability 제외·total_changes 무변경·재개방 보존. 로그 `target/accounts-store-check.log`, Store Clippy 통과. Domain **13개**, Application **38개** 대상 검사와 Clippy도 통과했다. Account response는 실제 행 수와 local_store/cached 관측 정보만 반환하며 1,040,000 bytes를 넘으면 전체 QUOTA_EXCEEDED로 거부한다. 세부 로그 `target/accounts-response-check.log`.

독립 읽기 검토에서 확정 결함 없음. 실제 인증된 core adapter·wire에 연결하기 전까지 제품 bootstrap의 계정 조회 거부는 유지된다. 이 로컬 서비스 구현을 실제 계정 연결·P0 전체 완료로 표시하지 않는다. 통합 검사를 다시 실행한다.

**계정 읽기 통합 검증:** `check_all.py` 종료 코드 0, 문서 29개·빌드 도구 58개·Rust 230개, 총 **317개 통과**. 전체 build/fmt/Clippy·의존/문서 무결성·추적표 대조 통과. 로그 `target/accounts-read-full-check.log`, SHA-256 `DA4AC98368CBA5D981FEEDA7112CB6E1B4817ECFC483277739D439EAB6F045CC`, source hash `96d7820caf111314e4f8b922d0d7b69960bbd49f366442fc8c79829fd5976616`. Linux x86_64·macOS arm64 workspace Clippy도 통과했다. 실제 사용자 서버 CNG 키 1개 유지, 테스트 CNG/자동 시작 registry 잔여 0개를 확인했다.

다음은 binary transcript의 bounded canonical decoder와 실제 인증 wire 연결이다. 등록 UI·trusted client profile/pin·정책과 연결되기 전에는 보호 도구를 사용자 입력의 승인 주장으로 활성화하지 않는다. 전체 GUI 기술은 native Win32 승인 창/Windows Hello 직접 수신을 유지하고, 순수 Rust egui/eframe을 우선 후보로 조사했다. 전체 GUI 프레임워크 성능·한글 IME·접근성·HTML 격리는 아직 측정/채택하지 않았으며 새 GUI 의존성을 추가하지 않았다.

### 2026-10-30 — M1-02/09 인증 wire와 코어 조립

canonical transcript decoder는 bounded 문자열/개수·역할·버전·길이·정렬·재인코딩 동일성을 검증한다. 공통 lowercase hex, 양측 strict JSON 인증 codec, Windows 연결 소유 handshake를 추가했다. 시작 요청/응답 5초·finish 30초·인증 프레임 128KiB, 보호 요청/응답 1MiB를 분리한다. lookup은 begin/finish에서 최신 등록을 읽고 오류/철회/키·epoch 변화에 연결을 폐기한다.

클라이언트는 trusted pending 값에서 ID/epoch를 취하며 내부 UTC와 pinned server 검증 후 서명한다. raw transport를 공개하지 않고 consuming exchange 하나만 제공한다. 응답 bytes는 method별 해석 전까지 untrusted이며 receipt는 전송 소비만 뜻한다. 서버도 한 번의 callback/응답/receipt 후 연결을 닫는다. 인증 제한을 보호 도구에도 적용하던 초기 오류를 exact1MiB 회귀 시험으로 수정했다.

Core는 실제 Store 등록과 지속 서버 키를 사용해 bootstrap/auth를 분기하며, private ExecutionContext가 세션 재검증·CSPRNG 감사 ID·전체 scope·정책·Store 계정 읽기·결과 감사를 연결한다. 현재 check_policy는 항상 POLICY_BLOCKED다. 등록을 생성하거나 키 pin을 자동 채택하는 경로는 없고 승인 UI/profile resolver는 미완료다.

대상 검증: client **21개**, server **21개**(하나의 OS 시험 안에서 기존 시나리오와 JSON 인증/영수증/재요청/크기/timeout 부정 사례 실행), Core의 실제 양측 wrapper+dispatcher 통합 시험 **1개** 통과. Core fixture에는 잘못된 pin/미등록/epoch변경/철회/대형 응답, bootstrap의 lookup/executor 0회, 실제 빈 Store의 재검증 실패와 감사 0건을 포함했다. 수동 codec fixture만으로 pair 통과라고 오인하지 않도록 실제 public client wrapper 호출로 수정했다. 루트 검토와 전체/교차 검사는 아래에 추가한다. 사람 보안 검토 필요, 제품 전체 AC 완료 판정은 유지 보류한다.

독립 검토 후 저장 공개 키의 잘못된 curve point를 INVALID_INPUT으로 분류하던 문제를 실패 회귀 시험으로 재현하고 Core 변환 경계에서 INTERNAL_ERROR로 수정했다. 첫 auth.finish 무응답 종료, begin frame/응답의 합산 5초 제한, 직렬화 후 deadline 재검사, 치명적인 내부 오류의 서비스 루프 전파도 보강했다. Core 단위 **10개+프로세스 도움말/오류 통합 2개 통과**(`target/core-auth-router-check.log`). 실제 Store 손상 행 주입 시험을 추가한 것은 아니며 Store metadata→CNG 변환과 루프 오류 보존을 각각 검사했다. OS 시험의 연속 리스너 교체 때 IOCP 취소가 handle을 잠시 유지하는 현상을 확인해 테스트 안에서만 startup Conflict를 최대 1초 기다린다. 제품의 첫 인스턴스/ACL 조건은 완화하지 않는다.

**인증/코어 통합 snapshot:** `check_all.py` 종료 코드 0, 문서 29개·빌드 도구 58개·Rust 258개, 총 **345개 통과**. 전체 build/test/fmt/Clippy·의존 경계·불변 요건/제안·추적표 검증 통과. 로그 `target/auth-router-full-check.log`, SHA-256 `1BB85D9EEA828D7C6BFF58A076A337D011CD571BF3C8443DA96B54287622035E`, source hash `40e8bff84c07992af91f198e431cfd132e13078ba614614941f0d5da5a6c3f4e`. Linux x86_64/macOS arm64 workspace all-target Clippy도 통과했다. 이는 실제 Mac 실행/제품 완성 증거가 아니다.

정상 사용자 프로필에서 새 코어를 실제 기동해 영어 JSON·한국어 상태와 미등록 계정 조회 거부를 확인하고 Ctrl+C로 stopping/stopped 이벤트를 확인했다. 종료 뒤 DB SHA-256은 이전과 같은 `CC171D6DABC8D9C00FA07234EB1C11B1C98CB1527E34C29DB95452C2E92736FD`다. 지속 서버 CNG 키 1개(기존 names digest 동일), 잔여 테스트 키 0개를 확인했다. 코어는 종료 상태로 두었다.

### 2026-10-30 — M2-01 연결 설정·독립 자격 증명

Domain에 검증된 IMAP/SMTP endpoint 모델을 추가했다. host/port/TLS/login/authentication과 endpoint별 버전을 분리하며 비밀값 필드는 없다. JSON 16KiB·알 수 없는/중복 필드·표준 IP/DNS·문자열 경계·Debug 마스킹을 검증한다. 임의 DNS의 16진수 형태 label까지 거부하던 오류는 실패 회귀 시험 후 수정했다. Domain 대상 21개 시험과 Clippy 통과(기존 13개 포함). 실제 TLS 접속·인증 증거는 아니다.

Secrets에 IMAP/SMTP별 app password/access/refresh 6개 slot을 추가했다. 원래 account ID bytes를 lower base32hex로 인코딩한 v2 파일명을 사용하여 Windows의 대소문자 충돌을 피한다. resolve는 존재 확인만 하며 verify는 별도로 DPAPI 결속을 확인한다. handle 소유 검사는 계정·slot에서 재도출한 정확한 경로를 요구한다. 같은 계정의 다른 slot 및 다른 계정으로 암호문을 복사하면 검증 실패한다. 재개방·교체/삭제 격리·손상·최대 길이 등 Secrets 단위 19개+DB 통합 12개가 통과했다(`target/mail-slots-check.log`). 기존 4개 legacy 파일명은 보존했고 대소문자 충돌 해결은 M3 제품 사용 전 남은 과제다.

ADR-006에 io-imap 0.7.1/lettre 0.11.23/mail-parser 0.11.9/mail-builder 1.0.0을 후속 통합 대상으로 기록했다. 분리된 experiments/mail-stack에서 Rust 1.95 Windows/MSVC locked/offline MIME smoke 1개가 통과했다. imap-codec의 nom alloc feature 누락은 분리 manifest의 명시적 nom 7.1.3/alloc으로 해결했다. 제품 workspace의 외부 의존성에는 아직 추가하지 않았다. 네트워크·TLS·AUTH·성능은 미검증이다.

설정 저장 계약에 canonical SHA-256 재계산, endpoint별 CAS, operation/key 중복 방지, exact replay의 과거 상태 복원 금지, 관측 버전 결속과 독립 상태를 정의했다. hash는 기존 Windows Platform CNG를 재사용하도록 Store→Platform 의존을 허용했으며 반대 방향·CLI 저장소 접근 금지 시험은 유지했다. 신규 의존 방향의 실패 시험 후 아키텍처 시험 13개 통과. v4 DDL/마이그레이션·Store 구현은 진행 중이며 다음 전체 검사에 포함한다. 사람 보안 검토와 전체 AC 완료 판정은 별도다.
**M2 저장 구현·통합 검증:** 기존 IMAP 계정의 endpoint별 v4 저장 API를 추가했다. SHA-256 입력은 계정·endpoint·버전·정규화된 연결값을 결속하며 실제 CNG로 재계산한다. BEGIN IMMEDIATE에서 설정 CAS·해당 관측 무효화·operation 삽입을 함께 commit한다. 동일 작업 replay는 이후 설정과 관측을 되돌리지 않으며 100,000행 한도에서도 조회만 수행한다. 관측 읽기는 설정과 같은 snapshot에서 버전을 대조한다. 잘못된 값·버전 고갈·시계 역행·손상 저장을 오류로 보존한다.

최초 구현에서 관측 조회 열 이름·hash 입력 경계·저장 metadata 검증 누락을 12개 시험 중 7개 실패로 재현한 뒤 수정했다. 두 endpoint 독립 상태·실제 재개방/replay·총 변경 수 0·저장 중 오류의 3개 데이터 원자 rollback·최대 operation 용량·손상 읽기를 포함해 대상 **12개 통과**했다. 독립 검토에서 확정 결함은 없었다. Store 전체는 단위 **71개+설정 통합 3개**, Clippy 통과(`target/mail-store-all-check.log`).

v3 DDL 보존과 v1→v2→v3→v4 순차 migration을 추가했다. 직접 v3 fixture에 계정·등록·설정·감사를 넣고 실제 003 migration 전후 기존 11개 테이블의 모든 행/열 값을 대조했다. 실제 003 뒤 schema 실패 rollback, v3 백업 충돌의 원본/백업 byte 보존, 손상 v3의 백업 전 거부도 통과했다. 향후 v5 전에는 v4 DDL도 고정 파일로 분리해야 한다.

**v4 전체 snapshot:** `check_all.py` 종료 코드 0. 문서 29개·빌드 도구 59개·Rust 287개, 총 **375개 통과**. build/test/fmt/Clippy·구조 의존·기준/제안 문서 무결성·추적표 대조 통과. 로그 `target/mail-connection-v4-full-check.log`, SHA-256 `18DFE357D9E1412E435AEC621DB033B8BEC8336E014B4D824279EB9C2D13C8D6`, source hash `fd355a1e0c6130ef1ae9abc17a0934b932acbdcaa1821aded0da7686de6971e8`. Linux x86_64/macOS arm64 workspace all-target Clippy도 통과했다. 실제 Mac 실행을 검증한 것은 아니다.

정상 프로필에서도 새 코어 기동·영어 JSON/한국어 상태·미등록 요청 거부와 정상 stopping/stopped 이벤트를 확인했다. 기존 서버 CNG 키 1개와 names digest 유지, 잔여 테스트 키 0개다. 설정 저장은 아직 trusted UI·계정 생성·TLS/AUTH broker와 연결되지 않았다. 전체 제품 AC 통과 표시는 여전히 0개이며 사람 보안 검토도 남아 있다. 다음 작업은 변경된 서버로 기존 비밀번호가 자동 전달되지 않도록 자격 증명과 endpoint의 결속을 먼저 정의하는 것이다.

정상 프로필의 v3 백업은 126,976 bytes, v4 DB는 159,744 bytes다. 종료 뒤 v4 DB SHA-256은 7B745366E98F9C44A0826E8B67FE836F507B1D997EAC044F854950BC3C0D32BE이며 코어 실행 프로세스는 0개다. 암호화 온라인 백업은 물리 bytes 동일성을 요구하지 않으며 위 fixture 시험에서 논리 행 보존을 대조했다.

### 2026-10-30 — M2-01 endpoint와 자격 증명 결속

[메일 자격 증명 계약](../contracts/mail-credentials.md)에 맞춰 SecretStore의 IMAP/SMTP 전용 입력·검증 API를 구현했다. DPAPI 내부 envelope에 계정·role·설정 버전·host·port·TLS·login·authentication을 모두 저장하고 정확히 비교한다. 설정 변경 뒤 이전 암호를 새 서버로 자동 전달하지 않는다. OAuth는 access slot만 선택하고 refresh/legacy/반대 role로 폴백하지 않는다. 일반 verify는 DPAPI 무결성만 확인하며 새 bound verify는 별도로 endpoint 결속을 확인한다.

입력은 app-password UTF-8 1,024 bytes 또는 OAuth visible ASCII 16,384 bytes로 제한하고 목적별 최대+1까지만 읽는다. 입력·envelope는 재할당 없는 plaintext buffer에 담아 Drop 시 지운다. 신뢰 UI·TLS broker는 아직 미연결이며 bound verify 성공만으로 인증·승인을 발급하지 않는다. 새 공개 plaintext getter나 callback은 없다. Secrets→순수 Domain 의존을 추가하고 반대 의존 거부 검사를 유지했다.

대상 검증은 초기 stub에서 의미 있는 실패 5개를 확인한 뒤 구현했다. 재개방·각 endpoint 필드/버전 변경·암호문 보존·정확한 UTF-8/공백·최대 입력·부분 reader 오류·trailing/length/제어 문자 손상·다른 account/role/auth로 재암호화한 envelope 거부를 포함해 신규 **10개 통과**했다. Secrets 전체 **29개+DB 통합 12개**와 Clippy 통과(`target/mail-credentials-check.log`). 독립 검토에서 확정 결함은 없었다.

**자격 증명 통합 snapshot:** `check_all.py` 종료 코드 0. 문서 29개·빌드 도구 60개·Rust 297개, 총 **386개 통과**. build/test/fmt/Clippy·의존·문서 무결성·추적표 대조 통과. 로그 `target/mail-credentials-full-check.log`, SHA-256 `0E5FE51AEC2A931F866981E33DE14FBE422FA3947690F6561FEC1EBDED3BAE50`, source hash `3e36b97ef27d68bdd361cae5f5a38fe7e115b84866aa0e941a6a83f062a4d054`. Linux x86_64/macOS arm64 workspace all-target Clippy도 통과했다. 정상 서버 CNG 키 1개·기존 digest 유지, 테스트 키 잔여 0개다. 이번 검사는 실제 메일 연결·Mac 실행·전체 제품 AC 완료 증거가 아니다.

다음 통신 통합 전 버전별 API를 조사했다. io-imap의 실험 feature에는 내장 TLS helper가 없으며 IO-free engine과 별도 driver가 필요하다. lettre의 응답 크기 검사는 read_line 뒤에 수행되므로 악성 무개행 입력의 메모리 상한으로 충분하지 않다. 분리된 mail-stack 실험에서 내부 stream의 총 수신 budget·deadline 및 in-memory TLS 시험 경로를 검증한 뒤 제품으로 가져온다.
### 2026-10-30 — M2-01 SMTP TLS/AUTH probe 통합

분리 mail-stack 실험에서 MIME·무할당 bounded read·STARTTLS·자체 implicit TLS 등 **22개** 시험을 통과했다. 실제 네트워크 대신 Duplex와 공개 Rustls test CA를 사용했으며 원본·라이선스·hash를 fixture에 보존했다. 이 시제품의 통과는 제품 경계 검증과 구분한다.

[SMTP probe 계약](../contracts/smtp-probe.md)을 추가하고 Windows Secrets 안에 일회성 연결 검사를 구현했다. OS verifier/hostname 확인 → 지원하는 인증 방식 하나 선택 → 실제 DPAPI envelope와 endpoint/version 결속 재검사 → AUTH 235 → QUIT 221 순서다. 비밀·session·socket·임의 command/callback을 공개하지 않고 발송 명령을 구현하지 않는다. 실제 Core/GUI/IPC에는 아직 연결하지 않았다. wire 256KiB와 전체 30초 deadline, 최대 8개 연결 주소, 취소 시 future/transport 폐기를 적용했다.

초기 stub에서 대상 8개 중 7개 실패를 확인한 뒤 구현했다(`target/smtp-broker-red.log`). 실제 DPAPI+TLS 시험으로 두 TLS 방식, PLAIN/LOGIN/XOAUTH2 선택, 인증서/hostname/STARTTLS 오류의 unseal 0, 미지원 방식, stale/missing envelope, AUTH·QUIT 오류와 정확한 성공 코드, 과도한 응답/대기를 확인했다.

독립 검토에서 Lettre가 STARTTLS 전 평문 잔여 bytes를 외부 BufReader에 유지하는 문제를 발견했다. 220 뒤 가짜 EHLO/AUTH/QUIT를 붙인 실패 회귀에서 실제로 성공 receipt가 반환되는 것을 재현했다(`target/smtp-starttls-injection-red.log`). 하위 bounded buffer → 1-byte 전달 gate를 두고 TLS+재-EHLO 완료 뒤 bulk read로 전환하여, 앞서 읽은 평문이 TLS 아래에서 검증되도록 수정했다. 재검토에서 배치와 counter 유지가 맞음을 확인했다. 이 buffer/gate의 취소·전환·예산 유지 시험도 추가했다.

현재 Secrets 전체 대상·Clippy 검사와 전체 workspace/교차 검증을 진행한다. 이전 **386개** snapshot 이후 변경이므로 전체 검사 통과 수와 source hash는 새 결과로 갱신한다. 사람 보안 검토·실제 제공자 연결·전체 제품 AC 완료는 아직 남아 있다.

**SMTP 통합 snapshot:** `check_all.py` 종료 코드 **0**. 문서 29개·빌드 도구 60개·Rust 319개, 총 **408개 통과**. 전체 build/test/fmt/Clippy·의존 경계·기준/제안 문서 무결성·추적표 대조 통과. 로그 `target/smtp-broker-full-check.log`, SHA-256 `4321C731A3C235979FE3294D5535F91688B030B79B55BAB3841F6B0FF0F15F9F`, source hash `f809b700cfc3831ac9fd21bea40056fd9263c163b8a5b459a040fb30bb7292bf`. Linux x86_64·macOS arm64 workspace all-target Clippy도 통과했다. SMTP broker의 현재 실행 지원은 Windows이며 교차 검사는 Mac에서 실제 연결을 입증하지 않는다.

Secrets 대상은 단위 51개+DB 통합 12개, 총 **63개**와 Clippy 통과다(`target/smtp-broker-all-check.log`, `target/smtp-broker-clippy.log`). 기존 envelope 검증은 새 private unseal 경로를 재사용하며 plaintext 복사를 추가하지 않았다. 실제 30초 deadline의 가상 시계 시험과 buffered gate 취소/하위 transport 폐기도 통과했다. 정상 사용자 서버 CNG 키 1개·기존 names digest 유지, 잔여 테스트 키 0개를 확인했다. 코어·실제 계정은 실행하지 않았다. IMAP TLS/AUTH driver와 제공자 capability 탐지·설정 UI를 이어서 구현하며, 전체 제품 AC 341개 중 완제품 범위 완료 표시는 계속 0개다.

### 2026-10-30 — 다음 IMAP/승인 화면 경계 조사

io-imap 0.7.1 배포 소스에서 `ImapSend`가 첫 tagged response의 tag를 요청 tag와 비교하지 않고, `ImapStartTls`가 tag를 임의 위치에서 찾으며 OK/NO/BAD를 판정하지 않는 것을 확인했다. 해당 고수준 경로를 제품에 직접 넣지 않는다. 공개 codec을 이용해 CRLF·literal 거부·16KiB frame·정확한 응답 tag/status를 먼저 검증하는 private gate를 분리 실험에서 구현 중이다. 개발 빌드의 `imap-types::Secret` Debug와 raw trace 경로도 확인했으므로 제품 도입 시 `log` compile-time 비활성화를 Secrets 의존에서 강제하는 방안을 함께 검증한다. 현재 제품에는 io-imap을 추가하지 않았다.

Windows 승인 창의 공식 projection은 `windows 0.62.2`와 `windows-future 0.3.2`로 확인했다. `IUserConsentVerifierInterop`가 생성되어 있어 자체 COM ABI 선언이 필요 없다. `experiments/hello-bindings`의 격리된 컴파일에서 factory·HWND 기반 verification·완료 callback·Cancel/Close·STA/메시지 루프 서명을 확인했고 루트도 locked/offline all-target Clippy를 통과했다(`target/hello-bindings-check.log`). 실제 창/Hello 호출·등록·승인 발급은 실행하지 않았다. 이 실험의 raw HWND와 callback 예제는 제품의 소유권/일회성 증명을 대신하지 않는다.

### 2026-10-30 — IMAP 경계 시제품과 로컬 Git 기준선

private IMAP response gate는 초기 13개 실패 후 13개 통과했다. CRLF·16KiB frame·256개/atom 128-byte capability 한도·tag 정확 일치·PREAUTH/리터럴/금지 DATA 거부·오류 후 재사용 금지·Debug 비노출을 확인했다. 이를 소비하는 codec 전용 driver의 in-memory TLS 시험 5개도 통과했다. implicit/STARTTLS와 PLAIN/LOGIN/XOAUTH2/SASL-IR, TLS 오류·STARTTLS NO/미지원/평문 주입·잘못된 CAP/AUTH tag·literal·응답 폭주·stall·잘못된 logout을 포함한다. 전체 분리 mail-stack 시험은 **40개**와 all-target Clippy 통과다(`target/imap-probe-experiment-all.log`, `target/imap-probe-experiment-clippy.log`). 제품에는 아직 IMAP driver를 넣지 않았다.

`git init --initial-branch=main`으로 로컬 저장소를 만들었다. .gitignore에 native probe `*.obj`와 `.omx/`를 추가했다. 기존 .tools/target/개인 실행 데이터 제외 규칙을 유지한다. runtime 측정 JSON/build manifest는 기존 성능 보고서의 재현 근거이므로 보존하고, 공개 test CA/key는 provenance/license와 함께 포함한다. 실제 DB/비밀 저장소는 workspace 밖이며 GitHub remote·업로드·릴리스는 만들지 않았다. 기준선 커밋은 자동 시험을 통과한 현재 개발 상태의 기록이며 사람 보안 승인이나 제품 완성 표시가 아니다.

새 Git checkout에서 core.autocrlf에 의해 계약/스크립트가 CRLF로 바뀌어 추적 hash 검사가 실패하는 것을 재현했다(`target/git-checkout-traceability-red.log`). `.gitattributes`에 모든 hash 입력의 LF 보존 규칙을 추가하고 기존 CRLF 입력 8개를 정규화했다. frozen v2 SQL은 줄바꿈만 바뀌었으며 Store의 DDL 비교는 `split_whitespace`로 정규화하므로 기존 DB 구조 판정은 유지된다. 두 번째 index checkout에서 traceability와 문서 29·빌드 도구 60개가 통과했다. 로그: `target/git-checkout-traceability-check.log`, `target/git-checkout-docs-check.log`, `target/git-checkout-build-check.log`.

정규화 후 전체 검사도 **408개**(문서 29·빌드 도구 60·Rust 319), build/fmt/Clippy/아키텍처/기준 문서/추적 검사 통과다. `target/git-baseline-full-check.log` SHA-256 `BBF287280697C24ADA4322DDDD4FE7F39D2CA1997DB23998ACAC176C11A80F78`, source hash `a2db7c49422adf94ca8d305d2ca8feb568108dc52b6fc0771a17baa3d8ade029`. 이전 SMTP snapshot 이후 기능 코드는 바뀌지 않았다. 이 검증은 개발 기준선과 checkout 재현성을 확인하며 전체 기능/보안의 사람 검토 완료를 뜻하지 않는다.

로컬 기준선은 `0b8d50f`에 커밋했다. 원문 보존 문서/CSV의 의도된 공백·CRLF는 유지했으며 제품/도구/시험 코드의 Git whitespace 검사는 통과했다. 외부 remote/push는 없다.

### 2026-10-30 — M2-01/02 IMAP TLS·인증 probe와 기능 snapshot

`hantong-secrets::probe_imap`을 구현했다. OS 검증 TLS, 엄격한 STARTTLS 전환, TLS 후 CAPABILITY, endpoint 전체에 결속된 DPAPI 해제, 단일 PLAIN/LOGIN/XOAUTH2 인증, 인증 후 CAPABILITY와 BYE+tagged OK LOGOUT을 수행한다. 고수준 io-imap coroutine 대신 private bounded response gate/codec을 사용하고 log facade를 컴파일 시 비활성화했다. 입력 tag/문법/literal/frame/challenge와 전체 수신·송신·시간 한도를 검사한다. receipt는 settings_version과 알려진 기능 boolean만 포함하며 session·비밀·원문·임의 command를 노출하지 않는다. 메일 읽기/쓰기/발송은 아직 없다.

기존 SMTP transport를 동작 변경 없이 `mail_transport.rs`로 옮겨 수신 byte 제한을 재사용했다. 12개 transport 회귀 시험과 기존 SMTP/DB 시험도 통과했다. [IMAP 계약](../contracts/imap-probe.md)·[ADR-006](../adr/ADR-006-mail-libraries.md)에 순서·한도·rev1/rev2 기능 해석·strict ASCII/CRLF 호환성 한계를 기록했다. 실제 제공자/OS-root 네트워크 시험과 사람 보안 검토는 남아 있다.

초기 제품 시험은 stub에서 **5개 모두 실패**했다(`target/imap-broker-red.log`). 구현 후 gate 15·TLS/인증/상태 11 = **26개**가 통과했다. TLS 두 방식, PLAIN/LOGIN/XOAUTH2 및 SASL-IR, 서버 신뢰/hostname 오류 시 unseal 0, STARTTLS 거절/평문 주입, tag/리터럴/폭주, 미지원 방식, stale/missing credential, 조기 성공/비정상·추가 challenge, AUTH 누적 frame 제한, post-auth 기능 갱신/실패, 30초 가상 시계 timeout/drop을 포함한다. Secrets 전체는 **89개**(단위 77·DB 통합 12)와 Clippy 통과다. 독립 검토에서 중대한 결함은 발견되지 않았으나 사람 승인으로 기록하지 않는다.

추적 source hash가 컴파일에 포함되는 공개 TLS fixture 확장자 `.cert/.fullchain/.key`를 빠뜨리는 것을 실패 시험으로 확인하고 포함했다. fixture가 바뀌면 검증 증거도 무효화된다. `target/tls-fixture-hash-red.log` → `target/tls-fixture-hash-check.log` **11개 통과**. 이 파일들은 provenance가 있는 공개 테스트 재료이며 실제 자격 증명이 아니다.

최종 `scripts/check_all.py`: **435개**(문서 29·빌드 도구 61·Rust 345), build/fmt/Clippy/아키텍처/기준 문서/추적 검사 통과. 로그 `target/imap-broker-full-check.log`, SHA-256 `8D4A8D7EADC4773904ADC5E0AB2158DACA16A4330FB0CB9F6583347AE29D5009`, source hash `7a0279d2d9a9b05ce65d39194b74b82790a4eff4037e2733c747875ae4551d81`. Linux x86_64·macOS arm64 workspace/all-target 교차 Clippy도 통과(`target/imap-broker-linux-check.log`, `target/imap-broker-macos-check.log`). 실제 Mac 실행이 아니다. 검사 후 제품 CNG 키는 기존 서버 1개이며 inventory hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`로 유지됐다. 실제 계정·메일·Windows Hello는 사용하지 않았다.

상태: M2-01/02 진행 중, Secrets/계약은 **[사람 검토 필요]**. 전체 제품 AC 완료는 여전히 0개이며 UI·등록 프로필·공통 정책·실제 메일 수신을 다음 작업으로 연결한다.

## 2026-10-30 CLI 기본 등록·신뢰 창·프로필 연결 (진행 중)

**범위:** M1-02/M1-06의 미등록 거부 경계에 M6-02/INST-04 등록 절차의 최소 연결을 구현한다. 일반 기술 선택·계약 정리는 사용자 연속 개발 위임을 따르며 사람 보안 검토 완료로 간주하지 않는다.

**현재 변경:** 순수 등록 review 모델과 영/한 36개 리소스·라이트/다크/시스템/고대비 presentation, Core 전용 Windows 발급 crate, 요청/로그온/시각 결속 일회 결과, default 공개 프로필 codec/보호 loader, 등록 ID로 복구 가능한 CNG selector, Store approved+provisioning/active CAS/deny/read helpers를 추가했다. `hantong clients register`와 `clients status <request_id>`는 요청·진행 조회만 제공한다. 코어는 bounded UI queue와 기존 인증 wire를 함께 실행한다. 파일 publisher와 전체 연결 검증은 진행 중이다.

**지금까지 확인한 부분 검사:** review 15개, native/binding 10개, profile codec 12개, profile loader 14개, platform 키/경로 기존 포함 31개, Store 전이 기존 포함 85개, IPC client/server 기존 포함 63개가 각 대상 실행에서 통과했다. 이후 편집으로 이 숫자들을 합쳐 전체 통과로 표시하지 않는다. 최신 전체 검증 기준선은 위 435개 snapshot이며 현재 소스의 전체 검사는 아직 하지 않았다. 소유 모듈 별 red→green과 Clippy 로그는 `target/registration-*.log`, `target/profile-*.log`, `target/native-*.log`에 기록한다.

**검토에서 수정 중인 사항:** Win32 resize 중 mutable borrow와 동기 paint 충돌, foreground가 아닌 창에서 Hello 시작, 고대비 색상 변화 재적용, 취소 뒤 terminal Close 순서를 보완했다. binding 불일치는 ScopeDenied로 분리하여 사람이 누른 Deny만 denied로 저장한다. handler 등록 실패·OS 비응답 시 최대 5초 cleanup 후 terminal 정리를 보장하지 못하는 한계는 registration-review 계약에 명시했다.

**남은 검증:** publisher의 ACL·handle rename·기존 파일 보존, 중단 복구/코어 연결 독립 검토, workspace 전체/교차 검사와 추적표 갱신. 실제 Hello, 실제 사용자 프로필 등록, 메일 로그인/발송, GUI 시각·접근성 검증은 실행하지 않았다. 제품 완료·보안 사람 검토 완료·공개 배포 완료가 아니다.

### 등록 연결 최종 자동 검증 snapshot

위 진행 중 항목의 파일 배포·복구·전체 검사를 마쳤다. 보호된 Known Folder 경로와 모든 상위 handle을 유지하며 기존 ACL을 변경하지 않는다. 새 파일은 제한된 ACL로 create-new하고 flush 후 열린 handle로 rename한다. 동일 프로필만 재사용하며 다른 파일은 보존하고 CONFLICT를 반환한다. 파일 handle은 Store active CAS까지 유지한다. loader 15개·publisher 13개 시험이 통과했다. 완전한 전원 장애 내구성을 주장하지 않는다.

독립 검토에서 발견한 키 생성 직후 중단 문제를 보완했다. 먼저 저장된 요청 ID에서 등록 ID와 키 선택자를 도출하고, bounded orphan 목록과 등록 링크 검사를 통해 연결되지 않은 정확한 키만 정리한다. 커밋/조회가 불확실하면 키를 보존한다. 정리 실패는 재생성을 차단하며 원래 stage 오류와 정리 오류를 별도로 진단한다. 시작 목록 오류는 시작 실패로, 개별 복구 오류는 안정 코드로 남긴다. Store 91개·Core 27개 시험과 독립 재검토를 마쳤다. 아직 배포되지 않은 default 등록을 처음 도입한 변경이며 과거 무작위 등록 ID 데이터의 호환성을 약속하지 않는다.

`hantong clients verify`는 고정 프로필과 기존 CNG 키·서버 pin으로 상호 인증한 뒤 연결을 닫는다. 프로필/키를 자동 생성하지 않는다. CLI·MCP에 승인 발급 기능을 추가하지 않았다. 계정 목록 도구의 정책은 아직 기본 거부이며 다음 작업에서 연결한다.

`scripts/check_all.py` 종료 코드 **0**: **553개**(문서 29·빌드 도구 62·Rust 462), build/fmt/Clippy·아키텍처·불변 요건·추적 검사 통과. 로그 `target/registration-final-full-check.log`, SHA-256 `E1BB43E383FE5EC1C0459F22F489FF085AB7A64575682D2D1794777708D6AF53`, source hash `69a2de2aaa456c45389c1a7bc88d338eba1268de6404b054d1d7a8d8172991b8`. Linux x86_64/macOS arm64 all-target Clippy도 종료 코드 0이다(`target/registration-final-linux-check.log`, `target/registration-final-macos-check.log`). 교차 검사에서 Windows 전용 응답 helper의 cfg 누락을 수정한 뒤 전체 검사를 다시 실행했다.

검사 뒤 제품 CNG 키는 기존 서버 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`로 유지됐다. 실제 사용자 프로필 등록·Hello·외부 계정·메일을 실행하지 않았다. 실제 화면/접근성·OS Hello와 **[사람 검토 필요]** 상태는 유지한다. 전체 제품 AC 완료 표시는 계속 0개다. 다음은 등록된 CLI 계정 목록의 정책·호출 한도·엄격한 응답 검증 연결이다.

등록 변경의 로컬 커밋은 `a83732b`다. 외부 push/공개/릴리스는 실행하지 않았다.

## 2026-10-30 M1-06/M1-09 등록된 CLI 계정 읽기 (진행 중)

구현 전에 [초기 읽기 정책](../contracts/policy-read.md)과 [계정 응답 소비 경계](../contracts/accounts-read.md)를 정했다. 실제 인증·최신 등록·감사·전체 scope를 거친 `mail.accounts.list`만 허용하며 발송 경로는 차단한다. 프로세스 수명의 등록별 단조 시계 60초 한도는 최대 1,024 bucket/각 600회로 제한한다. 재시작 때 한도가 초기화되고 거부 호출도 감사 용량을 사용한다는 한계를 명시했다. DB 스키마·제3자 의존성은 추가하지 않는다.

정책/Core와 IPC client/CLI를 분리 구현한다. CLI는 고정 프로필의 명시적 계정 범위를 사용하고, 원본 응답 bytes에서 중복/알 수 없는 필드·추가 계정·부분 결과·표시값을 검사한 타입만 출력한다. 변경 후 새 전체 검사와 독립 검토를 수행할 예정이며 **553개 snapshot은 직전 등록 커밋의 증거**다. 현재 진행 중 변경 전체의 통과를 뜻하지 않는다.

### 계정 읽기 최종 자동 검증 snapshot

`ReadPolicy`는 Core 프로세스 수명 동안 유지되며 최신 Store 등록 한도를 매번 읽는다. 순수 한도 시험 7개, 실제 Windows pipe/CNG·격리 Store 시나리오에서 읽기/감사 수치, 연결 간 한도 공유, 최신 한도 감소/증가, scope·철회·epoch 거부, 발송 차단을 확인했다. 합성 DB 준비는 제품에서 의존할 수 없는 dev-only `hantong-test-support` 안에 두었다. 새 제3자 패키지나 DB 스키마 변경은 없다.

초기 응답 decoder 검토에서 중첩 Value 변환이 중복 키를 잃는 문제, positional array/필수 nullable 누락 허용, ID 정렬과 계정 번호 정렬 혼동, 개수/UTC 검증 누락을 발견했다. 실패 회귀 7개를 먼저 확인한 뒤 직접 스트림의 object/typed visitor로 보완했다(`target/accounts-codec-regression-red.log` → `target/accounts-codec-regression-green.log`). 추가 경계 사례를 포함한 decoder 14개와 실제 Core 성공 응답의 decoder/re-serialization 대조가 통과했다. 임의 중첩 필드·오류 원문·추가 계정·부분 결과·잘못된 날짜/시간·capability 경계를 검사한다.

독립 검토에서 `--include-capabilities`를 다른 명령에 붙여도 등록 동작이 먼저 실행되는 순서 문제를 찾아 모든 동작 전 거부하도록 수정했다. 영어/한국어 상태·주소·제공자·기능 라벨, 언어 독립 JSON, 터미널 방향 제어 문자 escape를 확인했다. 최종 프로세스 시험은 보호된 계정 조회 대신 고정 거부 명령으로 오류 envelope를 검사하여 실제 사용자 프로필 상태에 의존하지 않는다.

`scripts/check_all.py` 종료 코드 **0**: **581개**(문서 29·빌드 도구 62·Rust 490), 전체 build/fmt/Clippy·아키텍처·기준 요건·추적 검사 통과. 로그 `target/accounts-read-full-check.log`, SHA-256 `8F081044CD14F5C86551B05DC4F3584C566A41E41931E40CB0136630313E48FE`, source hash `468be36868cc438881a194aeace1fb9cb02fcb039408872e3b46f1f469211771`. Linux x86_64/macOS arm64 all-target Clippy 종료 코드 0(`target/accounts-read-linux-check.log`, `target/accounts-read-macos-check.log`). 실제 Mac 실행은 아니다.

검사 뒤 정상 CNG 서버 키 1개와 names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`를 확인했다. 실제 사용자 등록·Hello·메일 로그인/발송은 실행하지 않았다. 정책·보안 계약은 **[사람 검토 필요]**이며 전체 제품 AC 완료는 0개다. 새 설치에서는 아직 계정 생성 화면이 없어 전체 설정 과정이 완성되지 않았다.

다음 개발의 병목은 새 accounts 행과 endpoint/비밀 저장의 중단 복구다. 현재 M2-01 설정 API는 기존 계정을 요구하지만 실제 추가는 계획상 ACC-01/M3-10에 있다. 카드 정의를 임의 변경하지 않고 이 선행 공백을 기록하며 필요한 저장 전이부터 구현한다. 기존 빈 scope 등록에 새 계정을 자동 허용하지 않고 별도의 사람 검토와 profile epoch 갱신으로 연결한다.

계정 읽기 변경은 로컬 커밋 `74664e5`에 기록했다. 외부 push/공개/릴리스는 실행하지 않았다.

## 2026-10-30 새 IMAP 계정 저장 전이 (진행 중)

[account-provisioning 계약](../contracts/account-provisioning.md)을 작성했다. 기존 v4 operation 원장의 hash domain을 분리해 account+endpoint+원장을 원자 생성하고, exact replay는 고정 1/1 버전 receipt만 반환한다. 최초 표시명/번호 같은 변경 가능 snapshot은 replay하지 않는다. 동일 host/login은 신규 생성에서 거부하고 자동 합치기나 기존 계정 변경을 하지 않는다. DB migration·새 외부 패키지는 필요하지 않다.

Domain `NewImapAccountDraft`는 ID·표시명·색상만 갖고 login을 이메일/제공자 식별자로 추정하지 않는다. 기존 표시값 검증을 재사용했다. 대상 4개 실패를 확인한 뒤 Domain 전체 **25개**와 Clippy 통과(`target/account-draft-red.log`, `target/account-draft-green.log`, `target/account-draft-clippy.log`). Store stage/첫 probe 결과 원자 전이는 별도 구현 중이며 새 변경 전체 검증 전이다. 직전 **581개 snapshot**을 현재 새 기능의 전체 검증으로 해석하지 않는다.

이 단계는 M2-01의 실제 사용 가능 연결을 위한 account-provisioning 선행 통합이다. ACC-01/M3-10의 재인증·해제·표시 편집·5계정 연결 완료를 주장하지 않는다. 보호 입력 창·실제 로그인·기존 CLI 권한 갱신은 후속 연결이다.

### 새 계정 저장 전이 최종 자동 검증 snapshot

계정+endpoint+operation의 원자 생성, 고정 1/1 receipt 재처리, 첫 probe 다섯 결과의 account/endpoint/observation CAS를 구현했다. 중복 검사는 모든 기존 IMAP endpoint를 한 행씩 canonical/Domain 검증하며, 일치 이후의 손상 행도 검사한다. 계정 수 제한은 추가하지 않았다. 기존 계정·SMTP·식별자·권한을 보존한다. stage replay의 연결 대상 누락/손상과 중복 검사 보완은 실패 회귀로 확인했다.

신규 Domain 4개·Store 18개가 추가됐다. Store 전체 **109개**, Domain **25개**와 각 Clippy 통과. 독립 재검토에서 HIGH/MEDIUM 결함은 발견되지 않았다. 이 결과를 사람 보안 승인으로 기록하지 않는다.

`scripts/check_all.py` 종료 코드 **0**: **603개**(문서 29·빌드 도구 62·Rust 512), 전체 build/test/fmt/Clippy·아키텍처·기준 문서·추적 검사 통과. 로그 `target/account-provision-full-check.log`, SHA-256 `CD9E232BF17D86D27F6E508F3B2F39FEE0EC874C47374EABEB5D9DF9791A7D7C`, source hash `381000af9b8afd2743adb5ebdfbf3e69e5feef58d2931baa5328e955ab1d09a9`. Linux x86_64/macOS arm64 all-target Clippy도 종료 코드 0이다(`target/account-provision-linux-check.log`, `target/account-provision-macos-check.log`). 실제 Mac 실행은 아니다.

검사 뒤 CNG 서버 키 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`가 유지됐다. 실제 사용자 프로필·Hello·계정 로그인·메일 발송은 실행하지 않았다. 계약은 **[사람 검토 필요]**, 전체 제품 AC 완료는 **0개**다. 현재 결과는 저장 전이이며 설정 UI와 실제 로그인 성공을 뜻하지 않는다. 다음은 Secrets 소유 password EDIT와 불투명 pending credential을 네이티브 설정/사람 확인 흐름에 연결한다.


## 2026-10-30 보호 입력·새 IMAP 계정 확인 (진행 중)

새 계정 저장 전이는 로컬 커밋 `fa32477`로 기록했다. 다음 범위는 [보호 입력 계약](../contracts/protected-mail-input.md)과 [설정 확인 계약](../contracts/imap-setup-review.md)이다. Secrets-owned EDIT와 plan→input→opaque pending, native review가 pending을 직접 소유하는 Hello 결과를 구현한다. parent HWND는 배치 정보이며 권한 증거가 아니다. 사람 확인 뒤에만 Core가 저장/접속을 연결하도록 한다.

순수 설정 모델의 초기 커밋 `67fed4a` 이후 표시 escape·시간/대상 검증·영/한 리소스를 보완했다. 기존 ReviewModel의 first paint/만료/취소 규칙을 generic snapshot에 재사용하며 기존 등록과 setup 상태 회귀를 통과했다. 현재 전체 snapshot은 직전 **603개**이며 진행 중 변경의 전체 검증은 아직 아니다.

Secrets 초기 구현을 실제 숨김 Win32 fixture로 검토하니 nested root/중복 control ID 허용, 정상 컨트롤 상태 오판, Unicode 처리와 callback 수명 문제가 있었다. 대상 4개 실패를 먼저 재현하고 root가 native 구현을 수정하여 4개 통과했다(`target/protected-input-native-regression-red.log` → `target/protected-input-native-regression-green.log`). subclass는 자체 Rc 참조를 유지하고 NCDESTROY 뒤 wrapper가 숫자 HWND를 재사용하지 않는다. 고정 UTF-8/UTF-16 buffer와 scratch를 지우며, 실패 cleanup은 style/layout 검증과 소유 identity 검증을 분리한다. 추가 수명/DPAPI/경계 fixture를 작성 중이다.

GetWindowSubclass가 기본 Common-Controls v5에서 export되지 않는 실제 loader 실패를 확인하여 v6 manifest를 native 제품/시험 build에 embed했다. source hash가 manifest 변경을 놓치는 실패 회귀도 보완했다(`target/native-manifest-hash-red.log` → `target/native-manifest-hash-green.log`, 11개 통과). 새 외부 package/version은 없고 Cargo.lock은 approval-windows→Secrets 내부 edge만 추가했다. 현재 native UI 담당자의 대상 검증과 root 통합 검토가 진행 중이다.

다음 Core 연결에는 등록된 클라이언트/기존 계정을 전제로 한 AuditBegin을 그대로 쓸 수 없다. v4 DDL을 고정한 뒤 전용 setup request/event 저널을 v5에 추가하고, proof→stage→DPAPI→probe 각 경계의 중단 상태를 기록할 계획이다. 공개 CLI setup 메서드는 이 저장/복구 경계가 준비된 후 연결한다. 실제 사용자 창·Hello·계정·비밀번호·메일은 시험하지 않았다.

### 보호 입력·설정 확인 최종 자동 검증 snapshot

Secrets 보호 입력은 실제 숨김 Win32 fixture 14개와 순수 경계 4개를 통과했다. 원래 thread/root/subclass 소유권, parent 파괴와 숫자 HWND 재사용, 변경된 style/parent/limit, 잘못된 surrogate·제어 문자·UTF-8 한도, cancel/drop/만료, 최대 길이, DPAPI 대상/만료 불일치 때 기존 암호문 보존을 검사한다. 독립 검토가 찾은 visibility/scroll-style 누락과 native 길이 오류 분류는 실패 시험 2개로 확인하고 수정했다(`target/protected-input-expanded-red.log` → `target/protected-input-expanded-green.log`). Secrets 전체는 107개다.

기존 등록·언어·테마·OS verifier와 창 동시 실행 gate를 재사용했다. 설정 확인 결과가 정확한 pending 값을 직접 소유하며 OS Verified 외에는 발급하지 않는다. native/binding 전체 30개와 순수 approval 23개가 통과했다. root가 native summary의 CRLF 중복 변환을 실패 시험으로 재현해 수정했다(`target/imap-setup-linebreak-red.log`, `target/imap-setup-final-green.log`). 독립 재검토에서 추가 HIGH/MEDIUM 결함은 발견되지 않았다. 자동 검토는 사람 검토를 대체하지 않는다.

`scripts/check_all.py` 종료 코드 **0**: **637개**(문서 29·빌드 도구 63·Rust 545), 전체 build/test/fmt/Clippy·아키텍처·기준 문서·추적 검사 통과. 로그 `target/protected-setup-full-check.log`, SHA-256 `E00ECB3974F2A54A643CA6B3EE8BA00FEC31D7C4048D44F7815E6CF88F28C2BB`, source hash `d8f57155163c02d90f3f58b9eabd78f8d2d009b7c6e0461bdb97e544cb393a29`. Linux x86_64/macOS arm64 all-target Clippy 종료 코드 0(`target/protected-setup-linux-check.log`, `target/protected-setup-macos-check.log`). Windows 전용 시각/시험 helper의 cfg 누락은 교차 검사에서 수정했다. 실제 Mac 실행은 아니다.

검사 뒤 CNG 제품 키는 기존 서버 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`로 유지됐다. 새 제3자 package/version은 없다. 실제 사용자 창·Hello·비밀번호·계정 로그인·메일 발송은 실행하지 않았다. Secrets/승인/계약은 **[사람 검토 필요]**, 보이는 창·IME·접근성은 수동 검증 전이고 전체 제품 AC 완료는 **0개**다. 현재 UI primitive는 아직 CLI/Core 설정 실행 경로에 노출하지 않았다.

M5-06의 보호 입력 선행 부분을 진행 중으로 기록한다. 카드 정의와 선행은 보존하고 SMTP 입력·CLI/Core 종단·수동 검토가 남았음을 표시한다. 다음은 v4 보존 → v5 setup request/event 저널 → Core·CLI 연결 순서다.

## 2026-10-30 신규 IMAP 설정 요청·효과 저널 (진행 중)

보호 입력·설정 확인은 로컬 커밋 `48c9162`로 기록했다. [setup 저널 계약](../contracts/imap-setup-journal.md)을 먼저 정하고 v4 DDL을 보존했다. v5는 신규 요청과 이벤트만 추가하며 기존 계정에 가짜 확인 이력을 만들지 않는다. request/account stage/event 및 첫 probe/account/observation/event를 각각 원자 commit하고, credential 저장과 probe 시작 기록은 외부 효과보다 먼저 commit한다. 시작 기록 재호출은 성공으로 재전달하지 않는다.

이번 slice는 신규 요청 단일 시도·조회·중단 위치 보존이다. 기존 계정 reset/명시 resume와 공개 CLI/Core 활성화는 저장 경계를 검증한 다음 구현한다. 시작 시 기록을 검증할 뿐 비밀을 덮어쓰거나 서버에 자동 접속하지 않는다. 과거 **637개 snapshot은 직전 커밋의 증거**이며 v5 진행 중 변경의 전체 통과를 뜻하지 않는다. 관련 카드 정의/선행·기준 요건은 유지하고 마법사 선행 부분만 개발한다.


### 설정 저널·준비 codec 최종 자동 검증 snapshot

v4 DDL을 고정하고 v5 요청/이벤트 테이블을 추가했다. 이전은 실제 설정·계정·endpoint·observation·operation 값 보존, 백업 충돌/손상 거부, 변경 뒤 스키마 검증 실패의 rollback을 검사한다. 초기 저널 행동 시험 14개는 stub에서 실패한 뒤 통과했다. 기존 operation ID 충돌과 stage 시각 연결을 보완했으며, 시작 이벤트의 중복 CAS는 성공을 재전달하지 않는다. 독립 검토에서 추가 HIGH/MEDIUM 결함은 발견되지 않았다.

Contracts/client/server의 요청·조회 codec과 등록/설정 공용 dispatcher를 준비했다. 실제 pipe/Core/CLI는 아직 이 신규 메서드를 실행하지 않는다. 원본 frame 한도·callback 가족/요청 ID 일치·권한 주입 거부·strict object·상태/다음 행동·오류 구분을 검사한다. root가 유효한 전체 positional array와 오류 코드 단정으로 시험을 강화했다. dispatcher는 stub에서 5개 실패를 확인한 뒤 5개 통과했다.

`scripts/check_all.py` 종료 코드 **0**: **675개**(문서 29·빌드 도구 63·Rust 583), build/test/fmt/Clippy·아키텍처·기준 문서·추적 검사 통과. 로그 `target/setup-journal-full-check.log`, SHA-256 `95A4E621EB4822DD42156BF0853B2312778EA799C7B7612AC3CAB4162E2CB6D8`, source hash `1aee6f912cc08159d9ffda59ba6da2f6c344a736038dfffb5ec1e9195b324793`. Linux x86_64/macOS arm64 all-target Clippy도 종료 코드 0이다(`target/setup-journal-linux-check.log`, `target/setup-journal-macos-check.log`). 실제 Mac 실행은 아니다.

검사 뒤 제품 CNG 키 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`가 유지됐다. 실제 사용자 DB 이전·프로필 등록·Hello·계정 로그인·메일은 실행하지 않았다. 계약/보안은 **[사람 검토 필요]**, 전체 제품 AC 완료는 계속 **0개**다. M9-01은 선행 저널 구현으로 진행 중이며 카드 정의/선행은 보존했다. 다음은 Core와 CLI 연결 및 중단된 설정의 명시 재개다.


## 2026-10-30 신규 IMAP Core·CLI 연결 (진행 중)

설정 저널·준비 codec은 로컬 커밋 `9e1adb4`에 기록했다. 신규 설정 request/status를 실제 OS 검증 pipe에 연결하고, 기존 등록과 같은 단일 queue에서 native 확인을 실행한다. Core는 native 결과 소비 후 저널 stage→credential 시작/결과→probe 시작/결과 순서를 따른다. CLI는 서버·로그인·표시 정보만 전달하고 영/한 도움말·상태·다음 행동과 JSON을 제공한다. 비밀 인자·승인 완료 명령은 허용하지 않는다.

효과 함수의 초기 stub 시험 8개 실패를 확인한 뒤 통과했다. fixture의 trailing-dot host와 앞뒤 공백 login은 기존 Domain이 거부하는 값이므로 기존 정책을 유지하고 유효 fixture와 거부 시험을 구분했다. 시작/반환 commit 실패·불확실한 응답·취소·중복 호출의 다음 효과 0건을 검사했다. queue worker drop 후 pending으로 남는 실패 회귀를 확인하고 중단 상태로 수정했다. 실제 native proof를 시험에서 제조하거나 실제 사용자 창을 열지 않는다.

Core 전체 36개와 Clippy 통과 뒤 접속 중 종료·오류 분류 시험을 추가 검증했다. 현재 직전 675개 snapshot은 `9e1adb4`의 증거이며 진행 중 통합 변경의 새 전체 검증은 아직이다. 새 외부 의존성 없이 Core→Secrets 내부 edge만 추가했다. status의 기존 등록 안내는 활성 인증 성공 주장이 아니며 기존 등록의 상태·범위 재검토를 뜻한다. 명시 재개·기존 프로필 범위 갱신·전체 마법사·실제 Hello/화면 검토는 남아 있다.


### Core·CLI 설정 연결 최종 자동 검증 snapshot

실제 pipe의 설정 요청·상태·원본 공백을 포함한 16KiB 한도·권한 필드 거부를 검증했다. 등록 API는 공통 dispatch/server에 위임해 인증 수명주기 중복을 제거했다. Core의 한 대기열에서 같은 요청은 중복 창을 만들지 않고 다른 등록/설정 요청은 제한한다. 새 Core 시험 11개가 단계별 실패/불확실한 commit, probe 중 종료, 상태 분류, stale 재실행, queue 종료를 검사한다. 전체 Core 38개·CLI 23개를 포함한 검증이다. 도움말/버전과 명령 혼합은 모든 효과 전에 거부하고 미지원 resume/reset은 INVALID_INPUT으로 분리했다.

독립 검토의 기존 등록 안내 지적은 계약 대조 후 철회됐다. existing 등록이 비활성이어도 새 등록을 중복 만들 수 없으므로 상태·계정 범위 재검토로 안내하며, ready가 CLI 인증 성공을 뜻하지 않음을 문구에 명시했다. 남은 HIGH/MEDIUM 결함은 없었으나 사람 보안 검토는 대체하지 않는다.

첫 전체 검사에서 기존 account-provision 시험의 임시 Store::open이 INTERNAL_ERROR로 한 번 실패했다(`target/setup-activation-full-check.log`). 실패 fixture에는 0-byte DB와 보호된 키 파일이 있었고 오류 원인은 특정하지 못했다. 해당 단독 시험과 Store 전체 129개 재실행은 통과했다(`target/setup-activation-store-repro.log`, `target/setup-activation-store-recheck.log`). 원인 추정으로 제품 코드를 바꾸거나 시험을 제외하지 않았다. 일시 초기화 실패의 원인 미확인은 알려진 제한으로 남긴다.

다시 실행한 `scripts/check_all.py` 종료 코드 **0**: **689개**(문서 29·빌드 도구 63·Rust 597), build/test/fmt/Clippy·아키텍처·기준 문서·추적 검사 통과. 로그 `target/setup-activation-final-full-check.log`, SHA-256 `AC3855715ECB4637F8FCBA526334CD008FE5FACAD7DAFADE7AF8DAD880FF6EFC`, source hash `8db2bec5fda78c13900caf458b402d95bed4a63fd1afd17392d3ca2857665271`. Linux x86_64/macOS arm64 all-target Clippy 종료 코드 0(`target/setup-activation-linux-check.log`, `target/setup-activation-macos-check.log`). 실제 Mac 실행은 아니다.

검사 뒤 CNG 제품 키 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`를 확인했다. 실제 사용자 Core 실행·DB 이전·프로필 등록·Hello·계정 로그인·메일은 실행하지 않았다. 계약/보안은 **[사람 검토 필요]**, 전체 제품 AC 완료는 **0개**다. 다음은 v5를 보존하는 v6 재개 시도 이력과 CLI/native 연결이다.


## 2026-10-30 IMAP 명시 재개와 v6 이력 연결 (진행 중)

직전 Core·CLI 연결은 로컬 커밋 `5be5ff2`로 보존했다. [명시 재개 계약](../contracts/imap-setup-resume.md)에 따라 v5 DDL을 고정하고 새 시도마다 parent/child 연결·새 ID·TTL·결속 hash를 추가한다. 최초 계정 생성 이력과 기존 v4 operation은 그대로 유지하고, 기존 계정 재개에서는 endpoint/SMTP/identity/grants를 변경하지 않는다. 부모 snapshot과 현재 account/observation version이 정확히 일치해야 한다. 매 시도에 새 보호 입력과 Hello가 필요하며 시작·조회는 외부 효과를 재실행하지 않는다.

저장소 초기 재개 시험 8개, wire/CLI와 Core queue에서 구현 전 실패를 확인했다. 새 wire/CLI 시험과 실제 pipe 재개 시험이 통과했고, Core **40개**(단위 38·lifecycle 2)가 통과했다(`target/setup-resume-core-green.log`). 같은 활성 leaf 재요청은 새 job을 만들지 않는다. 중단 후 child를 새로 만들며 Ready leaf 재요청은 저장된 결과만 반환한다. 새/기존 계정 stage 분기와 fresh hash, 이전 부모 효과 차단도 검사했다.

현재 Store 확장 시험·v5→v6 이전·별도 보안 검토와 전체 검사를 진행 중이다. 위의 689개 통과는 직전 커밋의 증거이며 현재 변경 전체의 검증 결과가 아니다. 기준 요건과 카드 정의는 보존하며 실제 사용자 계정·Hello·메일은 실행하지 않는다. 계약/보안은 **[사람 검토 필요]**다.


### 명시 재개 최종 자동 검증 snapshot

v6는 과거 v5 이력과 모든 기존 표의 값을 보존한다. Store **148개**(단위 145·설정 3), 재개 대상 **14개**가 통과했다. 이전 fixture의 마지막 이벤트 시각과 request updated 시각 불일치(15/16)를 strict reader가 거부하여 fixture를 일치시켰다. 손상 연결 시험의 정상 unique index 충돌도 격리 fixture에서만 해제했다. 제품 검증을 완화하지 않았다. v5 journal hash 손상의 backup 전 거부와 실제 DDL 실패 후 모든 v5 행/스키마 rollback을 추가 검증했다. 별도 코드 검토에서 HIGH/MEDIUM 결함은 발견되지 않았다.

`scripts/check_all.py` 종료 코드 **0**: **715개**(문서 29·빌드 도구 63·Rust 623). 전체 build/test/fmt/Clippy·아키텍처·요건 무결성·추적 검사 통과. 로그 `target/setup-resume-full-check.log`, SHA-256 `631419DF2F03F46FFC8D17409BD53BF5DB1DE5E188454BC3A6D1BF245C09B8EB`, source hash `c64f2ff56bae23e97af3a2e7cda2fee961205e8f7ffef2951f1b36b245c73a0e`. Linux x86_64/macOS arm64 all-target Clippy도 종료 코드 0(`target/setup-resume-linux-check.log`, `target/setup-resume-macos-check.log`). 실제 Mac 실행은 아니다.

CNG 제품 키는 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`로 유지됐다. 실제 사용자 DB·Core·프로필·Hello·계정·메일을 실행하지 않았다. 새/기존 계정 확인 문구는 영/한으로 구분하지만 실제 화면·접근성 검토는 남아 있다. 계약/보안은 **[사람 검토 필요]**이며 전체 AC 완료는 **0개**다. 다음은 등록 ID·키·서버 pin을 보존하는 기존 default CLI의 명시 계정 범위 갱신이다. DB와 프로필 파일은 단일 원자 트랜잭션으로 묶을 수 없으므로 비활성 준비→보호 파일 배포→활성화와 중단 복구를 별도 계약으로 정의한다.


## 2026-10-30 기본 CLI 계정 범위 갱신 (진행 중)

IMAP 재개는 로컬 커밋 `6594a02`로 기록했다. [기본 CLI 범위 갱신 계약](../contracts/client-scope-refresh.md)을 추가하고 기존 request/키 예약과 섞이지 않는 v7 전용 이력을 설계했다. 같은 등록 ID·CNG 키·서버 pin을 보존하고 기존/새 계정 목록을 영/한 신뢰 창에서 검토한다. 읽기 전용 accounts.list/60 rpm만 유지하며 새 도구·쓰기 권한을 추가하지 않는다. `clients refresh`는 현재 계정 snapshot에 대한 새 사람 확인 요청이고 CLI 승인이 아니다.

DB와 파일은 단일 트랜잭션이 아니므로 stage에서 기존 인증을 중지하고, 열린 handle로 exact old 프로필을 요청별 backup에 옮긴 후 새 프로필을 no-replace 발행한다. 그 guard를 유지한 상태에서 정확한 새 grant를 활성화한다. 중단 시 known old/backup/new 조합만 복구하며 불일치는 보존하고 거부한다. 기존 read-only guard를 열어둔 채 별도 DELETE handle로 승격할 수 없다는 Win32 공유 규칙을 공식 문서로 확인했다. 관련 근거와 같은 사용자 공격·전원 차단 내구성 한계는 계약에 기록했다.

Store 초기 12개 시험은 11개 실패와 기존 일반 등록 보존 1개 통과로 시작했다(`target/scope-refresh-store-red.log`). wire/client/server와 dispatch의 구현 전 실패 후 대상 시험이 통과했으며 기존 IPC 전체 110개도 통과했다. CLI 26개, 순수 approval 27개가 통과했다. v6→v7 이전의 실패 시험 2개 후 보존/backup 충돌/실제 DDL rollback 3개가 통과했다(`target/scope-refresh-migration-green.log`). native 파일 교체의 초기 구현에서 recovery 조합 누락·handle access 오류를 수정했고 대상 7개와 approval-windows 37개가 통과했다. 이후 native scope binding/paint/gate 회귀 시험을 추가했으므로 전체 재검증은 남아 있다.

현재 Store 전이 구현과 Core 연결·독립 보안 검토를 진행 중이다. 최신 전체 **715개** 통과는 직전 재개 커밋의 증거이며 현재 v7 변경 전체의 완료를 뜻하지 않는다. 실제 사용자 profile/Hello/계정/메일은 사용하지 않는다. 계약/보안은 **[사람 검토 필요]**다.


### 기본 CLI 범위 갱신 최종 자동 검증 snapshot

v7 전용 이력은 원래 등록 request/ID/key/pin/승인 metadata를 보존한다. 새 native 확인 뒤 stage에서 기존 인증을 중지하고, 보호 파일 guard를 유지해 새 grant를 활성화한다. Store **167개**(단위 164·설정 3), 범위 이력 대상 **15개**, Core **46개**가 통과했다. Core의 5개 실패에서 구현을 시작했고, 실패·불확실한 commit 후 다음 효과가 실행되지 않음, 기존 키만 reopen, 초기 등록 복구에서 갱신 제외, queue 중복/종료/명시 복구를 검사했다.

보호 파일 허용 조합(old/new/backup)과 충돌·reader 잠금·guard 수명을 격리 경로에서 검증했다. native 승인 **40개**가 통과했으며 독립 검토에서 발견한 최대 128+128개 계정 snapshot의 화면 상한 문제를 64Ki UTF-16 공통 한도로 수정하고 영/한 최대 입력 회귀를 추가했다. 별도 Store/Core 재검토에서는 추가 HIGH/MEDIUM 결함이 없었다. 이는 사람 보안 검토를 대체하지 않는다.

기존 일반 정책 fixture는 default CLI의 고정 정책을 벗어나는 rpm/tool 변경 시험이므로 전용 표시명으로 분리했다. 제품의 기본 프로필 검증은 완화하지 않았다. 기존 schema 최종값 단언 6개와 테이블 수 17→18, Core 조건문 Clippy 지적을 수정했다. 이전 실패 로그 `target/scope-refresh-store-all.log`, `target/scope-refresh-full-check.log`, `target/scope-refresh-final-full-check.log`는 보존한다.

`scripts/check_all.py` 종료 코드 **0**: **766개**(문서 29·빌드 도구 63·Rust 674). 전체 build/test/fmt/Clippy·아키텍처·요건 무결성·추적 검사 통과. 로그 `target/scope-refresh-verified-full-check.log`, SHA-256 `D05B60D965C39DD4C2F09D2DA38825EC2892A42A343FF534C5EFB3FFC910522B`, source hash `aff19bea5e8bc5f8f8363bb234a939d8c4d5fc18b69ebd238157f9ee56b8fecb`. Linux x86_64/macOS arm64 all-target Clippy 종료 코드 0(`target/scope-refresh-linux-check.log`, `target/scope-refresh-macos-check.log`). 실제 Mac 실행은 아니다.

CNG 제품 키는 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`로 유지됐다. 실제 사용자 DB·Core·프로필·Hello·계정·메일은 실행하지 않았다. 외부 의존성 추가 없이 구현했고 카드 정의/선행·기준 요건을 보존했다. M6-02/M6-10은 MCP·선택적 도구/프로필 편집·철회 UI·전체 종단 검토가 남아 진행 중이다. 계약/보안은 **[사람 검토 필요]**이며 전체 제품 AC 완료는 **0개**다. 다음은 IMAP 서버 기능의 공통 표현과 별도 폴더 탐색 계약·읽기 경계다.


## 2026-10-30 서버 기능 공통 타입·IMAP 폴더 탐색 (진행 중)

기본 CLI 범위 갱신은 로컬 커밋 `8e9f1a1`로 보존했다. [서버 기능 계약](../contracts/provider-capabilities.md)과 [읽기 전용 폴더 탐색 계약](../contracts/imap-folder-discovery.md)을 먼저 작성했다. 기존 probe는 그대로 두고 Secrets 내부의 TLS/AUTH 뒤 고정 LIST만 실행하는 별도 경계를 준비한다. 서버 기능과 권한을 구분하고, INBOX 외 이름은 그대로 보존하며, 특수 폴더가 불명확하면 임의로 선택하지 않는다.

공식 RFC와 고정 codec/types 소스에서 LIST 이름이 literal일 수 있음을 확인했다. 선언 길이를 body 전에 제한하고 전체 결과가 성공한 경우만 snapshot을 반환하도록 설계한다. 순수 Domain 이름/기능 모델·adapter 매핑은 별도 담당자가 시험부터 구현하고, Core/Secrets 연결은 root가 맡는다. 새 외부 의존성 없이 기존 내부 Domain edge만 사용한다. 폴더 DB 영속화·메일 수신·공개 도구는 후속이다. **766개 통과는 직전 커밋의 증거**이며 진행 중 변경 전체의 검증을 뜻하지 않는다. 계약/Secrets는 **[사람 검토 필요]**다.


### 서버 기능·폴더 탐색 최종 자동 검증 snapshot

IMAP 여섯 기능의 고정 ID와 엄격한 저장 변환을 Domain 한 타입으로 통합했다. 기존 Secrets 경로는 re-export하고 Store/Core의 중복 문자열 변환을 제거했다. Domain **36개**, adapter **2개**가 통과했다. 64개 기능 조합, 잘못된 권한 문자열, 긴 modified UTF-7·한글·emoji·surrogate·padding, INBOX 중복과 다른 wire 이름 보존, 모호한 특수 폴더 후보를 검사했다.

Secrets의 별도 LIST는 기존 TLS/AUTH 뒤에서만 실행하고 완전한 목록·LOGOUT BYE+OK 확인 뒤 snapshot을 반환한다. 기존 probe 한도와 의미는 유지한다. Secrets **117개**, IMAP 전체 **48개**(LIST gate 13·discovery 9 포함)가 통과했다. 1,024개 목록·1,024-byte literal의 정상 경계와 초과, 2MiB 누적 plaintext, frame 폭주, TLS/AUTH 실패, 취소 시 연결 폐기, 잘못된 tag·중복 identity·불완전 LOGOUT을 검사했다.

독립 검토에서 untagged NO/BAD 뒤 OK를 성공으로 처리하는 결함을 재현하고 고쳤다(`target/imap-discovery-status-red.log`, `target/imap-discovery-green.log`). 잘못된 `{4}x {4}` 선언이 body를 먼저 소비하는 결함도 RED→GREEN으로 고쳤다(`target/imap-list-header-red.log`, `target/imap-list-header-green.log`). 고정 codec 배포 소스는 완전한 literal frame을 직접 처리함을 확인하여 literal→quoted 변환을 삭제했다. 원본 frame을 strict raw 검증 후 codec으로 해석하며 이름·delimiter·속성을 대조한다. 이 변경 뒤 IMAP 48개가 통과했다(`target/imap-list-original-frame-green.log`). UTF-7의 u32 shift overflow라는 초기 검토 지적은 실제 반례가 없고 Rust 연산 의미와 일치하지 않아 철회됐다. 최종 독립 검토의 잔여 HIGH/MEDIUM 지적은 없으며 사람 검토를 대체하지 않는다.

초기 discovery 시험의 고정 command 기대값은 typed builder의 실제 `LIST "" *`와 맞췄다. 1,024개 결과 fixture의 TLS flush 누락으로 생긴 timeout은 시험 서버에 flush를 추가하여 해결했다. 제품 deadline을 늘리거나 실패 시험을 제외하지 않았다.

`scripts/check_all.py` 종료 코드 **0**: **801개**(문서 29·빌드 도구 63·Rust 709). 전체 build/test/fmt/Clippy·아키텍처·요건 무결성·추적 검사 통과. 로그 `target/imap-folders-full-check.log`, SHA-256 `9AE07955C36A94E4D1E06DE2B3CC877C13999A50BA0CB2E44EE667CBFE054B9B`, source hash `f068e857e55c37d45ee5b1f6e9263613a20d1285599842f3d633ee8a190b9e82`. Linux x86_64/macOS arm64 all-target Clippy 종료 코드 0(`target/imap-folders-linux-check.log`, `target/imap-folders-macos-check.log`). 실제 Mac 실행은 아니다.

CNG 제품 키는 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`로 유지됐다. 실제 사용자 DB·Core·프로필·Hello·계정·메일을 실행하지 않았다. 추가한 Cargo edge는 기존 내부 Domain뿐이며 외부 패키지 버전 변경은 없다. M2-02/M2-03은 진행 중이고 전체 제품 AC 완료는 **0개**다. 폴더 DB·Core 정책 경로·공개 도구·실제 서버 상호 운용성이 남아 있으며 계약/Secrets는 **[사람 검토 필요]**다. 다음은 v7을 고정 보존하고 폴더 관측 catalog만 별도 원자 저장하는 v8 경계다.


## 2026-10-30 IMAP 폴더 catalog v8 저장 (진행 중)

직전 폴더 탐색은 로컬 커밋 `a688f6a`로 기록했다. [폴더 관측 저장 계약](../contracts/imap-folder-store.md)을 먼저 작성하고 최신 snapshot 한 행과 별도 operation 이력을 설계했다. 폴더 관측은 UID/message/cursor와 분리하며 계정/endpoint/세대 CAS와 strict Domain codec을 적용한다. replay는 과거 성공 결과만 반환하고 최신 catalog를 덮지 않는다. Domain codec·Store 전이·v7→v8 이전을 나누어 시험부터 구현한다. 공개 도구나 Core 자동 접속은 이 변경에 포함하지 않는다. **801개 통과는 직전 커밋의 증거**이고 현재 변경 전체의 검증은 진행 중이다. 계약/보안은 **[사람 검토 필요]**다.

### v8 catalog 최종 자동 검증 snapshot

Domain canonical codec과 Store 원자 교체를 연결했다. 잘못된 provider/version·operation ID/범위·payload hash·stale endpoint의 hash 연결을 검증하고, exact replay가 최신 snapshot을 덮지 않도록 했다. 독립 검토 후 잔여 HIGH/MEDIUM 지적은 없었다. Domain 41개, catalog 전용 15개, v7 이전/제약 4개를 포함한다. commit 시점 deferred FK 실패까지 실제 SQLite transaction으로 주입해 rollback을 확인했다.

`scripts/check_all.py` 종료 코드 **0**: **825개**(문서 29·빌드 도구 63·Rust 733). 로그 `target/imap-catalog-final-full-check.log`, SHA-256 `9473F9DC90ABF3854496314A697DFDBA11D81ABF7BE75D3978CA3FCCA15D60F1`, source hash `c532d7e02b0200afe1ab02c6e6894197015076e8a8485ca8cecaca8f86131c43`. 전체 build/test/fmt/Clippy·문서·추적·아키텍처 검사 통과. 앞선 전체 검사는 시험 helper의 Clippy needless borrow 두 곳에서 실패했고 수정 후 전체를 재실행했다. Domain codec·Store 손상·migration의 RED→GREEN 로그는 `target/imap-catalog-*`에 남겼다.

Linux x86_64/macOS arm64 all-target Clippy 종료 코드 0(`target/imap-catalog-linux-check.log`, `target/imap-catalog-macos-check.log`). 그 뒤 변경은 Windows 전용 시험의 불필요한 참조 제거다. 실제 Mac 실행은 아니다. CNG 제품 키 1개와 names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D` 유지. 실제 사용자 DB·프로필·Hello·계정·메일은 사용하지 않았다. **[사람 검토 필요]**, 전체 제품 AC 완료 0개를 유지한다.

## 2026-10-30 적극적 병렬 구현 — SMTP·권한·폴더 CLI (진행 중)

v8 저장 기준선은 `52d19f6`으로 보존했다. 공통 계약을 먼저 작성하고 SMTP 기능·폴더 권한/프로필·폴더 client/CLI를 세 담당자가 나눠 구현했다. root는 v9 DB 이전과 통합을 맡았다. SMTP는 별도 담당자 검토에서 HIGH/MEDIUM 지적이 없었으며, 시험 누락을 root가 확인해 pre/post TLS 차이·LOGIN/XOAUTH2·unknown extension 검증을 보강 중이다.

권한 lane의 초기 native 검사는 raw Cargo가 검증된 Perl 경로를 설정하지 않아 실패했다. root가 `scripts/cargo_native.py`로 다시 실행했으며 이 실패를 환경상의 영구 차단으로 처리하지 않았다. 교차 검토에서 tool-only 변경 거부, 계정만 비교하는 provenance, hash version 추정, legacy reader의 빈 DB 거부를 발견했다. 실제 grant 변조를 허용하는 RED 시험(`target/folder-scope-authority-red.log`)을 재현하고 전체 RequestedScope 결속과 explicit hash_version으로 수정했다.

`target/folder-scope-store-green.log`의 scope 관련 **34개 통과**에는 v8→v9 백업/rollback, 실제 v1 pending·staged·active 이력을 원래 hash 그대로 이전하고 뒤에 v2 권한을 연결하는 시험이 포함된다. 승인/Windows 도우미/Core targeted 검사도 통과했지만 Core의 기존 fixture가 legacy 권한만 사용하는 누락을 발견해 다른 담당자가 folder grant와 recovery를 추가 검증 중이다. 전체 회귀와 소스 hash 고정은 아직 전이며 최신 완결 검증은 위의 825개 기준선이다.

folder client/CLI는 strict 요청·cursor·receipt·영/한 명령을 구현했으나 공통 Application/Core의 실제 폴더 dispatch는 다음 통합 단계다. 새 grant가 있다고 실제 폴더 기능 완료로 표시하지 않는다. 각 lane의 자체 보고를 그대로 완료로 채택하지 않고 실제 diff·시험 실행 개수·다른 담당자의 검토를 대조한다. 모든 계약/보안 영역은 **[사람 검토 필요]**이며 전체 제품 AC 완료는 아직 0개다.


### SMTP·v9 권한·폴더 client 최종 자동 검증 snapshot

SMTP 5개 고정 기능과 실제 AUTH 선택을 post-TLS EHLO에 결속했다. STARTTLS 이전에만 광고된 기능·unknown extension의 미노출, LOGIN/XOAUTH2를 포함한 SMTP 대상 12개가 통과했다. v9는 기존 v1 범위 갱신 이력의 hash·행을 보존하고 도구 집합까지 명시 결속한다. grant 변조·legacy DB 손상·hash version·tool-only 변경·재개 프로필을 교차 검토했다.

folder client의 수동 JSON scanner에서 escaped duplicate key 검사를 우회하는 결함을 RED로 재현한 뒤 bounded serde visitor로 교체했다. 잘못된 UTC 날짜·세대 범위·cursor 순서·상태 execution ID·계정/tool 권한을 거부하고, 22개 안정적 오류 응답·깊은 JSON·프레임 상한·intent SHA 벡터를 검증했다. CLI는 가짜 callback을 주입하며 실제 프로필/Core에 접근하지 않는다. setup 명령에 폴더 옵션이 섞여 무시되는 결함도 RED→GREEN으로 수정했다.

첫 전체 검사(`target/parallel-wave-full-check.log`)는 새 복구 시험에서 서로 다른 임시 계정의 프로필 바이트를 같다고 비교하여 실패했다. 동일한 작업을 publish 후 중단하고 recover하는 시험으로 바로잡았다. base/target의 계정과 도구 전체도 비교한다. 제품 검증을 완화하거나 실패 시험을 제외하지 않았다.

`scripts/check_all.py` 종료 코드 **0**: **864개**(문서 29·빌드 도구 63·Rust 772). 로그 `target/parallel-wave-final-full-check.log`, SHA-256 `377A0D8734128969B7C15437E000D55FC86D113ECEC2E5EA91AD174DC20C5EB2`, source hash `fa51dab59a3586aba2a75a3133cf2e0cd2ae47ba03af51fbbb1d7ecc9d10a507`. 같은 소스의 Linux x86_64/macOS arm64 all-target Clippy 종료 코드 0(`target/parallel-wave-final-linux-check.log`, `target/parallel-wave-final-macos-check.log`). 실제 Mac 실행은 아니다.

CNG 제품 키 1개와 names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D` 유지. 실제 사용자 DB·프로필·Hello·계정·메일은 사용하지 않았다. 공개 폴더 요청은 아직 Application/Core dispatch가 연결되지 않아 제품 사용 완료로 표시하지 않는다. **[사람 검토 필요]**, 전체 제품 AC 완료 0개를 유지한다. 다음 묶음은 v10 작업 이력·공통 admission·Core worker를 통한 폴더 기능 연결이다.


## 2026-10-30 폴더 공개 도구와 v10 작업 원장 (전체 검증 진행 중)

직전 SMTP·v9 범위·CLI 묶음은 로컬 커밋 `d7f06c3`로 보존했다. 계약/페이지/Application, migration, Core worker를 병렬 배정했고, root가 Store 작업 전이와 공통 파일을 통합했다. 별도 담당자는 Store/Core 실패 경로를 독립 검토했다. 담당별 보고는 실제 소스와 검사 로그로 대조했으며 미완성 API 연결·미실행 빌드를 완료로 채택하지 않았다.

`mail.folders.list`, `mail.folders.refresh` request/status를 기존 인증→감사 start→scope→policy 경로에 연결했다. 목록은 세대에 결속한 로컬 페이지이고 갱신은 capacity-one worker의 실제 channel permit을 먼저 예약한다. 접수·중복 receipt·queue full/closed는 v10 ledger와 audit에 결속하며, 성공 catalog/operation/job/audit를 한 트랜잭션으로 저장한다. 실행 전·중·commit 전 등록/계정/endpoint/세대를 검사하고 변경 시 네트워크 future를 폐기한다. 종료·조회·중복 요청은 미완료 작업을 자동 재실행하지 않는다.

v9 DDL은 직전 커밋과 byte-normalized 대조로 고정했다. 9→10은 기존 audit parent/child·capacity·AUTOINCREMENT high-water를 보존한다. 첫 migration 검증에서 과거 8→9 단계에 현재 DDL을 사용한 문제, CREATE DDL 공백 불일치, 빈 aggregate가 만든 순번 중복/NULL과 빈 INSERT가 만든 불필요 seq=0을 발견해 수정했다. 6개 migration 시험이 통과했고 Store 전체에서 과거 schema fixture가 v10 표/DDL을 남긴 문제와 테이블 수 단언을 수정했다. Store **213개**(단위210·설정3)가 통과했다(`target/folder-v10-store-verified.log`).

독립 검토는 과거 성공 job의 결과 hash 검증, 저장 operation 100,000건 quota의 terminal 처리, 성공 commit 오류 후 두 번째 실패 기록, CredentialMismatch 매핑을 지적했다. 회귀와 구현을 보완했고 Store job **11개**가 통과했다(`target/folder-job-review-green.log`). 최종 코드 재검토에서 잔여 HIGH/MEDIUM은 없었다. DB/검사 내부 오류와 불확실 commit은 worker/Core로 전파하며 unresolved 상태를 보존한다.

실제 인증 pipe의 가짜 제공자 연결 시험은 목록 없음/계정 scope 거부, 갱신 receipt/조회, 중복 무효과, queue full, 2페이지 strict client 해석, 실행 중 catalog 변경으로 취소, 중단 후 status/replay 무재실행을 검증한다. 성공 commit 오류의 추가 Core 주입 시험은 열려 있는 Store에 다른 DB 연결을 열려다 정상적인 exclusive lock에 거부됐다(`target/folder-v10-core-final.log`). 이 잘못된 주입 방식을 제거했으며 제품 검사를 우회하는 hook은 넣지 않았다. 해당 오류의 저장 원자성은 Store 회귀, worker 전파는 코드 검토로 확인했고 end-to-end 오류 주입은 검증 공백으로 남긴다.

현재 전체 build/test/fmt/Clippy와 교차 정적 검사를 진행한다. 위 **864개** 최신 전체 검증은 직전 커밋의 증거이며 v10 완료 증거로 사용하지 않는다. 실제 사용자 Core/DB/profile/Hello/계정/메일은 실행하지 않는다. 계약·정책·감사 변경은 **[사람 검토 필요]**이며 전체 제품 AC 완료는 **0개**다.


### 폴더 v10 최종 자동 검증 snapshot

`scripts/check_all.py` 종료 코드 **0**: **892개**(문서29·빌드 도구63·Rust800). Core **50개**, Store **213개**를 포함하여 전체 build/test/fmt/Clippy·아키텍처·기준 문서·추적 검사가 통과했다. 로그 `target/folder-v10-full-check.log`, SHA-256 `05005BFCDD8ADD5BD103D476FBAE9317CBB0DBCA49C8C7457DBF5AB8965BB237`, source hash `998a9125ee61725ca5b71a07b9ff441282648cf06daa258c79c2af897c27e999`. 같은 소스의 Linux x86_64/macOS arm64 all-target Clippy도 종료 코드 0(`target/folder-v10-linux-check.log`, `target/folder-v10-macos-check.log`)이다. 실제 Mac 실행 검증은 아니다.

CNG 제품 키 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`로 유지됐다. 신규 의존성은 없다. 실제 사용자 Core/DB/profile/Hello/계정/메일은 실행하지 않았다. 계약·정책·감사 변경은 **[사람 검토 필요]**, 제품 전체 AC 완료는 **0개**다. 다음은 M2-04의 읽기 전용 UID 메타데이터 페이지·원자 cursor 저장이며, 기존 메시지 FLAGS/expunge 대조·UIDVALIDITY 재동기화·전체 메일 UI는 후속으로 구분한다.


## 2026-10-30 UID 메타데이터 페이지와 v11 cursor (진행 중)

폴더 연결은 로컬 커밋 `a946075`로 보존했다. [UID 페이지 계약](../contracts/imap-uid-page.md)을 먼저 작성하고, RFC3501/RFC4549·고정 io-imap0.7.1 근거와 별도 설계 검토를 거쳤다. Domain DTO, private IMAP transport, Store 저장/시험을 분리했으며 root는 DDL/migration과 통합을 맡는다. 초안에서 실제 통신·strict reader·DB 시험이 누락된 부분은 완료로 표시하지 않고 재배정·보완한다.

범위는 EXAMINE→고정 폭64 UID의 metadata FETCH 한 번→EXAMINE 재검증이다. UIDVALIDITY 변화와 UIDNEXT 역행은 전체 페이지를 폐기한다. UID gaps/빈 페이지도 covered-through cursor를 전진시키며 본문/첨부나 기존 메일 FLAGS/expunge 대조·재동기화 완료를 주장하지 않는다. Message-ID와 sequence number는 identity로 쓰지 않는다. 공개 도구나 자동 실행은 아직 연결하지 않는다.

Domain 초기 구현의 범위 밖 UID 허용·Recent 제거 전 입력 한도 누락을 실패 시험으로 확인했다(`target/uid-domain-review-red.log`). 별도 검토가 찾은 mixed-case INBOX 왕복 불일치·크기 오류 분류도 재현했다(`target/uid-domain-canonical-red.log`). canonical wire+encoding으로 identity를 고정하고 standalone metadata codec·NIL/empty/group/raw bytes 보존·비정규 JSON 거부 시험을 추가했다. Domain **53개 통과**(`target/uid-domain-canonical-green.log`), Clippy도 수정 후 통과(`target/uid-domain-clippy-fixed.log`).

v11은 기존 v10 DDL을 고정하고 operation/cursor/message 세 표를 추가한다. 원자성·보존 migration 시험은 구현 전 3개 중2개 실패를 확인했다(`target/uid-v11-migration-red.log`). Python in-memory SQLite의 전체 DDL 대조에서 v10+incrementalDDL과 freshv11이 같고 FK 오류가 없었다. 메시지 operation 조회에는 전용 (operation_id,uid) covering index를 추가해 전체 메타데이터 스캔을 피한다. 이 대조는 제품 SQLCipher migration 시험을 대신하지 않는다.

실제 격리 SQLCipher/DPAPI Store 시험 **19개를 작성**했고 실행은 전송 모듈 컴파일 연결 뒤 진행한다. 과거 receipt와 이후 cursor 보존, endpoint/계정/encoding/CAS, page+metadata hash와 원본 snapshot 연결, 세 표 쓰기 실패 rollback, quota·시계 역행·고아 replay를 포함한다. 현재 작성 개수는 통과 개수가 아니다. 제품 Store는 canonical metadata·cursor·operation을 한 Immediate transaction으로 적용하고, 시작 시에는 DDL/FK/개수만 검사하며 각 읽기/적용에서 해당 JSON/hash/연결을 검증한다.

현재 전체 회귀·실제 전송 통합 시험은 아직 미완료다. 최신 전체 **892개** 증거는 v10 기준선이며 이 v11 변경의 완료 증거가 아니다. 실제 사용자 DB·계정·메일·Hello와 외부 공개는 실행하지 않는다. 계약·Secrets·저장 보안은 **[사람 검토 필요]**다.


### UID v11 최종 자동 검증 snapshot

실제 격리 SQLCipher/DPAPI UID 저장 시험 **21개**가 통과했다. 초기 19개는 fixture의 중복 synthetic login을 수정한 뒤 통과했고, 독립 검토가 지적한 삭제된 metadata 행과 정상 UID gap 구분을 2개 회귀로 보완했다(`target/uid-store-missing-row-red.log`, `target/uid-store-missing-row-green.log`). 관측 범위의 누락은 covering operation snapshot과 자식 행으로 대조한다. 새 coverage index는 fresh/migration DDL에 동일하게 포함된다. 전체 Store **237개**가 통과했다.

Private IMAP UID 전송/경계 시험 **32개**가 통과했다. finite UID 범위, 단일/상한 근처 UID, EXAMINE 재검증, TLS/STARTTLS, UTF-8 mailbox literal, 총 8MiB·30초 deadline·취소와 hostile 응답을 검사했다. pinned codec의 상한 근처 `*` 변환은 좁은 typed command 검증으로 보완했다. literal 마지막 CR 뒤의 LF가 panic을 유발하는 독립 검토 지적을 실제 RED로 재현하고 boundary 검사로 수정했다(`target/uid-literal-newline-red.log`, `target/uid-literal-newline-green.log`). 최종 별도 정적 재검토에 잔여 HIGH/MEDIUM은 없었다.

`scripts/check_all.py` 종료 코드 **0**: **958개**(문서29·빌드 도구63·Rust866). 전체 build/test/fmt/Clippy·아키텍처·문서·추적 검사 통과. 로그 `target/uid-v11-full-check.log`, SHA-256 `8FFFA26D92BEA4B9265F2D15CEAB12DEEE34C66BEDB8B8EED8C831DF2F41AE61`, source hash `c164b5247171a129c755d81541c343fc565e391cd1c1cd6845e77e2287ed3431`. 같은 소스의 Linux x86_64/macOS arm64 all-target Clippy 종료 코드0(`target/uid-v11-linux-check.log`, `target/uid-v11-macos-check.log`). 실제 Mac 실행은 아니다.

CNG 제품 키 **1개**, names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D` 유지. 새 의존성 없음. 실제 사용자 DB/Core/profile/Hello/계정/메일은 사용하지 않았다. Store와 transport는 각각 실제 구현의 격리 fixture로 확인했으며 공개 sync 요청부터의 종단 연결은 후속이다. 기존 FLAGS/expunge 대조·UIDVALIDITY 복구·본문/첨부·메일 UI도 남아 있다. **[사람 검토 필요]**, 전체 제품 AC 완료 **0개**를 유지한다.


## 2026-10-30 UID 공개 요청과 v12 작업 원장 (진행 중)

UID transport/저장 기반은 `827fefc`로 보존했다. 새 공개 계약을 고정하고 세 담당자가 공개 DTO·strict client/CLI, 명시 scope v3/보호 확인, 영속 UID 작업/원자 완료를 병렬 구현한다. root는 audit·공통 Application·v12 migration·Core 연결·실제 인증 pipe 시험을 맡는다. 구현 담당자는 다른 묶음을 교차 검토한다.

공개 `mail.sync.page`는 cursor/request/status와 한 페이지 실행만 지원한다. 범위/UIDVALIDITY/endpoint는 서버가 현재 상태에서 도출하고, 요청 intent와 결과 page hash를 분리한다. 성공 metadata/cursor/operation/job/audit를 원자 저장하며 중복·상태 조회·시작 시점은 자동 수집을 실행하지 않는다. folder/UID worker는 하나의 활성 slot을 공유하고, 각 worker의 소유 ID를 따로 보관해 다른 worker 종료가 활성 slot을 풀지 않도록 했다.

기존 legacy/folder 프로필과 scope v1/v2 이력은 보존하고, 새 명시 refresh만 UID toolset을 제안한다. native 영/한 확인 후 v3 이력으로 활성화하며 U→F/L 권한 하향 전이와 v1/v2에 UID를 끼워 넣는 변조를 거부한다. 담당자의 targeted 검증은 UI/Contracts/Profile42개, Store/Core scope52개 통과다. root의 v12 migration은 RED2개를 확인한 뒤 기존 UID 행·미완료 folder job·audit sequence 보존/backup 충돌/실제 DDL rollback3개가 통과했다. Python SQLite fresh/incremental DDL 대조도 일치한다. 로그 `target/uid-scope-ui-client-green.log`, `target/uid-scope-store-core-green.log`, `target/uid-v12-migration-red.log`, `target/uid-v12-migration-green.log`.

공개 parser는 RED1개를 재현한 뒤2개가 통과했다(`target/uid-public-parser-red.log`, `target/uid-public-parser-green.log`). 나머지 클라이언트·작업 원장·worker·실제 pipe 통합과 전체 회귀는 진행 중이며, 위 최신 전체958개는 직전 v11 기준선이다. 실제 사용자 Core/DB/profile/Hello/계정/메일은 사용하지 않는다. 계약·권한·저장 보안은 **[사람 검토 필요]**이며 제품 전체 AC 완료0개다.


### UID v12 최종 자동 검증 snapshot

공개 Contracts/client/CLI UID 시험16개, Core UID worker7개가 통과했다. Store 작업17개와 기존 UID 저장21개 및 전체 Store262개가 통과했다. 초기 Store 시험1개는 suspension 이후 audit begin을 시도해 올바르게 ClientSuspended를 받은 fixture 오류였고, audit 시작과 admission 사이에 suspension하도록 시험만 수정했다. 기존 보안 검사를 완화하지 않았다. 로그 `target/uid-public-client-tests.log`, `target/uid-public-worker-tests.log`, `target/uid-v12-job-store-green.log`, `target/uid-v12-existing-sync-green.log`, `target/uid-v12-store-all.log`.

실제 인증 pipe에서 folder-only grant의 UID 거부, cursor→request→receipt→status, exact duplicate 무재수집, UID 대기 중 folder 갱신의 shared-slot 제한, catalog 변경 시 future 폐기, worker 재생성 뒤 unresolved/replay 무재접속을 확인했다. worker 재생성은 같은 Store를 유지하는 시험이며 실제 DB close/reopen 보존은 별도 Store 회귀가 담당한다. Client parser는 실제 Core 응답을 엄격하게 해석했다.

독립 검토의 유일한 MEDIUM은 8192-byte UID frame 제한이 클라이언트에만 적용되던 문제였다. raw signed handshake로8193-byte 공백 패딩 요청이 실행되는 RED를 재현했다(`target/uid-frame-limit-red.log`). 서버가 decode 뒤에도 원래 byte 길이를 보존해 callback 전에 거부하고, UID response도 같은8KiB 상한으로 직렬화하도록 수정했다. 정확히8192-byte 허용/8193-byte 실행0/과대 response INTERNAL_ERROR와 전체 UID pipe 시나리오가 통과했다(`target/uid-frame-limit-green.log`). 작성자가 아닌 검토자의 최종 재검토에 잔여 HIGH/MEDIUM은 없었다.

`scripts/check_all.py` 종료 코드 **0**: **1,016개**(문서29·빌드 도구63·Rust924). 전체 build/test/fmt/Clippy·아키텍처·문서·추적 검사 통과. 로그 `target/uid-v12-full-check.log`, SHA-256 `F189EF332A4A06D5C8262653FAAB79C845A11FCF94E9F1EB1BD402A95B3A17C9`, source hash `6a427bc34fb3cf0432b3c0d68ab451e94db1fa3de3941e110a7d16a5d94d6c7e`. 같은 소스의 Linux x86_64/macOS arm64 all-target Clippy 종료 코드0(`target/uid-v12-linux-check.log`, `target/uid-v12-macos-check.log`). 실제 Mac 실행 시험은 아니다.

CNG 제품 키1개와 names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D` 유지. 새 의존성 없음. 실제 사용자 Core/DB/profile/Hello/계정/메일과 외부 공개는 실행하지 않았다. **[사람 검토 필요]**, 전체 제품 AC 완료0개 유지. M2-04 전체·전체 GUI·작성/발송·설치 릴리스 완료를 뜻하지 않는다.

## 2026-10-30 기존 UID 상태 대조와 v13 overlay

[고정 계약](../contracts/imap-uid-reconcile.md)과 RFC 근거·독립 설계 검토 후 Domain/Secrets, Store, 공개 Contracts/Application/client/CLI를 세 담당자에게 나눴다. root가 v13 migration, 공유 UID worker, Core adapter와 실제 인증 pipe 연결을 맡았다. 원본 append page/message/cursor는 불변이며, 기존 메시지 FLAGS와 원격 부재 관측만 별도 저장한다. 각 명시 요청은 한 페이지이며 새 append가 추가되어도 진행 중 pass의 상한은 유지한다.

Domain/Secrets 신규16개 및 해당 두 crate 전체232개, 공개 네 package211개가 통과했다. 공개 경계는 신규13개(기존 EN/KO 출력 byte 보존 포함)이며 테스트를 먼저 썼지만 첫 실행이 green이어서 관측한 RED로 기록하지 않는다. 프로토콜은 missing-API RED 후 GREEN이 있다. 로그 `target/reconcile-protocol-{red,green,regression,clippy}.log`, `target/reconcile-public-{targeted,regression,clippy}.log`.

root migration은 v12에서 그대로 머무는 보존/backup 시험2개의 RED 후 실제 SQLCipher migration3개가 통과했다. v12 기존 행·대기 UID/folder 작업·감사/권한 이력을 보존하고, backup 충돌과 실제 DDL 실패에서 원본을 유지한다. Python SQLite fresh/migration DDL도 정확히 일치했다. 로그 `target/reconcile-migration-red.log`, `target/reconcile-migration-green.log`.

Store는 원본 보존·FLAGS/부재·고정 pass·replay·손상·quota100000·각 쓰기 ABORT/IGNORE rollback 등17개가 통과했다. source 선택 때 과거 전체 JSON을 읽던 경로는 수치 index와 선택된 페이지 검사로 줄였다. 별도 검토에서 수치 손상으로 실제 child가 있는 source를 숨길 가능성을 찾아 child count 검증을 추가 중이다. 계정 상태 시험은 존재하지 않는 suspended 대신 실제 domain 상태 reauth_required로 fixture만 수정했으며 제품 검사를 완화하지 않았다.

공개 경계 독립 검토는 HIGH/MEDIUM 없음. Store 교차 원장 replay가 충돌 확인 전에 과거 receipt를 반환하던 MEDIUM은 양방향 선행 검사와 회귀 시험으로 수정했다. Core 검토 HIGH는 Store admission의 불확실한 외부 오류를 Application으로 돌려 감사 terminal을 다시 쓰려는 경로였다. folder/append/reconcile 모두 durable admission 시도 뒤에는 AuditHandoff로 소유권을 유지하도록 수정하고 Core 분류 시험·Application 추가 terminal 금지 시험을 넣었다. 이는 commit 성공 주장과 구분한다.

Core62개(단위60·lifecycle2)가 통과했다(`target/reconcile-core-first.log`). 실제 서명된 pipe에서 append 두 페이지→FLAGS/부재 대조, 공유 folder/append slot 제한, 새 append가 생긴 뒤에도 frozen pass 유지, 과거 replay가 최신 overlay를 되돌리지 않음, catalog 변경 시 취소·무반영, worker 폐기/재생성 후 unresolved/status/replay 무재접속을 확인했다. 같은 Store를 유지하는 Core 재생성 시험과 실제 DB 재개방을 하는 Store 시험은 구분한다. 전체 인증 시험20초와 추가 대조 흐름15초 제한을 둬 실패 시 무제한 대기를 방지한다.

현재 전체 회귀는 마지막 수정 후 실행할 예정이며 위 최신1,016개는 직전 v12 기준선이다. **[사람 검토 필요]**이고 제품 전체 AC 완료0개를 유지한다. 실제 사용자 계정·메일·DB·Core·Hello나 공개 배포는 사용하지 않았다.


### UID v13 최종 자동 검증 snapshot

Store source count 보완은 실제 child count와 declared count를 비교한 뒤 선택하여, 숨겨진 nonempty page·누락 child·zero-span을 거부한다. 해당 회귀 시험은 RED 후 GREEN이었다(`target/reconcile-source-count-red.log`, `target/reconcile-source-count-green.log`). 선택되지 않은 모든 JSON을 decode하지 않는 성능 경계도 유지했다. Store18개 targeted 및 Store Clippy, 공개/Core/Store 독립 최종 검토에 남은 HIGH/MEDIUM은 없다. Core의 무한 대기 지적은 기존 전체20초 제한을 놓친 표현이어서 정정했고 추가 local timeout은 진단 보완으로 기록한다.

`scripts/check_all.py` 종료 코드 **0**: **1,069개**(문서29·빌드 도구63·Rust977). 전체 build/test/fmt/Clippy·아키텍처·문서·추적 검사 통과. 로그 `target/reconcile-v13-full-check.log`, SHA-256 `2F50607BDD7566F7026356B68981F9B36B0440B57CA318B6AE8D3D8D274ECD7C`, source hash `e88fddaa214563a92cb7697d6906a11139ac238db5ed08d811570f9de1c4e1d9`. 같은 소스의 Linux x86_64/macOS arm64 all-target Clippy 종료 코드0(`target/reconcile-v13-linux-check.log`, `target/reconcile-v13-macos-check.log`). 실제 Mac 제품 실행 검증은 아니다.

CNG 제품 키1개/names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D` 유지. 새 의존성 없음. 실제 계정·메일·사용자 DB·Core·Hello와 외부 공개를 실행하지 않았다. **[사람 검토 필요]**, 전체 제품 AC 완료0개 유지. 다음은 M2-04 안에서 UIDVALIDITY 변경을 안전하게 다루는 기반 또는 조회 연결 범위를 정하며, 선행 카드가 끝나지 않은 M2-07 전체 완료/시작으로 표시하지 않는다.


## 2026-10-30 UIDVALIDITY 복구 기반 v14 (진행 중)

직전 v13 전체1,069개 통과 기준선은 `5f27187`이다. [복구 계약](../contracts/imap-uid-recovery.md)을 먼저 작성하고 M2-04/SYNC-02의 사용 불가 cursor 복구 기반으로 진행한다. M2-07의 선행을 완료 처리하지 않는다. 서브에이전트가 지적한 epoch FK·strict cursor 전환·reconcile 시작 경계 문제를 계약에 반영했다. namespace 전환은 원본/관측/과거 작업을 모두 보존하는 원격 읽기이므로 기존 U 범위 안에서 명시 요청하며, 요건이 요구하지 않은 추가 Hello 승인 절차는 만들지 않는다.

Domain/Secrets 담당은 새 validity의 첫 metadata 페이지 읽기와 기존 codec 보존을 구현했고 신규14개·관련 전체246개 시험과 Clippy가 통과했다(`target/recovery-protocol-{red,green,regression,clippy}.log`). Store 담당은 epoch/영속 작업/역사적 읽기·reconcile 분리, 공개 담당은 Contracts/Application/client/영한CLI를 병렬 구현한다. root는 한 번의 v14 migration과 공통 Core worker/실제 pipe 연결을 맡는다. 현재 v14 제품 연결과 전체 검증은 미완료다.

공개 경계는 신규12개 RED→GREEN, 담당6패키지296개 회귀·Clippy·서식 검사를 통과했다(`target/recovery-public-{red,green,regression,clippy}.log`). 구현자와 다른 담당자가 wire/권한/응답 결속을 검토했고 HIGH/MEDIUM 지적은 없다. 기존 다섯 action의 schema와 scope hash/history는 유지됐다. Store migration과 실제 인증 pipe 시험은 아직 진행 중이며 이 수치를 전체 v14 통과로 기록하지 않는다.

v13→v14 SQLCipher migration 시험4개가 통과했다(`target/recovery-v14-migration-green.log`). 과거 행/JSON/hash·대기 audit 보존, 최초 epoch backfill, backup 충돌 시 원본 bytes 유지, DDL/backfill 실패 rollback, 불일치 namespace 거부를 확인했다. 독립 Store 검토에서는 실행 ID 재사용, 누락 epoch의 metadata 오류 처리, 과거 receipt의 epoch 결속, orphan epoch startup 검사 보완을 찾아 회귀 시험과 함께 수정 중이다. 아직 v14 전체 통과/완료를 주장하지 않는다.

Core 실제 서명 pipe 복구 시나리오 포함 **63개**(단위61·lifecycle2)가 통과했다(`target/recovery-core-integration.log`, 단위13.62초, 기존 전체20초 제한 유지). epoch7→9 동일UID 다른 metadata 격리, 과거 원본/관측/replay 보존, 모든 IMAP 작업의 공유 slot, 새 epoch source floor, catalog 변경 취소, worker 재생성 후 unresolved 무재접속을 확인했다. Store 교차 검토 HIGH2/MEDIUM3과 인자 수 Clippy 경고는 수정 중이므로 이 결과를 최종 통과로 취급하지 않는다.

독립 Store 검토의 HIGH3/MEDIUM3을 모두 재현 시험과 함께 수정했다. 처음5개 RED→GREEN 뒤, 한 단계 오래된 outgoing recovery provenance 누락1개를 추가로 RED→GREEN 확인했다. 각 epoch의 직전/직후 경계만 검증하며 전체 이력을 재귀 순회하지 않는다. 최종 focused 검토는 CLEAR이며 남은 HIGH/MEDIUM 없음. 공개/Core 교차 검토도 CLEAR다.

Store 복구 시험26개에는 18개 ABORT/IGNORE 저장 실패 지점, 시계 역행, 실제 DB 재개방, 계정 재인증/새 catalog, endpoint/CAS/권한 변경, 옛 작업 차단, 원장 간 예약 충돌과 두 개의 실제10만 행 quota fixture가 포함된다. quota 시험은 선택된 정상 행을 유지한 수량 한도 검증이며 모든 생성 행의 의미적 이력을 검증했다는 뜻은 아니다. Store **313개**·수정 후 Core **63개**·두 패키지 all-target Clippy가 통과했다(`target/recovery-store-full.log`, `target/recovery-core-after-store.log`, `target/recovery-store-core-clippy-green.log`).

소스 동결 뒤 Linux x86_64/macOS arm64 workspace all-target Clippy가 종료 코드0으로 통과했다(`target/recovery-v14-linux-check.log`, `target/recovery-v14-macos-check.log`). Windows 전체 `check_all.py`는 실행 중이다.

### UID v14 최종 자동 검증 snapshot

`scripts/check_all.py` 종료 코드 **0**: **1,126개**(문서29·빌드 도구63·Rust1,034). 전체 build/test/fmt/Clippy·아키텍처·문서·추적 검사 통과. 로그 `target/recovery-v14-full-check.log`, SHA-256 `CCE9F2BFD6B92AF5105D4C6B6F350CE4A6B56D38B164D4771FADDF4FBB7ADC21`, source hash `e557bd0eb9beaca919f3f7566deee490ae9c1ea3ed99062a981581125bccda75`. 같은 소스의 Linux x86_64/macOS arm64 all-target Clippy 종료 코드0. 실제 Mac 실행 검증은 아니다.

CNG 제품 키1개/names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D` 유지. 새 의존성 없음. 실제 사용자 DB·Core·Hello·계정·메일과 외부 공개는 실행하지 않았다. **[사람 검토 필요]**, 전체 제품 AC 완료0개 유지. 아직 없는 알림·초안·대기 변경의 AC 완료를 주장하지 않는다.

다음 묶음은 이미 연결된 복구 후 append/reconcile을 재구현하지 않고, 현재 저장 범위가 마지막 관측 상한까지 도달했는지 보여 주는 로컬 요약이다. 전체 목록·본문은 M2-05/M2-09 범위에 속하며 M2-04의 무조건 미완료 사유로 붙잡지 않는다. 요약은 실제 서버의 현재 최신성과 구분하며 자동 접속을 추가하지 않는다.

## 2026-10-30 로컬 UID 관측 요약 병렬 묶음 (설계 검토)

v14 기준선은 커밋 `1e332aa`로 저장했다(전체1,126개 통과, 작업 트리 clean 확인). [관측 요약 계약](../contracts/imap-uid-inspection.md)은 기존 cursor/recover 응답을 바꾸지 않고 `inspect`를 추가한다. Store 담당은 한 트랜잭션에서 현재 catalog/cursor/epoch를 읽고, 공개 담당은 strict DTO/IPC/영한CLI를, root는 Core와 실제 인증 pipe를 연결한다. 별도 담당자가 의미·권한·무변경 경계를 검토한다. migration/의존성/자동 네트워크를 추가하지 않는다.

관측 요약 설계의 현재 catalog/binding 의미와 cursor 부재 시 orphan 이력 탐지를 명확히 한 뒤 독립 검토 CLEAR를 받았다. Store9개 시험은 구현 전 모두 기대한 행동 실패를 확인했다(`target/uid-inspection-store-red.log`). 공개 경계도 Application 행동 RED를 확인했고, CLI/응답 형식 1차 검토에서 HIGH/MEDIUM 없음이다. 현재 개별 GREEN·Core 연결과 전체 검증은 진행 중이다.

관측 요약 공개 신규14개·관련6패키지310개, Store10개·양쪽 Clippy가 통과했다. Store 호출 전후 모든 제품 테이블 행이 같은지 확인하며 pending job만 있는 미관측 상태와 orphan 이력을 구분한다. 공개·Store 독립 검토는 CLEAR다.

Core 실제 인증 pipe 시험은 연결 전 CAPABILITY_UNSUPPORTED로 RED(`target/inspection-core-red.log`)를 확인한 뒤 단일 Store API+renderer를 연결해 **63개 GREEN**(단위61·lifecycle2, 단위14.16초, `target/inspection-core-green.log`)이었다. 복구 중 inspect가 기존 slot을 유지하고 provider를 추가 호출하지 않음, 복구/append 후 요약, 같은 mailbox의 catalog 세대 증가, 잘못된 계정/F권한 거부, 감사 완료를 확인했다. Core 교차 검토도 CLEAR다.

동결 소스의 Linux x86_64/macOS arm64 all-target Clippy 종료 코드0이다(`target/uid-inspection-linux-check.log`, `target/uid-inspection-macos-check.log`). Windows 전체 검사 진행 중이며 DB schema는 v14 그대로다.

### UID 관측 요약 최종 자동 검증 snapshot

`scripts/check_all.py` 종료 코드 **0**: **1,150개**(문서29·빌드 도구63·Rust1,058). 전체 build/test/fmt/Clippy·아키텍처·문서·추적 검사 통과. 로그 `target/uid-inspection-full-check.log`, SHA-256 `C0751375E81D1B05DE63CD822BE36D7E50E383345622985C86EA2F7B183131EA`, source hash `6622966c34defa4c1ebdd404d6357449a39655fcd6e096a667a94494a0eecf26`. 같은 소스의 Linux x86_64/macOS arm64 all-target Clippy 종료 코드0. 실제 Mac 실행 검증은 아니다.

CNG 제품 키1개/names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D` 유지. migration·새 의존성 없음. 실제 사용자 DB·Core·Hello·계정·메일과 외부 공개를 실행하지 않았다. **[사람 검토 필요]**, 전체 제품 AC 완료0개 유지.

다음 목록의 표준 도구는 요건의 `mail.messages.search`/`hantong search`다. 첫 버전은 정확한 IMAP 사서함 하나의 로컬 metadata 조회로 한정하는 계약을 검토한다. 헤더 공개를 기존 U 권한에 몰래 추가하지 않고 새 명시 native 갱신 경로를 설계하며, 과거 v1~v3 scope 이력은 그대로 보존해야 한다. 이 계획은 새 기능의 구현 완료나 실제 권한 부여를 뜻하지 않는다.

## 2026-10-30 로컬 메일 목록·명시 권한 v15 병렬 묶음 (자동 검증 완료 · 사람 검토 필요)

직전 기준선 `f70cdf3`의 1,150개 전체 검사 통과를 보존한다. [메타데이터 검색 계약](../contracts/imap-metadata-search.md)과 [권한 갱신 v4 계약](../contracts/client-metadata-scope-refresh.md)을 작성하고 각각 다른 담당자가 검토했다. 선택된 source page·현재 epoch·cursor 결속, byte budget, 헤더 안전 표시, v1~v3 이력 고정, pending/staged 재개 구분을 보완했다. 일반 연속 개발 위임에 따른 구현이며 **[사람 검토 필요]**다.

| 병렬 담당 | 소유 범위 | 검증 계획 |
| --- | --- | --- |
| Store | 불변 IMAP 위치 ID, 단일 snapshot 목록, 최신 관측·필터, cursor, 원자 mapping | mapping 누락/변조·원자 rollback·과거 epoch 격리·빈 page/byte budget 진행 |
| 공개 경계 | 공통 DTO, Application, IPC strict parser, 영/한 CLI, schema | 입력/응답 결속·부분 결과·제어문자 escape·한 번 호출 |
| 권한 | M 도구 집합, scope v4, native 표시/opaque proof, 기존 profile/history | 기존 L/F/U 보존·명시 확인·과거 pending/staged 그대로 재개·rollback |
| root | v14 동결·v15 migration, 감사/정책, 실제 서명 pipe 연결 | 모든 과거 행/순환 FK/sequence 보존·권한 거부·조회 중 worker 무호출·전체 회귀 |

먼저 migration이 아직 v14를 반환하는 행동 RED, 정책이 새 검색 도구를 인식하지 못하는 행동 RED를 확인했다(`target/metadata-v15-migration-red.log`, `target/metadata-policy-red.log`). Store mapping 신규4개와 권한 공개 경계3개도 각각 구현 전 행동 실패를 확인했다(`target/metadata-identity-red.log`, `target/metadata-scope-public-red.log`). 검색 DTO는 신규 모듈 부재의 compile RED를 확인했으며 행동 검증과 구분한다. 현재 v15 migration의 빈 DB DDL 일치/FK 정적 점검은 통과했지만 실제 SQLCipher 데이터 이전·전체 연결 검증은 진행 중이다.

기존 공통 오류·response schema, 최초 L 등록, 과거 payload/hash bytes를 유지한다. 전체 제품 완성이나 M2-05 전체 카드 완료로 기록하지 않는다. 본문/첨부·전체 GUI·실제 제공자·성능 목표·공개 설치는 후속 검증이 남아 있다.

v15 SQLCipher migration6개가 통과했다. 기존 모든 표의 행/JSON/hash, 두 UID epoch와 양쪽 reconcile cycle, 성공·미완료 recovery 작업, audit sequence high-water/부재를 보존한다. ABORT/IGNORE/삽입 후 ID 변조 및 최종 DDL 실패를 주입해 원본 rollback을 확인했다. 권한 공개 회귀214개·Store scope33개·Core scope11개, Application63개(신규7개)도 통과했다. 각 수치는 중복 경계를 포함하므로 더해 전체 검사 수로 표시하지 않는다.

큰 header 시험은 기존 Platform hash의32KiB 입력 제한으로 저장 자체가 실패하는 경계 오류를 찾았다. 인증 transcript는 기존 제한을 유지하고 Store payload용8MiB+4096 해시 경로를 추가했다. 동일 SHA-256 결과·독립64KiB 벡터·최대/초과 경계의 Platform 시험과 실제 큰 header 저장/페이지 예산 시험이 통과했다. 새 의존성은 없다.

IPC strict decoder는 원본 JSON의 duplicate/missing/unknown/null/array 대체, 구조 item·account 복제·header canonical hex/NIL·timestamp·오류 envelope·실제 byte cap을 검증한다. 신규4개 행동 RED 후 확장8개 GREEN(`target/metadata-client-{red,final-green}.log`)이다. 다른 담당자의 IPC/Contracts 검토는 CLEAR다.

실제 서명 pipe 통합은 미연결 시 CAPABILITY_UNSUPPORTED RED 후 단일 Store API+renderer 연결로 GREEN14.63초를 확인했다(`target/metadata-core-{red,green}.log`, 기존20초 제한 유지). L/F/U에서 header 조회 거부, M의 미관측/페이지/필터, 관측 변경 cursor 만료, UIDVALIDITY 복구 후 ID 격리, 네트워크 slot 무변경과 emitted count 감사 완료를 확인한다. 이는 통합 scenario1개 실행이며 Core 전체 회귀 완료와 구분한다.

교차 검토의 CLI2건은 누락된 Unicode 범위와 다음 cursor/부분·미관측·stale 표시였다. 두 건 모두 행동 RED 후 수정해 CLI44개 GREEN(`target/metadata-cli-review-{red,green}.log`), 재검토 CLEAR다. Scope/native/profile/Application 독립 검토도 CLEAR다. Store 손상 이력 거부 추가 보완과 전체 회귀/Clippy·교차 컴파일은 진행 중이며 최종 소스 동결 전이다.

### v15 최종 교차 검토와 통합 시험 분리

Store 전체361개(단위358·통합3)가 통과했다(`target/metadata-store-full-green.log`). 검색17개·identity9개·v15 migration6개를 포함하며 중복 합산하지 않는다. cursor가 없는 초기 append의 cv0 admitted/running/failed와 독립 audit는 허용하고, 과거 성공·후속 append·recovery·reconcile orphan은 손상으로 거부한다. 성공 reconcile replay/receipt에서도 누락된 identity mapping을 거부한다. 다른 담당자의 최종 Store 검토와 저장 payload hash cap 검토는 모두 CLEAR다.

최초 전체 검사에서는 실제 인증 pipe 통합 하나가 합산20초 제한을 넘겼다(`target/metadata-full-check.log`). 계측 실행에서는 policy까지4.60초, folder까지6.15초, reconcile2.78초, recovery/metadata5.25초, 전체16.24초였다(`target/metadata-core-timing.log`). 서로 독립적인 Store·provider 시나리오가 인증 시험 하나의 타이머를 공유한 구조를 확인했다. 인증·정책·폴더·append·reconcile·recovery/metadata를 여섯 독립 시험으로 분리하고 각각 기존20초 제한과 내부2/3/10/15초 제한, 모든 assertion·실제 서명 RPC를 보존했다. 고정 pipe 이름은 시험 전용 async mutex로 직렬화하며 잠금 대기는 시나리오 시간에 포함하지 않는다. 이전 IOCP handle 정리의 bind Conflict만 기존 방식대로1초 이내 재시도하며 시험 본문은 재시도하지 않는다. 제품 코드·제품 timeout은 바꾸지 않았다.

임시 계측을 제거한 Core 전체70개(단위68·lifecycle2)가 통과했다(`target/metadata-core-isolated-green.log`, 단위15.02초). 검토 담당자가 기존 모든 실행 경로와 실패 시 listener/key/Store 정리를 재검토해 CLEAR를 보고했다. 같은 수정 소스로 전체 검사와 Linux/macOS 교차 검사를 다시 실행하며 최초 실패 로그도 보존한다.

### 다음 병렬 묶음의 범위 검토

현재 M2-04·05는 부분 구현 상태다. M2-06~09의 선행 완료나 ADR-005 채택을 만들어내지 않고, 기존 진행 카드의 남은 범위를 먼저 준비한다. 다음 후보는 아래와 같으며 현재 묶음의 완료 항목과 구분한다.

| 담당 후보 | 기존 카드의 제한된 후속 범위 | 먼저 고정할 계약과 검증 |
| --- | --- | --- |
| A | M1-05·12 저장된 CLI 언어 읽기 | 명시 --lang → 저장값 → 영어 우선순위, 인증된 최소 표시설정 조회·미등록/미기동 fallback; CLI의 DB 직접 접근·별도 설정 파일 금지 |
| B | M1-10 비민감 진단 집계 | 집계 기간·상한·등록 소유 범위·고정 상태 집합; 계정/제목/주소/경로 미노출, 같은 snapshot의 bytes 일치, provider/queue 호출0 |
| C | M2-05 최초 동기화 진행·일시정지 모델 | 목표 UID 상한·pause/resume·재시작·예산·세대 무효화; sparse UID를 메일 건수로 표시하지 않고 암묵적 재개/자동 page loop 금지 |
| root | 계약·공유 모듈·Core/정책/감사 통합 | 신규 모듈 중심 파일 소유권 배정, 작성자와 다른 교차 검토, 실제 signed pipe와 전체 회귀 |

이 표는 다음 계약/구현 준비 계획이다. 일반 설정 쓰기, 전체 FTS 검색, IDLE, 본문/첨부 또는 제품 활성화 완료를 뜻하지 않는다. 실제 사람 보안 검토·제공자 상호 운용성·성능/GUI/배포 검증은 계속 남는다.

### 로컬 메일 목록 v15 최종 자동 검증 snapshot

`scripts/check_all.py` 종료 코드 **0**: **1,232개**(문서29·빌드 도구63·Rust1,140). build/test/fmt/Clippy·아키텍처·요건/문서/추적 검사 모두 통과했다. 로그 `target/metadata-full-check-green.log`, SHA-256 `1CB48A4D5AC31441E6AFB868316C0874C6D793187D5D0DA564396AD534A7BF9A`, source hash `a95541550f3a1108fa277486d6c0e81595914cba7b2c1063e29519f884526448`. 이 전체 실행의 Core 단위68개는21.39초에 통과했으며 기능별20초·기존 내부 제한을 유지했다. lifecycle2개도 통과했다. 검사 수 증가는 신규 검증과 기존 독립 시나리오5개의 분리를 함께 포함한다.

같은 소스의 Linux x86_64/macOS arm64 workspace all-target Clippy 종료 코드0이다(`target/metadata-linux-check-green.log`, `target/metadata-macos-check-green.log`). 실제 Mac 실행 검증은 아니다. Windows CNG 제품 키1개/names hash `3B692D49F1D08F7D3442806CF17694CDC02AF36B44839930FC18EB9C83CE9D4D`를 유지했다.

이번 변경은 `hantong search`의 영/한·JSON·안전한 원시 헤더 표시, M 권한의 명시 갱신, 현재 UID namespace의 로컬 목록/미읽음/즐겨찾기 필터, 엄격한 페이지 cursor, v15 identity mapping과 이전·rollback 검증이다. 실제 사용자 DB·Core·Hello·계정·메일이나 외부 공개는 실행하지 않았다. 새 의존성은 없다. **[사람 검토 필요]**이며, M2-05 전체 최초 동기화·성능·본문 지연 다운로드와 제품 전체 AC 완료0개 상태는 유지한다. 위 다음 병렬 묶음 표는 후속 계획이다.

## 최신 결과: 승인된 두 번째 값 삭제 완료, 합성 전달 검증 중단

2026-10-31 10:10:51 GMT+9에 생성된 Disabled 두 번째 값만 명시적 승인 후 영구 삭제했다. 삭제 후 해당 행이 0개이고, 원본(2026-10-30 20:18:52 GMT+9)은 Enabled이며 Add client secret 버튼이 활성화된 것을 확인했다. 새 비밀값은 생성하지 않았다.

실제 저장소와 네트워크 인증을 사용하지 않는 별도 복사본 safe-transfer-fixture에 합성 UI 전달 테스트를 만들고 빌드했다. 브라우저의 합성 복사 버튼은 값이 포함될 수 있는 접근성 라벨을 읽지 않고 CSS 접두사 선택자로 찾을 수 있었다. 하지만 네이티브 창 `HanTong SYNTHETIC TRANSFER TEST - no credentials`를 공식 `cua.getApp`으로 선택하는 단계에서 컴퓨터 제어 승인이 300초 대기 후 만료됐다. 그 사이 테스트는 240.12초 입력 대기 후 실패했다. 복사·붙여넣기 성공이나 안전 전달 경로 확보를 주장하지 않는다. 이는 제품 전체 회귀 실패가 아니라 격리된 수동 UI 사전검증의 입력 시간 초과이다.

새 값 생성·실제 보호 저장·OAuth·메일 목록/읽기는 미실행이다. 다음에는 공식 도구의 합성 테스트 창 접근 승인을 완료한 상태에서 합성 전달 검증을 재개해야 한다. Windows Hello가 실제 설정 중 나타나면 사용자가 직접 확인해야 한다. 새 값 생성 전 사전검증 조건은 유지한다. 제품 코드와 기존 전체 검사 1,621개 통과 결과는 변경하지 않았다. **[사람 검토 필요]**.

증거: safe-transfer-preflight-result.json, synthetic-transfer-build.log, synthetic-transfer.stdout.log, synthetic-transfer.stderr.log. 합성 테스트 소스는 제품 패치와 분리된 safe-transfer-fixture에 있으며 실제 자격증명을 포함하지 않는다.


## 최신 결과: 합성 전달 검증 통과 — 실제 자격증명 설정 전

공식 브라우저의 합성 복사 버튼 → 사용자 직접 Windows 보호 입력에 붙여넣기·F12 → 네이티브 캡처의 경로에서 exact_match=true를 확인했다. 저장·네트워크 전송은 모두 false이며 대화나 로그에 입력값을 출력하지 않았다. 자동 네이티브 붙여넣기는 도구가 사용자 입력을 감지해 실행되지 않았으므로 자동 입력 경로까지 검증됐다고 주장하지 않는다. 최종 실행 PID는 91308, 입력 시작은 2026-10-31 21:44:09 KST, 유효 만료는 21:49:09 KST이다. 최종 대화형 시험은 1개 통과, 별도 캡처 테스트 6개와 만료 테스트 2개도 통과했다.

앞선 시험 오류는 두 가지였다. 첫째, fixture 대기를 900초로 늘리면서 보호 입력의 300초 TTL을 놓쳐 잘못된 만료 시간을 안내했다. 실제 보호 TTL은 변경하지 않고 fixture 종료·표시·receipt를 같은 시각으로 맞췄다. 앞선 InputExpired 실행은 F12 완료 분기까지 도달했으므로 사용자 조작 실패로 설명하지 않는다. 둘째, 추가 진단에서 완료 시 입력 길이 0, empty=true, 오류 InvalidCredential이 확인됐다. 소유권·클래스·스타일·포커스는 모두 유효했고 만료가 아니었다. 빈 입력이 된 정확한 사용자 동작은 추정하지 않는다.

수정은 별도 safe-transfer-fixture에만 적용했다. 최상위 STATIC 창을 정상 등록한 Win32 창으로 바꾸고, 준비 단계에서는 보호 입력을 만들지 않도록 분리했다. 입력칸 표시를 보완했고 빈 완료 입력은 같은 원래 만료 안에서 대기하게 했다. 제품의 빈 자격증명 거부·소유권·스타일·TTL 검증은 그대로 유지했다. 오류 enum·길이·empty·검증 boolean만 기록하며 값은 기록하지 않는다. 정상 합성값, 빈 값, style 변경, 제어문자, 만료, 빈 제출 후 정상 입력을 자동 검증했다.

기존 원본 Google secret은 Enabled이고 노출됐던 두 번째 값의 승인된 삭제는 완료된 상태다. 삭제 후 새 secret 생성·실제 보호 저장·OAuth·메일 목록/읽기는 아직 미실행이다. 다음 실제 설정은 검증된 사용자 직접 붙여넣기 경로와 Windows Hello 직접 확인으로 진행한다. 제품 전체 검사 1,621개 통과 결과는 유지되며 제품 실행 코드는 이번 fixture 수정으로 바뀌지 않았다. **[사람 검토 필요]**.

증거: synthetic-safe-transfer-success.json, synthetic-final-reviewed-run의 receipt/result/metadata 및 stdout, synthetic-reviewed-capture-check.log, synthetic-reviewed-deadline-check.log. fixture 전용 diff는 synthetic-fixture.patch이고 제품 diff와 분리했다. 롤백은 별도 fixture를 사용하지 않으면 되며 제품 보호 코드·사용자 저장소에는 이번 시험의 변경이 없다.


## 최신 실제 결과: Google OAuth·보호 저장 성공, 메일 읽기는 로컬 CLI 권한에서 중단

기존 Desktop client에 승인된 대체 secret 1개를 2026-10-31 21:54:33 KST에 추가했고 원본 Enabled 값은 유지했다. 값은 DOM·접근성 라벨·스크린샷·로그·CLI 인자·일반 파일로 읽거나 출력하지 않았다. 이미 검증한 복사 버튼과 사용자 직접 보호 입력 경로를 사용했다.

첫 실제 요청에서는 보호 입력과 Windows Hello가 성공했지만 Google 미검증 안내의 Continue 클릭이 자동 승인 심사에서 거부됐다. 재시도·우회하지 않았다. 해당 화면은 Aside 창이었으며 사용자에게 전달되기 전에 300초 콜백 대기가 끝났다. 이 요청은 callback_timeout / AUTH_REQUIRED로 종료됐고 저장되지 않았다. 같은 새 값의 기존 복사 버튼을 재사용했으며 새 secret은 추가하지 않았다. 오래된 콜백 주소·code·state는 재사용하지 않았다.

두 번째 실제 요청은 22:10:06 KST에 시작했다. 사용자 직접 입력·확인 뒤 callback_validation, token_exchange HTTP 200, token_response, refresh_credential, identity가 모두 성공했고 공식 요청 상태가 ready가 됐다(22:12:24 KST 확인). 소스 흐름상 ready는 최종 사용자 계정 확인, 보호 저장, 계정 활성화 이후에만 설정된다. 따라서 OAuth·보호 저장은 성공으로 기록한다. refresh_credential 단계는 갱신 자격증명 유효성 확인이며 별도의 실제 토큰 갱신 요청까지 시험했다고 주장하지 않는다.

최소 메일 읽기를 준비하며 공식 accounts list를 호출했으나 exit 11 / SCOPE_DENIED였고 clients verify도 SCOPE_DENIED였다. Gmail 목록·본문 읽기는 아직 미실행이다. DB나 토큰 저장소를 직접 읽어 우회하지 않았다. 소스상 기본 clients register는 mail.accounts.list 권한을, 현재 clients refresh는 default_cli_send_tools()를 요청한다. 후자는 초안·발송까지 포함하므로 현재 읽기 전용 범위로 실행하지 않았다. 필요한 후속 경로는 해당 계정에 대한 로컬 CLI의 accounts.list/messages.search/messages.get 읽기 권한만 명시적으로 요청·확인하는 것이다. 기존 refresh를 실행하거나 새로운 로컬 권한을 부여하지 않았다.

제품 전체 검사 1,621개와 합성 캡처·만료 8개, 합성 전달 1개 통과 결과는 유지된다. 이번 실제 검증 중 제품 코드는 바꾸지 않았다. 검증된 수정 코어 PID 3708을 실행 중이며 원본 바이너리와 기존 백업은 보존했다. 이제 새 보호 자격증명이 저장됐으므로 원본 코어 전환이나 백업 복원은 연결 상태·형식 호환성을 검토한 뒤 별도로 해야 한다. 자동 롤백·백업 복원·commit·push·메일 쓰기는 수행하지 않았다. **[사람 검토 필요]**.

증거: actual-oauth-ready-evidence.json, actual-manual-setup/request-retry1.json(로컬 요청 metadata), ready-account.json(로컬 계정 metadata), 값 없는 단계 로그. 사용자 보고에는 비밀값·메일 내용·계정 내부 식별자를 포함하지 않는다.



## 최신 정정: 메일 grant가 아니라 보호 프로필 상위 폴더 ACL에서 차단

공식 `clients verify`에 값 없는 단계 진단을 추가한 결과, security_principal_allowlist → product_directory_security → profile_load 순으로 실패했다. 프로필 내용 읽기·인증 handshake·메일 권한 검사 전이다. 따라서 앞서 제안한 읽기 grant 추가가 현재 실패를 해결한다고 단정한 설명을 정정한다. 현재 실제 grant 부족 여부는 미확정이다.

읽기 전용 ACL metadata 확인: LocalAppData/mail-hantong의 소유자는 허용 주체이고, inheritance_protected=false, ACE 7개 중 허용 목록 밖 주체의 상속된 Allow ACE가 4개다. 제품은 현재 Windows 사용자·SYSTEM·로컬 Administrators 외 주체를 허용하지 않아 정상적으로 차단한다. 실제 SID/프로필 내용/토큰은 출력하지 않았다. ACL·grant·보호 정책은 변경하지 않았다.

변경 파일은 trusted_profile.rs(4단계 고정 진단, 기존 결과 보존 테스트 1개), profile_file.rs(고정 실패 분류만 추가), 자동 추적표다. 검사·오류 반환·short-circuit·허용 주체·보호 경계는 유지했다. 관련 IPC 테스트 142개 통과. 프로필 테스트는 sandbox junction fixture 생성 제한으로 2개 실패 후 네이티브 실행에서 18개 모두 통과했다. 최종 전체 검사 결과: exit 0, passed=True, Rust 1504개 / 30 suites. Build 검사의 한국어 출력 디코딩 경고 여부는 True이며 종료 결과와 구분한다.

기존 코어 PID 3708의 SHA-256을 확인해 보존하고 전체 검사 동안만 중지했다. finally에서 동일 바이너리를 PID 97804로 복구했다. 경로는 auth-stage-verification/mail-hantong-core-preserved.exe다. 원본 preview.4·백업·OAuth 자격증명은 보존했다. 새 OAuth/secret/권한 요청·메일 쓰기는 없다.

다음 실제 변경 전에 필요한 승인은 해당 한통 폴더의 ACL을 현재 사용자·SYSTEM·Administrators만 허용하도록 복구하는 범위다. 이것은 보호 기준을 맞추는 보안 설정 변경이므로 아직 실행하지 않았다. 다른 주체의 폴더 접근이 제한되는 영향을 설명하고, 복구용 ACL metadata를 보존하며 적용 범위를 검토해야 한다. ACL 해결 뒤 공식 verify와 현재 읽기 scope를 다시 확인하고, 실제로 필요할 때만 별도 최소 읽기 grant를 요청한다.

정확히 mail.accounts.list / mail.messages.search / mail.messages.get 세 도구만 요청하는 공식 CLI 경로는 현재 없다. 기존 fresh refresh는 쓰기 포함 C 묶음, B 읽기 묶음도 8도구다. 세 도구의 제한 경로는 새 strict DTO·account 결속·native proof·역사적 hash 보존·DB migration까지 필요한 별도 설계로 readonly-local-scope-proposal.md에 기록했다. 실제 기능 구현/연결/저장 형식 변경은 하지 않았다. **[사람 검토 필요]**.

진단 패치 롤백은 trusted_profile.rs·profile_file.rs의 고정 로그와 해당 테스트만 제거하고 추적표를 재생성하면 된다. 실제 grant/ACL/OAuth 저장을 되돌리는 작업은 포함하지 않는다.



## 최신 결과: 승인된 ACL 복구 성공, 다음 차단은 CLI 미등록

사용자가 한통 폴더 및 상속받는 하위 폴더 2개·파일 23개의 접근을 현재 Windows 사용자/SYSTEM/Administrators로 제한하는 변경을 승인했다. 대상·소유자·재분석 지점·상속을 재확인하고 ACL metadata를 별도 보호된 로컬 폴더에 저장·재읽기·hash 검증한 뒤 루트 DACL에 한 번 적용했다. 상위 LocalAppData, 소유자, 앱 내부 grant는 바꾸지 않았다. 루트의 기존 세 허용 주체의 FullControl와 상속 flags는 보존했다. 승인된 범위 밖 주체의 상속 접근만 제거됐다. 루트는 상속 보호/명시 Allow 3개가 되었고 총 26개 대상에서 허용 목록 밖 주체·Deny 0개, Allow 3개를 확인했다.

첫 사전 검사는 파일의 directory-only ACE 제외 차이(폴더 7개/외부 4개, 파일 6개/외부 3개)를 처리하지 못해 적용 전에 멈췄다. 두 번째 준비 검사는 .NET에서 아직 inherited인 ACE를 개별 제거할 수 없는 동작 때문에 적용 전에 멈췄다. 모두 실제 대상 ACL 쓰기 0회였다. 최종 준비는 상속 ACE를 메모리에서 제거하고 기존 허용 규칙 세 개의 권한·상속 flags를 그대로 명시 규칙으로 복사하여 검증했다. 최종 실제 ACL 적용 1회와 사후 검증이 성공했고 rollback은 필요하지 않았다. 이 실행 스크립트는 반복 실행용 제품 기능이 아니라 특정 승인 범위의 유지보수 기록이다.

코어는 동일 해시의 보존 바이너리로 PID 41156에 복구했고 공식 core status에서 ok=true/transport_ready=true를 확인했다. 공식 clients verify는 이제 CLIENT_NOT_REGISTERED / native exit 22다. profiles 폴더가 없어 profile_load/ancestor_open 단계에서 멈춘다. 이전 ACL SCOPE_DENIED는 해결됐지만 CLI 등록이 필요한 상태다. Google OAuth/보호 저장은 다시 실행하거나 변경하지 않았다. 프로필·token·DB 내용은 열람하지 않았다.

정식 기본 CLI 등록은 별도의 native 승인 경로다. 최초 등록은 registrable 계정 snapshot과 mail.accounts.list만 제시하며, mail.messages.search/get을 포함하지 않는다. 실제 대상 계정은 승인 화면에서 확인해야 하며 임의로 전체 계정 동의를 가정하지 않는다. 이번 승인은 ACL 복구뿐이므로 등록 요청과 지속 grant 변경은 하지 않았다. 메일 목록·본문 시험은 미실행이며, 정확히 세 읽기 도구의 제한 경로 설계안은 별도로 보존한다.

ACL 복구 자료는 oauth-deliverables/acl-repair-20261031T134652Z/acl-metadata-backup.json에 로컬 보관했다. 이 파일은 SID와 상대 경로를 포함하는 ACL metadata이며 자격증명/메일/DB 내용은 포함하지 않는다. 외부 업로드하지 않았다. SHA-256: AC7DF9BA4C93D016DFA71546142CE9F44C8E15AC94B7E833D96BE608C2874369. 실제 hash의 단일 기준은 같은 폴더 result.json이다.

제품 코드는 이번 적용 중 바꾸지 않았으므로 최신 전체 1,622개 통과 결과는 그대로다. 관리 스크립트 apply-approved-hantong-acl.ps1의 구문 검사와 실제 26개 ACL 검증·공식 인증 검사를 별도로 수행했다. **[사람 검토 필요]**.



## 최신 결과: 단일 Gmail 계정의 읽기 전용 등록 경로 준비

Google OAuth·보호 저장과 승인된 ACL 수정은 성공 상태로 보존했다. 실제 등록·권한 부여는 이번 단계에서 실행하지 않았다. 기본 프로필이 없고 공식 clients verify는 CLIENT_NOT_REGISTERED다. DB 등록 행의 존재는 미확정이다. 직접 DB를 열거나 등록 요청을 탐침으로 쓰지 않았다.

Gmail 목록·본문 기능은 암호화 로컬 캐시를 읽으므로 실서버 읽기에는 mail.sync.page와 토큰 갱신이 필요하다. 이전 3개 권한 전용 R 제안은 이번 완료 경로가 될 수 없어 대체했다. R 집합·DB 마이그레이션은 구현하지 않았다. 기존 B의 8개 읽기 도구를 선택한 Gmail 계정 하나에 결속하는 선택 기능을 추가했다. 기본 clients refresh는 모든 계정·발송 도구를 요청하므로 사용하지 않는다.

변경: Contracts의 bounded GoogleReadAccount, CLI register-readonly/refresh-readonly, IPC client/server의 엄격한 요청 필드 검증, Core의 단일 계정 L 등록/B 갱신을 추가했다. 다른 계정·쓰기 target의 pending/staged/running 요청 재사용은 CONFLICT로 중단한다. 기존 등록을 교체하지 않는다. grant 활성화, CNG 키·profile 결속, native proof, Windows Hello, 300초 TTL 및 정책 검사는 유지한다. 양 언어 도움말 및 native 안내에 Gmail 토큰 갱신과 본문 캐시 저장을 명시했다. 실제 등록 전이므로 source patch 역적용과 기존 검증 빌드로 되돌릴 수 있고 운영 ACL/OAuth를 되돌릴 필요는 없다.

검증: 새 보안 시험 11개(계정·scope 충돌 5, CLI 1, IPC 4, 안내 1)를 추가했다. 첫 sandbox 합성 Store 시험 3개는 보호 저장소 생성 InternalError로 중단했고, 동일 시험을 Windows 사용자 환경에서 다시 실행해 통과했다. 보호 검사를 완화하지 않았다. 최종 scripts/check_all.py exit 0: Docs 29 + Build 89 + Rust 1515 = 총 1633개 통과, 실패·무시 0개. fmt/Clippy(-D warnings)/전체 빌드/요건·아키텍처·추적표 검사 통과. Build 도구의 한국어 subprocess 출력 디코딩 경고 6건은 남았으나 89개 assertion과 전체 명령은 성공했다. 로그 SHA-256: ab786156fc93f44cca317a8cbdefdc9831587cbf25cba41b8ee90f48792f781a.

검사 중 named-pipe 충돌을 막기 위해 기존 코어를 잠시 중지하고 동일 보존 바이너리/해시로 PID 64136에 복구했다. 공식 core status 성공, verify CLIENT_NOT_REGISTERED, profiles 부재를 확인했다. 새 선택 빌드는 target/debug에 준비됐지만 사용자 코어에 적용하지 않았다. 실제 토큰 refresh·메일 동기화·목록·본문 시험은 미실행이다.

한 번에 요청할 다음 개별 승인 범위는 readonly-local-scope-proposal.md에 있다. 기존 Gmail 계정 하나의 CLI 등록, B 8개 도구의 지속 권한, CNG 보호 키·공개 profile 생성, 기존 Google credential 갱신과 받은편지함 최대 1건 캐시/읽기가 대상이다. 권한은 자동 만료되지 않는다. 사용자 Windows Hello 두 번이 필요하다. 실제 요청을 만들지 않아 현재 승인 카드나 타이머는 없다. **[사람 검토 필요]**.



## 최종 실제 시험 성공 — 2026-11-01 08:06:00 KST

사용자가 2026-10-31 22:59:28 UTC에 기존 Gmail 계정 하나의 CLI 등록·Windows 보호 키 생성, 읽기 8개 도구의 지속 권한, 기존 Google 인증 갱신과 받은편지함 최대 1건의 암호화 캐시/목록/본문 시험을 명확히 승인했다. 메일 발송·수정·삭제 권한은 요청하지 않았다. 실제 적용 전 PC 연결, 코어 부재, 프로필 부재와 검증된 코어/CLI 해시를 재확인했다. 기존 데이터·백업·Google secret은 유지하고 검증 빌드 코어 PID 37720을 실행했다.

첫 등록 요청은 기존 계정 하나와 mail.accounts.list만 포함했다. 사용자의 첫 Windows Hello 확인 뒤 공식 상태 ready 및 clients verify 성공을 확인했다. 그 뒤 같은 계정의 기존 B 읽기 도구 8개 요청을 하나 생성했다. 사용자가 두 번째 Windows Hello를 완료한 뒤 공식 refresh-status ready 및 인증 성공을 확인했다. 두 요청은 순차 생성했고 각 300초 만료 시각을 즉시 전달했다. 등록 충돌이나 추가 권한 변경은 없었다. 이 로컬 grant는 자동 만료되지 않는 지속 권한이며, 사용자는 이 점을 포함해 승인했다.

실제 시험 결과: 계정 목록 1개 확인 성공, 받은편지함 limit=1 동기화 요청 1회, 공식 completed 및 downloaded_count=1, 암호화 캐시 저장 성공, 받은편지함 목록 1건 조회 성공, 같은 캐시 메시지 본문 1건 조회 성공. 본문 요청 상한 1,024바이트를 지켰고 truncated=true였다. 이는 제한된 본문 조회 시험의 성공이며 전체 본문을 무제한 조회한 것으로 표현하지 않는다. 메일 제목·주소·ID·본문 및 토큰/secret 값은 도구 응답·일반 파일·변경 기록에 출력하거나 저장하지 않았다. CLI 응답은 메모리에서 처리하고 성공·건수·제한 boolean만 보존했다.

토큰 갱신은 실제 동기화 경로가 보호 저장소 내부의 refresh_verified를 통과한 후 Gmail을 fetch하도록 구현되어 있고 해당 동기화가 완료됐다는 근거로 성공을 확인했다. 별도의 HTTP 응답 원문/토큰은 읽거나 저장하지 않았다. 새로운 Google 로그인·secret 생성·서버 메일 변경은 없었다.

시험용 실행 스크립트는 처음에 제품 완료 상태를 succeeded로 가정하여, 실제 계약의 completed를 unknown으로 표시하고 목록/본문 전에 멈췄다. 제품 오류가 아니라 실행 스크립트의 상태 해석 오류였다. 계약과 Store terminal 코드를 확인해 스크립트만 수정하고 기존 execution ID의 공식 status를 조회했다. 새 동기화나 추가 메일 fetch 없이 이미 저장된 1건의 목록·본문 시험을 완료했다. 원인/수정은 run-approved-minimal-read.py 및 resume-approved-minimal-read.py에 남겼다. 일반 파일에는 operation/idempotency/execution 식별자와 결과 metadata만 저장했고 provider page token은 저장하지 않았다.

제품 구현은 앞서 통과한 1,633개 검사 이후 바뀌지 않았다. 이번 단계는 승인된 실제 적용과 조회 검증이며, source 변경은 진행 기록·자동 추적표 갱신뿐이다. 누적 source patch와 파일별 해시를 로컬에서 재생성한다. 신규 등록·권한을 되돌릴 때는 profile 삭제나 DB 직접 수정으로 우회하지 않고 공식 폐기/복구 경로를 별도 확인해야 한다. 기존 ACL 복구 자료와 보존 바이너리는 유지했다.

현재 기능 시험의 남은 차단은 없다. 보안 관련 코드의 **[사람 검토 필요]** 표시는 유지한다. 사용자의 최신 지시에 따라 이번 단계에서 외부 파일 공유·업로드를 전혀 수행하지 않았다. 최신 결과는 PC 로컬 산출물에만 반영되며 기존 Library 버전은 최신 완료 결과가 아니다.



## 2026-11-01 새 작업: Google 다계정 온보딩 UX

사용자의 다계정 사용성 개선 요청으로 기존 실제 읽기 성공 이후 별도 소스 작업을 진행했다. 같은 Desktop 앱의 보호 자격증명 재사용, 계정별 토큰 격리, 선택 계정만 기존 읽기 권한에 추가하는 안내, 인증 실패와 권한 부족 구분, 계정 상태 목록 및 Google/로컬 승인 재개 명령, 단계별 300초와 정적 콜백 안내를 구현했다. 자동 동기화·전체 계정/쓰기 권한 요청은 온보딩에서 제거했다.

기존 성공 계정·캐시·백업은 보존한다. 실제 새 계정 연결·실권한 변경·OAuth/토큰 갱신·credential 발급·ACL 변경·외부 업로드·커밋·배포는 이 작업에서 실행하지 않는다. 전체 검사를 위해 기존 코어/CLI 바이너리를 해시로 보존하고 동일 버전을 복원한다. **보안 코드 사람 검토 필요**. 세부 계약은 `docs/contracts/google-onboarding.md`, 사용자 안내는 `docs/latest/GOOGLE-MAIL.md`, 최종 검사 결과와 패치는 작업 폴더 `oauth-deliverables/ux-onboarding-*`에 기록한다. 최초 연결 저장 전 취소와 코어 재시작 전 미완료 요청은 그대로 복구할 수 없다는 한계가 남는다.


## 사전 점검 갱신 — 2026-11-01 Google 쓰기 요청

이 절은 앞선 테스트 사용자 미등록 차단을 대체한다. 사용자 개별 승인 후 같은 프로젝트의 테스트 사용자에 지정 계정을 추가했고, Audience에서 기존 계정과 함께 2명을 확인했다. 운영 모드 전환은 Google의 Publish app disabled 및 Branding 구성 완료 요구로 미실행이며 여전히 Testing이다. 홈페이지·개인정보처리방침·약관 URL은 비어 있고 개별 필드의 정확한 차단 기여는 미확정이다. URL을 임의로 입력하거나 검증 심사를 제출하지 않았다.

이후 사용자가 private-account 계정의 초안/발송/읽음·라벨 변경과 두 계정 사이 각 1통의 합성 첨부 시험을 요청했다. 새 실제 OAuth/권한/메일 단계는 권한 범위 확정을 위해 보류했다. 현재 Gmail 구현은 readonly이며 SMTP 발송 원장만 존재한다. Gmail POST·첨부 승인 결속·첨부 수신 검증·변경 도구와 계정별 선택 grant는 추가 구현 대상이다. 기존 검사 1,645개 통과는 쓰기 기능/실제 송수신 검증이 아니다.

정확한 최소 OAuth 제안은 private-account: openid/email/gmail.modify, private-account: openid/email/gmail.readonly/gmail.send이다. modify는 Google 자체에서 휴지통 이동/초안 영구 삭제도 허용하므로 앱의 삭제 차단과 구분해 설명해야 한다. Google 전체 mail scope는 요청하지 않는다. 실제 grant 확대는 새 구체 승인 전 수행하지 않았다. 개발 소스와 합성 시험은 다음 단계이며 현재 제품 코드 변경은 없다.

기존 코어 PID44160/정확 경로 존재를 Windows 사용자 관측으로 확인했고 실행 파일 SHA-256은 기존 25CDD6E2F1D29FF9C68925678AE3071A7C0E937E1E97F06D4EF6E9B221E68253와 같다. sandbox에서 PID가 보이지 않는 관측을 코어 부재로 해석하지 않았다. 새 UX 실행·OAuth·credential·grant·메일 작업 및 외부 업로드는 이번 단계에서 모두 미실행이다. 상세 구현 공백·권한·23바이트 합성 첨부 시험·남은 차단은 로컬 oauth-deliverables/gmail-write-preflight.md에 기록했다. 보안 코드 **[사람 검토 필요]** 유지.


## Gmail 쓰기 구현 중간 기록 — 2026-11-01T05:40:29.169923+00:00

이전 preflight의 “제품 코드 변경 없음” 이후 쓰기 모드/보호 번들/제한된 전송/첨부 스냅샷/schema 20/발송 승인 연결을 구현 중이다. 전체 대상 컴파일 및 집중 테스트 28개가 통과했다. 기존 imap 전용 계정 검증 연결, CLI/IPC 첨부·권한·수신 첨부·전용 마이그레이션 테스트와 전체 회귀가 남아 있으며 실제 Gmail 쓰기는 미실행이다. 실제 코어/계정/DB는 변경하지 않았다. 자세한 변경·시험·제한·승인 범위는 oauth-deliverables/gmail-write-checkpoint.md에 기록했다. **[사람 검토 필요] / WIP / 실제 시험 미실행**.


## Gmail 발송·첨부 구현 검증 — 2026-11-01T06:01:15.830493+00:00

전체 check_all exit 0, 1674개 통과(Rust 1556/Python 118). 명시 모드 OAuth, 승인·계정·첨부 결속 Gmail 발송, CLI/IPC/MCP 첨부, 선택 계정 C11 요청, opt-in 수신 첨부 digest, schema 20 합성 마이그레이션/복구를 검증했다. 원본 core/CLI 복구, 실제 OAuth/grant/DB/메일 쓰기 미실행. 라벨 계정별 승인 및 원격 Gmail 초안 공개 흐름은 미완료이며 전체 사용자 목표 완료가 아니다. 자세한 기록은 oauth-deliverables/gmail-write-implementation-report.md, 남은 경계는 gmail-write-remaining-design.md. **[사람 검토 필요]**.


## 2026-11-01 Gmail 서버 초안·라벨 변경 후속 작업

[사람 검토 필요] 계정 한정 변경 권한, 네이티브 단일 실행 승인, 불확실 재실행 차단 원장 및 schema 21 마이그레이션을 추가했다. 초기 합성 원장/마이그레이션 13개와 문서 29개, 전체 Clippy 통과. 전체 회귀 진행 중이며 실제 OAuth 확장·사용자 DB 마이그레이션·메일 변경은 미실행이다. 기존 schema 19 코어를 보존한다.


## Gmail 서버 초안·읽음·라벨 승인 구현 — 2026-11-01T06:33:36.649802+00:00

전체 check_all exit 0, 1693개 통과(Rust 1575, Python 118). 계정별 변경 grant/proof v7, native review/Hello 결속, 단회 원장·감사·unknown 차단, CLI/IPC, schema21 합성 마이그레이션/복구 검증. 실OAuth/grant/DB/메일 변경은 0건, 원본 runtime 복구. Production은 Branding 설정 완료 안내로 비활성이며 구체 필드 미확정. 상세 기록: oauth-deliverables/gmail-write-implementation-report.md. **[사람 검토 필요]**.

## 2026-11-02 Gmail 첨부 수신 실검증 보완 (진행 중 / 사람 검토 필요)
양방향 각 1통 발송 및 수신·제목·본문(끝 줄바꿈 제외) 확인. 첨부 검증을 켠 동기화만 INVALID_INPUT 발생. 무비밀 단계별 진단과 합성 재현으로 원인을 좁히고 기존 두 메일만 재조회한다. 실제 재발송·메일 변경·권한 변경·외부 업로드 없음. 기존 schema21 코어/CLI와 수정 전 소스는 oauth-deliverables/attachment-fix-20261102에 보존. 전체 회귀 및 첨부 실검증은 아직 미완료.


## 원본 프로젝트 기존 공개 릴리스 기록 보존 (2026-10-30)

아래는 통합 전 원본 HEAD 2852a31의 추가 기록이다. 이번 로컬 개발/실제 계정 시험과 시점을 구분하며 기존 공개·설치 검증 이력을 보존한다.

Google 다중 계정 로그인·Gmail 읽기 구현(2026-10-30): 브라우저 OAuth/PKCE, 계정별 DPAPI 갱신, v19 캐시·동기화, native 확인과 CLI·터미널·공통 도구 연결을 구현했다. 최종 전체 검사 **1,612개**, preview.3→4 설치·업데이트·반복·제거, 공개 다운로드 재설치 검증을 통과했고 [preview.4](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-preview.4)를 게시했다. 실제 Google 프로젝트 등록·로그인·메일 호출은 수행하지 않았다. 보안·계약 변경은 **[사람 검토 필요]**다.

직전 검증 산출물은 `artifacts/cli/0.1.0-preview.3/`에 보존한다. 최신 label은 `0.1.0-preview.4`이다. 미리보기는 완성된 정식 제품을 뜻하지 않는다. 첨부·예약·주소록·규칙 자동화·실제 제공자 상호 운용성과 사람 보안 검토가 남아 있다.

| 현재 카드 | 사용자 요청의 **다중 Google 계정 로그인·Gmail 읽기와 CLI preview.4 배포**를 구현·검증했다. 상태는 **사람 검토 필요**이며 실제 Google 로그인·공개 앱 인증·새 OS·정식 출시 게이트 완료는 아니다. M8 GUI와 나머지 P0는 후속이다. |

| 최신 검증 | **1,612개 통과**(문서 29·빌드 도구 89·Rust 1,494). Windows check-all의 fmt/Clippy/build/test·문서/의존/추적 검사, Linux 전체 target cross-Clippy 통과. 5계정 병렬·격리·서명 IPC·v19 이전/롤백, PowerShell5.1 preview.3→4 설치/업데이트/반복/제거와 최종 공개 ZIP 재설치·데이터 보존을 확인했다. 실제 Google·새 OS·Mac 검증은 남아 있다. |

| GitHub | 공개 저장소 [tz1012/mail-hantong](https://github.com/tz1012/mail-hantong), 소스 `afd8ac5c567e70ff9f7b840d83f3c81f4f1d874a`, [개발 미리보기 v0.1.0-preview.4](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-preview.4)와 ZIP/SHA256SUMS/BUILD-METADATA 게시 완료. 원격 CI는 별도 실행 중이며 로컬 통과와 구분한다. 실행 데이터·에이전트 상태는 소스에 포함하지 않는다. WinGet/MSI·서명은 후속이다. |

- **패키지**: `artifacts/cli/0.1.0-preview.4/` 생성. ZIP 21,657,672바이트, SHA256 `fac65f8bb44232846f60ffbd971cf67f33e860197cb33ba433a1a9b659d648ad`. 실제 빌드 소스 해시·모든 manifest 파일·체크섬·4개 실행 파일의 Windows 시스템 DLL 의존과 외부 VC++ 런타임 불필요를 확인했다. Google 사용 안내를 패키지에 포함했다. Windows PowerShell5.1에서 preview.3→4 설치/업데이트/반복, 공백·한글 경로, 모든 launcher 도움말/버전, JSON 오류/종료 코드, 제거와 임의 데이터 보존을 통과했다. 로그 `target/google-preview4-install-smoke.log`, SHA256 `7fff3f02c8f273d4390781b43393a9441017730d3507b64f21e0f6f8bdd45a9a`. [공개 릴리스](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-preview.4) 실제 다운로드·체크섬·설치·실행·제거도 통과했다. 로그 `target/google-preview4-public-download-smoke.log`, SHA256 `db7919c97e7e9033a78c5211b67111d55f19c056fcfa0a256ddf2ee9464bf54d`. 이 시험은 실제 코어 시작·Google 계정·메일을 사용하지 않고 사용자 PATH를 영구 변경하지 않았다.


## 2026-11-02 원본 통합 및 양방향 첨부 실검증 완료 — 사람 검토 필요

기존 두 시험 메일 모두 수신·발신자·제목·본문(끝 줄바꿈 제외)과 합성 txt 파일명/23바이트/SHA-256 일치를 확인했다. 총 발송 2통, 재발송 0. 첨부 ID의 잘못된 256자 제한을 별도 4,096바이트 검증으로 수정하고 쿼리 주입을 차단했다. 정확한 메시지 1건을 지정하는 읽기 동기화로 새 메일 도착 경쟁을 피한다. 추가 schema 변경이나 인증/권한 경계 완화는 없다.

원본 작업파일 82개의 바이트·해시 보호 백업 후 코드/문서 116개를 통합했다. 원본 공개 릴리스 기록을 보존했다. 작업 사본 및 원본에서 각각 전체 검사 1,699개 통과. 원본 재개 문서 `docs/latest/RESUME-20261102.md`, 변경 기록·검증 요약 `docs/local-verification/2026-11-02/`를 작성했다. 기존 전체 대화가 프로젝트로 이동된 것은 아니다. 보안 코드 사람 검토·Production 게시·Ask Aside 확인·Hello 일괄 승인 개선 및 실제 초안/라벨 시험은 별도다. 커밋/푸시/업로드/재발송 없음.


## 2026-11-02 로컬 개발 재개 — 교차 빌드 복구·Gmail 변경 통합 검증 (사람 검토 필요)

최신 재개 문서·진행 기록·실제 git 상태를 확인하고 기존 미커밋 변경과 새 파일을 보존했다. CLI 우선 방침을 유지하며 현재 hantong은 명령형 CLI다. 전체 화면 TUI는 미구현이며 GUI 디자인 추가는 하지 않았다. 이번 작업은 M5-09/M6-10 등의 후속 공통 코어 검증과 ACC-02 온보딩 플랫폼 경계 보완이다. 전체 카드나 제품 완료를 선언하지 않는다.

Linux 교차 검사에서 verify_default_readonly의 Windows 인증/tokio 참조와 플랫폼 cfg 누락을 재현하고 수정했다. apps/mail-hantong-core/src/main.rs의 Windows 모듈 경계, crates/hantong-ipc-client/src/google_mail.rs의 비Windows probe 거부, hantong-secrets/src/google/{client_secret,diagnostics,write}.rs 및 hantong-store/src/mail_send.rs의 플랫폼 경계를 정정했다. 보호 자격증명 타입의 비공개 필드는 비Windows에서도 보존한다. 계약·schema·승인 발급·권한은 바꾸지 않았다.

google_mutation.rs에 전용 시험을 연결하고 새 google_mutation_tests.rs의 APR-01/SYNC-07 시험 4개를 추가했다. claim 전 공급자 미호출, accepted/rejected/unknown 결과, 호출 한도·스냅샷 변경 admission 거부, 실행 중 credential 변경의 future 취소/SEND_UNKNOWN/새 키 재실행 차단을 검증했다. google_integration_tests.rs에는 서명된 읽기 프로필의 변경 요청/상태 조회 거부를 추가했다. 집중 검사 5개 통과(기존 scope 시험 1개 포함). fixture의 계정 사전 삽입과 Store 동시 열기 오류는 공개 activate→Store 닫기→합성 SQL fixture→재열기 순서로 수정했다. 제품 보호 검사는 완화하지 않았다. 새 시험의 Clippy 경고도 수정했다.

최종 python -X utf8 scripts/check_all.py exit 0. Rust 1,585개/30 suites, 문서 29개, Build 도구 78개 통과: 실제 통과 합계 **1,692개**, 실패 0개. Build suite는 89개 중 pwsh 미설치로 기존 설치 시험 11개가 건너뛰어졌다. 이를 통과로 세지 않는다. Rust ignored 0. fmt/전체 Clippy(-D warnings)/전체 build/요건·아키텍처·추적 검사 통과. Linux x86_64 및 macOS arm64 workspace/all-target 교차 Clippy exit 0. 실제 해당 OS 실행 시험은 아니다.

전체 검사 동안 기존 코어를 실행 파일 hash로 보존하고 잠시 중지한 뒤 같은 바이너리·기동 인수로 복구했다. 최종 PID 48600, 원본 SHA-256 6FE42CF1D958594ABAD72009936C900E3EF65A27791C9420FAB101F6FB72F209. 공식 core status exit 0 / ok=true / transport_ready=true. 실제 계정 데이터/DB/프로필/비밀값을 직접 열지 않았다. 로그인·토큰 갱신·권한 확대·메일 조회/발송/변경·공개·커밋은 실행하지 않았다.

검증 로그 target/resume-full-check.log, 구조화 증거 docs/local-verification/2026-11-02/local-development-verification.json. 전체 로그 SHA-256 6378898790c366d60af91075dc256dda3634ad6a5fe616a3b87ece8231b91e39. 교차 로그 target/resume-linux-verified.log / target/resume-macos-final.log. 자동 추적표를 갱신했다. 남은 일은 pwsh 설치 시험, 사람 보안 검토, 별도 승인 범위의 서버 초안/라벨 실제 검증과 기존 재개 문서의 후속 항목이다. 실제 작업 승인 경계와 기존 중단 상태를 유지한다. **[사람 검토 필요]**.

기록 정정: 위 작업 기록을 PowerShell의 기본 파이프 인코딩으로 전달하면서 두 개의 새 항목에 한글이 '?'로 기록되었다. 손상된 원문은 target/resume-progress-corrupt-backup.txt에 보존하고, 해당 두 항목만 이 내용으로 복구했다. 이전 진행 기록은 보존했다.

## 2026-11-02 사용자 이용 흐름 우선순위 정정

사용자가 설치→실행→계정 연결→권한 승인→이용의 간단한 흐름을 요구하고 Nylas CLI를 참조로 제시했다. 실제 installer/CLI/Core/Google 온보딩 계약을 확인했다. 설치는 실행 파일·shim·PATH만 준비하고, 코어는 foreground run만 제공하며, 빈 CLI 실행은 안내형 첫 실행 마법사로 연결되지 않는다. login google 내부의 계정 연결·로컬 등록/읽기 권한 안내는 있으나 코어 수동 실행, BYO 앱 설정, 명시적 동기화/목록 명령이 사용자에게 남는다. 따라서 개별 기능 시험 통과를 간단한 제품 이용 흐름의 완성으로 표현하지 않는다.

공식 Nylas 시작 안내(https://cli.nylas.com/guides/getting-started, 2026-11-02 확인)는 init에서 Nylas 가입/로그인·앱/API key·첫 메일 연결을 묶는다. 단일 진입점과 안내 흐름을 참고하며 외부 메일 중계 서비스 도입은 결정하지 않았다. docs/latest/DESIGN.md에 CLI 첫 실행 UX와 완료 기준, 코어 수명·설치 실행 파일 신뢰 경로·제품 OAuth 준비의 미해결 경계를 기록했다. 새 런타임/승인 계약, 제품용 Google 앱 설정, 첫 실행 기능은 이번 문서 변경만으로 구현 완료가 아니다.

이 항목에서는 코드·실행 코어·사용자 설정·계정·권한·비밀값·외부 서비스에 변경하지 않았다. 이전 작업에서 인코딩이 손상된 새 진행 기록 두 항목만 원문 백업 후 복구했다. 과거 진행 기록과 미커밋 작업은 보존한다. 다음 구현 우선순위는 단일 시작 경로의 코어 준비→계정 연결→명시적 권한 승인→이용 안내다. 기존 로컬/실계정 승인 경계와 보안 코드 사람 검토 상태는 유지한다.

문서 변경 검증: tests/Docs 29개 통과, traceability --check valid=true. 코드 변경이 없어 Rust 전체 검사는 반복하지 않았다. 구현을 추가한 뒤에는 해당 통합 검증이 필요하다.



## 2026-11-02 Google 운영 모드 게시 완료

승인된 웹 파일 5개를 gh-pages에 게시하고 세 안내 페이지·CSS HTTP 200/원문 일치를 확인했다. 동일 Google 앱 Branding URL/도메인 저장과 In production 전환 완료. main/개발 코드·credential·scope 변경 없음. 게시 후 core status 종료 0, gmail accounts 종료 10으로 두 계정 ready 재확인 미완료. Google 검증은 미제출, 100명 제한 및 미검증 안내 별도. 소유 확인은 요구되지 않았다. 상세와 화면/JSON 증거는 docs/local-verification/2026-11-02/google-publishing/. 기존 앱 검사 1,699개 결과 재사용(이번에는 앱 소스 수정 없음). 보안 코드 사람 검토 필요 유지.


## 게시 후 로컬 인증 진단 — 2026-11-02

공식 clients verify에서 로컬 IPC handshake 실패를 확인했다. 프로필 읽기/파싱/기존 클라이언트 키 준비 통과, 동일 CLI·코어 해시 및 Windows 사용자/로그온 문맥 일치. Google 동의나 계정별 메일 요청 전 단계다. handshake 하위 원인은 현재 진단으로 미확정이며 보안검사를 유지한 무비밀 단계 진단 보완이 다음 최소 조치다. 재동의·키·권한·ACL·코어 변경 없음. 상세는 docs/local-verification/2026-11-02/google-publishing/LOCAL-AUTH-DIAGNOSIS.ko.md.

## 2026-11-02 Aside 브라우저 MCP 연결 및 Google 게시 상태 읽기 전용 재확인

사용자가 Aside MCP 연결 방법의 인터넷 확인을 요청했다. 공식 https://docs.aside.com/help/developers.md 의 Windows CLI 설치·stdio MCP 안내를 확인했다. Aside 브라우저와 데몬은 이미 실행 중이었지만 CLI는 없었다. 공식 설치 스크립트의 Authenticode Valid, AT YOUR SIDE INC 발행자, 타임스탬프를 확인하고 CLI 1.26.1028.2335를 사용자 로컬 디렉터리에 설치했다. SkipPathUpdate로 PATH는 변경하지 않았다. codex mcp add aside로 기존 서버들을 보존하면서 절대 실행 경로와 mcp 인수만 추가했다. 비밀값 파일은 열지 않았다.

실제 stdio initialize 및 tools/list 성공: browser_profiles, repl, memory_search, exec. 현재 세션의 동적 도구 목록에는 새 서버가 즉시 나타나지 않으므로 같은 공식 CLI의 repl로 기존 Aside 탭 목록과 브라우저 접근을 검증했다. CLI 도움말의 getTabs는 설치 버전에서 미정의였고 listBrowserTabs는 정상 작동했다. openTab은 명령 종료 시 닫히는 자동화 탭이므로 지속 표시되는 사용자 탭을 열었다고 주장하지 않는다. Aside.exe 직접 창 실행은 도구의 자동 정책 검토에서 차단됐고 구체적인 이유는 제공되지 않았다. 기존 작업 탭은 변경하지 않았다.

Aside의 기존 로그인 세션에서 지정 프로젝트 mail-hantong-test-20261030의 Audience를 읽었다. Publishing status는 In production, User type External, OAuth user cap은 2 users / 100 user cap이다. Verification Center에는 branding이 사용자에게 표시되지 않는다는 안내와 data access가 검증되지 않았다는 안내가 있다. 운영 모드 게시와 검증 심사 완료를 구분한다. 로그인·비밀번호 입력·설정 저장·검증 제출·메일 작업은 수행하지 않았다. 이전 게시 완료 기록과 일치하며 기존 IPC handshake 진단 미해결 상태는 별개로 유지한다. CLI 첫 실행 구현의 통합 검사·수정은 계속 진행한다.

## 2026-11-02 CLI 첫 실행 마법사 구현 및 통합 검사

사용자 지정 기능: 설치→실행→계정 연결→사람 권한 승인→이용. 상태: 로컬 구현·자동 검사 완료 / **[사람 검토 필요]**. 전체 제품 출시 완료가 아니다. 요건 CLI-01/04, ACC-02/03, SETUP-01, INST-03/04, APR-01, SYNC-02.

변경: apps/hantong-cli/src/core_start.rs, onboarding.rs, google.rs, main.rs와 한·영 리소스/build.rs/bootstrap 시험. hantong init 및 인터랙티브 빈 실행으로 번호형 안내를 시작한다. 기존 Core를 재사용하거나 동일 설치 패키지의 해시·경로·중복 JSON 키를 검사한 sibling Core를 한 번만 백그라운드 실행한다. PATH의 실행 파일을 찾거나 시작 프로그램을 등록하지 않는다. JSON/no-input/리다이렉트는 부작용 전에 거부한다. Google 연결과 로컬 권한 승인은 기존 보호된 경로를 사용하고 일반 메뉴에는 내부 식별자를 표시하지 않는다. 연결만으로 메일을 읽지 않으며 받은편지함 메뉴를 고른 뒤에만 INBOX 최대 20건의 단일 페이지 동기화·상태·목록을 호출한다. 번호를 고르면 본문을 읽는다. 전체 화면 TUI는 아니다.

packaging/windows/install.ps1은 release-manifest.json의 정확한 바이트를 별도 소유 파일로 설치·제거하여 최초 실행 검증에 제공한다. 활성화 실패 전에 사용자 PATH를 변경하던 순서를 수정했다. tests/Build/test_cli_onboarding_install.py는 가짜 실행 파일과 임시 설치 루트만으로 실제 Windows PowerShell 5.1 설치→동일 버전 재설치→설치된 제거 스크립트 실행→사용자 데이터 보존을 검증한다. docs/contracts/cli-onboarding.md와 CLI-ONBOARDING-PLAN.md를 추가하고 CLI-LAUNCH.md에서 새 작업 트리와 기존 공개 preview.4의 사용법을 구분했다. 리뷰의 00 계정 선택 오류를 수정하고 회귀 시험을 추가했다. 실제 계정·외부 메일 시험은 추가하지 않았다.

최종 python -X utf8 scripts/check_all.py exit 0: Rust 1,602개/30 suites/ignored 0, 문서 29개, Build 도구 91개 중 실제 79개 통과·12개 건너뜀. 실제 통과 합계 **1,710개**, 실패 0개. fmt, 전체 workspace Clippy(-D warnings), 전체 build, 요건/아키텍처/추적 검사 통과. 초기 stale traceability 실패 후 재생성했고, 다음 실행의 IPC bind Conflict는 검사 도중 재등장한 운영 Core와의 충돌이었다. 실패 로그는 각각 target/onboarding-first-check-stale-trace.log 및 onboarding-pipe-conflict.log에 보존했다. 최종 로그 target/onboarding-full-check.log SHA-256 d5dd193cb7aab4ac72afd41c9a51e08a21af0e0bae19fb4076aacec71d1e0eac. 최종 실행에서는 관찰한 원본 Core의 hash/인수를 보존하고 잠시 정지한 뒤 같은 바이너리로 복구했다. 복구 PID 8472, 원본 SHA-256 6FE42CF1D958594ABAD72009936C900E3EF65A27791C9420FAB101F6FB72F209. 공식 core status ok=true, transport_ready=true. 계정 인증 성공을 의미하지 않는다.

비 Windows 분기의 needless return을 수정한 뒤 CLI Linux x86_64·macOS arm64 교차 all-target Clippy 및 Windows CLI all-target Clippy를 각각 exit 0으로 확인했다. fmt와 최종 traceability --check도 통과했다. 최종 source_hash 50134e2bbead474679b9ff39198e22f75e496d017fc3fc8e6365085fb8c0941f. 교차 검사는 해당 OS에서의 실제 실행 시험이 아니다. Build 건너뜀 12개는 pwsh를 요구하는 기존 시험이며 새 Windows PowerShell 설치 시험은 실제로 통과했다.

남은 일·제약: 보호 저장된 Google 앱 설정이 없는 깨끗한 PC는 여전히 제품용 앱 설정 제공 단계가 필요하다. 실제 client ID/secret을 추출·내장하거나 기존 사용자에게 새 비밀값을 요구하지 않았다. 기존 운영 환경의 IPC handshake 진단 문제와 계정 ready 재확인 미완료는 유지한다. 실제 첫 실행·권한 창·메일 이용의 실계정 종단 검증, 사람 보안 검토, 제품 앱 설정 배포, 새 패키지 게시를 자동 검사 통과로 대체하지 않는다. 커밋·푸시·배포·Google 설정 변경·메일 발송은 하지 않았다. 기존 미커밋 파일과 사용자 변경을 보존했다. 다음 작업은 무비밀 IPC handshake 단계 진단과 깨끗한 설치의 제품 앱 설정 경로 보완이다.


## 2026-11-02 로컬 handshake 재시작 복구 확인

원래 동일 해시 코어 재시작 후 기존 두 계정 ready/transport 정상을 확인했다. 최초 실패는 challenge 수신 전 종료였으며 서버 내부 근본 원인은 재시작 후 미재현이다. 보안검사를 유지한 고정 단계 진단과 시험 하네스 출력 보존을 원본에 추가했다. 최종 전체 검사 1720개 통과, 디코딩 오류 0. 키/권한/ACL/Google 재동의/메일 변경 없음. 현재 코어는 원래 바이너리, 진단 코드는 원본에 준비된 상태다. 상세: docs/local-verification/2026-11-02/handshake-recovery/HANDSHAKE-RECOVERY.ko.md. 보안 코드 사람 검토 필요.


## 후속 동시 변경 뒤 재발 관측 — 2026-11-02

02:44 UTC 최종 복구 PID 55868에서는 기존 두 계정 ready/transport를 확인했다. 이후 마지막 패치 검증 중 코어가 0개였다가 PID 8472로 바뀌었다. 새 PID는 같은 원래 경로·해시이나 계정 조회 exit 10 및 공식 clients verify AUTH_REQUIRED가 재발했다. 고정 단계: hantong_handshake stage=challenge_read outcome=failed, hantong_auth stage=handshake outcome=failed. 이번 기록 단계에서는 그 코어를 중지·재시작하거나 권한/키를 변경하지 않았다.

동시에 원본 core_start.rs와 docs/traceability.md가 02:47 UTC에 저장 당시 해시와 달라졌다. 진단 소스 여섯 파일은 기록 해시 그대로다. 패치의 Windows 줄바꿈은 LF로 정규화했으며, 후속 변경된 두 파일을 제외한 진단 패치 역적용 check는 exit 0이다. 전체 패치는 02:44 저장 시점의 검토 산출물이며 현재 후속 변경 전체에 무조건 적용 가능한 것으로 간주하지 않는다. 후속 코드를 덮어쓰지 않았다.

1,720개 통과는 최종 검사 당시 소스에 대한 사실이다. 현재 후속 소스 변경 및 PID 8472의 ready까지 검증한 결과가 아니다. **현재 계정 조회는 다시 차단돼 있으며 근본 원인 미확정**이다. 실행 코어의 후속 소유 작업과 조율해 재시작을 멈춘 뒤 실패 중인 인스턴스의 무비밀 증거 수집을 이어가야 한다. 사용자에게 Google 재동의/등록/권한 확대를 요구할 근거는 없다. Google In production 게시 성공은 변함없다.

## 2026-11-02 사용자 이용 흐름 마무리: 저장소 분리 원인 확인 및 수정 진행

사용자가 사용자 관점의 설치→실행→계정 연결→승인→이용을 모두 해결하라고 재지시했다. 최신 기록·실제 미커밋 소스를 기준으로 재조사했다. 현재 일반 실행의 clients verify는 과거 challenge_read보다 앞선 profile product_directory_security의 SCOPE_DENIED다. 일반 LocalAppData의 mail-hantong에는 data/secrets만 있고 profiles가 없지만, OpenAI.Codex 패키지 LocalCache/Local/mail-hantong에는 profiles/data가 존재한다는 디렉터리 메타데이터를 확인했다. 실제 프로필·키·DB 내용은 직접 열지 않았다.

원인은 SHGetKnownFolderPath flags=0으로 인해 패키지 호스트와 일반 프로세스가 다른 AppData를 선택하지만 IPC는 같은 사용자 SID 주소를 공유한 것이다. 이전 코어 재시작 복구/재발 기록은 실행 경로·해시만 같고 저장소 선택 문맥은 달랐다는 점으로 정정한다. PID 8472의 transport_ready는 기존 두 계정 ready 증거가 아니다. 현재 코어와 실제 저장소/ACL은 이번 조사에서 변경하지 않았다.

로컬 수정: 플랫폼에 KF_FLAG_NO_PACKAGE_REDIRECTION 적용 및 패키지/일반 resolver 회귀 시험. Core는 Store 초기화 전에 기존 native profile-publication 로직으로 제품/profiles 디렉터리를 보호 ACL로 생성하고 수명 동안 handle을 유지한다. 기존 unsafe ACL은 수정하지 않고 거부한다. 신규 설치의 부모 디렉터리가 Secrets 초기화에서 먼저 일반 상속 ACL로 생성되는 문제를 막는다. 기존 패키지 데이터의 자동 이동/합치기/삭제는 하지 않는다. ADR-026 제안에 사람 검토와 명시적 복구 경계를 기록했다.

사용자에게 개발자 설정을 입력시키지 않도록 선택적 배포 자산 publisher/google-desktop.json을 Secrets에서만 해석하는 경로와 manifest/해시/크기/중복키/고정 Google endpoint 검증을 추가했다. implicit 첫 연결은 게시자 자산을 사용하며 기존 보호 계정 자격증명 재사용과 explicit 고급 capture를 보존한다. package_cli는 소스 트리 밖의 불투명 자산만 선택적으로 포함하고 실제 포함 여부만 metadata에 기록한다. 값/원본 경로는 metadata에 넣지 않는다. 실제 자격증명은 제공·추출·내장·게시하지 않았다. ADR-018의 "No publisher secret is shipped"는 아직 유효하므로 실제 자산 배포는 별도 명시적 결정이 필요하다. 새 계약은 그 결정을 대신하지 않는다.

검증 진행: 기존 설치 시험은 PowerShell 7 전용 호출을 Windows PowerShell 5.1 fallback으로 보완하여 12개 전부 실제 통과했다. 게시자 packager 9개 및 bootstrap 5개 통과. 신규 보호 디렉터리 시험 2개, 경로 플랫폼 시험, publisher parser 시험을 실행했다. 통합 전체 검사와 실제 release 정적 CRT 패키지 생성은 이어서 수행하며, 최종 결과는 아래 후속 항목에 기록한다. **[사람 검토 필요]**. 기존 실제 계정·비밀값 작업 중단 상태와 모든 미커밋 변경을 보존한다.

## 2026-11-02 설치 흐름 최종 로컬 검증 및 실제 적용 경계

최종 source_hash ebb97e1562d5bbb17c4d017dc0acc0d6c154d16841bc51ae0df194d8ab032cf6에서 check_all exit 0. Rust 1,611개/30 suites/ignored 0, Docs 29개, Build 93개 실제 통과, skip 0. 총 **1,733개 통과**, 실패 없음. fmt, 전체 workspace Clippy(-D warnings), 전체 build, 요건/아키텍처/추적 검사 통과. Linux x86_64 및 macOS arm64의 Secrets/CLI/Core all-target 교차 Clippy도 exit 0. 실제 해당 OS에서 제품 실행 시험은 아니다.

리뷰 후 Core 기동 실패 시 자신이 방금 생성한 자식만 kill/wait하는 guard와 정상 준비 시 유지 시험을 추가했다. 기존 실행 코어는 guard 대상이 아니다. implicit Google 연결에 matching 게시자/재사용 credential이 없으면 HUMAN_STEP_REQUIRED로 종료하고 개발자용 secret capture를 열지 않는다. explicit 고급 client-ID 설정만 capture를 유지한다. publisher JSON의 멤버 순서, 중복 키, 잘못된 endpoint/web client, manifest 크기 불일치, hash 변조를 거부·검증했고 raw secret 입력은 SensitiveBuffer/SecretJson으로만 해석·정리한다.

실제 정적 CRT release build로 artifacts/cli-onboarding/0.1.0-preview.5 패키지를 생성했다. 초기 build 중 소스 변경은 packager가 감지해 산출물 게시를 거부했고, 안정된 최종 소스로 재실행하여 built_from_current_workspace와 위 source_hash를 확인했다. ZIP 22,812,859 bytes, SHA-256 77ce88adaf3ca1f578967e84f30a8500d904a8aaf0ad4f2f7fe289faba87a2a1. 실제 Google 게시자 자산 포함 여부는 false다. 이 파일은 검토용 개발 패키지이며 새 PC에서 Google 연결까지 완료하는 제품 배포본이 아니다.

해당 실제 ZIP을 별도 임시 폴더에 풀어 Windows PowerShell로 설치하고 모든 설치 파일의 manifest/hash/size를 확인했다. 설치된 CLI version/help exit 0, headless init은 HUMAN_STEP_REQUIRED exit 24로 부작용 전에 거부했다. 설치된 제거 스크립트 exit 0 및 사용자 데이터 sentinel 보존을 확인했다. 이전 manifest 미포함 설치 버전을 모사한 업데이트에서도 이전 실행 파일 보존과 새 버전 활성화·제거를 검증했다. dumpbin /DEPENDENTS에서 release Core의 추가 Visual C++ runtime DLL 의존성이 없음을 확인했다. 이 확인이 깨끗한 Windows VM이나 실제 OAuth 종단 시험을 대신하지 않는다.

최종 전체 로그 target/clean-install-full-check.log SHA-256 e25dd01aa821d66410bec0578339a83a7e9ef45902462042354cabc0f9aae137. 최초 trace 갱신 전 실패와 중간 검증 로그도 별도 이름으로 보존했다. 검사를 위해 관찰한 기존 Core만 원본 hash/인수를 보존하고 중지·복구했다. 최종 PID 47944, 원본 SHA-256 6FE42CF1D958594ABAD72009936C900E3EF65A27791C9420FAB101F6FB72F209. 공식 core status ok=true / transport_ready=true이며, 기존 계정 ready 복구 증거로 표현하지 않는다. 실제 데이터 경로 복구는 아직 실행하지 않았다.

증거: docs/local-verification/2026-11-02/clean-install-verification.json 및 clean-install-package-verification.json. 남은 실제 작업과 승인 근거는 docs/review/CLEAN-INSTALL-DECISIONS-20261102.md에 정리했다. (1) ADR-018의 게시자 secret 배포 금지를 변경하여 소스·로그 제외 데이터 자산으로 실제 설치 패키지에 제공할지, (2) 기존 Codex 패키지 내부 상태와 일반 루트를 각각 보존한 뒤 정상 설치 경로로 복구할지 결정이 필요하다. 현재는 실제 자격증명 제공/추출/포함/게시·실계정 새 로그인·재동의·grant 확대·메일 발송·실데이터 이동·ACL 변경을 하지 않았다. Git 커밋/푸시/공개 배포도 하지 않았다. **[사람 검토 필요]**와 기존 중단 상태를 유지한다. 사용자에게 프로그램 이용까지 완료됐다고 보고하지 않는다.

## 2026-11-02 두 결정 승인 및 기존 계정 저장소 복구

사용자가 “오케이 승인할게. 둘다. 유출되어 문제될 위험은 없지?”라고 명시적으로 승인했다. 승인 범위는 게시자 Google Desktop 앱 설정의 설치 데이터 자산 배포와 기존 두 저장소를 보존하는 명시적 복구다. ADR-018, publisher-google-configuration 계약, CLEAN-INSTALL-DECISIONS 문서에 이를 기록했다. 사용자 토큰·비밀번호·서버용 자격증명의 배포나 실제 보안 코드 검토 완료로 해석하지 않는다. Desktop 앱 자격증명은 추출 가능하며 앱 사칭·한도 남용 위험이 있으므로 위험 0을 보장하지 않는다고 설명했다. 근거: https://developers.google.com/identity/protocols/oauth2/native-app 및 https://datatracker.ietf.org/doc/html/rfc8252#section-8.6.

승인된 복구를 ignored target/recover-account-store.ps1로 수행했다. 암호화 파일은 불투명 바이트로 복사·SHA-256 대조했으며 실제 토큰·키·DB·프로필 내용을 직접 읽지 않았다. 초기 두 시도의 PowerShell Set-Acl audit 권한 오류는 동일한 보호 부모의 합성 재현에서 확인한 Directory.SetAccessControl로 해결했다. 긴 파일명 해시 오류는 extended path 스트리밍 해시로 해결했다. 검증용 자식 lock 핸들이 상위 폴더 rename을 막는 현상은 합성 재현으로 확인했고, 모든 복사·해시 검사 후 자체 핸들만 닫고 제품 프로세스 부재를 재확인해 해결했다. 실패 시 원본 코어를 복구했으며 원본 루트를 덮어쓰지 않았다.

최종 복구 성공: Codex 패키지 LocalCache/Local/mail-hantong 원본을 유지하고 복사본을 canonical LocalAppData/mail-hantong으로 배치했다. 기존 canonical 루트도 별도로 보존했다. 보호된 백업 루트는 C:/Users/cyary/AppData/Local/mail-hantong-recovery-20261102-c0310aa880c7402e84ce0e55c4c1a9b9 이며 private 원본 해시 목록·검증 응답은 그 안에만 보관한다. 새 preview.5를 사용자 Programs/mail-hantong-cli에 설치했고 PATH는 변경하지 않았다. 설치된 Core PID 15560, 공식 core status·clients verify·accounts list 모두 exit 0 / ok=true, 기존 Gmail 두 계정 ready를 확인했다. core status만으로 인증 완료를 판단하지 않았으며 clients verify와 계정 목록을 별도로 검증했다. 재로그인·재동의·grant 확대·메일 읽기/발송/변경은 하지 않았다. 기존 원본·실패 시도별 백업·미커밋 변경은 보존했다. 복구 스크립트 종료 후 별도 실행에서도 동일한 세 검사와 두 계정 ready를 재확인했다. 문서 수정 후 Docs unittest 29개 통과(exit 0). 제품 소스 변경이 없어 전체 Rust 검사는 재실행하지 않았으며 앞선 전체 검증과 구분한다.

Aside 공식 CLI를 통해 Google Cloud의 기존 mail-hantong 테스트 Desktop 클라이언트를 읽기 전용 확인했다. 기존 secret 두 항목의 비활성화·추가 버튼은 있었으나 재다운로드 기능은 확인되지 않았다. Downloads의 client_secret*.json 파일은 0개였다. Google 공식 관리 문서도 전체 secret의 생성 시 1회 조회/다운로드를 명시한다: https://support.google.com/cloud/answer/15549257?hl=en. 설정을 변경하거나 기존 계정 보호 번들에서 값을 추출하지 않았다. 원래 저장한 Desktop JSON의 로컬 경로를 사용자에게 요청했으며 값 자체는 채팅에 요청하지 않았다. 현재 preview.5는 여전히 publisher asset 미포함이므로 깨끗한 PC의 Google 연결과 공개 배포 완료를 주장하지 않는다. 다음 작업은 원본 Desktop 자산을 보호된 경로에서 확인한 뒤 승인된 패키징·검증을 수행하는 것이다. **[사람 검토 필요]** 유지.

## 2026-11-02 발견한 과거 OAuth JSON의 적용 제외

사용자가 D:/Downloads에 5월 생성 OAuth JSON이 있으나 현재 한통 클라이언트와 ID가 다르며, 현재 인증정보는 보호 입력으로만 저장했다고 밝혔다. 이 최신 사용자 증거에 따라 과거 JSON은 배포 후보에서 제외했다. 에이전트는 해당 파일을 열거나 복사·검사·패키징하지 않았다. 앞선 Downloads 검색은 C:/Users/cyary/Downloads만 대상으로 했으므로 모든 다운로드 위치에서 파일이 없다는 의미가 아니었다. 현재 한통의 원본 Desktop JSON 확보 여부는 여전히 미확인이다.

기존 보호 번들에서 게시자 설정을 추출하지 않고 기존 Google 클라이언트·secret·계정 연결을 유지한다. 대안은 기존 클라이언트와 분리된 배포 전용 Desktop OAuth 클라이언트 생성이지만, 직전 확인 질문에 대한 생성 승인은 아직 받지 않았다. 기존 설정 배포와 저장소 복구의 승인은 유지되며 이를 신규 외부 등록 승인으로 확대 해석하지 않는다. 이번에는 진행 기록만 추가했다. 제품 코드·계약 변경이 없어 빌드·전체 검사를 재실행하지 않았고 기능 완료를 새로 주장하지 않는다. **[사람 검토 필요]** 유지.

## 2026-11-02 배포 전용 Desktop 클라이언트 생성 및 설정 포함 preview.6 설치

사용자가 신규 Desktop OAuth 클라이언트 생성 확인 질문에 “허용할게”라고 명시했다. Aside 공식 CLI의 실제 로그인된 Google Cloud 콘솔에서 프로젝트 mail-hantong-test-20261030 아래 배포 전용 Desktop 클라이언트를 생성했다. 기존 mail-hantong 테스트 Desktop 및 기존 두 계정 연결·자격증명·grant는 변경하지 않았다. 5월의 다른 클라이언트 JSON은 열거나 사용하지 않았다. Audience·검증 심사·API 범위 설정을 변경하지 않았다.

생성 과정의 보안 사고와 조치: 첫 생성 완료 화면에서 복사 버튼의 aria-label 목록을 출력했는데 Google이 해당 라벨에 secret 전체를 넣어 둬 새 client secret이 도구 출력에 노출됐다. 에이전트의 확인 방식 실수이며 사용자에게 즉시 알렸다. 이 첫 클라이언트는 실제 계정 연결·토큰 교환·배포에 사용하지 않고 삭제했다. DELETE 확인을 거친 뒤 실제 클라이언트 목록에서 사라진 것을 확인했다. 해당 값은 문서·소스·테스트·커밋에 복사하지 않았으며 과거 도구 출력 자체를 지웠다고 주장하지 않는다. 삭제된 첫 클라이언트는 복원하지 않는다.

대체 클라이언트 mail-hantong Windows CLI distribution 20261102 v2를 Desktop app 유형으로 별도 생성했다. 이번에는 생성 완료 화면의 snapshot/aria-label/입력값을 출력하지 않고 Download JSON 버튼의 브라우저 다운로드를 직접 받았다. 415바이트 원본을 보호 DACL의 Aside artifacts 폴더에 저장하고 C:/Users/cyary/AppData/Local/mail-hantong-publisher-20261102/google-desktop.json으로 불투명 복사·보존했다. 실제 JSON 내용을 에이전트가 파싱하거나 출력하지 않았고 기존 계정 보호 번들에서 값을 추출하지 않았다. 기존 클라이언트와 새 v2만 활성 목록에 있는 것을 확인했다. Desktop 앱 설정은 소유자가 이미 승인한 배포 자산이며, 사용자 토큰·비밀번호는 포함하지 않는다.

제품 변경은 apps/mail-hantong-core/src/main.rs의 시작 단계다. 기존 Secrets loader로 선택적 설치 publisher asset을 검사하고 즉시 drop한 뒤 Store를 연다. 설정 없음은 기존 동작이며, 존재하지만 잘못된 설정은 기존 안정 오류로 중단한다. 실제 값은 Secrets 밖에서 파싱하지 않는다. 합성 lifecycle 시험·fmt·Core Clippy 통과. 에이전트의 중간 Core 단독 전체 시험은 실행 중 실제 코어와 mock pipe가 충돌해 97 통과/10 실패였고, 시험을 삭제하거나 건너뛰지 않았다. 이후 관찰한 preview.5 코어만 hash/인수를 보존해 잠시 중지한 최종 전체 검사에서 전부 통과했다. 검사 후 동일 설치 코어를 복구했다.

최종 python -X utf8 scripts/check_all.py exit 0: Rust 1,611개/30 suites/ignored 0, Docs 29개, Build 93개/skip 0, 실제 합계 1,733개 통과. workspace fmt·Clippy(-D warnings)·전체 build·요건·아키텍처·추적 검사도 통과. 로그 target/publisher-release-full-check.log SHA-256 ed72ff67334a6317cda340c5149a39b16f687f221e87a9d9e0f453cff76d056d. source_hash 3096e5378147f4693162ae7e7f79cce225693f6f4853d58581bd422a245db438. 추적표를 갱신했고 설치 후 --check도 exit 0이다.

실제 설정을 --publisher-config로 제공해 소스·로그·테스트에 값을 넣지 않고 preview.6을 새 빌드했다. artifacts/cli-onboarding/0.1.0-preview.6/mail-hantong-cli-0.1.0-preview.6-windows-x86_64.zip SHA-256 1549f8b22a0596aac33183591fd79e0d57bfb3d8f078a373e4efd72487c51ae0. BUILD-METADATA는 built_from_current_workspace, google_publisher_config_included=true다. artifacts는 git ignored이며 실제 설정 자산은 소스 커밋에 포함하지 않는다. 별도 임시 설치에서 manifest/hash/size·CLI help/version·headless init HUMAN_STEP_REQUIRED exit 24·제거·사용자 데이터 sentinel 보존을 확인했다. 설정 파일에 불투명 공백을 추가한 변조 패키지는 실제 Core가 Store를 열기 전에 CORE_UNAVAILABLE exit 25로 거부했고 credential 값은 출력하지 않았다.

사용자 Programs/mail-hantong-cli의 실제 설치를 preview.6으로 갱신하고 소유 launcher를 사용자 PATH에 추가했다. preview.5와 원래 데이터·백업을 보존했다. 실제 새 Core 시작은 Secrets의 실제 Desktop 형식·manifest 크기·SHA-256·고정 endpoint 검사를 통과했다. PID 55532, 설치 스크립트 종료 후 별도 실행에서도 core status / clients verify / accounts list 모두 exit 0, 기존 Gmail 두 계정 ready다. 새 Google 로그인·재동의·메일 읽기/변경/발송은 실행하지 않았다. 검증 자료는 docs/local-verification/2026-11-02/publisher-package-verification.json, publisher-tamper-verification.json, publisher-release-verification.json이다.

사용자 실행: 새 터미널에서 hantong --lang ko init. 현재 PC의 기존 계정은 유지되며 새 계정 연결에는 사용자가 Google 로그인/동의와 Windows 보호 승인을 직접 수행해야 한다. 실제 새 클라이언트의 OAuth 종단 시험, 깨끗한 Windows 설치 시험, Google 검증 심사와 보안 코드 사람 검토는 남아 있다. Git 커밋/푸시/GitHub release 게시는 이번 작업에서 하지 않았으며 기존 공개 preview.4와 로컬 preview.6을 구분한다. **[사람 검토 필요]** 유지.

## 2026-11-02 실제 연결 시험·클린 재설치 및 추가 보호창의 잔존 확인

사용자가 실제 연결 시험을 요청하고 이어 “기존 버전을 아에 클린 삭제 후 다시 시도”라고 지시했다. 실제 비밀번호·PIN 입력과 로컬 승인 발급은 대신하지 않고, 먼저 기존 설치와 활성 데이터 루트를 보호 백업으로 보존한 후 공식 제거·재설치하도록 진행했다. 미커밋 소스·새 파일·이전 백업과 과거 클라이언트는 보존한다. 이를 깨끗한 Windows VM 시험이나 Windows 사용자 CNG 키 전체 삭제로 표현하지 않는다.

실제 시험 준비에서 새 연결의 client ID 선택이 legacy 저장값을 publisher 자산보다 먼저 사용함을 발견했다. setup_google.rs를 새 연결의 명시 ID → 설치 publisher ID → legacy ID 순서로 수정했다. 명시적 기존 계정 재인증은 원래 bound client ID를 유지하고 다른 ID는 Conflict로 거부한다. 회귀 시험 4개와 최종 전체 검사 1,737개(Rust 1,615/30 suites, Docs 29, Build 93, ignored/skip 0)가 통과했다. 해당 로그 target/clean-reconnect-full-check.log SHA-256 4aa757571b17eaef30ee0e6a5964de9a9e1df617f75bb91c03cbbb0322c4773e, source_hash 675be31c5dbf6ab4663ab8d191cf14403f312fa7ea9907f95bfe7cd3a6f0922d. 설정 포함 preview.7을 빌드했다.

클린 제거는 ignored target/clean-reinstall-publisher.ps1 및 opaque-state-snapshot.py로 수행했다. 기존 Core의 정확한 경로·인수·hash를 확인해 중지하고 사용자 데이터 전체와 프로그램 전체를 보호된 고유 백업으로 보존했다. 프로그램 복사본의 snapshot을 검증한 뒤 공식 uninstall을 적용했다. 실패 시 프로그램·데이터·소유 PATH 항목·원래 Core 인수를 되돌리는 절차를 별도 읽기 검토했다. 첫 시도는 백업 뒤 긴 Windows 파일명 검사에서 실패했고 원상 복구됐다. extended path 처리 후 재시도에서 데이터 28개와 프로그램 530개 파일의 보존 전후 hash 목록이 일치했다. 성공한 백업은 C:/Users/cyary/AppData/Local/mail-hantong-clean-test-20261102-93e812bb31f743579929da0b8b277d52 이다. 기존 소유 프로그램을 제거하고 preview.7을 설치했으며 설치 직후 canonical 사용자 데이터 루트가 없음을 확인했다. 실패 시도도 삭제하지 않았다.

실제 첫 init은 INVALID_INPUT으로 실패했다. 원인은 core_start.rs가 실제 manifest 파일을 32KiB 제한의 IPC용 sha256 함수로 검사하는 것이었다. 실제 Core는 약 16.7MB이고 THIRD-PARTY.json도 32KiB를 넘는다. 기존 Core가 실행 중인 경우에는 해당 검증 경로를 지나지 않아 기존 검사에서 드러나지 않았다. IPC 제한을 높이는 대신 Windows CNG 기반 sha256_release_file을 별도로 추가했다. 64KiB 청크·512MiB 상한·처음/읽은/마지막 파일 크기 일치를 검사하며 전체 파일을 메모리에 올리지 않는다. CNG 해시/객체 버퍼/알고리즘의 RAII 해제 순서 문제는 부모 검토에서 발견해 hash → object → algorithm 순서로 수정했다. 독립 코드 검토에서 추가 CRITICAL/HIGH 차단은 없었으며 기존 검증-실행 사이 파일 교체 가능성은 별도 주의점으로 남았다. IPC 32KiB·저장 payload 제한과 기존 계정 정책은 유지한다. 합성 큰 파일 회귀·CLI manifest/변조 시험·fmt·Clippy는 통과했고 최종 통합 검사와 실제 cold start 재검증을 이어간다. preview.8은 해제 순서 수정 이전 산출물이므로 설치·게시하지 않는다. 수정 이후 preview.9를 새 빌드했다.

Google 통신 시험은 설치된 Core7을 먼저 정상 시작하고 새 init의 계정 없음 → 연결 선택으로 진행했다. 이 우회 준비를 자동 Core 시작 성공으로 주장하지 않는다. 활성 저장소에는 기존 계정/legacy 앱 설정이 없고 설치된 새 v2 publisher 설정만 있었다. 기본 Chrome 제어 연결은 unavailable이어서 같은 공개 pending 요청을 Aside 공식 CLI에서 이어갔다. OAuth URL은 보호된 Aside 세션 파일에서만 취급하고 client secret·계정 token·callback code를 출력하지 않았다. 초기 wizard와 이어보기 CLI가 동시에 진행하지 않도록 해당 wizard 프로세스만 중지했다. 사용자는 “대신 동의 진행”이라고 명시했다. Aside의 기본 로그인 계정을 선택하고, 요청 범위가 정확히 openid/email/gmail.readonly임을 확인한 뒤 Google Continue를 실행했다. 새 v2 client ID가 Google 페이지에 사용되는 것을 확인했다. Core의 고정 단계 로그에서 callback_wait 성공, token_exchange HTTP 200, token_response/refresh_credential/identity 성공을 확인했다. 메일 본문 조회·발송·변경은 하지 않았다.

Google 이후 한통의 계정 확인/Windows 보호 승인창이 별도로 열렸다. 사용자는 “한통보호창이 왜 열려 그런 거 없앤 거 아냐?”라고 지적했다. 없앤 것은 수동 앱 비밀값 입력창이며 계정·로컬 권한 승인창은 실제로 남아 있다고 설명했다. Google 동의만으로 연결이 끝나는 흐름처럼 안내한 부분은 부정확했다고 정정했다. Google 동의를 대신하라는 지시를 한통 로컬 승인/Windows Hello 대행으로 확대하지 않았고, 추가창을 자동 승인하거나 정책을 우회하지 않았다. 사용자가 로컬 승인을 진행하지 않은 상태로 요청은 APPROVAL_EXPIRED가 됐다. 따라서 현재 결과는 새 배포용 Google OAuth 통신 성공이며, 계정 저장·CLI 등록·로컬 읽기 grant·프로그램 이용까지 완료된 종단 성공이 아니다. 창 제거 또는 승인 통합은 구현 완료되지 않았다. **[사람 검토 필요]** 유지.

## 2026-11-02 cold start 수정본의 최종 검사와 preview.9 클린 설치 확인

최종 source_hash c63cf270e892a9d314e1c26ce05560d89cd09940b28627b8a9560e53077691d4에서 python -X utf8 scripts/check_all.py exit 0. Rust 1,616개/30 suites/ignored 0, Docs 29개, Build 93개/skip 0, 합계 1,738개 통과. workspace fmt·Clippy(-D warnings)·전체 build·요건·아키텍처·추적 검사도 통과했다. 로그 target/cold-start-fixed-full-check.log SHA-256 437a3a660b2497975bb6abf1de85c382eccfa4d4876c1afaeac82fb7e8f30467. 검사를 위해 관찰한 Core7만 hash·인수를 보존해 중지했고 동일 경로로 복구했다. 미승인 실제 OAuth 요청은 검사 전에 이미 만료됐으며 진행 중 동의를 강제 중단하지 않았다.

수정 후 새 빌드 preview.9 ZIP SHA-256 d824928f2a1a14ed15f3a564baeafdc73d88cd1200dcfed9614e73f4e1c1c517. 경로 artifacts/cli-onboarding/0.1.0-preview.9/mail-hantong-cli-0.1.0-preview.9-windows-x86_64.zip, BUILD-METADATA source_relation=built_from_current_workspace, google_publisher_config_included=true. preview.8은 설치·게시하지 않았다. 기존 preview.7의 프로그램 전체와 초기화 데이터도 별도 보호 백업으로 보존한 뒤 공식 제거하고 preview.9를 클린 설치했다. 이때 데이터 4개·프로그램 270개 파일의 보존 전후 snapshot hash가 일치했다. 백업은 C:/Users/cyary/AppData/Local/mail-hantong-clean-test-20261102-9a921c8ac78144cfbfa3fa669b7bb590. 초기 버전 문자열 치환으로 checksum 선택 정규식이 맞지 않았던 시도는 어떤 프로세스·데이터도 건드리기 전에 실패했고 수정 후 재실행했다.

실제 cold start를 위해 Core 프로세스가 0개이고 canonical 사용자 데이터 루트가 없는 것을 선행 확인했다. 설치된 preview.9의 hantong --lang ko init만 실행했고, 수동 Core 시작 없이 자동 기동한 뒤 “연결된 Gmail 계정이 없습니다 / Google 계정 연결·종료” 메뉴에 도달했다. 종료 2를 선택한 CLI는 exit 0이었다. 이후 공식 core status exit 0 / transport_ready=true, 실제 실행 경로는 preview.9, PID 7940이다. default 클라이언트 profile은 없으며 이전 preview.7 실행 파일도 활성 설치 경로에 없다. 최초 빈 profile 조회의 고정 진단 두 줄은 아직 첫 메뉴 전에 표시되며 이는 남은 안내 품질 문제다. 이 검사에서 Google 로그인이나 새 권한 승인을 다시 시작하지 않았다.

최신 결과는 설치·자동 Core 시작 정상, 새 배포 클라이언트의 실제 Google OAuth token 교환/계정 확인 성공, 한통 추가 보호창 미승인으로 계정 등록·이용 종단은 미완료다. 기존 계정과 키는 이전 보호 백업에 있고 현재 활성 설치는 새 상태로 남겼다. 추가 계정 확인/로컬 권한 보호창은 구버전 잔재가 아니라 새 버전의 현재 구현이며, 이를 없앴다고 주장하지 않는다. 변경 파일은 apps/mail-hantong-core/src/setup_google.rs, apps/hantong-cli/src/core_start.rs, crates/hantong-platform/src/windows_identity.rs 및 lib.rs, 추적표와 검증/진행 문서다. 실제 계정 본문 조회·발송·변경, 비밀값의 소스/로그 출력, Git 커밋/푸시/GitHub release 게시는 하지 않았다. **[사람 검토 필요]**와 추가 보호창 UX 미해결을 유지한다.

## 2026-11-02 Google 동의·개인 CLI 기능 선택 전환 — 통합 구현 중

사용자가 “실제 승인은 구글 동의만 하고 한통 승인이 아닌 기능 선택”이라고 결정했다. 일반 개인 CLI의 추가 한통 계정 확인/Hello를 제거하고 읽기 또는 읽기·작성·발송 기능을 Google 전에 선택해 최소 OAuth 범위를 요청하도록 구현한다. 실제 발송·대량 변경의 사람 승인, MCP·외부 AI의 별도 활성화 및 고급 네이티브 설정은 유지한다. 사용자 UX 결정은 보안 코드의 사람 검토 증거가 아니다. 기준은 docs/contracts/google-personal-cli.md, docs/adr/proposed/ADR-027-google-consent-personal-cli.md, docs/latest/GOOGLE-FEATURE-SELECTION-PLAN.md에 기록했다.

기능 선택 enum/엄격한 wire 검증, 실제 OS pipe peer context 전달, CLI 선택/취소/기존 접근 확인, Google OAuth 전용 등록 출처와 scope hash v8/DB v22를 구현 중이다. 기존 미커밋·새 파일과 보호 백업은 보존한다. 구현 통합 검토에서 새 provenance를 기존 parser가 거부하는 문제, native hash v7 유지, v8 고정 L→B/C 제한, 기존 FK 참조/DDL 제약을 보존하는 migration 문제를 확인해 수정하고 있다. 기존 테스트를 삭제하거나 skip하지 않는다.

중간 검증: Docs 29개 통과. IPC/contracts 검사에서 새 enum·wire 테스트는 통과했으나 OS 고정 pipe 시험은 실행 중 preview.9 Core와 충돌했다. Store 최초 Windows 통합 검사 442 통과/25 실패였으며 migration 및 source/hash 회귀를 수정 중이다. 수정 후 새 Google Store 등록/출처/고정 scope/변조 거부 테스트 2개는 통과했다. 이 중간 결과를 최종 통합 통과로 표현하지 않는다. 다음은 보완된 migration·Core authority/profile 통합 검사, 전체 check_all 및 새 설치본 검증이다. 현재 설치된 preview.9와 실제 계정 상태는 아직 변경하지 않았다. **[사람 검토 필요]**.

## 2026-11-02 개인 CLI 기능 선택·Google 단일 동의 구현 및 preview.10 실제 연결 확인

일반 개인 CLI는 이제 `hantong --lang ko init` → Google 연결 → 읽기 또는 읽기·작성·보내기 선택 → Google 동의 → 작업 메뉴로 이어진다. 개인 경로에서는 추가 한통 계정 확인·Windows Hello를 호출하지 않는다. 기능 선택을 Google 전에 받아 필요한 제공자 범위만 요청한다. 선택 취소/EOF에는 OAuth 효과가 없고, 준비 완료만으로 받은편지함을 조회하거나 발송하지 않는다. 실제 발송·대량 변경의 네이티브 사람 승인, 고급 네이티브 등록, MCP·외부 AI 별도 활성화는 유지한다. 전체 화면 TUI 구현으로 표현하지 않는다.

주요 변경: apps/hantong-cli/src/onboarding.rs·google.rs와 한국어/영어 리소스, crates/hantong-contracts/src/google_setup.rs, IPC client의 setup/정확한 개인 기능 검증과 IPC server의 strict wire·SetupRequestContext·OS pipe context 전달, apps/mail-hantong-core/src/main.rs·setup_google.rs·registration.rs·scope_refresh.rs, Store 등록 reader/stage·scope lifecycle·v22 migration/시험을 연결했다. GoogleBootstrapAuthority와 외부에서 생성할 수 없는 move-only permit은 request·관찰된 user/logon peer·계정·subject·client·credential version·mode/profile·expiry에 결속하고 소비 시 현재 Store binding과 OS peer를 재검증한다. Store는 google_oauth 출처의 단일 Google 계정 L 등록 후 고정 B/C로만 전환하며, v8 hash에 출처를 포함한다. 다른/혼합 기존 범위를 자동 대체·확대하지 않고 Conflict로 거부한다. 중단된 source-bound pending/staged scope는 기록에 따라 재개하며, 계정 존재만으로 새 권한을 만들지 않는다. L 직후 중단된 경우 동일 계정의 새 OAuth 권한으로만 이어간다.

DB v21→v22는 48개 테이블·16개 인덱스를 명시적으로 재생성하고 이전 행·FK·scope hash v1~v7·audit sequence 부재/최댓값을 보존한다. 중간 구현의 registration FK rename, v8 도구 길이 제약, 네이티브 hash v7/source, 현재 등록 범위와 v8 단일 전이의 검증 혼동을 수정했다. 이전 버전 재현 시험은 새 v22 등록 DDL을 섞지 않도록 정확한 v8 DDL로 되돌리고, quota fixture는 명시 컬럼 INSERT로 갱신했다. 테스트 삭제/skip은 하지 않았다. 새 Store 테스트는 L→B/C stage·activate·재열기·source 변조 거부를, Core 테스트는 exact profile 직렬화·publish 전 실패 시 기존 범위 보존·재개를 검증한다. 비어 있는 permit을 다른 모듈에서 생성할 수 있던 초기 구현과 pending 복구 누락은 독립 검토 지적 후 수정했다. 재검토에서는 추가 CRITICAL/HIGH 권한 우회를 발견하지 못했으나 사람 보안 검토를 대체하지 않는다.

최종 source_hash 94e13d9e17ae08fe6c8af27d8d5891c5a21eceb71bf48383317ef345c69ac0a3에서 `python -X utf8 scripts/check_all.py` exit 0. Rust 1,633개/30 suites/ignored 0, Docs 29개, Build 93개/skip 0, 합계 **1,755개 통과**. fmt·workspace Clippy(-D warnings)·전체 build·요건·아키텍처·추적 검사도 통과했다. 로그 target/google-feature-full-check.log SHA-256 404fef1082de00401339be6f0b6f8130bd054012fedfa9eba61878028271f945. 첫 전체 시도의 기존 scope 회귀 12개는 보존한 target/google-feature-full-check-attempt1.log와 구분한다. 검사는 관찰한 preview.9 Core의 경로·hash·인수를 보존해 잠시 중지한 뒤 동일 바이너리로 복구했다.

같은 소스의 publisher 설정 포함 preview.10을 실제 release/static CRT로 빌드했다. ZIP artifacts/cli-onboarding/0.1.0-preview.10/mail-hantong-cli-0.1.0-preview.10-windows-x86_64.zip SHA-256 d650112d7916c963a0f85007d29031ed319f57064641a1de4c2ffcb7af145a3f. BUILD-METADATA의 built_from_current_workspace/source_hash/설정 포함을 확인했다. 기존 활성 데이터 4개를 보호된 C:/Users/cyary/AppData/Local/mail-hantong-feature-test-20261102-2f38f7f60961419482a3e7b734afb4db에 불투명 복사하고 전후 snapshot SHA-256 f0dc6f4e996793e09ef26e74646a3446730b6801a5d3610451f92924fcbe4f94 일치를 확인했다. 공식 installer로 preview.10을 활성화했으며 preview.9와 이전 계정 백업은 보존했다. 설치 뒤 Core 0개에서 init만으로 자동 기동하고 기능 메뉴·취소·exit 0을 확인했다. 실제 v21→v22 backup 파일도 생성됐다. 새 Windows VM/사용자에서의 클린 시험은 아니다.

사용자가 앞서 허용한 실제 연결 시험과 Google 동의 대행 범위에서 **읽기 전용**으로 시험했다. Aside MCP 연결이 이번 실행에서 사용 가능해 공식 browser_profiles/repl을 사용했다. Aside로 URL을 옮기기 위해 개인 읽기 공개 setup 요청을 먼저 시작하고 init이 동일 OS peer의 같은 요청을 이어받게 했다. 요청 범위 openid/email/gmail.readonly를 정확히 확인한 뒤 이전 시험 계정을 선택해 Google Continue를 진행했다. OAuth URL은 보호된 파일에서만 전달했고 token·client secret·callback code를 읽거나 출력하지 않았다. 추가 한통 계정 승인/Hello 조작 없이 CLI가 “계정 연결이 완료되어 선택한 기능을 이용할 수 있습니다”와 작업 메뉴에 도달했다. 종료 3을 선택했으며 받은편지함·본문 조회, 초안 작성, 발송·변경은 실행하지 않았다. 개인 setup status ready/error 없음, 공식 core status·clients verify·accounts list 모두 exit 0/ok=true 및 Gmail 계정 1개 ready를 확인했다.

이어서 관찰한 Core10만 hash를 확인해 중지하고 init의 자동 시작으로 재기동했다. 동일 계정 선택 → 재동의/추가 승인 없이 작업 메뉴 → 종료까지 성공했다. 재시작 후에도 공식 transport/authentication/accounts 검사 전부 exit 0/ok=true, Gmail 1개 ready다. 최종 Core 경로는 Programs/mail-hantong-cli/versions/0.1.0-preview.10/bin/mail-hantong-core.exe, 관찰 PID 28136. 실제 활성 계정과 비밀은 제품 보호 저장소에만 유지하며 원문을 별도로 읽지 않았다. 근거: docs/local-verification/2026-11-02/google-feature-selection-verification.json; 상세 private 응답은 보호 백업 디렉터리에만 있다.

상태: 기능 구현·통합 검사·이 PC의 실제 읽기 전용 연결 및 재시작 확인 완료, **[사람 검토 필요]**. 실제 ReadSend 제공자 동의/작성/발송 시험과 Google 검증 심사, 정식 공개 배포는 이번 결과에 포함하지 않는다. Git 커밋/푸시/GitHub release 게시는 하지 않았다. 최초 빈 프로필의 고정 진단 두 줄은 아직 메뉴 전에 표시되는 안내 품질 제약이다. 기존 미커밋 변경·새 파일·보호 백업과 실제 발송 승인 경계는 유지했다. 다음 검토 대상은 보안 코드 사람 검토와 별도 승인 범위의 쓰기/발송 제공자 시험이다.

## 2026-11-02 실제 조회·발송 및 조건부 베타 — 통합 검증 중

사용자가 실제 메일 조회·발송을 검토하고 정상 작동 시 베타를 배포하도록 지시했다. 기존 읽기 계정에서 공식 인증 CLI로 폴더 90개, 제한된 받은편지함 동기화·목록을 확인했다. 첫 96,724바이트 메시지는 본문 처리에서 QUOTA_EXCEEDED로 실패했다. 이후 55,250바이트 메시지는 본문 1,603바이트를 정상 반환했다. 원문·주소·토큰은 검사 출력에 남기지 않았으며 실제 제공자 확인은 자동 테스트와 분리했다.

원인은 디코딩 전 텍스트 파트의 64KiB 제한으로 정상적인 큰 메일까지 전체 거부하는 구현이었다. 제공자 응답 2MiB·MIME 원문 1MiB·깊이/파트 제한을 유지하면서 안전한 HTML/문자셋 렌더링 후 표시 텍스트를 UTF-8 경계에서 64KiB로 제한하고 truncated를 반환하도록 수정했다. Secrets 대상 sync02 시험 13개 통과. 큰 일반 텍스트·한글·HTML·multipart의 표시 예산 및 활성 콘텐츠 제거를 검증했다. 수정된 설치본의 실제 큰 메일 재검증은 아직 남아 있다.

기존 개인 읽기 B에서 작성·발송 C를 활성화하는 메뉴 4를 추가하고 3=종료는 유지했다. 동일 계정의 새 Google ReadSend 동의와 기존 OS peer/credential 결속 permit만으로 B epoch2→C epoch3를 허용하며 추가 계정/기능 Hello는 호출하지 않는다. Store v23·전용 hash v9를 도입하고 v1~v8 의미를 고정했다. 통합 검토에서 남아 있던 L 전용 Core gate와 개인 재동의 금지를 수정했다. 실제 발송은 기존 메시지 결속 네이티브 사람 승인 경계를 유지한다. 구현 근거는 BETA-MAIL-VERIFICATION-PLAN.md 및 google-personal-cli/google-mail 계약에 기록했다.

CLI 대상 85개, Store 대상 472개 통과 보고를 받았으나 최종 통합 통과로 간주하지 않는다. 실행 중인 Core와 겹친 Core 단독 전체 검사 실패는 별도이며, 관찰된 런타임을 보존·일시 중지한 전체 검사로 다시 판정한다. v22 활성 v8 이력의 v23 마이그레이션/롤백, 기능 승격, 새 설치본의 실제 조회·발송·수신을 검증한 후에만 베타 공개 조건을 충족한 것으로 기록한다. 현재 preview.10 실제 계정·프로그램과 기존 변경/백업은 보존했다. **[사람 검토 필요]**. 베타는 아직 게시하지 않았다.

첫 통합 검사는 Docs 29개·Build 94개를 통과한 뒤 새 소스에 맞춰 추적표가 갱신되지 않아 Rust 검사 전에 중단됐다. 기존 Core10은 동일 바이너리로 복구했으며 이 결과를 전체 성공으로 표현하지 않는다.

추적표 갱신 후 전체 검사에서 fmt·Clippy·build·Store 474개는 통과했고 Core 119개 중 새 복구 시험 하나가 실패했다. publish 실패 후 등록은 기존 B 범위·키를 보존하지만 epoch3/Provisioning으로 접근을 차단하는 계약인데 테스트가 기존 Active record 전체 일치를 기대한 것이 원인이었다. 제품 코드는 바꾸지 않고 시험을 실제 fail-closed 계약에 맞게 수정했다. 실패 로그와 원래 런타임 복구 결과는 target/beta-mail-full-check.log에 보존한다. 공개 소스는 별도 release/beta-20261102 worktree에 준비했으며 실제 자격증명 형태 검사 0건, 비공개 산출물 40개 제외를 확인했다. 원래 index·미커밋 파일은 유지한다. Git 저장 시 LF로 바뀌는 42개 파일은 별도 배포 복사본에서만 index bytes와 일치시켰고, 그 최종 소스로 전체 검증·패키징한다.

브라우저 확인 실수: Aside의 탭 목록 전체를 출력하면서 이전 OAuth 콜백 주소가 도구 출력에 포함됐다. 사용자에게 알렸으며 값을 문서·소스·파일에 복사하거나 재사용하지 않는다. 이후 탭 목록은 OAuth 주소를 가리고 확인한다. 과거 도구 출력이 삭제됐다고 주장하지 않는다.

별도 배포 worktree의 소스 해시 2fbb6982fe26cced7a1cc62126eead9259f004fe3667a345c40193467d801ea4에서 전체 검사 exit 0: Rust 1,643개/30 suites/ignored 0, Docs 29개·Build 94개/skip 0, 합계 1,766개 통과. fmt·Clippy·build·요건·아키텍처·추적 검사 통과. 로그는 별도 worktree의 target/beta-release-mail-core-check.log, SHA-256 3e8aae75c8638033723f5a32a108ff7bb67fb9fd497017dfa206c3252e15f302. 기존 Core10 동일 바이너리 복구를 확인했다. 이 결과는 이후 추가한 일반 작성 메뉴까지 포함한 최종 판정은 아니다.

사용자 이용 흐름 재검토에서 읽기는 메뉴에서 메일 번호로 가능하지만 작성·발송에는 아직 내부 ID/해시 명령이 필요함을 확인했다. 기존 3=종료·4=발송 기능 활성화에 5=메일 작성을 추가 중이다. 정확한 개인 C 권한을 먼저 확인하고 기존 immutable draft/네이티브 발송 요청 API만 사용한다. 취소·EOF·권한 거부·발송 요청 1회·결과 대기/unknown 무재시도 시험을 추가하고 최종 통합 검사한다. 실제 계정과 설치된 preview.10 상태는 아직 변경하지 않았다.

일반 작성 메뉴 구현: guided_compose.rs와 compose EN/KO 리소스, onboarding 메뉴5/Effects, 기존 attachment snapshot loader를 연결했다. 본문 입력은 기존 메뉴의 256바이트 trim 입력과 분리해 전체 compose 예산 안에서 긴 문단·앞뒤 공백을 보존한다. 빈 첨부 경로와 빈 MIME 기본값을 수정했다. 초안 완성 뒤 발송 확인창 열기 선택에서만 요청을 한 번 만들고 최대 300회 상태만 조회한다. unknown·대기 종료 시 재요청하지 않는다. fake Effects의 권한 거부·취소·본문 중 EOF·초안 뒤 취소·pending→sent·unknown/timeout 무재시도, 긴 한글/공백/UTF-8 입력 경계를 검증한다. 부모의 보완 전 CLI 90개, 공백 입력 보완 후 91개 대상 시험 통과는 최종 추가 시험까지의 전체 검사와 구분한다. 독립 정적 검토에서 추가 승인 발급 경로·자동 재발송·원문 출력 또는 정상 입력 차단은 발견하지 못했으며 사람 보안 검토를 대체하지 않는다.

최종 일반 작성 메뉴 포함 통합 검사 exit 0: 배포 worktree source_hash 79601a1731b8da04c0527bb29647d51d8fcbfbf6a0e1736423ab1f779b24ea93, Rust 1,651개/30 suites/ignored 0(그중 CLI 93개), Docs 29개·Build 94개/skip 0, 합계 **1,774개 통과**. fmt·Clippy(-D warnings)·전체 build·요건·아키텍처·추적 검사 통과. target/beta-release-full-check.log SHA-256 9d8c73dfc563aa492b9436b70420fc194b5bbe3ac4920f1f10a6a1705dfdb297(별도 배포 worktree). 검사 후 Core10 동일 바이너리 복구, PID 18784. 원본 작업 트리의 같은 내용(CRLF 포함) source_hash는 e9d57981f7389e393ec2472032aeee8fadf0c8356dbc316de8613a52274d2bfc이고, 배포 복사본의 42개 입력 파일만 Git에 실제 저장되는 LF bytes와 일치시켰다. 원본 index/변경은 유지한다. 새 설치 패키지 빌드를 시작했으며 실제 읽기·ReadSend 동의·발송·수신 및 베타 게시는 아직 완료로 기록하지 않는다.

## 2026-11-02 새 설치본 실제 본문·Google ReadSend 확인, 메시지별 발송 확인 대기

검증된 배포 소스로 설정 포함 preview.11을 빌드했다. ZIP SHA-256 40f066ccf464fcf99a02df76cc9c3acca4631532b0d7e010382bfcaa56c3fc94, source_hash 79601a1731b8da04c0527bb29647d51d8fcbfbf6a0e1736423ab1f779b24ea93, built_from_current_workspace/설정 포함을 확인했다. 기존 Core10만 관찰 경로·hash를 재확인하고 중지한 뒤 실제 계정 데이터 7개 파일을 보호 백업했다. 전후 snapshot SHA-256 04d7ce5d6dd65181bdf71a7b67b26e7aa0921d30772dd65d5b979dd9014c5132 일치. 보호 백업 C:/Users/cyary/AppData/Local/mail-hantong-beta-test-20261102-c87b4ebb9f4c4f3cbd29201ab9f1b536. 공식 installer로 업데이트했고 이전 버전/백업을 유지했다.

Core 0개에서 설치된 init만으로 자동 기동해 기존 계정 선택→추가 동의/한통 계정 확인창 없이 작업 메뉴에 도달했다. 실제 schema22→23 시작과 기존 인증/계정 유지 경로를 확인했다. 공식 core status·clients verify·accounts list 모두 exit 0/ok=true, Gmail 1개 ready. 같은 계정의 폴더 90개·명시 단일 메시지 재동기화 후, 앞서 실패했던 96,724바이트 메일의 안전한 본문 텍스트 5,856바이트를 정상 반환했다. truncated=false이며 원문·제목·주소는 검사 출력에 남기지 않았다. 자동 테스트에 실제 계정/네트워크를 연결하지 않았다.

기존 계정 작업 메뉴 4로 읽기→작성·보내기 전환을 진행했다. Aside로 같은 공개 setup 요청을 옮겨 계정 선택은 공식 계정 목록과 정확히 대조하고 URL/주소를 가렸다. 요청 scope는 openid/email/gmail.readonly/gmail.send만 확인했다. Google의 미검증 앱 안내는 계속 표시되며 검증 심사 완료로 표현하지 않는다. Google 동의 후 setup ready/error 없음, 추가 한통 계정 확인/Hello 없이 작업 메뉴 복귀와 정상 인증을 확인했다. 메뉴 5의 수신자 입력 화면에 진입해 0 취소→메일 미발송→메뉴→3 종료도 실제 확인했다. 수동 pipe probe의 짧은 endpoint 생성 간격은 bounded 재연결로 처리했다. 일반 CLI에 노출하지 않는 local-profile 옵션을 이용한 별도 probe는 INVALID_INPUT으로 거부됐으며 제품 경로를 바꾸지 않았다.

사용자가 요청한 실제 발송 시험을 위해 본인 계정으로 보내는 synthetic 초안 한 통과 23바이트 텍스트 첨부를 기존 정책/감사/immutable draft API로 만들었다. 실제 생성 성공, native review request 1회 및 pending을 확인했다. Windows에 “mail-hantong — 보낼 메일 확인” 창이 존재함을 읽기 전용으로 확인하고 사용자에게 내용 확인·Windows Hello를 직접 완료하도록 요청했다. 이는 계정/기능의 추가 승인이 아니라 AGENTS §5.4~5의 메시지별 사람 확인이다. CLI·에이전트가 승인 증거를 만들거나 Hello를 대행하지 않는다. 현재 실제 sent·수신/본문/첨부 일치와 베타 공개 조건은 아직 확인되지 않았다. pending/unknown에서 자동 재발송하지 않는다.

0.1.0-beta.1 패키지도 로컬 준비했다. channel=beta, source_hash 동일, CLI/Core/MCP 모든 실행 파일 SHA-256이 실제 시험 중인 preview.11과 같음을 확인했다. ZIP artifacts/cli-onboarding/0.1.0-beta.1/mail-hantong-cli-0.1.0-beta.1-windows-x86_64.zip SHA-256 3b0128bedc866631ed422a5bd255d68176f450dfcea9fc1510b4ce2a51c48ae5. BUILD-METADATA SHA-256 7b2af5852e031ab28a784e3023a27275722ff2bc3fbbb61c6e73387540c1b2dc. **아직 GitHub 베타 게시하지 않았다.** 현재 Core11 PID 38860과 실제 계정/키/백업, 원본 미커밋/새 파일을 보존한다. 상태 **[사람 검토 필요]**, 실제 발송 확인 대기.

이어서 실제 beta.1 ZIP을 원래 설치와 분리한 .local/beta-package-smoke 고유 경로에 공식 설치했다. 모든 release 파일의 size/hash, CLI/Core/MCP help/version exit 0, 격리 제거 exit 0 및 synthetic 사용자 데이터 sentinel 보존을 확인했다. 실제 Core/계정/설치는 변경하지 않았다. 결과 target/beta-package-smoke-summary.json. 별도 배포 브랜치에 로컬 커밋 0aafc3249deb7ad6ba3a1db0634f3e6e054d2b16을 만들었으며 원본 main의 HEAD/index/미커밋 변경은 그대로다. 원본 staged 변경 0개를 재확인했다. 아직 push/tag/release 게시하지 않았다.

19:12 KST 실제 발송 status는 expired/APPROVAL_EXPIRED였다. 사용자의 메시지별 확인이 5분 안에 완료되지 않아 시험 메일은 전송되지 않았고 실제 sent/수신 검증은 미완료다. 새 발송 확인 요청이나 재발송은 만들지 않았다. 수동 확인 스크립트가 처음 terminal receipt의 비정상 종료 코드를 일반 오류로 해석해 UNKNOWN을 출력한 것은 스크립트 판정 문제였으며, 상태/안정적 오류 코드를 읽도록 수정해 실제 APPROVAL_EXPIRED를 확인했다. 이 UNKNOWN 출력을 SEND_UNKNOWN이나 실제 네트워크 전송 불확실성으로 기록하지 않는다. 기존 immutable 시험 초안과 보호된 receipt는 유지하며, 사용자가 직접 확인 가능한 시점에 같은 초안의 새 네이티브 확인 절차를 진행해야 한다. 실제 기능 검증 완료가 조건인 베타 공개는 보류한다. 독립 로컬 구현·전체 검사·실제 큰 본문·동일 계정 기능 전환·베타 패키지 준비는 완료했고, 메시지 발송/수신과 공개만 이 사람 단계 이후에 남아 있다. **[사람 검토 필요]** 유지.

## 2026-11-02 개인 메일 발송 간소화 재설계·구현 중

사용자가 메일 발송 시 Windows Hello가 필요한 이유를 확인하고 간단하게 이용하도록 재설계를 지시했다. 개인 Google 기본 CLI의 정확한 단일 계정 발송 범위만 네이티브 전체 내용 확인·보내기 버튼으로 완료하고 Hello를 제거한다. 다른 등록과 보호 설정은 기존 OS 확인을 유지한다. CLI 등록은 실제 사람 실행 증거가 아니며 같은 사용자 UI 자동화 방어가 약해지는 결정을 ADR-028 제안과 requirements 변경 제안에 명시했다. 사용자 UX 결정은 사람 보안 검토 완료가 아니다.

AGENTS, root/최신 DESIGN, native-send-ledger·CLI·Google 계약, ADR-015 한정 예외, 보안 불변 조건과 사용자 안내를 먼저 갱신했다. Store의 분류·동일 트랜잭션 재검사와 native 별도 opaque confirmation을 독립 구현하고 Core에서 선택·소비·단일 claim을 연결하는 중이다. wire 승인 플래그·CLI 발급 기능·새 DB 스키마는 추가하지 않는다. 미커밋 변경과 실제 계정·preview.11·보호 백업은 보존하며 이전 만료 메일을 재발송하지 않았다. 최종 통합 검사는 구현 후 수행한다. **[사람 검토 필요]**, 실제 발송/수신과 조건부 베타 게시는 아직 미완료다.

Windows native 도구 환경에서 Core·CLI·Store·approval-windows 통합 check exit 0. 별도 배포 사본의 첫 전체 검사는 Docs 29개 통과 후 Build 94개 중 반복 설치 시험 1개가 File.Replace의 파일 잠금 오류로 실패했다. 같은 반복 설치 시험을 단독 재실행하자 1개 통과했으며, 잠금 주체는 미확정이다. 설치 코드를 추정 수정하지 않는다. 검사 전 보존한 동일 preview.11 hash로 Core를 복구했다. 개인 창의 공통 등록 거절·접근 권한 문구를 추가로 제거하고 보내기·취소와 개인 상태 문구만 표시하도록 보완했다. 수정 후 새 안정 소스에서 전체 검사를 다시 수행한다.

통합 검사 보완: Docs 29/Build 94 및 요건·아키텍처·추적은 재실행 통과했다. fmt가 발견한 새 요약 시험의 서식을 workspace formatter로 맞췄고, 새 Windows 시험의 잘못된 상수 import·고정 만료 시각을 수정했다. 다음 전체 검사에서 Store 476개 통과/새 provenance race 시험 1개 실패를 확인했다. 감사 기록을 변조 전에 만들도록 준비 순서를 고쳤으며, OAuth 출처가 불변 scope history와 충돌할 때의 정확한 거부 코드는 PolicyBlocked가 아닌 InternalError임을 고정했다. 발송·outbox가 생기지 않는 불변 조건은 유지한다. Core의 새 성공 시험은 열린 Store의 재개방 및 중복 공개 키 때문에 fixture가 Conflict를 낸 것을 확인해, 실제 등록·scope 전이 API와 별도 합성 공개 키로 교체했다. 변경 후 Core 발송 통합 시험 5개 통과(exit 0). 기존 실패 시험을 삭제하거나 skip하지 않았다. 제품의 실제 계정·비밀값·기존 만료 초안은 변경하지 않았다.

최종 안정 소스의 `python -X utf8 scripts/check_all.py` exit 0. 배포 사본 source_hash `ac33a88a0dbc70e030e25bb25aaa4ce278fee606d3f192e2348396a429b20a25`: Rust 1,659개/30 suites/failed 0/ignored 0, Docs 29개, Build 94개/skip 0, 합계 **1,782개 통과**. fmt·Clippy(-D warnings)·전체 build·요건·아키텍처·추적 검사 통과. 로그 `target/personal-send-full-check.log` SHA-256 `e593e64a5ccaff355e5d7f2b41f8d61eb8bc55d25eef60fcc8da4af13ee35daa`(별도 배포 worktree). 검증 전 관찰한 Core11의 동일 경로·hash·인수를 복구했고 PID 34992다. 원본 미커밋·새 파일과 index를 보존했다. 새 경로의 정책·원장·네이티브 proof를 독립 코드 검토했으나 사람 보안 검토를 대신하지 않는다. 새 설치본 준비와 실제 발송/수신·조건부 베타 게시는 다음 단계다. **[사람 검토 필요]** 유지.

## 2026-11-02 설치본 확인·실제 송수신 성공과 터미널 UX 재정정

검증된 source_hash ac33a88a0dbc70e030e25bb25aaa4ce278fee606d3f192e2348396a429b20a25로 preview.12를 만들었다. ZIP SHA-256 74052e8e0fa321c93f556316a9ad82c437c6767694a5624d60ded49586ed4ecf. 기존 Core11을 관찰한 뒤 중지하고 실제 사용자 데이터 9개 파일을 보호 백업했다. 전후 snapshot SHA-256 2650cbb3eb69e62b543a611e7b1bd7064a50a3de53acb3d8d93a913cedb0c645 일치. 백업은 C:/Users/cyary/AppData/Local/mail-hantong-simple-send-test-20261102-2544e48433bd4342bc49986793468ac0. 기존 버전·키·계정·원본 변경을 유지했다.

설치 후 Core 0개 상태에서 preview.12 init만 실행해 자동 기동·기존 계정 선택·작업 메뉴·정상 종료를 확인했다. 추가 Google 동의/Hello 없음. 공식 core status/clients verify/accounts list 모두 exit 0, Gmail 1개 ready, Core12 PID 40608. 실제 96,724바이트 메일의 텍스트 본문 5,856바이트/truncated=false를 재확인했다. 기존 만료 시험 초안의 새 review를 한 번 열고 사용자가 보내기를 선택했다. 공식 상태 sent, 본인 계정 수신과 합성 본문/23바이트 첨부 해시 일치, 자동 재발송 0을 확인했다. 원문·주소·비밀값은 진행 기록에 쓰지 않는다.

beta.2도 로컬 준비했다. ZIP SHA-256 0332cce4e8f3a7b100e06872fb92855074eaf77a7c79b875bff4575e52b022c0, 같은 source_hash/built_from_current_workspace, metadata SHA-256 09a46e0eb403bf07857cf3a89b1ddf1ca91d29dddfe068248c7f2597ee18f0a3. CLI/Core/MCP 3개 바이너리는 실제 설치 preview.12와 동일하다. 실제 beta ZIP의 격리 설치·전체 파일 hash·help/version·제거·synthetic 데이터 보존을 통과했다. 별도 배포 사본 로컬 커밋 4b4336514c7f5cd004748630b887d2cd8d8e0e42; push/tag/release 게시 없음.

사용자는 실제 확인창을 보고 별도 창 없이 CLI나 TUI에서 모든 것을 해결하라고 정정했다. 앞 네이티브 확인창 구현을 출시 기준으로 취급하지 않는다. 최신 AGENTS·ADR-029·terminal-personal-send 계약으로 교체하고 prepare(전체 preview, 효과 없음) → 터미널 보내기/취소 → confirm(단일 작업) 경로를 구현 중이다. 터미널 선택은 요청 의도이며 OS 사람 존재 확인이 아니고, TTY로 AI와 사람을 구별한다고 주장하지 않는다. 기존 계정·초안·성공 receipt·백업과 미커밋 파일은 보존하며 같은 메일을 재발송하지 않는다. 새 UX 통합 검사와 설치본 검증 전 beta 공개는 보류한다. **[사람 보안 검토 필요]** 유지.

## 2026-11-02 모든 CLI 경로의 동일 터미널 확인으로 전환 중

최신 사용자 지시를 개인 Gmail에 한정하지 않고 CLI 등록·MCP 등록·범위 변경·고급 Google/IMAP/SMTP 설정·메일 변경·보호 발송에 적용한다. Google 표준 OAuth 브라우저만 외부 단계로 남긴다. terminal-workflows 계약과 AGENTS 우선 지침을 추가했으며 앞 네이티브/Hello 지침은 해당 CLI 경로에서 대체된다. 터미널 선택은 사용 의도이고 OS 사람 존재 증거가 아니다.

개인 Gmail은 mail.send.request prepare→전체 불변 preview→터미널 보내기/취소→confirm 1회로 연결했다. JSON·리다이렉션·--no-input에서는 확인하지 않는다. cancel/EOF는 발송하지 않으며 결과는 최대 5분 상태 조회만 하고 재전송하지 않는다. Core의 등록·scope·메일 변경·보호 발송에는 전용 티켓/작업을 추가했으며 아직 전체 통합 완료는 아니다. 보호 발송도 정책·감사·버전/hash·outbox claim·SEND_UNKNOWN 처리를 유지한다. 만료 티켓 정리와 64개 상한을 추가했다.

같은 콘솔에서 실행하는 패키지 검증된 Core foreground helper와 별도 typed IPC를 추가 중이다. 서버는 pipe에서 관찰한 프로세스의 SID/logon·Core 실행 파일 경로/hash를 확인하고, helper도 연결된 서버의 동일 실행 파일을 확인한다. PID·approved·human을 JSON으로 받아 증거로 쓰지 않는다. Secrets는 콘솔 echo를 끄고 입력을 민감 버퍼와 DPAPI staging 내부에서만 처리하며 평문을 CLI/IPC/로그로 반환하지 않는다. Core가 발급한 정확한 요청·계정·endpoint·버전·서버·만료 결속 티켓으로 소비하도록 연결 중이다.

Store schema23→24와 frozen v23 검사, terminal_cli 출처·scope hash10을 추가했다. 기존 approval_helper/google_oauth history는 보존한다. 대상 Store migration 시험 2개와 check 통과 보고를 받았다. 대상 개인 Core 시험 2개, 새 pure ticket 시험 2개, Secrets 238개 통과 보고는 최종 전체 검사와 구분한다. 부모 첫 통합 check는 병렬 구현 중 terminal_google::Pending 가시성 오류로 실패했고 해당 소유자에게 전달했다. 고급 Google·IMAP·SMTP 실제 wire/helper 연결과 최종 보안 검토·전체 검사는 진행 중이다. 아직 새 패키지 설치·schema24 실제 적용·베타 게시 없음. 실제 사용자 데이터·계정·키·성공한 기존 발송 receipt·보호 백업·원본 미커밋/새 파일을 유지한다. **[사람 보안 검토 필요]**.

터미널 Core helper의 IMAP/SMTP Get→Core-issued secret ticket→숨김 capture→opaque staged handle→SecretSubmit→기존 credential/probe 전이를 실제 연결했다. 일반 CLI는 Secrets에 의존하지 않으며 백그라운드 Core는 콘솔 입력을 읽지 않는다. 고급 Google client ticket/stage는 pipe-observed 동일 Core image/SID/logon으로 제한하고, foreground에서만 숨김 입력한다. Google 계정 검토의 실제 만료·전체 binding hash·재연결 버전을 부작용 전에 재검사하도록 보완했다. Ctrl+C 입력 처리·초과 입력 drain·UTF-16 버퍼 zeroization·endpoint-bound credential envelope을 보강했다.

독립 검토가 메일 변경 request의 TerminalGoogleMutation 즉시 enqueue를 발견했다. 이를 제거했고 `apr01_terminal_mutation_request_and_preview_never_schedule_before_confirm`가 실제 Windows 합성 Store에서 통과했다. request/preview/잘못된 hash 동안 worker 작업·provider claim이 없고, Confirm만 queued를 만들며 재사용 ticket은 거부됨을 검증한다. typed snapshot 전체 hash, frame 길이 제한, 제어·bidi·CR·U+2028/U+2029 escape 시험을 추가했다.

첫 전체 검사(source_hash 5c7a18f27ab35348e1b896fc742a152ef265a4f477cd7723e1e0317f73ccfd65)는 Docs 29/Build 94·요건·아키텍처·추적·fmt 통과 후 Clippy가 CLI escape 분기의 중복을 발견해 exit101로 끝났다. 중복 분기를 합쳤으며 후속 정적 검사와 통합 검사를 이어간다. 같은 preview.12 경로·SHA-256 76bd1c20d5ae9ed379532a8c9199a624b6cf96165a1fdfd245125c3e8a348595의 Core를 복구했다(PID55680). 테스트 중 실제 사용자 데이터에 새 schema를 적용하지 않았다.

추가 Store 검토에서 terminal_cli 판독 누락, hash10 SQL CHECK/계정 변경 검증의 과도한 제한과 초기 출처·epoch 고정으로 정상 terminal scope 전이가 막히는 문제를 발견했다. 대상 수정과 실제 Windows stage/reopen/history 회귀 시험을 진행한다. 따라서 cargo check·일부 pure hash 시험 통과만으로 전체 터미널 전환 또는 출시 완료를 선언하지 않는다. 새 패키지 설치·베타 게시 전 전체 검사와 설치본 확인이 남아 있다. 원본 HEAD/index/미커밋·새 파일, 실제 계정·비밀·백업·기존 성공 메일 receipt를 보존한다. **[사람 보안 검토 필요]**.

## 2026-11-03 전체 CLI 터미널 흐름 통합·검증

사용자의 “별도 창 없이 CLI나 TUI에서 모든 게 해결” 지시를 개인 Gmail뿐 아니라 고급 Google/IMAP/SMTP 설정, CLI/MCP 등록·권한 변경, 메일 변경과 보호 발송까지 적용했다. Google의 표준 로그인·동의 브라우저는 유지하며 한통 자체 검토·비밀 입력은 기존 콘솔에서 진행한다. 터미널 선택은 요청 의도이며 OS 사람 존재 증거가 아니다.

Core 소유 foreground helper와 실제 pipe peer 이미지 검증, 원래 요청의 계정·등록·epoch·내용 hash·원래 만료에 결속한 일회용 ticket을 연결했다. Secrets가 숨김 입력·DPAPI staging·서버 endpoint 결속·일회용 소비를 담당한다. 요청 생성은 공급자 작업을 예약하지 않으며 Confirm 이후에만 작업을 한 번 예약한다. 사람 선택이 필요한 요청은 비대화형·JSON·입출력 리다이렉션에서 효과 전에 HUMAN_STEP_REQUIRED로 중단한다. SEND_UNKNOWN을 자동 재전송하지 않는 원칙을 유지한다.

통합 중 수정한 문제: Google 변경의 Confirm 이전 작업 예약, terminal scope 범위·기존 channel 재사용·legacy native job 차단, 고급 설정의 실제 SecretSubmit 연결과 Google StageClient 뒤 OAuth 재개, 만료 후 효과/실패 보고 순서, SMTP 실패 상태 보존, 16KiB method 제한과 U+2028/U+2029 이스케이프, 구버전 manifest 없는 설치본의 업데이트 호환성. 설치기는 검증된 이전 Core만 식별·재관찰 후 중지하고 데이터·키·이전 버전을 보존한다. 제품에 테스트 환경변수 우회는 없다.

기존 전체 검사 실패도 보존했다. 설치 시험의 legacy manifest 호환성 실패를 수정했고, 새 MCP 교체 시험은 기존 등록과 같은 공개키/이전 등록 없음으로 구성한 fixture 오류를 고쳤다. 제품의 중복키·교체 전이 차단을 완화하지 않았다. 코어 통합 시험에서 계정 없는 초기 CLI→첫 명시 계정의 v10 이력 판독을 거부하는 구현 문제를 수정하고 stage/reopen/activate 회귀 시험을 추가했다. 대상 계정은 계속 비어 있을 수 없다. 이전 native queue와 legacy 출처를 가정한 시험을 새 계약에 맞췄고, 같은 대상이라도 legacy pending/staged 행을 terminal 요청이 재사용하지 않는 회귀 시험을 추가했다. GUI 앱의 명시적 등록 연결은 유지하고 CLI에서 GUI 요청으로 폴백하는 경로를 막았다. 독립 정적 재검토는 알려진 차단 문제의 해결을 확인했으며 사람 보안 검토를 대체하지 않는다.

원본 HEAD/index/미커밋·새 파일과 실제 계정·비밀·보호 백업·이전 성공 메일 receipt를 보존한다. 별도 release worktree에서 LF 소스로 검증하며 공개 대상에서 로컬 계정 검증 자료와 비밀 파일을 제외했다. **[사람 보안 검토 필요]**.

Core targeted offline integration: 137 passed / 0 failed / 0 ignored; original Core12 restored with identical SHA-256, PID 13604. Empty-base Store regression: 1 passed. Final frozen-source check-all, new-package install, real terminal send and beta publication remain pending.

최종 별도 배포 사본 source_hash `c33ad5e43110d784e0c8a80fd3392461b1979042e8b9abb3814ca67f83d47ce2`에서 check-all exit 0. Rust 1,703개/30 suites/ignored 0, Docs 29개·Build 101개/skip 0, 합계 **1,833개 통과**. fmt·Clippy(-D warnings)·전체 build·요건·아키텍처·추적 검사 통과. Store 484개와 Core 137개를 포함한다. 로그 `target/all-terminal-full-check.log` SHA-256 `0dd90fd618786ffefea835b46c436c296c65ade8485f2e30705ab1be2bace191`. 기존 Core12 동일 SHA-256 복구, PID 65196. 원본 index는 비어 있고 HEAD와 미커밋·새 파일을 유지한다. 이 자동 검사 결과는 실제 계정·네트워크 시험, 새 패키지 설치 또는 베타 게시 완료를 뜻하지 않는다. 다음 단계는 새 preview.13/beta.3 패키징과 보호 백업 후 설치본의 터미널 취소·실제 합성 메일 발송·수신 점검이다. **[사람 보안 검토 필요]**.

## 2026-11-03 설치본 터미널 이용·실제 합성 송수신 확인

검증된 소스 `c33ad5e43110d784e0c8a80fd3392461b1979042e8b9abb3814ca67f83d47ce2`로 preview.13과 beta.3를 각각 새 빌드했다. 두 BUILD-METADATA는 `built_from_current_workspace`, build_performed=true, 승인된 Desktop 설정 포함을 확인했다. preview.13 ZIP SHA-256 `ea1dd02b87e099ecb8f0e547a6c697633b0d67d83df5d7a0c5817ee3594b4e51`, beta.3 ZIP SHA-256 `64cd3747944e8947d9672704557835bcbd1eb3e2bacb7bd713a0d22605397ba1`.

Core12 동일 경로·hash·실행 인자를 대조해 중지하고 기존 데이터 9개 파일을 보호 백업했다. 전후 snapshot SHA-256 `7e2a315cebf757bf78c9c469409c5765affb75c5095318e6233970de2d840469` 일치. 기존 데이터·키·이전 설치 버전을 보존하고 공식 설치기로 preview.13을 적용했다. Core 0개에서 설치된 init만 실행해 자동 기동→기존 계정 선택→작업 메뉴까지 추가 Google 동의·한통 창·Hello 없이 도달했다. 실제 schema23→24 시작과 backup-before-v23 생성, ready Gmail 1개 및 transport/authentication 정상 상태를 확인했다.

실제 계정·네트워크를 사용하는 수동 점검은 자동 시험과 분리했다. 이전에 성공한 시험 메일은 재발송하지 않았다. 새 합성 초안 2개를 각각 본인 주소로 만들고, 터미널 전체 내용·23바이트 첨부 preview에서 하나는 취소해 interrupted/APPROVAL_REQUIRED를 확인했다. 별도의 새 합성 메일은 사용자에게 위임받은 실제 시험 범위에서 보내기를 1회 선택했고 sent exit 0을 확인했다. 표준 Google API 수신 동기화·본문 읽기·첨부 검증으로 수신 1개, 본문·첨부 SHA-256 일치, 자동 재발송 0을 확인했다. 이 선택을 OS 사람 존재 증거나 보안 코드의 사람 검토 증거로 기록하지 않는다.

beta.3 실제 ZIP의 격리 설치·모든 manifest 파일 크기/hash·help/version·삭제·사용자 데이터 sentinel 보존 검사가 통과했다. 이어 실행 중인 실제 preview.13 Core를 공식 beta.3 설치기 스스로 식별·종료하고 새 버전으로 전환하는 경로를 확인했다. Core 0개에서 beta.3 init이 자동 기동하고 기존 계정·작업 메뉴를 유지했다. beta.3에서 이전 취소 receipt가 미발송 상태, sent receipt가 sent, 수신 1개 및 본문·첨부 일치를 다시 확인했으며 새 발송 요청은 만들지 않았다. transport/authentication/accounts 모두 exit 0/ok=true이다.

현재 설치는 `0.1.0-beta.3`이며 이전 버전과 보호 백업을 보존한다. 새 사용자 Google 로그인·동의, 실제 IMAP/SMTP 계정 시험은 이번 수동 점검에 포함하지 않았다. 공급자 표준 브라우저와 Google 앱의 미검증 안내는 한통 터미널 전환과 별개다. 다음 단계는 이 검증된 beta.3와 소스를 GitHub에 게시하고 다운로드 무결성을 확인하는 것이다. **[사람 보안 검토 필요]**.

## 2026-11-03 beta.3 공개 게시·공개 설치 경로 확인

검증된 구현 소스 커밋 `d65dd6f`와 설치 점검 기록 `0d1ba5c`를 GitHub main에 fast-forward 게시했다. 원본 작업 트리의 HEAD `2852a31d0d8383d3a593a6479c20bb798b0d798e`, 비어 있는 index, 미커밋·새 파일은 유지한다. GitHub prerelease [v0.1.0-beta.3](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.3)에 ZIP·SHA256SUMS·BUILD-METADATA.json 3개 자산을 게시했다. draft=false/prerelease=true를 확인했다. 공개 다운로드 ZIP SHA-256 `64cd3747944e8947d9672704557835bcbd1eb3e2bacb7bd713a0d22605397ba1` 및 빌드 정보 checksum이 로컬 검증본과 일치한다.

공개 main의 install.ps1을 직접 다운로드해 검증된 소스와 바이트 일치를 확인하고, 이 공개 스크립트가 GitHub에서 beta.3를 내려받아 별도 경로에 설치하는 전체 절차를 실행했다. 설치본 help/version exit 0, 삭제 시 합성 사용자 데이터 sentinel 보존, 실제 사용 중인 주 설치본 Core와 release-manifest hash 일치를 확인했다. 공개 releases의 기본 latest 선택도 beta.3이다. 이 점검은 실제 사용 설치본·계정·실행 중인 Core를 변경하지 않았다.

결과: 현재 실제 설치 `0.1.0-beta.3`에서 기존 계정·키·발송/취소 이력을 유지하며 CLI 안에서 내용 확인·취소·발송한다. Google 표준 로그인·동의 브라우저는 별개다. 실제 Gmail 합성 송수신·본문/첨부 일치와 전체 1,833개 자동 검사 결과를 위 항목에 누적했다. 신규 Google 로그인 및 다른 실제 공급자 계정의 추가 점검, 보안 코드의 사람 검토는 남아 있다. 공개 베타 배포를 제품 전체의 정식 출시·사람 보안 검토 완료로 표시하지 않는다. **[사람 보안 검토 필요]**.

## 2026-11-03 현재 베타 배포 확인·홈페이지와 AI 이용 안내 보충

사용자가 현재 버전 배포와 페이지 설명 보충을 요청했다. 공개 최신 릴리스와 실제 설치가 모두 `0.1.0-beta.3`임을 재확인했다. 앱 코드 변경 없이 같은 버전의 바이너리·체크섬을 중복 생성하거나 교체하지 않고 기존 공개 릴리스 설명을 보충했다.

별도 gh-pages 작업 사본에서 홈페이지에 설치→실행→계정 연결→기능 선택/Google 동의→메일 이용 순서, 공개 설치 명령, 자동 코어 시작, 작업 메뉴 4번 활성화와 5번 작성, Codex/Claude 연결 명령과 `/provider`·`/model`·`/mode`·`/fast`·`/agent` 전환을 추가했다. AI 공식 실행 프로그램 별도 설치, Claude Console/API 범위, 메일 도구 등록→명시적 `hantong review`→상태 확인→에이전트 연결 순서, 외부 전송 범위를 설명했다. 실제 AI 로그인·유료 추론 미검증과 나머지 베타 제약을 구분했다. 숨겨진 추가 터미널 실행 경로는 광고하지 않았다.

개인정보·이용 안내에서 이전 네이티브 창·Windows 확인 설명을 현재의 같은 터미널 검토로 정정했다. 기존 저장·철회·삭제·외부 AI 공유 안내와 라이선스는 보존했다. 표·설치 명령이 좁은 화면에서 넘치지 않도록 스크롤·메뉴 줄바꿈 스타일을 보충했고 분석 스크립트·로그인 폼은 추가하지 않았다. gh-pages 커밋 `ed48beac725b2f1f58ed3668809527430dcf491d`를 게시했다.

검증: HTML 3개 구조·고유 앵커·내부 링크·필수 베타/AI 내용 검사 통과, 문서 시험 29개 통과(12.935초), 변경 파일 diff check 통과. GitHub Pages build status=built/error=null이며 공개 홈페이지·개인정보·이용 안내·CSS 4개가 HTTP 200 및 커밋 바이트와 정확히 일치한다. https://tz1012.github.io/mail-hantong/ 에 반영됐다. 저장소 README와 CLI-LAUNCH의 후보 버전·이전 창·수동 코어 필수 설명도 최신 beta.3에 맞게 정정했다. 릴리스 설명은 기존 내용을 보존하고 홈페이지·AI 안내 링크를 덧붙였다.

앱 바이너리·계정·권한·비밀값·Google 콘솔 설정은 변경하지 않았으며 실제 로그인·AI 유료 호출·메일 발송을 새로 수행하지 않았다. 문서만 변경했으므로 Rust 전체 검사는 재실행하지 않고 위 beta.3의 1,833개 결과를 기존 검증으로 구분한다. 원본 HEAD/index/미커밋·새 파일을 보존한다. 보안 코드의 사람 검토 필요 상태도 유지한다.

## 2026-11-03 `/help` 명령 설명 구현·AI 자동 메일 도구 변경 제안

사용자가 `/help`에서 이용 가능한 명령 목록과 설명을 요구했다. `apps/hantong-cli/src/ai.rs`와 AI 한국어·영어 리소스에 명령별 설명을 추가했고, 로그인·모델 호출 없이 `hantong ai help`와 JSON 도움말을 제공한다. `main.rs`는 최상위 `/help`를 기존 `--help` 별칭으로 처리하고 AI 명령 설명을 포함한다. `onboarding.rs`와 시작 메뉴 리소스는 계정 선택·계정 연결·기능 선택·작업 메뉴의 현재 선택지 설명을 출력하고 같은 입력 위치에서 계속 기다린다. 도움말은 메일 작업을 실행하지 않으며 메일 본문·비밀값 입력을 명령으로 해석하지 않는다. 숨겨진 추가 실행 경로는 도움말에 노출하지 않았다.

통합 검토에서 계정 선택 메뉴의 도움말 종료 번호가 2로 잘못 표시되는 문제를 0으로 고치고 회귀 시험을 추가했다. 최초 CLI 시험은 한국어 안내를 다른 메뉴 문구로 검사하는 새 시험 1개 실패(99 통과)였으며 실제 메뉴 문구·반복 도움말·부수 효과 0을 검증하도록 수정했다. 최종 `python -X utf8 scripts/cargo_native.py test -p hantong-cli --bin hantong --locked --offline`: **101 통과/실패 0/ignored 0**. CLI all-targets Clippy `-D warnings` exit 0, 수정 Rust 3개 rustfmt check 통과, 해당 diff check 통과. 로그는 `target/help-cli-tests.log`, `target/help-cli-clippy.log`. 추적표 생성 valid=true/341 criteria, source hash `8109be174a252e061f6394b8d34dde4931c48c59e313a740f85c3bea2016116c`이다. 실행 중인 실제 Core와 계정은 건드리지 않았고 전체 workspace 검사는 이번 도움말 변경에 재실행하지 않았다.

사용자는 이어 내장 AI에 메일 접근·본문·작성·발송·삭제 도구가 자동 제공되어야 한다고 정정했다. 현재 AI는 기본 off이며 MCP 고정 identity를 통해 연결하고, 삭제는 계약·도구·공급자 경로에 없음을 실제 코드로 확인했다. 기존 MCP 등록을 자동 승인하면 외부 MCP 호스트에도 같은 권한이 열리므로 단순 `set_agent(true)`/기존 등록 자동 확장은 구현하지 않았다.

AGENTS.md §7의 계약 변경 제안 절차에 따라 `docs/proposals/AI-AUTOMATIC-MAIL-TOOLS-20261103.md`에 구체적인 변경안을 작성했다. 내장 AI 전용 identity·보호 프로필·제공자/계정별 전송 설정을 두고 설정 후 도구를 매 세션 자동 연결한다. 수동 MCP 등록 명령·요청 ID 복사·필수 `/agent on`을 일반 이용 흐름에서 제거한다. 읽기/본문/초안과 발송 요청을 제공하며 삭제는 우선 단일 메일 휴지통 이동으로 제한한다. 발송·삭제는 Core 소유 불변 preview·같은 터미널 확인·단일 claim·불확실 결과 무재시도를 유지한다. Google 읽기/발송 동의는 삭제나 외부 AI 전송 동의가 아니며 필요한 기능 설정·Google 추가 권한을 우회하지 않는다.

이 변경은 `ai-managed-clients.md`의 기본 off/독립 MCP 연결 및 `google-write.md`·`google-mutations.md`의 삭제 제외 계약을 바꿔야 한다. 제안에는 영향 파일·요건(AI-07/AI-09/ACC-02/APR-01/SEC-02/SEC-08/SYNC-07)과 synthetic 수용 검사를 명시했다. **AI 자동 연결·삭제는 아직 구현·배포되지 않았으며 계약 변경 검토 대상**이다. 공개 사이트·beta.3를 이 기능이 있는 것처럼 바꾸지 않았다. 실제 로그인·유료 AI 호출·메일 조회/발송/삭제·기존 권한 확대는 수행하지 않았다. 기존 미커밋·새 파일·HEAD/index를 보존한다. 다음 작업은 이 구체안에 따른 계약 확정 후 identity·migration·자동 연결·휴지통 요청의 구현과 통합 검사이다. **[사람 보안 검토 필요]**.

## 2026-11-03 승인된 AI 자동 메일 도구·휴지통 계약 반영 및 구현

사용자가 위 변경안의 진행과 기존 계약 수정을 승인했다. AGENTS와 `docs/contracts/builtin-ai-mail.md`에 승인 범위·최신 기준을 기록했고 기존 AI/Google/MCP/등록/프로필/저장/DB/터미널 계약 10개와 제안서의 대기 상태를 정정했다. 계정별 외부 AI 전송 기능 설정, 내장 AI와 외부 MCP 분리, 발송·삭제의 같은 터미널 확인, Gmail 단일 메일 휴지통 이동, 영구·대량 삭제 제외가 기준이다. 승인된 제품 방향을 사람 보안 검토 완료로 취급하지 않는다.

구현: `ClientKind::Ai`, Codex/Claude별 고정 등록·키·`ai-codex.json`/`ai-claude.json`, provider/registration/account/epoch/request/time에 결속한 `ai_consents`를 추가했다. schema25 전체 rebuild migration은 v24 DDL을 동결하고 기존 표·행·FK·감사 sequence highwater·과거 scope hash를 보존한다. AI stage와 activation은 `ai_feature` 채널만 사용하고 실제 준비된 Gmail/IMAP 계정만 후보로 삼는다. 같은 터미널에서 표시된 계정 전체 연결 또는 취소를 명시적으로 선택한다. 기존 등록·새 계정·외부 MCP 권한을 묵시적으로 확대하지 않는다.

AI 대화 시작은 설정된 제공자 프로필을 자동 연결한다. 최초 설정에는 내장 CLI가 Core 터미널 검토를 직접 조정하며 수동 MCP 등록 명령·요청 ID 복사·필수 `/agent on`을 요구하지 않는다. 제공자 변경은 해당 제공자의 별도 설정을 검사하고 이전 대화 맥락을 이관하지 않는다. `/agent off`는 현재 도구와 맥락을 해제한다. 프로필 인증·계정 동의는 일반 읽기·본문·지연 동기화·발송/휴지통 claim에서 재검사한다. 비대화형/JSON/no-input은 설정을 생성하지 않으며, 모델의 발송·휴지통 요청도 pending 생성 전에 HUMAN_STEP_REQUIRED로 거부한다.

새 `mail.messages.trash.request`는 request/status만 공개한다. MCP가 canonical payload hash를 로컬 계산하고 Store는 계정/메시지/캐시 버전/operation/idempotency/hash를 재검사한다. 기존 일반 라벨 변경의 TRASH 금지는 유지한다. 실제 Modify 권한이 있는 Gmail만 정확한 messages/{id}/trash 경로를 단일 호출한다. Core의 확인 전에는 실행 job이 없고 확인 후에만 durable claim을 소비한다. 불확실 결과는 자동 재시도하지 않는다. 확인 화면은 대상 제목·보낸 사람·요약과 휴지통 이동임을 표시한다. AI에 실행·확인 RPC는 광고하지 않는다. 다른 공급자 삭제와 영구 삭제는 지원으로 표시하지 않는다.

통합 수정: AI 도구를 read_only=true로 판정하던 조건, AI 승인 채널 reader/activation 불일치, 휴지통의 일반 Google 실행 grant 오용, MCP→CLI 검토의 `--account` 옵션 불일치, 등록의 준비되지 않은 계정 포함, 새 AI 등록 IPC 서버 parser 누락, 제공자 전환의 무조건 no-input 처리, 이전 schema migration의 current DDL 참조를 수정했다. v8 재구성 시험은 새 AI 동의 표가 비어 있음을 검증한 뒤 제거하여 실제 동결 v8 fixture를 복원한다. 계정 범위 오류를 대상 조회보다 먼저 검사하던 기존 Google 상태 조회의 경계도 보존했다. 실패 시험을 삭제하거나 skip하지 않았다.

검증 진행: 초기 CLI/AI/MCP/application/contracts/approval/IPC 묶음 493개 통과, AI Store 등록/권한/휴지통 5개 통과. 전체 Store 검사에서 458/32 실패 후 실제 전이·fixture 오류를 수정했고 다음 전체 검사에서 488/2 실패로 좁혔다. 남은 계정 격리 시험 1개와 v8 실제 이력 보존 시험 1개는 수정 후 각각 1개 통과했다. 문서 29개·Build 101개·요건/아키텍처·전체 Clippy/build가 통과한 실행을 확보했다. 최초 Build 반복 설치 File.Replace 잠금 실패는 단독 재실행 1개와 이후 전체 Build 101개 실행에서 통과했다. 최종 전체 검증 결과는 이 항목에 이어 기록한다.

현재 실제 설치 beta.3·실제 schema24·보호 저장소·계정·발송 이력은 새 빌드로 변경하지 않는다. 기존 코어를 검사 전 동일 경로/hash/인자로 보존하고 검사 후 같은 beta.3를 복구한다. 실제 AI 인증/유료 추론·Google 추가 동의·메일 발송/삭제는 하지 않았다. 새 구현은 아직 배포하지 않았으며 실제 공급자 연결 시험과 **[사람 보안 검토 필요]** 상태를 구분한다. 원본 HEAD/index와 기존 미커밋·새 파일은 보존한다.

최종 통합 검사: `python -X utf8 scripts/check_all.py` **exit 0**. Rust **1,726개/30 suites/실패 0/ignored 0**, Docs **29개**, Build **101개**, 합계 **1,856개 통과**. fmt·전체 Clippy(-D warnings)·전체 all-targets build·요건·문서 원본·아키텍처·추적표 검사도 통과했다. 전체 실행에서 마지막으로 발견한 기존 CLI 등록의 중복 요청 차단 조건 누락을 수정해 기존 등록에는 계속 Conflict를 반환하도록 복원했으며, 해당 회귀 시험 단독 1개 및 최종 전체 Core 141개가 통과했다. Store 490개도 최종 전체 검사에서 통과했다. 새 AI 등록 IPC 서버 parser와 실제 bootstrap callback·프레임 상한 검사도 전체 검사에 포함했다.

검증 소스 hash `2ad5e1917e700713f986d92b6a12397e078953549919b88c805f349c640920d7`; 로그 `target/ai-auto-full-check.log` SHA-256 `91ae37002dd4ecbd2fa42a313e59a4646e725ad6875a5f00282dc69c07999942`; 요약 `target/ai-auto-verification.json`. 검사 후 같은 beta.3 실행 파일 SHA-256 `710aeff80153953f8a2625ba041a0ff48f3f434f41c74efb50781c1397f0b80c`와 실행 인자로 서비스를 복구했다(PID 50332). 설치된 beta.3의 `--json core status`는 exit 0/ok=true/transport_ready=true이다. 이 bootstrap 상태의 mail_ready=false/authenticated=false는 실제 계정·메일 도구를 조회하지 않는 기존 상태 응답이며 계정이 끊겼다는 판정으로 쓰지 않는다. 최종 추적표 검사도 동일 source hash/341 criteria/valid=true이다.

사용자가 ‘코어’의 의미를 물었다. 사용자에게는 별도로 관리할 대상이 아니라 한통 실행 시 자동으로 시작되어 실제 메일 처리와 권한 검사를 맡는 백그라운드 프로그램이라고 설명했다. 이 설명을 새로운 GUI·서비스 설치 방식 변경으로 취급하지 않는다. 내부 개발 용어와 사용자 안내 표현은 구분한다.

## 2026-11-03 beta.4 공개 배포와 설치기 호환성 수정

사용자가 최신 버전 배포를 명시적으로 지시했다. 원본 작업트리 HEAD `2852a31d0d8383d3a593a6479c20bb798b0d798e`와 빈 index, 미커밋·새 파일을 보존하고 별도 `release/beta4-20261103` 작업트리에 공개 소스 856개 파일을 복사했다. 로컬 실계정 검증·심사 자료 41개는 복사하지 않고 이미 공개된 문서는 보존했다. 자격증명 형태 검사의 기존 공개 TLS 시험용 키 2개는 원격과 바이트가 동일함을 확인했으며 그 외 탐지 0개였다. 실제 키·프로필·DB는 포함하지 않았다.

`v0.1.0-beta.4`를 소스 `39ce15ea0b17e7fcc38d0b2c5a1785c867dc059c`에 결속한 GitHub 사전 릴리스로 공개했다. Windows x64 ZIP, SHA256SUMS, BUILD-METADATA.json을 게시했고 공개 다운로드를 다시 받아 ZIP SHA-256 `a885b73d476ea88f961bc1a5f1fa05fd34d9a43a74ffe7dfdde01b0379f5e1f3`, 25,003,405 bytes가 로컬과 동일함을 확인했다. manifest 260개 파일의 크기/hash, PE/help/version, 배포자 설정의 기존 승인 beta.3 자산과 동일성, 사용자 DB·프로필 미포함을 검증했다. 빌드 source hash는 기존 전체 **1,856개 검사**의 `2ad5e1917e700713f986d92b6a12397e078953549919b88c805f349c640920d7`와 같다. 초안 생성의 짧은 target SHA는 GitHub 422로 거절되어 전체 SHA로 수정 후 정상 공개했다.

별도 경로의 실제 설치와 `hantong /help`, `hantong ai help`, `hantong ai providers --json`, MCP version, Core/T help 6개 실행이 통과했다. 문서 29개와 패키지 검사 10개도 재실행해 통과했다. 공개 main 설치기의 Windows PowerShell 5.1 실험에서는 Invoke-RestMethod의 JSON 배열을 직접 `@(...)`로 감싸 중첩 배열이 되는 기존 버그를 발견했다. 반환값을 먼저 변수에 할당한 뒤 열거하도록 수정하고 네트워크 없는 native PS 5.1 회귀 시험을 추가했다. 처음 회귀 시험의 mock switch 선언 누락도 바로잡았으며 관련 설치 시험 2개 및 전체 Build **102개 통과**를 확인했다. 후속 설치기 수정은 `d950894ad41b952a71917fd9ef9dc7bb31c34d2a`로 main에 게시했고 릴리스 바이너리·태그·체크섬은 바꾸지 않았다. 후속 추적표 source hash는 `5fa2708797fcec28fe37020e6ee48da9eac7c10294fa396470a74b67a5d092a4`다. 바이너리 전체 검사와 후속 설치기 검사 수를 합산한 새 전체 검사 수치로 주장하지 않는다.

GitHub raw main CDN이 수정 전 설치기를 일시 반환하여 수정 commit의 공개 고정 URL로 bootstrap을 실행했다. 홈페이지의 설치 명령도 검증한 수정 commit URL로 고정하여 이 캐시의 영향을 피했다. 이 설치기는 여전히 최신 공개 릴리스를 자동 선택한다. 최신 릴리스 선택·ZIP 다운로드·체크섬·별도 경로 설치·설치된 AI 도움말까지 통과했다. 시스템 PATH와 실제 mail-hantong 데이터는 변경하지 않았다. 근거는 `target/beta4-source-snapshot.json`, `target/beta4-package-verification.json`, `target/beta4-smoke-verification.json`, `target/beta4-public-install-verification.json`, `target/beta4-bootstrap-build-tests.log`에 있다.

홈페이지 gh-pages `5318834`는 beta.4 다운로드, `/help`, 표시된 준비 완료 계정 전체 연결/취소, 제공자별 외부 전송 설정, 내장 AI 자동 도구와 외부 MCP 등록의 차이, Gmail 휴지통/Modify/터미널 최종 확인, 영구·대량 삭제 미지원으로 갱신했다. 개인정보·이용 안내도 보완했다. HTML 파싱·로컬 링크·diff 검사, Pages 배포 success와 공개 HTTP의 beta.4/자동 도구/help 문구를 확인했다. beta.3의 실제 설치·Gmail 합성 송수신 검증은 해당 버전의 근거로 구분했다.

공개 주소: https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.4 및 https://tz1012.github.io/mail-hantong/ . 이번 작업은 공개 배포이며 사용자 PC 기본 설치본 교체나 실제 DB의 schema25 이행은 수행하지 않았다. 실제 AI 로그인·유료 추론·Google 추가 동의·실제 발송/삭제도 실행하지 않았다. 새 버전 최초 실행 시 schema25 이행 후 이전 코어로 되돌리지 않는 제약을 릴리스에 명시했다. **[사람 보안 검토 필요]** 상태는 유지한다.

## 2026-11-03 beta.5 설치 진행률·업데이트 명령·Google 연결 흐름 수정

사용자가 설치 퍼센트/막대, 계정 연결 이전의 기능 선택 제거, `update hantong` 명령과 수정·검증 후 배포를 요청했다. 설치기는 EN/KO 별도 원본 리소스와 검증된 embedded JSON으로 단계 진행률을 표시한다. 다운로드는 PowerShell 바이트 진행을 유지하며 압축 해제·파일 검증·복사는 실제 항목 수를 사용한다. bootstrap/package 진행 ID를 분리하고 모든 오류 경로를 정리하며 성공 시에만 100%를 표시한다. 진행 정보는 progress stream을 사용하고 호출자의 progress 비활성화 설정을 보존한다. 사용자 PATH·데이터 보존과 체크섬·경로·롤백 검증은 유지했다.

설치된 새 CLI의 `hantong update`와 소유된 `update.cmd`의 `update hantong`을 구현했다. 기존 업데이트 명령을 덮어쓰거나 PowerShell alias/profile을 바꾸지 않는다. 구버전은 새 설치를 한 번 거쳐야 명령이 추가된다. 현재 exe→versions/version/bin 구조·owner marker·활성 버전·release manifest·updater 항목과 hash를 검증하고 같은 설치 root의 패키지된 update.ps1만 실행한다. 개발/unmanaged 실행·JSON/no-input/리다이렉션·불필요한 인자는 효과 전에 거부한다. 구버전의 updater 없는 설치/제거도 보존한다. reparse/junction은 canonicalize 전에 공통 파일·부모 검증을 거치며 Windows PowerShell은 환경 SystemRoot/PATH 대신 OS FOLDERID_System으로 찾는다. Known Folder의 COM 초기화는 성공한 호출만 해제하고 기존 MTA를 유지한다. 이 API 조건은 Microsoft 공식 Known Folders/SHGetKnownFolderPath 문서로 확인했다.

기본/추가 계정의 Google 연결은 PersonalRead로 브라우저 계정 선택·로그인·동의에 바로 진입한다. 연결이 끝나야 작업 메뉴와 작성/보내기 활성화 4번을 보여 주며 자동 본문 조회·발송·쓰기 권한 요청은 없다. queued 상태의 준비 안내 중복도 제거했다. 같은 계정의 정확한 active google_oauth B@epoch2 또는 C@epoch2/3 재연결은 새 등록·키·scope row·epoch 변경 없는 no-op으로 처리한다. 다른 계정·출처·상태·혼합 scope·예상 밖 epoch와 credential CAS 충돌은 계속 거부한다. 사용자가 보인 실제 CONFLICT의 단일 원인은 로그/DB를 읽지 않아 확정하지 않았다. 읽기 전용 설치 CLI 진단은 인증 성공 및 Gmail ready 1개만 확인했고 주소/계정 ID/토큰은 출력하지 않았다. 실패를 이유로 계정/등록을 삭제하거나 권한을 자동 확대하지 않았다.

통합 과정에서 리소스 빈 줄의 빌드 오류, updater root 계산·manifest 필수 항목·shim 인자 위치 오류, 합성 시험 경로 경합/해시 이후 변조 fixture, Clippy 불필요 borrow를 수정했다. 기존 설치기 process override 금지 검사는 해당 process gate 블록에 계속 적용하고 progress 정적 JSON을 오인하지 않도록 범위를 맞췄다. import된 TestCase의 중복 수집도 제거했다. 관련 설치 27개, updater plan/junction 5개, Google exact-grant 재연결 1개와 Windows 경로/COM 2개가 통과했다. agent 읽기 검토의 junction/SystemRoot/COM/metadata reparse 지적을 반영했으며 이는 사람 보안 검토 증거가 아니다.

최종 `python -X utf8 scripts/check_all.py` **exit 0**: Rust **1,738개/31 suites/실패 0/ignored 0**, Docs **29개**, Build **105개**, 합계 **1,872개 실행(중첩 하네스 포함)**. fmt·전체 Clippy(-D warnings)·all-targets build·요건·문서 원본·아키텍처·추적표도 통과했다. 첫 통합 검사 이후 리뷰 보완이 있어 최종 검사를 재실행했으며 최종 근거는 `target/beta5-verification.json`, `target/beta5-full-check.log` SHA-256 `6a1dfadaaac9f9e35c185ed7bc36c66eaa8e66ee69949ba47acab94a68b3b0af`이다. source hash `ec15d7ef32c7a0161d25e54832467de321bb6f6f554fda637cf83d480a631e3d`. 기존 beta.3 Core를 검사 후 같은 경로/인자/hash로 복구했다(PID 41244, SHA-256 `710aeff80153953f8a2625ba041a0ff48f3f434f41c74efb50781c1397f0b80c`).

beta.5 ZIP 25,037,586 bytes의 SHA-256은 `0a04590ba47a2b0184394f3d162c7ff94e34546719b611a429eae1ab1d2cd26d`이다. manifest 261개 파일, updater 원문, 이전 승인 publisher 자산 동일성, 실제 DB·프로필 미포함을 검증하고 별도 경로 설치가 통과했다. 설치된 실제 CLI의 도움말/AI 안내/각 binary 6개 실행과 `update hantong`이 정확히 CLI의 headless guard까지 도달하는 것을 확인했다(24/HUMAN_STEP_REQUIRED). 잘못된 명령/추가 인자는 2로 거부했다. 공개 배포와 공개 설치/업데이트 검증 결과는 아래에 누적한다. 실제 기본 설치·DB·계정은 보존했고 추가 Google 동의·실제 메일/AI 작업은 실행하지 않았다. **[사람 보안 검토 필요]**.

### beta.5 공개 게시·격리 업데이트 검증

검증한 소스를 별도 worktree의 `39ab49981150e906210bdf5055e34772aa17befa`로 게시하고 GitHub main 및 `v0.1.0-beta.5` prerelease를 갱신했다. ZIP·SHA256SUMS·BUILD-METADATA.json을 공개했으며 공개 ZIP을 다시 내려받은 SHA-256이 로컬 검증본과 일치했다. 실제 사용자 설치를 바꾸지 않고 별도 beta.4 설치에 공개 bootstrap을 실행해 beta.5 업그레이드, 이전 버전 보존, 한국어 진행률과 두 단계 완료 표시를 확인했다. 영구 PATH와 실제 설치 current.txt는 바뀌지 않았다(`target/beta5-public-upgrade-verification.json`). 이는 대화형 `hantong update` 전체를 실제 사용자 설치에서 실행한 검증과는 구분한다.

홈페이지 gh-pages `0286d01cd824f33409418f2e0a83058d37793f47`는 beta.5와 설치/업데이트 명령·직접 Google 연결 흐름을 안내하며 Pages 배포 success를 확인했다. 원본 HEAD·index·미커밋/새 파일은 보존했다.

## 2026-11-03 다른 PC beta.4 Core 시작 실패 조사

사용자가 다른 PC의 `hantong`에서 CORE_UNAVAILABLE, `hantong-t`에서 일반 요청 실패를 보고했다. 이 PC의 기존 beta.3 Core 상태와 섞지 않는다. 원격 PC current.txt는 `0.1.0-beta.4`, 해당 Core를 직접 실행한 결과는 `INTERNAL_ERROR: The core could not start or continue safely.`이다. 전달 과정에서 한 번 경로 구분자가 빠진 명령은 실행 실패였으며 Join-Path 명령으로 실행 파일을 찾은 뒤의 INTERNAL_ERROR와 구분한다. 기존 `hantong --version`의 0.1.0은 모든 베타에 같아서 실제 배포판 판별에 부족했다.

패키지 빌드에만 릴리스 라벨을 주입해 CLI 버전 출력을 보완했다(개발 빌드는 Cargo 버전 유지). Core는 고정 시작 단계와 안정 오류 코드만 출력하고, CLI는 자식 stderr를 16 KiB 한도로 읽어 실패 시 허용된 정확한 진단 행만 전달한다. 원시 오류·경로·계정·메일·비밀값은 전달하거나 저장하지 않는다. 기존 준비 제한 시간과 자식 프로세스 정리는 유지한다. docs/contracts/cli-onboarding.md에 진단 경계를 기록했다.

합성 한글·공백 경로에 존재하지 않던 제품/profile 디렉터리부터 보호 생성→저장소→서버 키→동일 키 재시작을 검증하는 시험을 추가했다. 시작 관련 8개, CLI 진단 필터 1개 및 패키지 10개가 통과했다. 이 결과로 다른 PC 오류의 근본 원인을 확정하거나 해결 완료로 표시하지 않는다. 추가 OAuth 동의, 계정/키/DB 삭제·교체·ACL 완화는 수행하지 않는다. 전체 검사와 후속 진단 배포 결과를 아래에 누적한다. **[사람 보안 검토 필요]**.

### beta.6 진단 보완 최종 검사

검토 중 stderr 캡처 한도 도달 후 파이프를 닫을 수 있는 문제를 수정했다. 처음 16 KiB만 보관하며 이후에도 출력은 계속 비우고, 완전한 고정 진단 행을 발견하면 즉시 전달한다. 32 KiB 출력 전체 drain 및 한도 이후 행 미전달 회귀 시험을 추가했다. 숨김 프로세스의 Windows 신호 처리 시험도 추가했고, 새 시험의 must_use 경고를 수정한 뒤 최종 검사를 다시 수행했다. 중간 검사와 최종 검사의 근거를 구분한다.

최종 `python -X utf8 scripts/check_all.py` **exit 0**: Rust **1,743개/31 suites**, Docs **29개**, Build **105개**, 합계 **1,877개 실행**, 실패·ignored 0. fmt·Clippy(-D warnings)·workspace all-targets build·요건·원본·아키텍처·추적표도 통과했다. `target/beta6-verification.json`, 최종 로그 SHA-256 `a17eb0c3ef36ec78d7d52125d97a55bc69c940f3555c18277426350cf3f4254f`, source hash `d92b268f81681764d1bab9f05211e088c6fd820e994953135e88132a3969fffe`. 기존 beta.3 Core는 같은 경로·인자·해시로 복구했다(PID 46248).

추가로 `RUSTFLAGS=-C target-feature=+crt-static` 및 배포와 같은 release/x86_64-pc-windows-msvc 설정에서 Core 시작 관련 **9개 시험 통과**를 확인했다(`target/beta6-release-startup-tests.log`). 한글 경로의 실제 SQLCipher/DPAPI 저장소와 CNG 키 초기화·재시작, CREATE_NO_WINDOW 신호 처리가 이 PC의 합성 환경에서 통과했다. 다른 PC의 원인과 성공을 입증하는 자료로 확대 해석하지 않는다. beta.6는 진단 개선 배포이며 실제 시작 오류는 원격 단계 출력 확인이 남아 있다.

### beta.6 공개와 다른 PC 설치의 API 제한 확인

소스 `5229edb0cda456dfd3b490d556f07f7eb5296731`을 별도 release worktree에서 게시하고 GitHub main을 fast-forward했다. `v0.1.0-beta.6` prerelease의 ZIP·체크섬·빌드 정보 3개 자산을 모두 업로드 후 hash 대조하고 공개했다. 공개 ZIP 재다운로드 SHA-256 `82e7d1ad20c48f99db6ea6f694ea38d04c1a5172f6bf548e3084d2882affe012`, 크기 25,088,890 bytes가 로컬 검증본과 일치했다. manifest 261개 파일과 이전 승인 publisher 자산 동일성, 실제 DB/profile 미포함을 검증했다. 별도 영문 경로 및 한글·공백 경로 설치, 배포 CLI의 `hantong 0.1.0-beta.6` 출력과 도움말/업데이트 shim 검증이 통과했다. 실제 설치와 실제 DB는 교체하지 않았다.

다른 PC에서 pinned bootstrap 실행이 GitHub 익명 REST API rate limit으로 중단됐다는 새 증거를 받았다. beta.6 설치 전의 실패이므로 이어진 구버전 Core 오류를 beta.6 결과로 표시하지 않는다. API를 사용하지 않는 고정 beta.6 ZIP 다운로드→공개 검증 SHA-256 대조→해당 패키지 설치 명령을 안내했다. 사용자에게 토큰을 요청하거나 API를 반복 호출하지 않는다. GitHub 공식 [REST 오류 처리](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api)의 403/429 제한 설명을 확인했고, 공개 repository Atom feed에서 beta.6/5/4 태그를 직접 확인했다. bootstrap의 제한 대응을 계약에 명시하고 보완한다. Core 원인 확정은 다른 PC의 새 버전 단계 출력이 남아 있다. **[사람 보안 검토 필요]**.

### 원격 beta.6 저장소 접근 거부·기존 계정 보존 정정

직접 ZIP 설치 후 beta.6는 `hantong_startup stage=storage outcome=failed code=INTERNAL_ERROR`를 출력했다. `data`·`secrets`·`profiles` 폴더가 존재하고, 공개 읽기 전용 진단 `e592e2e622ca1886c42bfac9e9d727f69ad1fcc1` 실행 결과 product/profiles와 모든 상위 경로는 정상이나 data/secrets leaf의 READ_ATTRIBUTES|LIST_DIRECTORY, READ_ATTRIBUTES|TRAVERSE, FILE_ADD_FILE 열기가 모두 Win32 오류 **5(접근 거부)**였다. Get-Acl도 두 leaf에서 거부되며 profiles는 현재 사용자 소유·protected·3개 규칙이었다.

사용자는 **그 PC에서 한 계정 연결 및 메일 조회에 성공한 이력이 있음**을 확인했다. 이전 Get-Item의 고정 DB/key/lock 파일 `Exists=False`는 접근 거부를 숨긴 결과일 수 있으며 파일 부재 증거로 사용하지 않는다. 초기화·quarantine·기존 폴더/DB/키 삭제·새 저장소 대체를 하지 않는다. 해당 계정과 암호화 키 보존이 필수다. 원격 오류를 한글 경로나 조상 LIST 권한으로 확정하지 않는다.

별도로 조상 LIST 권한을 불필요하게 요청하는 결함을 통제된 ACL fixture에서 RED(InvalidRoot)/GREEN으로 재현·수정했고, reparse 거부 회귀를 유지했다. 새 저장소 leaf에 명시적 사용자 소유자·protected OICI allowlist ACL을 주는 생성 API를 추가하고 Core 시작에 연결한다. 기존 leaf는 출시 Secrets 경로의 type/reparse·속성 읽기·통과·삭제 공유 금지 검사만 유지하고 ACL을 읽거나 수정하거나 새 whitelist 검사를 적용하지 않는다. 기존 broad readable ACL 및 logon-session Read/Execute ACL의 바이트 보존과 empty thread DefaultDACL에서 신규 생성·DPAPI 재열기까지 합성 시험 4개가 통과했다. 기존 접근 불가 폴더는 계속 거부한다. 이는 없는 폴더의 생성 보완이며 이미 막힌 원격 기존 계정의 복구를 뜻하지 않는다. 기존 ACL 보존은 새 사람 보안 검토 완료가 아니다.

API 403/429 때만 공개 Atom feed와 HEAD 길이 확인으로 전환하는 bootstrap은 DTD/외부 entity·호스트/경로·크기 경계를 검증한다. 새 모의 5개 시험과 기존 bootstrap 5개가 통과했다. 실제 공개 feed/HEAD/ZIP을 사용하되 API 403만 모의한 별도 설치가 beta.6 다운로드·체크섬·설치까지 통과했고 실제 설치·PATH·계정은 바꾸지 않았다(`target/beta7-rate-fallback-verification.json`). **[사람 보안 검토 필요]**.

추가 읽기 전용 ACL 진단 `f1db951eb53eb7258947254956abdf6b86e4f209`을 게시하고 공개 원문 동일성을 검증했다. 원격 일반 및 사용자가 관리자 실행으로 보고한 결과 모두 data/secrets의 READ_CONTROL 열기 오류 5이며 product/profiles는 현재 사용자 소유·protected·3개 allow 규칙이었다. data/secrets 소유자·DACL은 아직 미확인이다. 강제 소유권 변경이나 ACL 교체를 하지 않고, 백업 권한을 일시 활성화하는 읽기 전용 진단을 준비한다. DB·키·기존 계정 복구 성공은 아직 주장하지 않는다.

백업 권한 진단은 `5afe974a2bbd962fb384faa471605d11183245e8`로 게시하고 공개 원문을 대조했다. 관리자 여부·SeBackupPrivilege 활성화/복원 오류와 고정 폴더의 소유자 분류·DACL 요약만 출력하며 DB/key 파일은 열지 않는다. 로컬 비관리자 실행은 exit 0, 활성화 오류 1300, 실제 상태 미변경, 정상 폴더 4개 조회를 확인했다. 원격 관리자 결과는 대기 중이다. 읽기 권한도 거부된 기존 저장소에 소유권 강제 변경·ACL 수리·새 DB 생성을 시도하지 않았다. 별도 읽기 코드 검토에서 신규 생성/legacy 보존 변경의 차단 사항은 없었으며 이는 사람 보안 검토 완료가 아니다.

원격 백업 권한 진단의 첫 결과는 IsElevated=false/1300이었다. RunAs로 사용자가 직접 UAC를 완료한 뒤 받은 결과는 IsElevated=true, 백업 권한 활성화/복원 오류 0이다. data/secrets는 **Administrators 소유·unprotected·3개 ACE·현재 사용자 직접 허용 mask 0·deny 없음·추가 trustee 있음**으로 확인됐다. product/profiles는 현재 사용자 소유·protected·3개 정상 allow 규칙이다. 관리자 기본 권한으로 생성된 상태가 남았을 가능성이 있으며, 추가 trustee와 하위 파일 상태는 아직 확인되지 않았다. 실제 소유권/ACL 변경을 하지 않았다.

### beta.7 통합 검증

최종 `python -X utf8 scripts/check_all.py` exit 0: Rust **1,749개/31 suites/실패 0/ignored 0**, Docs **29개**, Build **110개**, 합계 **1,888회 실행**. fmt·Clippy(-D warnings)·all-targets build/test·문서/요건/아키텍처/추적표가 통과했다. 최초 실행은 진단 PS1 추가 중 source hash 변경으로 추적표 stale에서 중단되어, 소스를 고정하고 추적표를 생성한 뒤 전체를 다시 실행했다. 최종 source hash `1f27fb7e29a232aab42b915f8a4c8503d552e2f2b1ceb66a85a3c27974ddb122`, 로그 SHA-256 `fa8b04fb9531ba1753e380af9752e731dfaba9707a359a5519f6628b7871a892`, 근거 `target/beta7-verification.json`. 기존 beta.3 Core를 같은 경로/인자/SHA-256으로 복원했다(PID 13660). 실제 계정/DB 이행은 하지 않았다. beta.7 패키징과 원격 기존 저장소 복구는 별도 진행 중이다. **[사람 보안 검토 필요]**.

### beta.7 공개·원격 ACL 복구 시도 후 검증 중단

소스 `5052591441a03efa6a75011359421afa967c2199`를 main에 fast-forward하고 `v0.1.0-beta.7` prerelease를 공개했다. 자산 3개 size/digest를 게시 전에 검증하고 공개 ZIP 재다운로드 SHA-256 `bc87a1faef1250c4e6a9dc9fc82ed314fc1421bee716379883e416efbe146b1d`, 25,098,588 bytes를 대조했다. manifest 261개 파일과 승인 beta.3 publisher 동일성, DB/profile 미포함을 확인했다. 별도 설치 후 도움말·AI 안내·바이너리 6개와 update shim headless 거부가 통과했다. 문서 29개도 재실행했다. 사이트 `a42e39978f3015cf743b2245f9d06ea2bad1320b` Pages 성공과 공개 HTTP의 beta.7·1,888·고정 설치 commit을 확인했다. 기존 설치/실제 DB는 교체하지 않았다.

원격 읽기 전용 트리 진단 `ba56da8496c2936b9eb2e0a2cb8cfbb39bec5e3c`는 9개 객체(폴더 2/파일 7) 모두 Administrators 소유, 사용자 FullControl 없음, deny/unknown/unsupported ACE 없음, logon-session ReadExecute만 존재함을 확인했다. 사용자가 이 9개에 사용자 권한을 추가하는 데이터 보존 복구를 명시 승인했다. 별도 진단 branch의 `6c16164348e4a9c67bf615324b82cb8f0d72c3e7` 스크립트는 실제 Core와 같은 share=0 기존 DB/key lock을 유지하고 owner/기존 ACE를 보존하며 ACL 메타데이터 백업을 먼저 생성한다. 합성 user-owned 9개 fixture에서 hash·owner 보존/계획 불일치 무변경/배타 lock/주입 실패 역순 원복이 통과했다. **실제 Administrators owner/상속 부모 조건의 Apply는 로컬에서 검증하지 못한 제약**이 있었다.

원격 Apply 결과 첫 객체 변경 후 검증과 원복 검증이 실패하여 `RECOVERY_REQUIRED`, BackupSaved=true, RollbackComplete=false, NodesChanged=1로 중단했다. DB/key 파일 내용·소유권 변경·삭제는 하지 않았다. 동일 명령을 반복하지 않도록 안내했다. 후속 읽기 전용 조회의 data/secrets owner/보호 상태/3 ACE/사용자 mask 0은 초기와 같지만 이것만으로 원복 완료를 주장하지 않는다. 부모 상속 처리나 ACL 정규화가 검증에 영향을 줬을 가능성을 조사 중이며 원인은 아직 확정하지 않는다. 보호된 ACL 백업과 현재 9개를 비교하는 읽기 전용 `dd8521b09fb2624eb8e0211b2cee1ecc7614db08` 감사를 게시하고 공개 동일성, 합성 백업 9개 비교를 검증했다. 실제 추가 Apply는 중단 중이고 기존 계정 복구 성공은 **미확인**이다. **[사람 보안 검토 필요]**.

### 원격 백업 감사 결과와 Group 조회 누락 수정

사용자가 보낸 ACL 감사 결과는 **9개 모두 object ID·owner·protection·ACL revision 2·기존 ACE 바이트가 정확히 일치**했다. 사용자 FA는 9개 모두 없으며 원래 접근 권한은 돌아온 상태다. 첫 3개는 GroupMatches=false/descriptor bytes 차이, 나머지 6개는 descriptor 전체도 일치했다. 상속 ACE 추가나 ACL revision 변경을 원인으로 확정했던 가설을 폐기한다. 실제 코드는 `GetSecurityInfo(OWNER|DACL)`만 요청한 스냅샷에서 요청하지 않은 Group까지 비교했다. Microsoft 공식 GetSecurityInfo 문서상 Group 유효성은 GROUP_SECURITY_INFORMATION 요청을 필요로 한다.

v3 별도 복구 스크립트는 새 스냅샷마다 OWNER|GROUP|DACL을 명시적으로 요청해 primary group까지 저장·검증한다. 기존 보호 백업은 요청됐던 object ID/owner/protection/ACL revision/정확한 ACE 바이트와 9개 대상 수를 검증해 결속하고, 구형 스냅샷의 미요청 Group은 비교 근거로 사용하지 않는다. 새 쓰기 전후에는 실제 primary group 비교를 계속 유지한다. OS가 관리하는 defaulted/auto-inherited bookkeeping flags를 전체 descriptor 바이트 동일성으로 오인하지 않으며 DACL present/nonNULL·보호 상태·ACE의 SID/mask/flags는 검증한다.

`target/test_storage_repair_resume.ps1` 합성 시험 통과: 구형 백업 첫 3개 Group 누락/기존 ACE 9개 동일 조건 재현, 보호 백업에 결속한 재적용 9개 성공, 명시 조회한 primary group 9개 보존, 원본 DACL 불일치 재시도 무변경 거부. 기존 파일 hash/owner 보존·wrong plan 무변경·share=0 lock 차단·주입 실패 원복 시험도 통과했다. 호스트 비관리자 제약상 Administrators 소유 Apply는 여전히 실제로 실행하지 못했고 시험 전용 클래스에서 user-owned 9개 fixture를 사용했다. 제품 코드·설치·실제 DB는 변경하지 않았다. 실제 재적용은 아직 **미실행**이며 기존 9개 권한 복구 승인 범위를 유지한다. **[사람 보안 검토 필요]**.

### v3 원격 재실패 후 관리자 Windows 독립 재현·v4 검증

사용자 PC v3 적용은 `REPAIR_FAILED_ROLLED_BACK`, RollbackComplete=true, NodesChanged=0으로 실패했다. 원래 권한으로 돌아갔음을 스크립트가 검증했으나 사용자 FA 추가는 되지 않았다. 사용자가 반복 명령에 불만을 표시하여 추가 사용자 PC 적용/진단 요청을 중단하고, 에이전트가 소유한 별도 GitHub Actions Windows VM에서 재현했다. 기존 9개 복구 승인만 유지하며 새로운 권한 범위나 실제 메일 작업은 추가하지 않았다.

격리 branch `diagnostics/admin-storage-20261103`는 실제 관리자 토큰·SeBackupPrivilege·SeRestorePrivilege를 사용하는 Windows 2022/2025 VM에서 RUNNER_TEMP의 합성 객체만 만든다. 실제 LocalAppData mail-hantong 경로나 계정/DB/key에는 접근하지 않는다. 이 branch만 전체 제품 CI를 분리했으며 main의 workflow는 변경하지 않았다. `41575302278`에서 두 OS의 3개 primary-group 조건 모두 **첫 변경 뒤 ACL_CHANGED**, owner/group/protection/ACE는 모두 일치, 역순 원복 완료로 사용자 v3 실패를 재현했다. 원인은 쓰기 전 CAS가 여전히 전체 descriptor 바이트를 비교하여 OS가 바꾼 저장 표현/bookkeeping을 실제 권한 충돌로 오인한 것이다.

v4는 쓰기 전 두 CAS를 owner·명시 조회 GROUP·protected·전체 DACL bytes/revision/순서/SID/mask/ACE flags 비교로 통일했다. 임의 ACL 변경을 허용하거나 검증을 생략하지 않는다. 객체 핸들/ID·기존 보호 백업·배타 lock·실패 원복은 유지하며 실패 원인을 고정 코드로 남긴다. 새 보안 API나 종속성을 추가하지 않았다.

GitHub Actions [관리자 Windows 시험 41575717730](https://github.com/tz1012/mail-hantong/actions/runs/41575717730), commit `c8bcbc7e1529a4da7bcdebef30ea8b548a0ef90c`에서 **2개 OS × 9개 조건 = 18개 관리자 사례 전부 통과**했다. 각 OS는 primary-group 3종 × 정상 9개 적용/3개 변경 후 주입 실패 원복/실제 v3 실패 백업에 Group 누락을 재현한 뒤 v4 재개 적용을 검사한다. 합성 파일 SHA-256·owner·실제 primary group 보존과 실패 원복을 확인했다. 이전 12-case 관리자 시험도 통과했다. 근거 `target/admin-storage-ci-verification.json`, 전체 로그 SHA-256 `06afb447e45e13e8fed4353c77faa2584e08f5ab1ef6fd008ebcbb0787ec8e70`.

독립 정적 검토에서 추가 차단 요소는 없었다. 검증한 v4를 별도 진단 branch commit `0cde4303a8e00574191356b89fe31f699e46a3bf`로 게시하고 공개 원문의 LF SHA-256 `5df920ea3700729a0af9ca7afa53ef6a6200d912b0f7650b16758cad0e6d9991`이 CI 검증 소스와 동일함을 확인했다. beta.7 패키지/태그는 변경하지 않았다. 사용자 PC v4 적용과 일반 권한 Core 기동·기존 계정 이용은 아직 **미확인**이며 실제 추가 Apply는 수행하지 않았다. **[사람 보안 검토 필요]**.

사용자가 이후 일반 PowerShell의 `hantong_startup stage=storage outcome=failed code=INTERNAL_ERROR`를 보고했다. v4 복구 JSON은 아직 제공되지 않았으므로 v4 적용 성공·실패·미실행 중 어느 상태인지 추정하지 않는다. `Succeeded` 값 또는 직전 JSON만 확인 요청했으며 추가 진단·권한 변경·DB/key 초기화는 요청/실행하지 않았다. v4 성공 여부에 따라 기존 ACL 문제와 후속 저장소 오류를 구분한다.

### 사용자 PC v4 권한 복구 성공 확인

사용자가 직전 실패 출력이 v4가 아니라 v3였음을 확인할 수 있는 JSON을 제공했다. 이후 실제 v4 적용 결과는 **Succeeded=true, ErrorCode=OK, Applied=true, NodesChanged=9, BackupSaved=true**, 백업·복원 privilege 활성화/원복 오류 모두 0이다. 이전 승인 범위의 기존 폴더 2개/파일 7개에 사용자 권한을 추가하고 검증한 실제 결과이며 DB/key 내용·owner 변경이나 초기화는 하지 않았다. 결과의 MissingUserFullControlNodes=9/AdminOwners=9는 변경 전 계획 집계값이며, Applied/NodesChanged와 쓰기 후 검증이 실제 적용 성공을 나타낸다. ACL 백업은 사용자 PC에 보관됐다. 일반 권한 `hantong` 실행 및 기존 계정 표시·이용은 아직 확인되지 않았으므로 전체 계정 복구 완료로 확대 표시하지 않는다. 추가 권한 복구 명령을 반복하지 않고 일반 PowerShell 실행만 확인한다. **[사람 보안 검토 필요]**.

### 2026-11-03 추가 Google 읽기 계정 연결 충돌 수정

사용자 일반 PowerShell에서 Core 기동과 기존 계정 표시가 성공했다. 기존 저장소 접근 복구는 이 범위까지 확인됐으며 실제 메일 조회·발송 성공으로 확대하지 않는다. 이후 다른 Google 계정을 추가하면 OAuth 동의 다음 `CONFLICT`가 발생했다. 개발 PC의 beta.7 설치·실행 화면에서도 같은 오류를 확인했다. 실제 주소·콜백·자격증명은 진행 기록과 시험에 넣지 않는다.

원인은 Google 계정 저장 후 기본 CLI의 단일 계정 OAuth 등록 검사가 새 계정을 거부하는 순서다. 기존 정확한 본문 읽기 범위에서 다른 계정의 PersonalRead OAuth 연결은 기존 등록·키·epoch·권한을 그대로 두고 마친다. CLI는 기존 인증된 readonly probe와 `AddGoogleReadOnly` 터미널 기능 선택을 이어서 수행한다. OAuth만으로 새 계정의 메일 도구 권한을 자동 확장하지 않는다. 쓰기·발송·혼합 범위는 이 예외에 포함하지 않는다. 기존 다계정 읽기 범위의 재연결도 권한을 재발급하지 않는다.

변경: Core 등록 분기/회귀·실제 합성 Store 통합 시험, CLI 연결 후 handoff/시험, 기존 계약의 제한된 예외 명시. Google 응답 HTML의 폐기된 Hello 안내를 터미널로 돌아가는 문구로 바로잡았다. 실제 Google 로그인·동의는 사용자 단계로 유지한다. Core `acc02_second` 2개와 CLI bin 116개 시험이 통과했다. CLI 추가 시험의 Lang 비교 컴파일 오류를 matches!로 수정한 후 통과한 결과다. 전체 통합 검사·배포는 다음 검증 단계이며 실제 추가 계정 재시험 성공은 아직 미확인이다. **[사람 보안 검토 필요]**.

### 실제 C 범위 확인과 사용자 요구 정정 반영

`agent-desktop` 1.1.0의 공식 내장 desktop/platform 지침을 읽고 열린 WindowsTerminal 창과 추가 계정 오류 화면을 직접 확인했다. 기존 창 제어 도구가 놓친 창이 이 도구에서는 관찰됐다. 보호된 기본 프로필의 상호 인증 API로 계정 수·고정 도구 집합 여부만 확인한 결과 실제 설치는 단일 계정 C(읽기·작성·발송)였다. 주소·비밀값·메일·DB 내용을 진단하거나 출력하지 않았다. 따라서 초기 B 전용 수정만으로 실제 오류가 해결됐다고 표시하지 않는다.

사용자는 새 계정도 발송 가능해야 하며 계정 수와 Gmail/일반 메일 종류에 관계없이 조회·읽기·작성·발송·휴지통을 이용해야 한다고 정정했다. 읽기 전용 새 계정과 기존 C를 섞는다는 초기 가정을 폐기했다. 우선 Gmail 연결은 PersonalReadSend와 실제 Google ReadSend 동의를 사용하고, OAuth 직후 기존 C는 그대로 둔 채 보호된 현재 계정 목록과 이번 선택 계정만 합친 `RequestGoogleSend` 터미널 기능 선택으로 이어진다. Core는 선택 계정 전부의 Send capability와 기존 계정·도구 보존을 확인한다. OAuth만으로 계정 grant나 발송을 만들지 않는다. 발송은 매 요청 단일 계정·불변 preview·명시 확인·단일 claim·불확실 결과 무재전송을 유지한다.

임의 4계정 제한은 공통 RequestedScope의 128계정 자원 한도로 맞췄다. 고정 128 배열로 커진 enum은 Clippy가 검출해 bounded Vec로 수정했다. 일반 CLI 발송과 guided compose는 다계정 C probe를 공유하고, Store의 개인 발송 분류도 선택 계정이 명시 범위에 속하는지 확인한다. 중간 보안 검토에서 resume 흐름이 PersonalReadSend를 잃는 문제를 찾아 receipt의 Core 소유 local_profile routing metadata를 추가했다. 이것을 권한 증거로 사용하지 않는다. 실제 Store scope 전이로 과거 epoch 요청 무효화·새 epoch 두 계정 독립 발송 claim을 시험했다.

현재까지 Core 계정 추가 4개, CLI 117개, scope wire 9개, Store 개인 발송 4개와 targeted Clippy가 통과했다(뒤의 receipt 추가 전 결과는 최종 전체 검사와 구분). 전체 검사는 두 번 추적표 stale로 중단됐고 기존 beta.7 런타임은 각 시도 후 같은 경로·인자·해시로 복원했다. source가 안정된 후 추적표 갱신과 전체 검사를 다시 실행한다. 실제 추가 계정 동의 후 성공·새 패키지 배포는 아직 미확인이다.

일반 IMAP/SMTP와 휴지통은 별도 미완료 범위다. 기존 IMAP 조회/본문, SMTP 발송, 터미널 설정 경로는 있으나 기본 메뉴·guided compose는 Gmail 중심이며 일반 IMAP 휴지통 실행이 없다. Gmail 휴지통도 Modify와 별도 도구 grant가 필요하다. 사용자 최신 요구를 통합 기능의 완료 기준으로 유지하고 단순 parser/stub 또는 Gmail 일부 수정만으로 전체 완료라 표시하지 않는다. IMAP 휴지통은 RFC 9051/6851의 UID MOVE와 계정+mailbox+UIDVALIDITY+UID 결속, 명시 Trash 선택, 불확실 결과 무재시도, EXPUNGE/영구 삭제 폴백 금지를 기준으로 계약·구현을 이어간다. **[사람 보안 검토 필요]**.

### 2026-11-03 다계정 공통 관리 기능 통합 중

최신 사용자 요구는 Gmail과 일반 IMAP/SMTP의 모든 추가 계정에 조회·본문·작성·발송·휴지통 이동을 제공하는 것이다. beta.8 계정 추가 수정 검증과 공통 기능 개발은 별도 worktree로 분리했다. 원본 HEAD/index와 기존 미커밋·새 파일 및 실제 계정 저장소를 보존한다.

공통 작업 사본에 명시적 ManageAccounts(1..128개, 합계 ID 12,288 bytes), RequestManage IPC, Core의 기존 계정·도구 보존 전이와 Google Modify/일반 SMTP 설정 선검사를 연결했다. fresh Google permit의 혼합 공급자 연결은 grant를 변경하지 않고 터미널 기능 선택으로 넘긴다. 관리 profile 및 schema26/IMAP 휴지통 ledger/Secrets UID MOVE 경로는 통합 중이다. IMAP은 검증된 wire mailbox 인코딩을 사용하고 MOVE 전 실패와 전송 후 불확실 결과를 구분하며, 전송 후 NO/단절은 Unknown으로 보존하여 자동 반복하지 않는다.

현재 검증: 공통 변경의 대상 rustfmt 및 diff check만 확인했다. 공통 Cargo·합성 서버·Windows migration·CLI/TUI 전체 이용 시험은 아직 미실행이다. beta.8 최종 fullcheck는 기존 바이너리 자동 복원 wrapper 아래 OpenSSL 재빌드 중이며 완료로 기록하지 않는다. 소스 해시의 LF/CRLF 차이를 scripts/traceability.py의 canonical LF v2로 해결했고 raw tests/Corpus bytes는 보존한다. fresh Git archive와 원본의 source hash e6009aca2acdb04eb9efa733a069ed3e4ea7d9d016bbbfa1ce2772355fcf7619 일치를 확인했다. 새 패키지 재생성·공개는 최종 검증 이후 진행한다. 실제 계정 동의·메일 이동/발송은 자동 수행하지 않았다. [사람 보안 검토 필요].

### beta.8 최종 전체 검사와 재현 가능한 패키지

`python -X utf8 scripts/check_all.py` 최종 exit 0: Rust 1,763개/31 suites, Docs 29개, Build 113개, 합계 1,905개. 실패·ignored 0. fmt·Clippy(-D warnings)·전체 build/test·요건/아키텍처/추적표 검사가 통과했다. source hash e6009aca2acdb04eb9efa733a069ed3e4ea7d9d016bbbfa1ce2772355fcf7619, 전체 로그 SHA-256 1c520f5ba1db69803d6d0d8d182c37ea0f53a4151059afa110367e6a526c2ce7, 근거 target/beta8-verification.json. 기존 설치 beta.7 Core를 같은 경로·인자·실행 파일 SHA로 복원했다(PID 27348). 검증 중 실제 계정·메일·키 내용을 읽거나 변경하지 않았다.

새 source hash 기준 beta.8 release build와 패키지를 재생성했다. ZIP SHA-256 cb5e9c94b5d3168da4325004c48f551e8cc51cfd501e33c0e6ed56eb04b6a667, 25,143,465 bytes, manifest 파일 261개를 검증했다. 승인된 beta.3 publisher 자산과 동일하며 계정 DB/profile은 포함하지 않는다. 최초 미게시 패키지는 target/beta8-initial-package에 보존했다. 별도 설치·스모크·공개 배포는 이어서 검증한다. 일반 IMAP/SMTP 관리·휴지통 확장(schema26)은 별도 공통 worktree이며 이 패키지에 포함하지 않았다. [사람 보안 검토 필요].

### beta.8 공개·안내 페이지·현재 PC 설치 확인

소스 f351d17948b283affa8c304ce3028982314833b2를 main에 게시하고 v0.1.0-beta.8 prerelease를 공개했다. 업로드한 ZIP/BUILD-METADATA/SHA256SUMS 세 자산의 size/digest를 검증한 후 공개했고, 공개 ZIP 재다운로드 SHA-256 cb5e9c94b5d3168da4325004c48f551e8cc51cfd501e33c0e6ed56eb04b6a667과 25,143,465 bytes를 다시 확인했다. 별도 설치 경로의 도움말·AI 도움말·공급자 목록(추론 없음)·바이너리 도움말/버전·update shim headless guard가 통과했다.

안내 페이지 gh-pages ce4b684680d79454a05d14c7afc3d79ab52f2d36은 Pages built 및 공개 HTTP에서 beta.8/1,905/고정 installer 소스를 확인했다. 새 Google 연결을 읽기 전용으로 설명하던 문구를 읽기·작성·발송으로 수정하고, 일반 IMAP/SMTP 기본 메뉴·휴지통 확장은 이번 ZIP에 포함되지 않았음을 분리했다.

현재 PC의 설치본을 beta.7에서 beta.8로 갱신하고 manifest가 검증한 새 Core가 실행 중임을 확인했다(PID 64632). 기존 버전·계정 DB·보호 키를 삭제하지 않았고 명시적인 계정/메일 작업이나 실제 Google 동의는 수행하지 않았다. 근거 target/beta8-current-install-result.json. 원본 HEAD 2852a31d0d8383d3a593a6479c20bb798b0d798e/index는 보존한다.

일반 메일 확장 공통 worktree는 모든 workspace/all-targets Cargo check를 통과했다. 이후 실제 Store 회귀 시험에서 관리 scope hash11이 잘못된 decision_channel로 저장되는 결함을 발견해 terminal_cli로 수정하고 재검증 중이다. 컴파일 통과와 기능 완료를 동일하게 기록하지 않는다. [사람 보안 검토 필요].

### 업데이트 실제 오류 확인·재현 검사 추가 진행

사용자가 띄워 둔 Windows Terminal 화면에서 `update hantong` 실행 후 package-common.ps1의 Get-ChildPath/Join-Path에 기준 경로가 전달되지 않는 오류와 뒤이은 CORE_UNAVAILABLE 출력을 확인했다. 화면에는 기존 계정 정보가 있어 캡처는 target 아래에만 두고 공개하지 않는다. 기존 직접 설치/도움말 스모크와 bootstrap 내부 함수→ScriptBlock→공통 함수 호출의 실제 업데이트 경로를 구분해 합성 패키지로 재현 중이다. 실제 계정·메일·키 내용은 열지 않았다.

CLI의 PowerShell 실행/종료 실패는 기존 INTERNAL_ERROR 코드와 한국어·영어 업데이트 전용 안내로 구분하도록 수정했다. 설치본 manifest/소유권 검증 오류 경계는 유지했다. 설치 경로 전달 결함과 회귀 검사는 검증 중이며 아직 수정 완료·배포 완료로 표시하지 않는다. 기존 beta.8과 공통 메일 schema26 확장은 분리하며 원본 HEAD/index·미커밋·새 파일을 보존한다.

Assert-FileRecords가 주변 `$script:InstallBase`에 의존하던 경계를 명시적 Base 인자로 변경했다. 설치기의 두 호출은 `$root`, Read-InstallState는 이미 검증 중인 `$Root`를 전달한다. 중첩 설치/재실행과 ambient 변수가 없는 상태 조회 회귀 시험을 추가했다. Windows 설치 시험 19개 통과를 확인했다. 실제 현재 bootstrap 경로는 기존 코드에서도 성공해 원래 화면의 경로 값 소실 원인을 완전히 재현한 것은 아니다. ambient 값을 제거한 합성 조건에서 같은 Get-ChildPath 오류 사슬을 재현하고 해당 의존성을 제거한 것으로 구분한다. 전체 검사와 공개 패키지의 실제 업데이트 확인은 진행 중이다.

별도 자동 코드 검토에서 새 한국어 오류 안내의 인코딩 손상을 발견해 UTF-8로 수정하고 원본·릴리스 작업 사본·공통 작업 사본에 반영했다. 검토 결과를 사람 보안 검토 완료로 취급하지 않는다.

### beta.9 업데이트 수정본 전체 검사 통과

`scripts/check_all.py` exit 0: Rust 1,763개/31 suites, Docs 29개, Build 114개, 합계 1,906개. 실패·ignored 0이며 fmt·Clippy(-D warnings)·전체 build/test·추적표·아키텍처 검사가 통과했다. source hash `7af056c36a952792933283676ebc1e3556a1820590e7fa11e7f54e41eedcf4c9`, 로그 SHA-256 `ecbc811994863a68623b547e8359e4fd373d292c695a733dbbe426c776c5d2ac`, 근거 `target/beta9-verification.json`. 검사 동안 잠시 중지한 기존 beta.8 Core는 같은 실행 파일 SHA·경로·인자로 복원했다(PID 48244). 실제 계정/메일 작업과 폴더 권한 변경은 수행하지 않았다. 패키지 생성과 공개 후 실제 업데이트 확인을 이어간다. [사람 보안 검토 필요].

### 실제 업데이트 실패 재현 — Windows verbatim 경로 경계 확인

beta.9 공개 자산의 hash/size와 별도 설치 스모크까지 통과했으나, 설치된 beta.8의 실제 `update hantong`은 같은 Join-Path 오류로 실패했다. 설치 포인터는 beta.8, 기존 Core는 PID 48244로 유지됐다. 따라서 beta.9를 다시 draft로 전환하고 홈페이지 beta.9 안내 변경도 되돌렸다. 소스 커밋·검사·패키지는 보존했으며 성공 배포로 최종 보고하지 않는다.

추가 최소 재현에서 `Join-Path '\\?\C:\Users\cyary\AppData\Local\Programs\mail-hantong-cli' 'bin\hantong.exe'`가 Windows PowerShell 5.1의 동일한 PSArgumentNullException(인자 drive)을 발생시켰다. Base 값이 null이라는 앞선 추론은 잘못됐다. Rust canonicalize가 만든 verbatim Windows 경로를 PowerShell provider가 해석하지 못하는 경계가 실제 원인이다. 정상 C: 경로를 사용하던 기존 설치 시험이 이를 놓쳤다. canonical 검증은 유지하고 PowerShell 인자 경계에서만 동등한 일반 Windows 경로로 변환하며, 해당 실제 PowerShell 회귀 시험을 추가한다. 기존 beta.8 런처 자체에 이 버그가 있어 이 PC에는 검증된 새 패키지를 정상 경로로 직접 설치한 뒤 새 버전의 실제 update 명령을 다시 검사한다. 실제 계정·메일·키·ACL은 변경하지 않는다.

### 사용자 정정 — 변경 영향에 맞는 검사만 수행

사용자가 작은 수정마다 전체 1,906개 검사를 반복하는 것은 불필요하다고 지적했다. 이후 검증은 변경 영향에 해당하는 회귀 시험·컴파일/정적 검사·실제 이용 흐름을 우선하며, 큰 기능 묶음 통합 시 전체 검사를 모아서 수행한다. 이번 Windows 경로 수정은 직전 전체 검사 근거를 보존하고 업데이트 경로 회귀·CLI 검사·패키지/실제 update 실행으로 추가 검증하며 전체 검사를 다시 실행하지 않는다. 과거 전체 검사 수를 새 소스 전체 검사 결과로 재표시하지 않는다.

### beta.10 경로 수정의 대상 검증

PowerShell에 전달하는 -File/-InstallRoot 인자에만 verbatim DOS/UNC→일반 Windows 경로 변환을 적용했다. 원본 canonical 경로의 manifest·런처·파일 검증은 유지한다. 변환한 updater와 root를 다시 canonical 검증해 원래 경로와 정확히 일치할 때만 실행하며, trailing dot/space·예약 DOS 이름·ADS·wildcard·제어 문자·device/GLOBALROOT를 거부한다. 자동 재검토에서 경로 별칭 우회 지적이 해소됨을 확인했다. 사람 보안 검토와는 구분한다.

업데이트 시험 10개 통과: Unicode DOS/UNC, namespace/별칭 거부, 경쟁 normalized updater 무실행, 실제 Windows PowerShell 5.1 Join-Path, 기존 manifest/current/변조·junction·headless 경계를 포함한다. 대상 CLI Clippy(-D warnings) exit 0, 해당 파일 rustfmt 통과. 근거 target/beta10-update-tests.log와 beta10-cli-clippy.log. source hash `fc97eec5cecdcbb329bc9175a0e71b0d1033d41a214e51f36f7e58121d29d8d3`; 전체 검사는 반복하지 않았다. 패키지와 실제 설치/업데이트 확인은 이어서 진행한다.

beta.10 ZIP 공개 재다운로드 SHA-256 `c254b3aaece3e45f424f3e2fce3e373e7eb58f585cb8cabb28f2c3660cf4d673`, 25,150,655 bytes와 source hash 일치를 확인했다. 직접 설치는 beta.8→beta.10으로 성공하고 Core가 실행됐다. 이후 실제 `update hantong`은 경로 오류 없이 exit 0이었지만 beta.8을 선택해 설치 포인터가 내려갔다. 이를 업데이트 성공으로 표시하지 않는다. GitHub releases 응답에서 beta.10이 beta.8 뒤에 있어 첫 호환 항목을 최신으로 간주한 선택 결함이 드러났다. 숫자 prerelease를 포함한 SemVer 비교와 latest 자동 하향 방지를 별도 대상 설치 시험으로 수정한다. 기존 Core는 같은 검증된 실행 파일로 복원했다(PID 64340). 계정·메일·키·ACL 변경은 없다.

### beta.11 릴리스 선택·자동 하향 방지 대상 검증

REST/Atom 응답 순서 대신 유효한 호환 패키지의 버전을 비교한다. core 숫자·prerelease 숫자를 overflow 없이 비교하며 stable이 같은 core의 prerelease보다 우선한다. 기존 제품의 preview→beta 전환은 같은 core에서 beta를 우선하는 명시적 호환 규칙으로 유지한다. 그렇지 않으면 과거 preview.4가 beta.11보다 높게 해석되는 이력 문제도 함께 막는다. 정확한 버전 요청은 해당 tag만 선택하고, latest가 검증된 owned current보다 낮으면 다운로드·설치 전에 거부한다. marker/current와 root 조상의 reparse point, marker 정확한 내용·크기, current 크기·형식을 확인한 뒤에만 읽는다. 기존 다운로드 URL·압축 경로·manifest/hash 검사는 유지한다.

새 릴리스 선택 합성 시험 9개 통과(15.657초). 기존 API 제한/Atom fallback 시험도 5개 통과했고 이후 변경 없는 해당 경로는 반복하지 않았다. PowerShell parse와 diff check 통과, 좁은 자동 검토에서 구체적 차단 사항 없음. Rust 경로 변경은 앞서 통과한 10개 시험과 CLI Clippy 근거를 재사용하며 전체 검사는 재실행하지 않는다. 계약은 cli-update.md에 선택/하향 방지 규칙을 보충했다. 새 패키지·실제 update의 최종 버전 확인을 이어간다. [사람 보안 검토 필요].

공통 메일 별도 작업 사본의 IMAP 인증→SELECT→단일 UID MOVE 합성 종단 시험 4개가 새 독립 Cargo target에서 통과했다. 실제 서버·계정은 사용하지 않았다. READ-WRITE 응답의 scanner/typed decoder 불일치를 수정했으며, target을 원본 릴리스 검증과 분리해 작업 사본 간 오래된 artifact 혼입을 피한다. 공통 전체 기능 완료나 배포로 표시하지 않는다.


### 2026-11-03 사용자 PowerShell에서 실제 업데이트 완료 확인

사용자가 열어 둔 WindowsTerminal의 기존 Windows PowerShell(w-5642630, 빈 PS C:\> 프롬프트)에 agent-desktop으로 `update hantong`을 직접 입력하고 한 번 실행했다. 다운로드·압축 해제·기존 설치 검증 막대를 관찰했고, Installed mail-hantong CLI 0.1.0-beta.11 완료 및 프롬프트 복귀를 확인했다. 같은 창에서 `$LASTEXITCODE; hantong --version`을 실행한 출력은 `0`, `hantong 0.1.0-beta.11`이다. current.txt도 beta.11이며 기존 beta.11 Core PID 53416이 유지됐다. 이번 실행에 Join-Path 오류·구버전 하향은 없었다. 근거: target/beta11-user-terminal-verified.png, target/beta11-verification.json. 앞서 직접 설치로 이미 beta.11인 상태에서 실행한 최신 버전 선택/동일 버전 재설치 검증이며, 이를 구버전 런처에서의 직접 업그레이드 성공으로 확대하지 않는다.

사용자 기존 계정·메일·키·저장소 권한은 이 시험에서 읽거나 변경하지 않았다. 전체 검사는 반복하지 않았다. 설치 준비가 수 분 걸리며 과거 설치 버전 전체 파일을 검사하는 구간의 진행 안내가 부족한 점은 후속 개선 사항이다. 별도 common-mail 작업 사본은 테스트 fixture의 모듈 경로·move 오류를 최소 수정한 뒤 전용 target에서 workspace all-targets check가 29.37초에 통과했다(target/common-mail-check-isolated-complete.log). 이는 컴파일 검사이며 기능 시험/출시 완료는 아니다. 일반 IMAP/SMTP·휴지통 확장은 미출시이며 사람 보안 검토 경계는 유지한다.


### 2026-11-03 일반 메일 통합과 SMTP 발신 주소 분리

별도 common-mail 작업 사본에서 일반 SMTP 로그인 아이디와 발신 주소를 분리했다. sender_address가 없는 기존 endpoint의 직렬화와 hash는 유지하고, 명시한 주소는 계약·IPC·Core preview·Secrets credential 결속·MIME From·MAIL FROM에 반영한다. schema26에 선택적 SMTP 발신 주소 테이블을 추가했고, frozen 구버전 validator가 없는 테이블을 조회하던 문제를 테이블 존재 확인 후 legacy fallback으로 수정했다.

IMAP 휴지통은 Store audit begin과 claim/outbox를 하나의 transaction에 묶고 Core에서 새 원자 API를 사용한다. 복원 payload hash 재검사와 다른 등록의 동일 계정 operation/idempotency 충돌 거부를 보강했다. Core의 요청→터미널 Get/Confirm/취소→실행 future 전이 시험을 추가했다. 사용자가 선택한 기존 Google 계정의 개별 기능 활성화는 나중에 통합 범위 확인을 취소해도 유지될 수 있다는 안내도 정확하게 고쳤다.

현재 대상 검증: v25→v26 migration 1개, Store mail_connection 14개, domain mail_connection 10개, Secrets SMTP 21개, CLI 휴지통 5개 및 SMTP 설정/일반 작성 회귀가 통과했다. Store 휴지통 추가 시험은 두 번째 기본 CLI 등록을 만드는 fixture가 기존 singleton 제약에 걸려 수정 중이며 전체 통과로 표시하지 않는다. Core 새 전이 시험은 실행 중이다. 전체 검사를 작은 수정마다 반복하지 않는다. source 통합·배포·실제 한통 계정 연결은 아직 완료 전이다. [사람 보안 검토 필요].

사용자는 Daum/Kakao 계정의 실제 설정 확인·메일 불러오기·지정 Gmail 수신자로 시험 발송을 허용했다. Aside 저장 로그인으로 접속해 IMAP/SMTP가 이미 켜져 있고 로그인 이름과 스마트워크 발신 주소가 다른 것을 읽기 전용 확인했다. 비밀번호/앱 비밀번호는 읽거나 복사·출력하지 않았고, 메일 삭제·실제 발송은 아직 하지 않았다. 실제 계정 정보와 메일 내용은 코드·공개 진행 기록에 넣지 않는다. 앱 비밀번호가 필요하면 한통 Secrets 보호 입력에서 사용자 직접 입력으로 이어간다.

### 2026-11-03 공통 메일 원본 통합과 통합 검사 후속 수정

common-mail 작업 사본의 변경 86개를 원본의 기존 미커밋 변경과 3-way 비교해 충돌 없이 통합했다. 이전 바이트는 target/common-integration-preserved-20261103에 보존했고 원본 HEAD/index는 유지했다. root의 Store 휴지통 12개와 Core 휴지통 전이 5개가 통과했다. 전체 workspace build와 Clippy(-D warnings), 문서 29개 및 아키텍처 검사가 통과했다. 통합 검사에서 발견한 실패는 해당 범위만 재실행하며 전체 검사를 처음부터 반복하지 않는다.

설치 시험의 실제 설치 폴더 의존을 격리했고 해당 2개가 통과했다. Contracts의 잘못된 경계값·모드 변경·구버전 도구 집합 가정은 수정 후 83개 통과, IPC client의 아직 부여하지 않은 계정 범위 가정은 수정 후 167개 통과했다. MCP 시험은 이미 생성된 결속 필드를 다시 요청에 넣어 권한 거부보다 먼저 형식 오류가 나던 fixture를 원본 요청 기반으로 수정했다. 생성 필드 재입력 거부도 따로 검증하며 MCP 14개 통과했다.

IMAP 시험 서버 fixture에서 빈 응답을 연결 종료로 바꿔 기존 timeout 시험이 깨진 회귀는 명시적 disconnect marker로 분리했다. Secrets IMAP probe 117개가 통과했고 실제 서버/계정은 사용하지 않았다. schema26 추가 테이블 3개를 반영하지 못한 현재 스키마 수와 구버전 복원 fixture도 수정했다. terminal manage(hash11) 생성 경로가 허용하는 기존 approval_helper 등록을 재로드 validator도 동일하게 허용하도록 맞췄으며, OAuth 전용 hash12의 출처 제한은 유지했다. Store 관련 4개와 Core scope-refresh 전이 시험 및 독립 코드 검토는 진행 중이다. 배포 후보 패키지와 한통을 통한 실제 Daum 불러오기·발송은 아직 완료 전이다. 기존 beta.11 Core는 검사 후 검증된 동일 바이너리로 복원한다. [사람 보안 검토 필요].

후속 검증에서 Store 4개와 Core scope-refresh 25개가 통과했다(target/common-mail-focused-repairs.log, exit0). 독립 검토가 OAuth hash12의 복구 출처 재검사 차이를 발견해 후보 빌드를 중단했다. hash12 reader는 기존 등록 출처를 google_oauth로만 제한하고, Core startup은 기존 complete_google_oauth_scope의 출처 검사를 거쳐 복구한다. 새 Store 회귀는 원래 요청과 등록의 출처를 일관되게 변조해도 pending 조회/단계 전이가 실패하며 원시 계정·도구·epoch·version·phase가 바뀌지 않는지 확인했다(1 passed, target/common-mail-oauth-source-final.log). 변조 후 typed reader를 정상 조회하려던 시험 가정은 raw SQL 검증으로 바로잡았다. 비원자 claim_imap_trash는 cfg(test) pub(crate)로 축소해 제품에는 begin_and_claim_imap_trash만 노출한다. Core 복구 회귀와 최종 정적 검사·후보 빌드는 이어서 검증한다. 아직 실제 한통 Daum 연결·메일 발송·새 공개 릴리스는 수행하지 않았다. 자동 검토는 사람 보안 검토 완료가 아니다.

Core의 OAuth 복구 출처 회귀 1개가 통과했다(target/common-mail-oauth-recovery-final.log). code-reviewer 재검토에서 해당 차단 사항이 해소됐고 독립 architect의 최종 판정은 CLEAR다. 이는 자동 코드 검토이며 사람 보안 검토는 별도다. 최종 traceability 341개 기준 검사·fmt·workspace all-target Clippy(-D warnings)와 release build가 통과했다. source hash `b26544e519a780f810aa8a29121f8231a6548d85ad119673e8c96f9abda9510f`의 beta.12 로컬 후보 ZIP(25,777,486 bytes, SHA-256 `50798413e4e82db7c93f47f4e442d131c09f76d7ad7cffa382119274c149da04`)을 생성하고 manifest 전 파일·체크섬·승인된 기존 publisher asset 동일성을 검증했다. 사용자 데이터·프로필은 포함하지 않는다. 별도 설치 폴더 설치와 CLI/MCP/Core/TUI 도움말·버전·AI 도움말 및 headless 업데이트 경계 smoke가 통과했다(target/beta12-candidate-verification.json, beta12-candidate-smoke.json). 잘못 붙인 bootstrap 전용 SessionOnly 인자는 설치 시작 전 거부됐으며 실제 package installer 인자로 고쳐 별도 설치를 완료했다. 공개 beta.12 배포는 아직 하지 않았고, 기존 데이터를 보존하는 로컬 후보 설치와 실제 연결 시험을 이어간다. schema26 시작 이후 구버전 Core로 자동 되돌리지 않는다.

동일 후보를 개발 PC의 기존 설치에 적용했다. beta.12 Core가 manifest와 일치하는 SHA로 실행되고(PID 33188), 기존 Gmail 계정이 시작 메뉴에 보존된 것을 사용자 WindowsTerminal에서 확인했다. candidate install exit0이며 기존 데이터 삭제·권한 변경은 하지 않았다(target/beta12-candidate-install-result.json). 같은 PowerShell에서 일반 메일 연결을 선택해 확인한 Daum endpoint 메타데이터를 입력하고 Core의 숨김 자격 증명 입력까지 진행했다. 앱 비밀번호는 읽거나 복사하지 않았고 사용자 직접 입력을 기다린다. 실제 IMAP 인증·메일 불러오기·SMTP 시험 발송은 아직 수행하지 않았다. 보호 입력 화면 이후 자동 키 입력이나 화면 조회를 중단하며, 실제 값은 대화·소스·공개 기록으로 받지 않는다. 공개 배포/웹페이지의 출시 완료 표시는 실제 연결 시험 후 이어간다. 전체 검사 재실행 대신 검증 근거와 실패 수정 로그 해시를 target/common-mail-candidate-evidence.json에 기록했다.

사용자는 Daum 앱 비밀번호의 복사·붙여넣기 이용 가능 여부를 다시 물었다. 비밀번호 값을 도구 응답으로 읽지 않고 UI 복사 버튼을 이용하는 경로를 확인했다. 브라우저 세션이 풀려 저장 로그인 선택으로 재접속했고, IMAP/SMTP 설정의 비밀번호 안내에서 기존 앱 비밀번호 관리 메뉴로 이동했다. 현재 제공자가 추가 본인 인증을 요구하므로 해당 화면을 Aside에 표시하고 사용자 직접 인증을 기다린다. 인증번호 요청·신규 앱 비밀번호 발급·기존 비밀번호 삭제/변경·실제 비밀번호 복사는 수행하지 않았다. 화면에 복사 기능이 제공되는지와 한통 보호 입력 티켓의 유효성을 확인한 뒤에만 후속 전송을 진행하며, 실제 IMAP/SMTP 시험과 공개 배포는 계속 미완료다.

사용자가 Aside로 인증 진행을 요청해 만료된 제공자 인증 단계를 새로 시작하고 등록 이메일 인증번호를 한 번 요청했다. 인증번호를 읽거나 입력하지 않았다. 이어 사용자가 그 이메일은 더 이상 사용할 수 없다고 정정해 이메일 인증을 중단하고 등록 휴대전화 문자 인증 화면으로 전환했다. 문자 요청은 아직 하지 않았으며 등록 휴대전화 이용 가능 여부가 다음 분기다. 연결된 Gmail 탭은 CDP 재연결 시간 초과로 접근하지 못했고 메일 본문·인증번호를 읽지 않았다. 사용자에게 더 이상 사용하지 않는 등록 이메일 접근을 요구하지 않는다. 기존 앱 비밀번호/계정/설정은 변경하지 않았고 신규 발급도 없다.

사용자가 제공자 인증을 완료했다고 알려 앱 비밀번호 관리 화면에 진입한 것을 확인했다. 기존 항목 3개에는 삭제 버튼만 있고 복사/재표시 버튼은 없었다. 한통 전용 신규 항목의 이름만 입력하고 생성 버튼은 누르지 않았다. 기존 값 복사와 신규 지속 인증정보 발급의 범위가 달라 Computer Use의 보안 민감 접근 생성 확인 규칙에 따라 신규 1개 발급 여부를 확인 중이다. 비밀번호 값을 읽거나 출력하지 않았고 실제 IMAP/SMTP 연결·발송·신규 공개 배포는 아직 없다. 후속 보호 입력은 만료된 이전 요청에 붙여 넣지 않도록 유효한 티켓을 새로 확인해야 한다.
### 2026-11-03 Daum 앱 비밀번호 발급 승인 이후 — 만료 재시도 수정

사용자는 한통 전용 앱 비밀번호 1개 발급을 승인했다. 기존 3개는 보존하며 새 값은 UI 복사와 Secrets 보호 입력으로만 전달한다. 생성 제출에서 제공자가 권한 없음 화면을 반환했고, 보안 메뉴의 기존 항목 수는 3개로 확인되어 신규 발급 성공으로 기록하지 않는다. 비밀번호 값을 읽거나 복사하지 않았다. 제공자가 다시 등록 연락처 인증을 요구하며 실제 IMAP/SMTP 연결·발송은 아직 미완료다.

만료된 IMAP 설정 요청을 재사용하던 Core 경로는 상태를 닫은 뒤 새 pending 요청을 만들도록 수정했다. 살아 있는 요청은 재사용하고 작업 큐는 비워 둔다. CLI는 설정 실패 상태를 안정적 오류 코드로 표시하며, 보호 입력 만료는 취소 정리 실패와 구분해 APPROVAL_EXPIRED를 보존한다. Core 관련 시험 16개와 CLI 상태 매핑 시험 1개 통과(target/imap-setup-retry-checks.log). 시험 모듈 위치에 대한 Clippy 실패를 수정한 뒤 workspace all-target Clippy -D warnings 재실행이 통과했다(target/imap-setup-retry-clippy-fixed.log). 전체 검사는 반복하지 않았다. 좁은 독립 코드 검토에서 차단 사항은 없었으며 사람 보안 검토는 별도다.

beta.13 로컬 후보 빌드와 패키지 검증이 통과했다. source hash 4c227431687e7c5e95956947de87f373f6070f08f4ca2db35441f69a55e3443e, ZIP SHA-256 bf6a57637a3e45586960fe95e00947d3bd8ea869ba415f04fa1206a673447c46, 25,777,998 bytes, manifest 261개 파일이다. 승인된 publisher 자산은 바이트 동일성을 확인했고 사용자 데이터·프로필은 포함하지 않았다. 로컬 후보 설치가 exit0으로 완료됐고 manifest 일치 Core(PID16452)가 정상 실행 중이다(target/beta13-candidate-install-result.json). 사용자 PowerShell의 새 시작 메뉴에서 기존 Gmail 계정을 확인했다. 데이터 삭제·권한 변경은 없고 공개 배포는 아직 하지 않는다. Daum이 다시 요구한 카카오톡 인증 화면을 Aside 전면에 표시하고 인증번호 요청을 한 번 실행했으며, 실제 번호 입력은 사용자에게 넘겼다. 번호·비밀번호 값은 읽지 않는다. [사람 보안 검토 필요].
### 2026-11-03 Daum 전용 앱 비밀번호 발급·IMAP 실연결 성공, SMTP 수정

사용자가 제공한 일회용 인증번호로 Daum 본인 인증을 완료한 뒤, 승인된 한통 전용 앱 비밀번호 1개를 발급했다. 기존 3개는 보존했다. 비밀값은 읽거나 출력하지 않고 제공자 UI 복사 완료 안내를 확인한 뒤 동일 PC의 Core 소유 숨김 입력으로 붙여 넣었다. beta.13에서 실제 IMAP 인증이 성공하고 guided setup이 SMTP 단계로 진행한 것을 확인했다. 실제 메일 발송·휴지통 이동은 아직 실행하지 않았다.

별도 발신 주소를 사용하는 SMTP 설정이 비밀 입력 전에 INVALID_INPUT을 반환했다. Core setup_smtp의 endpoint 조립이 SMTP 전용 sender_address를 IMAP 구조에도 넣어 domain validator가 거부하는 원인이다. IMAP 메타데이터에서 해당 필드를 제외하고 SMTP에만 유지하는 작은 수정과 합성 회귀 시험을 추가한다. 발급된 값은 재생성하지 않으며 기존 계정·키를 보존한다. 최초 새 시험 컴파일은 지역 변수의 함수명 가림 때문에 실패해 시험 변수명을 고친 후 재실행한다. [사람 보안 검토 필요].
SMTP 분리 회귀 시험은 수정 후 1개 통과했다(target/smtp-explicit-sender-endpoint-fixed-test.log). Core all-target Clippy -D warnings도 통과했다(target/smtp-explicit-sender-clippy.log). 독립 검토는 기존 SMTP sender 결속·자격 증명 경계가 유지되고 차단 사항이 없음을 확인했으며 사람 검토와는 구분한다. beta.14 빌드·manifest 검증 통과: source hash b80837fff5e10b810d89f6cef57c465b5a455965d64fd49eb923318ea89effad, ZIP SHA-256 25b2b4a4ebc5e858f0e0bebadc5b66dd222de64c88576627472a3ef486c6ae4f, 25,790,286 bytes. 로컬 설치 후 이미 연결된 계정의 SMTP 단계만 재개하며 공개 배포와 실제 발송 성공을 아직 주장하지 않는다.
beta.14 로컬 설치와 Core 기동은 통과했다(PID23504, target/beta14-candidate-install-result.json). SMTP만 재개하는 보조 명령에서 --sender-address가 최상위 CLI 옵션 목록에 빠져 IPC 전에 INVALID_INPUT이 발생하는 별도 누락을 확인했다. main.rs에 메타데이터 옵션 전달을 추가하고 중복 거부를 유지했다. 실제 인자 경로의 회귀 시험 setup01_smtp_sender_address_is_forwarded_and_duplicate_is_rejected 1개 통과(target/smtp-sender-cli-test.log). IMAP 연결과 기존 앱 비밀번호를 보존하며, 아직 SMTP 연결·실제 메일 발송 성공으로 기록하지 않는다.
### 2026-11-03 실제 SMTP 송수신 확인·백그라운드 작업 전환

beta.15 로컬 설치와 Core 기동이 통과했다(PID43520). 실제 Daum SMTP 보호 입력 이후 ready를 확인했고, 등록된 기존 Gmail 두 계정과 새 IMAP 계정을 터미널 범위 확인 후 보존했다. 전체 내용을 터미널에서 확인한 합성 메일 1통은 sent 상태가 됐으며 지정 Gmail 수신 측 조회에서 정확히 1통과 합성 본문 표식 일치를 확인했다(target/daum-delivery-verification.json). 비밀값을 읽거나 출력하지 않고 두 보호 입력을 마친 후 제공자 비밀값 표시 화면을 닫고 클립보드를 비웠다. 실제 휴지통 작업은 실행하지 않았다.

실제 IMAP 폴더 카탈로그 17개 조회와 첫 저 UID 페이지 동기화는 성공했으나, 첫 64 UID 구간이 비어 최근 미리보기가 필요했다. 일반 IMAP TUI의 r이 Gmail에만 연결돼 있던 누락을 보완하고 최근 범위 2페이지의 기존 초기 동기화 계약으로 연결한다. 저 UID 페이지가 비었다고 메일함 전체가 비었다고 기록하지 않는다. 실제 서버의 INBOX wire는 정규화되지만 표시 이름 대소문자는 유지되는 점 때문에 초기 동기화의 표시 이름 포함 동등 비교가 NOT_FOUND를 반환하는 원인을 확인했다. 실제 카탈로그의 비공개 원문 대신 정규 표기 일치 여부의 boolean만 출력해 확인했으며 Store 메일함 식별자 결속을 보존하는 수정과 회귀 시험을 진행한다. 실제 IMAP 본문 조회와 공개 배포는 아직 미완료다.

사용자는 컴퓨터를 함께 사용해야 하므로 이후 UI 조작은 Vercel agent-desktop만 사용하라고 정정했다. 전면 창 전환·키보드/마우스 입력과 Aside/CUA 조작을 중단하고, 코드·빌드·공개 제품 CLI의 읽기 전용 검증 등 독립 작업을 백그라운드로 진행한다. 기존 미커밋 변경·계정·키·ACL은 보존한다. [사람 보안 검토 필요].
Store 초기 동기화의 두 카탈로그 결속 지점은 표시 이름 전체 동등성 대신 정규 wire bytes와 encoding으로 비교하도록 수정했다. 계정·selectable·세대·endpoint·카탈로그 해시 검사는 유지한다. 새 회귀 2개와 관련 initial_sync 시험 전체 16개가 통과했다(target/imap-inbox-identity-tests-fixed.log, imap-initial-sync-regression.log). 최초 새 시험의 String 인자·반환형 오류는 시험 코드에서 수정했으며 최초 실패 로그를 보존한다. Store/Core all-target Clippy -D warnings 통과(target/imap-identity-core-clippy.log). 독립 검토에서 식별자 검사 약화는 발견되지 않았다. 일반 IMAP TUI 최근 미리보기의 반복 갱신 세대 처리 보완은 계속 진행한다. 실제 Daum의 선택 가능한 광고된 Trash는 1개임을 읽기 전용으로 확인했으며 실제 이동·삭제는 하지 않았다.

2026-11-03: Added owner/account/canonical-mailbox-bound read-only initial-current lookup across Store/Application/Core/IPC/CLI. TUI queries authoritative generation on every refresh and resumes up to two recent pages with updated plan versions. Removed unused forward-page TUI helper in favor of the tested recent-preview path. Related tests: Application4, CLI5, IPC6, Store29; final TUI13; docs29. Affected all-target Clippy passed. Initial compile errors were corrected and original failure logs retained. Independent review found no CRITICAL/HIGH blocker. No full 1,906-test repeat. Human security review pending.

Installed beta.16 candidate and preserved accounts/data/keys. Actual Daum folder catalog17, two recent pages, cached messages19 and body read succeeded twice through the public product CLI, including repeated-generation refresh. Only booleans/counts logged. Prior single synthetic SMTP send and one matching Gmail message/body remain verified; no resend or actual Trash move. Proof: target/daum-imap-verification.json and daum-recent-beta16[-repeat]-live.log. Final public package metadata is being refreshed after correcting the contract documentation encoding. Human security review pending.


Published beta.16: source commit 528d8f057ea37f4ad0852d2fa7dcbd197b43e06d, canonical source hash fa242943034a775b697a5cc6bfe2c7142e18e22d20d04b4d478a61eec5ac5966. Final ZIP SHA256 2137f449c2841909f7ce8acddb4e4f3e87a3b7c954aa44e4e51c5183c8ca6a47, 25,848,377 bytes. Public re-download matched. Final metadata differs from the earlier local candidate only after the contract text encoding correction; all four installed binary hashes match the final package. The committed source archive matches the package source hash; public copy of PROGRESS anonymizes test identities while root evidence remains preserved. Release: https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.16 . Website commit 4777cd4 published; live page confirms beta.16 and installer pin 528d8f057ea37f4ad0852d2fa7dcbd197b43e06d. Original root HEAD/index and other dirty worktrees preserved. Human security review remains pending; no actual Trash operation.

TUI visual observation: used Vercel agent-desktop strict-headless screenshot of the already-open Windows Terminal window only. No focus, key, mouse or clipboard actions. The existing beta.15 process still displays raw Google account IDs, accumulated terminal output, and a prior generic request error. This is the older already-running process, not evidence of a new beta.16 failure. Current hantong-t remains a sequential text view rather than a polished full-screen multi-pane TUI. Functional read/repeated-sync evidence comes from installed beta.16 product CLI and targeted TUI tests, not a fresh interactive visual pass. Private screenshot stays under ignored target/ and is not published.


2026-11-03 communications-style TUI implementation: the user activated the previously deferred retro terminal and requested keyboard-first plus mouse support. Added fixed alternate-screen rendering, blue/green/amber/mono themes (--theme/F, session lifetime), account/folder shortcuts, message list/body split and narrow-body view, wrapped-body scrolling, click selection/double-click open/wheel/footer buttons, same-console cooked handoff for compose/Trash/AI/login and return. Ordinary hantong CLI and common Core/package remain shared; isolated build output was used only because existing debug executables are running and were preserved. No external dependency added: TUI reuses the existing platform crate Windows Console API. One local Cargo.lock edge and architecture allowlist updated.

Target validation: TUI26 and platform59 unit tests plus existing Windows IPC1 passed; affected all-target Clippy and architecture checks passed. Initial state/wrap/geometry/compiler findings were fixed without deleting failing tests; an obsolete helper test from the prior feature was not involved. Independent review found no remaining CRITICAL/HIGH blocker. Account switch failures clear prior mail/body data. Mouse coordinates use the same visible rows and footer segments. IMAP internal message IDs remain account/mailbox/encoding/UIDVALIDITY/UID-bound via the existing locate hash and current binding check; no new authority path exists. Platform restores original input/output modes and cursor visibility, including failed entry cleanup. Human security review remains pending.

Owned hidden-console integration passed: synthetic startup, F-key Green, mouse Amber, double-click Message01 body, PageDown and wheel-visible-content change, 80-column compact view, quit0, original modes/cursor restore and invalid-theme rejection. Runtime resizing of an already active alternate screen could not be exercised in the hidden Windows host (API87); it remains an explicit verification gap, not a passing claim. Window-size renderer cases are covered by pure tests. No real send, Trash, credential capture or user-window input occurred. Evidence: target/retro-tui-console-verified.log, retro-tui-release-regression.log, retro-tui-release-clippy.log; synthetic console buffer target/retro-tui-console-frame.json.

User requested a screenshot. Vercel agent-desktop direct launch lacked an interactive console; a separate host launch did not expose a capturable window. No user window was focused or typed into. Shared an image generated from the actual Rust renderer with synthetic data, explicitly labelled as a rendering preview rather than a native screenshot: target/hantong-t-blue-preview.png, green-preview.png. Preview-only launcher files stay ignored under target/. beta.17 packaging follows final validation; deployment is not yet complete.


2026-11-03 terminal usability integration (beta.17 candidate, not published): added ANSI8/16 themes, wider body/expanded reading, account-bound unified cached Inbox and display-only persistent aliases. Startup shows preparation animation, accounts, release version and optional cached/nonblocking public update notice before the prompt. Plain terminal now accepts bounded one-based row numbers through the existing open path; invalid numbers never clamp to another item. Full-screen arrows/Enter/viewport remain the default. Gmail INBOX and canonical IMAP INBOX sort first with remaining folder order preserved. Account addresses take priority over raw IDs, including the Core-owned registration preview; grant IDs/scopes remain unchanged.

Affected evidence: TUI38, CLI141, platform62 plus Windows IPC1, AI14, Core AI-registration4, packaging10; architecture valid. Fresh all-target Clippy passed before the latest Codex protocol delta and will be rerun after it. Synthetic owned hidden-console keyboard/mouse/scroll/restore integration passed, with runtime resize still an explicit gap. User desktop windows/clipboard were not operated. Codex startup failure was reproduced against installed 0.163.0: old onRequest and thread sandbox readOnly spellings were rejected. Corrected to on-request/read-only and wrapped dynamic tools in the current namespace schema; actual product /status now succeeds with no model prompt or mail operation. Further compatibility review identified that the old turn sandbox readableRoots object is not represented in the current wire schema; no real model turn is being run until the equivalent restricted permission profile is verified. No actual Trash, resend, credentials or ACL changes. Human security review pending.

Final compatibility decision: Codex 0.163.0 rejects temp-only filesystem permissions on this Windows host even with its already-configured elevated sandbox (effective root read required). The prior built-in-readonly /status proof does not establish the required isolation. Replaced legacy sandbox fields with request-local named permissions (root deny, empty temporary cwd read, network off), checks the returned exact activePermissionProfile, and returns localized CAPABILITY_UNSUPPORTED without a broad-read fallback. No UAC, global Codex config edit, model turn or actual AI mail operation occurred. Built-in AI registration remains ready/preserved. This is a current Codex-chat capability gap, not a fixed/verified conversational path. Protocol enum and dynamic namespace fixes remain covered by fake transport tests.

Final affected checks: TUI39, CLI142, AI15, Core registration4, profile diagnostics IPC/Windows targets, packaging10 and docs29 passed. Affected all-target Clippy (-D warnings) and architecture checks passed. Synthetic hidden-console integration now also proves down-arrow+Enter opens Message02 and unified inbox rendering; runtime resize remains unexercised. Fresh candidate packaging/install/publication follows these checks; beta.17 is not public at this point. Existing accounts and original root HEAD/index are preserved. Human security review pending.


Published beta.17: source 02dd4e9af5c9e4103f525619549505b4d17f52c1, canonical source SHA256 5abca0ed59d2994f62d9e216451acc582e1239e52915097ca1c8fea1d3260bbd. ZIP SHA256 26d9d7f5cd2858f854113147b9e7732435b605971909f29b8fb419bed57ee57c (26,308,395 bytes, 261 manifest files); committed source archive and public re-download verified. Release https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.17 . Site 4ad5093 published and live version/installer pin/TUI keys/Codex capability notice verified. Installed candidate beta.17 with existing three ready accounts and Gmail addresses preserved; Core PID54940 manifest hash d8793c56073a64a682a4c8bc9548ea4024fa0683e6dd2f2b83114a1676c731c6. Packaged CLI and TUI both report beta.17. Actual product Codex session now returns explicit CAPABILITY_UNSUPPORTED under the restricted named-profile requirement; no model turn or AI mail operation was run. Original root HEAD2852a31/index empty, user terminal/TUI processes, account data/keys/ACL preserved. Public PROGRESS copy anonymizes test identities. No actual Trash or resend. Security and contract changes remain human-review pending; runtime resize remains a validation gap.

## 2026-11-03 CLI 독립 화면·언어 전환·메일 정보 표시 보완

사용자는 CLI에 고유한 화면·키·이동 계획과 실행 중 언어 전환이 필요하다고 정정했다. CLI-INTERACTION-PLAN-20261103.md에 계정→폴더→메일 목록→본문 페이지→이전 선택 복귀 흐름을 기록했다. CLI 메뉴와 리더에 방향키/Enter, Esc, / 입력 즉시 명령 목록, /help, /ai, /agent, /language 선택 및 /language ko|en을 연결했다. 언어는 현재 세션에만 적용하며 저장된 설정을 바꿨다고 표시하지 않는다. TUI는 : 명령에서 language ko/en으로 전환하며 CLI가 TUI를 대신 실행하지 않는다.

메일 표시 공통 계층에 bounded RFC2047 B/Q 제목·주소 디코딩, Windows EUC-KR/KS_C_5601/CP949 표시 디코더, 제목·보낸 사람·받는 사람·참조·날짜와 본문을 추가했다. 본문 응답의 선택적 headers와 기존 JSON 캐시 호환 확장은 계약 제안 message-body-envelope-metadata-20261103.md에 기록했다. 권한·계정·UID·발송 정책과 DB 스키마는 확대하지 않는다. IMAP 본문 charset callback은 Core에 연결했다. Gmail body의 기존 charset 지원 범위는 유지한다. CLI는 메타데이터와 18줄 단위 본문 페이지, TUI는 전체 폭 읽기와 비동기 조회를 사용한다. TUI 조회 중 이동·계정 전환 시 generation을 무효화하고 이전 계정 결과를 표시하지 않는다. 언어 변경은 같은 요청을 유지하면서 생성된 정보 제목만 다시 표시한다. CRLF/tab과 제어문자·bidi를 화면에서 안전하게 처리한다.

대상 검증: platform 70, CLI 149, TUI 45, contracts message_body 8, IPC body 3, IPC Google 10, Store Google 원자 캐시 1, Secrets Gmail 2가 통과했다. 별도 소유 숨김 콘솔에서 방향키로 두 번째 값 선택, / 키 직후 Enter 전 명령 표시, 힌트 선택, Esc, 입력/출력 모드 복원을 검증했다(target/command-prompt-console-proof.json). 사용자 창·클립보드·입력은 조작하지 않았다. 검토 중 발견한 이전 계정 본문 혼입과 언어 변경 중 Loading 고착을 수정하고 회귀 시험을 추가했다. 변경 대상 all-target Clippy, 아키텍처 및 문서 검사와 후보 빌드/설치는 최종 확인 중이다. 전체 시험을 반복한 결과로 표시하지 않는다. 실제 발송·휴지통·비밀·ACL 변경은 수행하지 않았으며 보안/계약 변경은 [사람 검토 필요]. beta.18은 이 기록 시점에 아직 공개하지 않았다.

beta.18 최종 확인: 변경 대상 all-target Clippy(-D warnings), 아키텍처·traceability(341)와 문서29 검사가 통과했다. release 패키지와 커밋 archive의 canonical source hash가 c50182f5c90a1fe7cb9a03248e037d0834cef437ff673685fa878b6fa8147458로 일치했다. 추가 숨김 콘솔 검증에서 실제 패키지 CLI와 합성 TUI의 한국어→영어 실행 중 전환·종료·모드 복원이 통과했다. 설치한 beta.18 Core PID56180의 manifest SHA256 fdb9a8c9310358da280f9cf8c343f399b8085daf65a7e6716f4af16602d5f6f0을 확인했고 기존 계정3개가 모두 ready였다. 사용자 기존 창은 종료/입력하지 않았다.

공개 배포 완료: https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.18 , source 2f6fa93bccb8bd9ed523773a0e93ec66c8e11401. 공개 ZIP 재다운로드 SHA256 dd3ae6c10f872a55904389f01f93d613a4001487ba9170928ad9b899bc40a549(26,523,107 bytes, 261 manifest files)이 일치했다. 사이트05fc79a의 live beta.18 버전·설치 소스 pin·언어 전환 설명·Codex 제한 고지를 확인했다. root HEAD2852a31/index empty를 보존했다. 실제 메일 발송/휴지통·새 비밀 입력·ACL 변경 없이 검증했으며, 새 버전의 실계정 본문 UI 전 과정을 검증했다고 표시하지 않는다. 기존 Windows Codex 제한과 사람 보안/계약 검토 필요 상태를 유지한다.

## 2026-11-04 /ai codex 지원 제한과 오류 코드 보존

사용자가 beta.18 메일 읽기 화면에서 /ai codex 실행 후 CAPABILITY_UNSUPPORTED와 INTERNAL_ERROR가 연속 표시되는 증거를 제공했다. 현재 로컬 codex --version은 여전히 0.163.0이다. 첫 오류는 기존 문서에 기록한 Windows 임시 폴더 한정 named permission profile의 시작 제한이며, 실제 Codex 대화 지원이 완료됐다고 표시하지 않는다. 메일 등록이나 Google 계정 오류로 추정해 재연결/재동의를 요구하지 않는다.

독립적인 표시 결함은 apps/hantong-cli/src/ai.rs::run_terminal_command가 try_run의 모든 비정상 종료 코드를 InternalError로 덮어쓰던 것이었다. 고정된 ErrorCode::ALL 종료 코드 역매핑으로 원래 CAPABILITY_UNSUPPORTED/AuthRequired/Offline 등의 의미를 보존하고, 알려지지 않은 종료 코드만 InternalError로 둔다. 성공0과 모든 계약 종료 코드 및 unknown255 회귀를 추가했다. AI CLI 시험7개와 hantong-cli all-target Clippy(-D warnings), fmt가 통과했다(target/ai-terminal-cli-tests.log, ai-terminal-error-clippy.log). 실제 모델 턴·AI 메일 작업·새 로그인·비밀·파일 접근 확대·Codex 전역 설정 변경은 수행하지 않았다. 이 변경은 로컬 소스에 있으며 설치된 beta.18이나 공개 ZIP이 수정됐다고 표시하지 않는다. 실제 Codex 연결 제한은 남아 있다.

## 2026-11-04 Codex MXC 재검증과 Claude 공식 세션 연결 후보

최신 사용자 지시로 Codex 연결 차단과 Claude를 재확인했다. 기존의 ‘Windows Codex 0.163.0 자체가 제한 프로필을 지원하지 않는다’는 진단은 너무 넓었다. 시작 검사를 분리하니 기존 백엔드(root deny+임시 cwd read)와 :minimal만 추가한 경우는 거부됐지만, 세션 전용 windows.sandbox=mxc와 공식 :minimal runtime read를 함께 사용하면 정확한 hantong_mail_temp 프로필로 시작했다. root deny, 정확히 새로 만든 임시 cwd read, 도구 network=false, shell/web/apps/plugins/MCP 차단을 유지하고 전역 설정·UAC·계정·권한을 수정하지 않는다. 동적 메일 도구는 기존 provider별 기능 설정 범위만 사용한다. 지원하지 않는 호스트는 실패 종료하며 root/workspace read로 폴백하지 않는다.

실제 command-only MXC 합성 canary는 허용한 파일을 읽고 같은 상위 폴더의 비허용 파일은 읽지 못했다(target/codex-mxc-filesystem-proof.json). 모델 호출 없는 3가지 시작 비교는 target/codex-restricted-start-proof.json에 남겼다. 실제 제품 Session 라이브러리의 start→메일 도구 off→합성 질문 ask로 HANTONG_CONNECTION_OK 응답을 확인했다. 메일 내용은 질문에 넣지 않았고 모델 턴 동안 메일 도구는 꺼져 있었다(target/codex-mxc-model-proof.json). 메일 도구 연결 자체는 기존 동의에 결속한 로컬 MCP preflight를 통과했다. 실제 AI 메일 발송/휴지통 시험 결과로 표시하지 않는다. 최초 probe의 임시 폴더 정리는 MXC 핸들 종료 지연으로 오류가 나서 결과를 별도 확인하고, 이후 소유한 빈 임시 폴더의 best-effort 정리로 진단을 재실행했다. 사용자 파일은 삭제/정리하지 않았다.

Claude Code 2.1.306은 설치돼 있고 공식 auth status는 loggedIn=true, authMethod=oauth_token이었다. 한통의 Claude 계정 공유 설정은 headless 상태 확인에서 HUMAN_STEP_REQUIRED라 아직 없다(생성/동의하지 않음). SDK/stream-json에 구독 OAuth를 쓰는 차단은 유지하고, 명시적인 대화형 /ai claude만 같은 터미널의 수정되지 않은 공식 Claude Code에 제어를 넘기는 경로를 추가했다. stdin/out/err 상속, 빈 임시 cwd, strict MCP, built-in 도구/사용자 정의 차단, 세션 저장 off이며 프롬프트 전달·응답 파싱·토큰 추출·자동 도구 승인은 없다. JSON/--no-input/--prompt는 이 경로로 폴백하지 않는다. 기존 provider별 Core 기능 설정과 발송/휴지통 터미널 확인을 유지한다. 처음 쓸 때의 계정 공유 확인과 실제 Claude 응답·MCP 메일 작업은 아직 사용자 단계다. Console/API 방식은 기존 내장 대화 경로를 유지한다.

후보 변경 검증: AI library17, CLI150(추가 라우팅 가드 이전) 및 최종 CLI AI 관련8, AI/CLI all-target Clippy(-D warnings), fmt, architecture, traceability341 통과. fake provider가 root deny/:minimal read/network off/Windows MXC 조건을 확인한다. 고정 종료 코드 보존 수정도 함께 포함한다. docs/contracts/ai-managed-clients.md에 MXC와 native Claude의 경계·검증 한계를 기록했다. 소스·계약은 [사람 보안 검토 필요]. 아직 새 후보의 설치/공개 배포 전이다.

최종 검토 정정(2026-11-04): Claude의 --no-session-persistence는 print 모드 전용이므로 native 대화에서 제거했다. native 시작의 한국어/영어 안내에는 대화 기록이 Claude Code 자체 설정을 따르며 한통은 대화를 수집하지 않음을 명시했다. 앞 단락의 ‘세션 저장 off’는 후보의 잘못된 가정이며 최종 코드의 보장이 아니다. 계약도 실제 경계에 맞췄다. 또한 Codex thread/start의 모든 ProviderFailed→SandboxUnavailable 변환을 제거하고 알려진 Windows/MXC 설정 오류만 분류한다. 인증/모델/네트워크/EOF 실패를 격리 미지원으로 표시하지 않는 회귀를 추가했다. 최초 beta.19 로컬 후보는 미공개·미설치 상태로 보존하고, 최종 후보는 별도 출력 디렉터리에 재생성한다.


2026-11-04 beta.19 installed and published: final source 7843c3a243c516b7811c18b981454197ccca84ca; committed archive/package canonical source SHA256 54c9bb6935fffabc9515e6ad51472544f6354c27bf58ec2235eaf6c1679c2ae6. ZIP SHA256 88c44f7887498651682561a0264fc6c95741ded02aa435e0b7cd3f2db20b45ac, 26,531,538 bytes, 261 manifest files. Public re-download matched. Release https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.19 . Site commit 3a0bbf3 live version, installer source pin, MXC and native Claude notices verified.

Fresh final validation: AI library18, CLI AI routing8, documentation29; affected all-target Clippy(-D warnings), fmt and architecture passed. Actual Codex production Session synthetic response succeeded with mail tools disabled and no mail content in prompt; separate MXC canary proved allowed read and sibling read denial. Final packaged interactive Codex startup/quit restored console modes; installed Codex /status exit0. Existing three accounts all ready after installation; CLI/TUI report beta.19. Core PID42780, unchanged manifest SHA256 fdb9a8c9310358da280f9cf8c343f399b8085daf65a7e6716f4af16602d5f6f0. No actual send/Trash, credential input, global AI config edit, ACL change or user-window input. Original root HEAD2852a31/index empty preserved. Public progress copy anonymizes test identities.

Claude official CLI2.1.306 existing OAuth login was detected; the dedicated HanTong Claude mail profile still requires first-use same-terminal account-sharing confirmation. The new native handoff is implemented and route/argument tests pass; actual Claude model reply and MCP mail operation remain unverified. Native conversation history follows Claude Code settings. Security/contract changes remain human-review pending. Evidence: target/beta19-installed-smoke.json, beta19-candidate-install-result.json, beta19-public-verification.json, beta19-site-verification.json, codex-mxc-model-proof.json, codex-mxc-filesystem-proof.json, beta19-codex-interactive-console-proof.json. Full workspace suite was not repeated for this bounded change.


2026-11-04 actual conversation follow-up: user explicitly required conversations on both providers. Installed beta.19 Codex CLI completed two synthetic turns including follow-up context with /agent off; /quit exit0 restored console modes. Claude first-use same-terminal feature review for the existing three accounts was completed under the requested connection verification, without a send/Trash request. Actual Claude startup then reproduced AI_PROTOCOL_ERROR. Official resolved Claude auth status returned a valid eight-line JSON document; HanTong previously parsed only its first line. Added an auth-status-only bounded document reader (1 MiB, existing20s deadline/kill/reap), preserving all normal line-framed streams. AI20 tests and affected all-target Clippy passed; bounded automated review CLEAR. Actual Claude native conversation verification is still in progress, so neither actual Claude reply nor beta.20 publication is claimed here.


2026-11-04 beta.20 actual conversations verified and deployed: final package Codex and Claude each completed two synthetic turns, repeated the prior marker from context, and exited0 with console modes restored. No mail content was requested and no mail tool, send or Trash operation was invoked. Evidence target/beta20-packaged-conversations.json; final Claude native runtime banner2.1.317. Earlier native2.1.306 automatically updated itself during the diagnostic session; final launch now sets child-only DISABLE_AUTOUPDATER=1. No provider credential/config files were read or rewritten by HanTong.

The real auth-status failure is fixed by a bounded whole-document reader used only for Claude auth status; normal line-framed streams are unchanged. A missing mutability declaration found by the first compile was corrected. AI20, CLI AI8, all-target Clippy(-D warnings), formatting, architecture/traceability341 passed; no full workspace repeat. Automated bounded parser and native-launch reviews CLEAR, human security review still pending. Actual native safe-mode turned off even explicit MCP; replaced it with restricted/no-built-in-tools/strict explicit MCP, disabled commands/hooks/memory/connectors/Auto/Chrome and child-only memory/update controls. Ctrl+C twice is the verified native return path. Core consent/provider identity and mutation confirmation remain unchanged. Native mail-tool invocation itself is not claimed tested by these conversation checks.

Synthetic console evidence collection initially hit a Windows reader/atomic-replace sharing race and a native pasted-input Enter debounce; only the owned harness was adjusted (sharing retry, real console handles, key-up records and a500ms submit pause), then both final packaged checks passed. User windows/focus/clipboard were untouched. The existing Claude feature connection was completed through the normal terminal review for the three existing accounts under the user's connection/conversation verification request; no database or consent bypass was used.

Installed beta.20 CLI hash exactly matches the conversation-tested package; installed Codex status exit0 and Claude auth status exit0, existing three accounts all ready. Core PID53436 unchanged binary SHA256 fdb9a8c9310358da280f9cf8c343f399b8085daf65a7e6716f4af16602d5f6f0. Source66d28d6deb0f3dadc7d3591a7fb34350e510753f, committed archive/package source SHA256 3c77e499a7353e76033f8f55dc293954c1d847e8ca3bc080aec5de6e75227963. Public ZIP re-download SHA256 4d3407f754b8db7cfcb164d495eb87c87993962d4ac9680708dfa0dd8480ceee (26,546,690 bytes,261manifest files) matched. Release https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.20 . Site6dafcc9 live version/source installer pin/Ctrl+C instructions verified. Evidence target/beta20-installed-smoke.json, beta20-public-verification.json, beta20-site-verification.json. Root HEAD2852a31/index empty and existing account/secret/storage ACL state preserved; public progress copy anonymized.


2026-11-04 company-PC updater and platform-scope correction: user reported update hantong Join-Path null drive + CORE_UNAVAILABLE. Reproduced ArgumentNull,Microsoft.PowerShell.Commands.JoinPathCommand with a non-null verbatim \\?\ root; an ordinary path succeeds. This matches the legacy updater boundary fixed in beta.10; the remote company version is not yet established, so it is not asserted. Current updater regression10 and uninstall-preservation3 tests passed. Added WINDOWS-RECOVERY.md with direct-bootstrap replacement and optional owned program-only uninstall/backup/reinstall, keeping separate account/key storage. Executed the documented clean path against a synthetic Unicode/space installation: owned removal, unknown-file backup, clean reinstall, and account sentinel preservation passed; real developer installation unchanged. Evidence target/windows-recovery-guide-proof.json. No real-company operation was executed.

User corrected that Mac and Linux were already support targets, then explicitly requested full support update. Confirmed macOS follow-up in FEATURES.md and Linux core HOST-02 in adopted requirements. Prior wording incorrectly conflated unfinished distribution with target exclusion. Current Windows beta20 remains the only public working product package; cross-target builds are not native product support. New active scope is parity for current CLI/TUI install, account connection, Gmail/IMAP/SMTP read/compose/send/Trash, built-in AI/MCP, and update on Windows/macOS/Linux, preserving existing data/security boundaries. Native read-only mapping found operational Core/Store/Secrets/IPC non-Windows stubs, not merely missing packaging. Began an independent Unix terminal backend implementation and concrete Unix identity/keystore architecture/research. No Unix mail-support completion or release is claimed.

2026-11-04 Unix implementation continuation: Windows recovery guide published in source4ba0fe0 and site9618995. Approved ADR-030 implementation decision and Unix v2 contract/dependency proposal after explicit user confirmation. Windows v1 remains frozen; added independent v1 golden transcript. New exact Unix-only dependencies keyring-core1.0.1, Apple store1.0.2/keychain, zbus Secret Service store1.0.1/crypto-rust, and existing ring0.17.14/libc0.2.191 direct bindings fetched into Cargo.lock. This is approval to implement, not human security-code review or a support-completion claim. Linux headless/SSH remains required follow-up, not silently covered by desktop Secret Service.

Native Unix terminal: Linux/macOS TerminalSession, inline slash command/arrow-selection editor and selection list implemented; TUI full-screen gating enabled. Windows platform70 and TUI45 tests plus affected Clippy passed before the Unix-auth/security backend additions. Isolated WSL Ubuntu native Rust1.95 setup uses workspace target-only toolchain/cache; no user PATH/profile or credential configuration modified. Native Linux platform19 and TUI45 tests passed. Owned synthetic PTY proved arrow+Enter open, Escape back, resize, q/Ctrl-C normal exit, exact termios restore and non-TTY rejection before alternate-screen effects. Initial parser fixture, inferred hint type and guarded-match exhaustiveness defects corrected before passing. Rust1.95 diagnostic renderer also panicked while displaying a pre-existing Unix dead-code warning; actual Unix editor removed that unused path and a short diagnostic run exposed the source errors. These results do not establish Unix Core/mail support. Input-probe extension and new security modules are undergoing fresh integration checks; no real account/secret storage, send, Trash or ACL action performed.


2026-11-04 Unix security integration follow-up: added fd-pinned private Unix roots/profile loading, kernel-bound Unix local sockets, Ring verification, opaque OS-keystore signing, and SQLCipher adapter with synthetic key source tests. Enabled existing pure SQL Store repositories and portable Tokio/rustls IMAP/SMTP drivers for the supported native OS targets; these newly connected paths still require native compilation and runtime validation. Windows affected all-target check (platform/secrets/IPC client/server) passed before the latest Store/transport additions; architecture check and CI configuration tests4 passed. Native SQLCipher first build is still compiling vendored OpenSSL; no native DB test success is asserted yet. No real OS credential entries, account data, user terminal, send/Trash or ACL state was touched. Existing Unix codec tests passed earlier; connection-owned authentication engine and fake-signature tests are now being integrated, not yet product-auth completion. Human security review pending.

Fresh Windows verification after the Store/transport target expansion: affected Secrets+Store all-target check passed. A broader contracts/platform/IPC lib run reached the existing fixed-endpoint CNG server integration test and failed at LocalPipeServer::bind with Conflict while the installed Core remained running; no Core was stopped or data changed. This broader run is not reported as all green. The new Unix auth-v2 wire-only regressions were then run explicitly: client3/server3 passed. Windows auth-v1 golden contract is included in the contracts90 passed portion. New Unix connection auth engine still awaits native compilation/tests.


2026-11-04 approved Unix foundation native verification: user approval for ADR-030 authentication v2, Keychain/Secret Service and named dependencies remains in effect; human security review is pending. Linux native Secrets library run completed 120 passed / 1 failed; the failure was a synthetic Korean MIME fixture lacking an explicit UTF-8 charset. The fixture was corrected without relaxing the parser default, and its targeted rerun passed 1/1. All four encrypted database tests passed in the native library run (create/reopen, missing or wrong key fail-closed, locks and encrypted backup). SQLite NOFOLLOW rejected the intermediate /proc/self/fd symlink; the adapter now resolves the held directory FD through the OS, reopens the physical path with the guarded no-follow walk, checks directory identity and database leaf identity, and retains SQLITE_OPEN_NOFOLLOW. macOS uses F_GETPATH and still requires native runner validation. No real keyring entries were created.

Fresh native Unix IPC/platform batch: client8 + server6 + platform18 = 32 passed, including signed v2 transcript checks, bounded framed exchange/receipt deadlines, kernel socket peer checks and atomic public profile publication/recovery. Store native test compilation exposed Windows-only fixtures and unsupported-OS tests incorrectly compiled on Linux; those gates are being corrected, and Store is not yet reported green. Windows affected all-target check exposed a Secrets delete cfg return regression; fix/recheck is pending. Existing installed Windows Core and accounts remain untouched. scripts/package_cli.py external-publisher path handling regression tests11 passed; no actual publisher secret was read. Unix Core startup, registration/terminal secret staging, complete mail dispatch and OS-specific installation/update remain incomplete, so no Unix release or full-support completion is claimed.


2026-11-04 Windows preservation follow-up: fresh affected all-target check and Clippy (-D warnings) passed for Contracts, Platform, Secrets, Store, IPC client/server and Core after correcting the Windows SecretStore::delete cfg return. No installed binary, key, account, ACL or running Core was changed. Native Linux authenticated client8/server6/platform18 tests passed again with the production protected-exchange deadline helper regression. Store native schema tests exposed fixture metadata omissions and are being repaired separately; no Store all-green claim yet. Approved foundation source was copied to the owned unix-runtime-20261104 worktree, with private progress addresses redacted, and published only on feature/unix-runtime-20261104 at 2b393ca. Native four-platform CI started at https://github.com/tz1012/mail-hantong/actions/runs/41759293091 . This is a development branch, not a release or installation update.

A remaining architecture boundary was identified before client integration: CLI/MCP cannot load Secrets-owned Unix private signing keys without violating ADR-013. A concrete Core-owned stdio relay proposal is in docs/contracts/proposals/unix-core-relay.md (proposed and unapproved). It keeps the signer and authenticated socket in the Core helper, accepts only fixed profile selectors and bounded RPC frames, rechecks epoch/scope/revocation, never grants authority or replaces terminal confirmation, and preserves Windows v1. The already-approved ADR-030 backend is distinct from this new delegation interface; no relay implementation or client-to-Secrets dependency was added. Human security review remains pending.


2026-11-04 final targeted integration results: Linux native current-schema and v25-to-v26 fixture tests now pass2/2 after supplying metadata and the migration-framework CAS in fixtures, with production DDL unchanged. The native batch passed client8/server6/platform18/Store2 (34 tests). Core binary test now exercises an actual private Unix socket auth-v2 handshake in an isolated subprocess, fresh registration lookup, account/tool checks and revocation: one scenario passed (child and parent outputs both represent that same scenario). Google boundary tests42 passed, including newly portable localhost OAuth callback consumption/timeout and existing fake-provider read/write/Trash, MIME and diagnostic regressions. No external provider or OS credential entry was used. Windows DPAPI synthetic overwrite/reopen/delete regression passed1/1. Fresh Linux affected all-target Clippy (-D warnings, locked/offline) passed after scoped lint fixes; Windows affected all-target check and Clippy had also passed. These are targeted checks, not a claim of a new full-workspace test run.

First native CI feedback: Ubuntu stopped on six Platform Clippy diagnostics; both macOS15 Intel/ARM stopped on fchmodat mode_t being u16 on Darwin versus u32 on Linux. Corrected the native mode_t constant and style diagnostics. A bounded Linux proc-stat parser slice that could panic on a truncated closing-parenthesis record now rejects it, with regression inputs added. Mac revalidation is pending on the corrected source; no Mac runtime or full-mail success claimed. Root HEAD/index and original dirty/untracked work are preserved. Human security review and the unapproved Unix relay contract remain outstanding.

Final Platform delta check after the parser and native mode-type corrections: Linux native unix_ tests18/18 passed (target/unix-platform-native-delta.log). No further source changes are implied by this result.

Publication checkpoint: owned feature/unix-runtime-20261104 branch is now 0a9dede2960acc23d362029619e0e3ea7accd4e9, corrected-source native matrix https://github.com/tz1012/mail-hantong/actions/runs/41761089665 is in progress. The earlier 41759293091 run belongs to superseded 2b393ca and must not be used as the final-source verdict. No main merge, release/tag, package installation, real account action or key/ACL change occurred. Root HEAD remains 2852a31d0d8383d3a593a6479c20bb798b0d798e with an empty index and all original dirty/untracked work retained. This final pointer is recorded locally to avoid triggering another full native matrix for a status-only documentation push.


2026-11-04 user sequencing override: publish the current Windows build before reviewing Go/Rust hybrid feasibility and DietrichGebert/ponytail. macOS/Linux integration validation is deferred until Ponytail is applied. Both owned native CI runs 41759293091 and 41761089665 were cancelled, including their Windows matrix jobs; cancelled runs are not passing evidence. CI is temporarily Windows-only and its four configuration tests pass. Existing Unix source and the approved ADR-030 foundation are preserved; no Unix mail support/release claim or Go rewrite approval is inferred. The distinct Unix relay contract remains a proposal, not implemented.

Windows beta.21 release candidate: based on frozen source 0a9dede plus release documentation/CI sequencing. Includes external publisher configuration packaging fix for Windows alias paths; existing mail/AI behavior, Windows auth-v1, keys and schema26 remain. Fresh Windows affected all-target check/Clippy and synthetic Secrets overwrite/reopen/delete1, packaging11 and CI configuration4 tests passed. Earlier beta20 synthetic Codex/Claude conversations are historical evidence; no new actual provider/mail mutation is claimed. Package, committed-source and publication checks are in progress. Original root HEAD/index, user accounts, secrets and ACLs remain preserved.


2026-11-04 Linux product integration checkpoint: fresh native IPC client14/server6 Unix tests passed (20 total, target/linux-product-relay-tests.log), including fixed relay method allowlist, canonical framing, response ID/EOF/no-retry and auth-v2 checks. Added Core broker classification/readiness and bounded concurrent connection handling, normal auth-v2 loopback for fixed profiles, original kernel-peer binding for terminal review, and a separate larger decoder restricted to core.relay. New decoder cap regression is added but not yet run. Native product compilation exposed cfg return/export errors (fixed by IPC slice) and incomplete headless backend wiring (being corrected). Review also found Linux terminal credential staging must cross foreground-helper/Core processes; process-local staging is insufficient and is being replaced within Secrets by ticket-bound encrypted staging. These are implementation findings, not runnable Linux/complete mail/release evidence. Actual Windows installation/Core/accounts/keys remain unchanged. Human security review pending.


2026-11-04 Linux product integration continued: fixed Unix application framing to retain the existing schema_version/request_id/tool/params envelope inside JSON-RPC, with exact outer/inner identity checks and stable authorization errors. Typed setup/registration/terminal methods remain separate. The native registration fixture now uses the full terminal Get wire contract and strips only its owned copied test executable's debug symbols to preserve the production 64MiB executable admission limit. An earlier run failed on fixture wire shape; no successful account-to-mail integration is claimed from it.

Fresh latest headless vault tests: 4 passed, 0 failed, including wrong-passphrase/CAS, lifetime lock, lock symlink rejection and encrypted reopen (target/linux-product-headless-final.log). Fixed scrypt encrypt/decrypt work factor18 in Secrets to bound hostile ciphertext resource use; dev-only optimization of the existing KDF dependencies preserves the same security cost while avoiding unoptimized loop delays. Core-owned bounded credential-job queue now separates accepted opaque secret submission from cancelable client/socket lifetimes; CLI can poll existing setup status. Added verified pidfd update stop and isolated synthetic lifecycle tests, not yet executed. Native broker/registration/scope, cross-process encrypted staging, relay and lifecycle checks are running sequentially. Portable fake IMAP/SMTP TLS/submission/MOVE fixtures now have independent per-fixture headless vaults without a global backend. Human security review remains pending; no public Unix package, actual provider/login/send/Trash operation, original root product overwrite, installed Core stop, or real key/account mutation occurred.

2026-11-04 native product verification follow-up: Linux CLI138, MCP12, IPC-client Unix16 and Platform Unix29 tests passed. Core registration/terminal Confirm/authenticated account listing/scope Confirm/initial-current scenario passed, including no access before scope confirmation and wrong-account/replayed-ticket denial. Native Core bin suite was37 passed/10 failed: all failures came from the IMAP fixture missing a headless backend; fixture setup is corrected and targeted rerun is pending. Secrets full native library run was211 passed/3 failed (714.65s); it included actual synthetic TLS IMAP read/UID/MOVE and SMTP accepted/rejected/lost-ACK fixtures. The three failures were missing-credential error mapping and a subprocess exact-test filter; targeted reruns now pass IMAP1, SMTP1 and terminal-stage2, without repeating the full suite. Stage consumption verifies and unlinks the exact owned file and fsyncs its directory before releasing plaintext inside Secrets. No external account/server was used.

Fresh Windows workspace all-target check passed on the Unix product source, with one Unix-only unused helper warning subsequently gated to its actual callers; Windows Clippy remains pending. Original installed beta20 Core PID53436, real account storage and root index remain unchanged. Restored Windows/Ubuntu/macOS Intel/macOS ARM native CI after Ponytail application; CI configuration4 and documentation29 tests passed locally, native Mac results remain pending. Mac encrypted terminal staging and Mach-O/package support are connected, not claimed native-verified. Linux installer regression caught an uninitialized uninstall launcher-directory variable introduced during portable checksum handling; fixed and native quoted-path/install/reinstall/unknown-and-modified-file preservation test passed. Security/contract changes remain human-review pending; actual installed-binary PTY and public Unix release are still outstanding.

2026-11-04 native package follow-up: Linux release build completed successfully (four CLI/Core/MCP/TUI binaries, x86_64-unknown-linux-gnu). Fresh Python Windows/Unix packaging regression tests16 passed. Windows workspace all-target Clippy and the freshly built synthetic TUI keyboard/mouse/resize/restoration check passed. Linux Core full bin test checkpoint was44 passed/2 failed; the two Google status fixture failures were corrected and their targeted rerun passed2/2. These are scoped results, not a new whole-workspace pass. Native Mac CI exposed a test-only Linux fixture gate and a moved shutdown Rc; both are corrected in source0f73142. Missing third-party license provenance was added for four dependencies with exact upstream bytes/checksums; Linux292/Mac232 dependency inventories now resolve. Mac native package/check/installed-product workflow41791710819 is running on that corrected source. Linux installed headless package smoke and public beta22 publication remain pending. No actual provider/account/mail mutation, user installation/Core stop or original dirty-tree replacement occurred. Security implementation remains human-review pending.

2026-11-04 native correction checkpoint b1a08d0: macOS ARM workspace all-target Clippy and Core runtime tests2 passed; terminal stage fixture then exposed the OS /var-to-/private/var temporary path alias. Only the owned fixture directory is canonicalized; production no-follow guards remain unchanged. The preceding native Mac compile errors (stop-response serde import and unused Linux/Windows protected-send fixture) were repaired without adding a dependency. Native Linux broker batch6 passed/1 failed on one AUTH_REQUIRED response before expected scope denial; identical targeted product-flow rerun passed1/1 on unchanged Linux behavior,18.25s child. The failed run remains evidence of a timing/reliability concern, not deleted or converted to a pass. Release packaging detected intervening source changes and refused output as designed; stable-source packaging is being rerun. Mac workflow41793714900 and installed-product smoke are pending, no public beta22 asset yet.

2026-11-04 installed-product findings and corrections: Linux candidate-a7c642b failed its real protected-input PTY smoke because the generic bounded reader waited for EOF after Enter. Secrets now reads a bounded line and restores echo immediately; fresh terminal_secret_line_reader tests2/2 passed. The corrected d245f44 package reached the no-account menu, but post-CLI readiness exposed session SIGHUP terminating Core. CLI now requests an internal background startup; Core retains protected TTY input through recovery, then setsid/redirects stdio before serving. Foreground run and Windows run remain unchanged. This lifecycle correction is awaiting installed-package revalidation. The old candidates are preserved and not published. Durable scripts/test_linux_installed_product.py now verifies protected first-start, post-CLI readiness, encrypted DB, verified stop, wrong-unlock ciphertext preservation, reopen and non-TTY failure without real accounts.

Windows d245f44 beta22 package built successfully; isolated Unicode/space-path install/version/uninstall preserved unknown files, data sentinel, actual installation pointer and User PATH. Fresh Windows affected all-target Clippy(-D warnings) also passed after the Unix-only input/lifecycle corrections. Native Mac ARM/Intel compile, installer and cross-process encrypted stage tests passed; installed startup still failed at storage. macOS CI now explicitly prepares/restores a disposable unlocked Keychain rather than weakening product backend errors or adding plaintext fallback. Current source aafb818, native Mac workflow41797375449, beta22 publication pending. Original root HEAD/index, installed beta20 Core and real accounts/keys remain untouched. All security-sensitive implementation remains human-review pending.

2026-11-04 beta22 native release verification continued: source3eabc14b9ed8463aa58db0304dbf86d6da00bb94, canonical source SHA256 c50c01dd88fe75e2b643e580fb25ba85430639d6a4e9e55650cf7595c86332fd. macOS native package workflow41801224195 passed BOTH Intel and Apple Silicon jobs. Each completed native workspace all-target Clippy, scoped SQLCipher/Core/socket/stop/protected-stage/input tests, synthetic installer checks and actual installed-product first startup/encrypted DB/status/stop/reopen against an owned disposable CI Keychain. Previous key_read STORE_FAILURE occurred with a mismatched synthetic HOME; preserving the runner's real HOME while explicitly isolating program paths fixed the fixture. No product encryption fallback or user Keychain modification was introduced.

Fresh Linux final package installed smoke passed on the same source: GUI and D-Bus absent, explicit headless backend selection, echo-disabled passphrase initialization, no-account menu, Core readiness after CLI session exit, encrypted DB, verified stop, wrong-unlock ciphertext unchanged, correct reopen, non-TTY explicit init HUMAN_STEP_REQUIRED. Package SHA256 fad6468c293c7c935bb2e65786122d1ebed868316ef0391846867fc88e3aa9ab. Both Mac archives and Windows archive have matching source hash and verified per-file manifests/binary hashes. Final Windows packaging with updated guide, public beta22 assets and site remain in progress. No external provider login, real mail/send/Trash or AI conversation on Unix is claimed; prior synthetic-provider/runtime evidence remains separate. Original root dirty changes/index, installed beta20 Core PID53436 and real Windows accounts/keys remain preserved. Security-sensitive implementation remains human-review pending.
2026-11-04 beta.22 PUBLIC RELEASE completed: https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.22, immutable tag/source commit5eb5c93b63549bf8c01b668bd6d3a058e90ff066, executable source SHA256 c50c01dd88fe75e2b643e580fb25ba85430639d6a4e9e55650cf7595c86332fd. Ten assets include four native archives (Windows x64, Linux x64 glibc2.39+, macOS15 Intel/Apple Silicon), per-platform build metadata, committed source archive and combined SHA256SUMS. Every public asset was re-downloaded without authentication and its checksum matched. Internal manifests, binary hashes, shared approved publisher asset hash and committed-source canonical hash were verified before publication. Windows ZIP SHA256 fa1d9e00b3ea68eb44a26a6be701c3aef94a68a11f5f93c0bfb4d49df7fb9e60; Linux tar.gz fad6468c293c7c935bb2e65786122d1ebed868316ef0391846867fc88e3aa9ab; Mac ARM4acdf650c3bf88563fd42f1372036fd811e58a538bc1b14266624830f661b907; Mac Intel de50642e7e6d2b2ec83cd4640f5b798d4f83def0214b08c11f8897480109352a.

Fresh final Windows isolated install/version/uninstall passed, preserving unknown files, separate data, actual installation pointer and User PATH (target/beta22-published-install-verification.json). Public Linux installer selected beta22, downloaded and verified the release, installed successfully; the installed CLI's interactive `hantong update` also completed exit0 with synthetic data/unknown files preserved. Final Linux installed startup/restart proof and both Mac native CI jobs41801224195 remain the runtime evidence. Documentation29 tests and traceability341 criteria/source-hash check passed; no repeated whole-workspace test count is claimed. Site commit53c6f6ef159299f5851179651a1d95fd8a0cb9a6 deployed successfully (Pages41804688608), and live beta22/pinned Unix installer content was checked. README/CLI guide/UNIX-INSTALL/OS plan now distinguish supported package/runtime behavior from untested actual Unix provider login/mail/send/Trash/AI. Security-sensitive implementation remains HUMAN REVIEW PENDING. Linux desktop Secret Service, LinuxARM package, MacSSH vault, advanced custom Google client on Unix, real Unix accounts/provider conversations and macOS notarization remain explicit limitations; SMTP attachments and complete background synchronization are unchanged limitations. Original D:/Coding/emailclient HEAD2852a31/index empty/dirty changes and installed beta20 CorePID53436/hashfdb9a8c9310358da280f9cf8c343f399b8085daf65a7e6716f4af16602d5f6f0/real accounts remain preserved. No actual provider credentials, login, mail send/Trash, ACL repair or installed-user-Core replacement was performed during this Unix release work.
2026-11-04 user requested synthetic actual-use screenshots as KakaoTalk thumbnails on installation and distribution pages. Published site commita465c408262a01863cd23de0e2064e44d2bcdbff (Pages41807168836 success): index.html and new download.html now provide complete Open Graph/Twitter metadata with distinct canonical URLs and one versioned public HTTPS image. Added assets/hantong-share-v1.png (1280x640,180877 bytes) and hantong-tui-demo-v1.png (1600x992,93854 bytes), generated from the unmodified beta22 terminal_view.rs renderer with explicit synthetic Frame data only; fake personal/work tags, titles, From/To/Cc/date/body use example.invalid. The helper compiles only the pure renderer, never launches a real account/Core/provider or reads actual mail. Provenance/image hashes and reproducible Rust/Pillow authoring helpers live in the site tools/assets directories. PNG dimensions/size, both pages' unique OG fields, alt text, local links, four platform downloads and image hashes passed tools/check-share-preview.py; images were visually inspected. No product code/dependency or release binary changed.

Both deployed HTML documents and image bytes were fetched publicly and match local artifacts, including Kakao-style and generic scraper User-Agent requests. This proves public metadata/image access, not actual KakaoTalk cache/card rendering. GitHub documents repository social previews, but provides no supported per-release OG-image setting; rather than falsely claiming control of GitHub's release thumbnail, added the image and explicit Kakao-shareable download.html link to the existing public beta22 release body. Existing notes and all ten release assets were preserved; exact body and public rendered release image/link were verified. Installation share URL https://tz1012.github.io/mail-hantong/ and distribution share URL https://tz1012.github.io/mail-hantong/download.html. Original dirty root, installed Windows Core/accounts/keys and immutable beta22 artifacts remain untouched. No message was sent to KakaoTalk or any other person, no social/account login or repository security setting was changed. Browser automation through agent-desktop was unnecessary for publication; its read-only status exposed an existing private-state DACL rejection, so no ACL/settings repair was attempted. Old Kakao caches remain outside this deployment verification.
2026-11-04 beta22 explanation expansion + user zele.sh design/TOC direction completed. In owned site worktree, added site DESIGN.md from the actual public reference HTML and1440x1100 headless capture (target/zele-reference-1440.png). Adapted the reference's centered serif introduction, whitespace, subtle dividers and grouped sticky contents; HanTong's own multi-account/tagged Inbox, CLI plus BBS TUI, and AI with explicit terminal send review lead the page. No competitor logos/copy, calendar/free-address features or unmeasured performance claims were imported. Expanded beta22 source-grounded Windows/Mac/Linux conditions, source-pinned install commands, first-run/background Core explanation, Google versus IMAP/SMTP setup, work menu6, send/Trash feature scope, TUI keys/themes/demo, AI/native Claude versus API boundaries, updates/FAQ and current versus historical verification. Google and IMAP Trash prerequisites are distinguished; SMTP attachment omission and pending real Unix provider/AI/human-review gates remain explicit. Download page now links the guide's actual steps; theme-only browser-local storage is disclosed on privacy page. No product code, account/key, user browser or immutable release asset changed.

Fresh site checks passed: node syntax; duplicate HTML ID/local fragment links/two OG documents/four platform URLs/PNG provenance; light+dark text/link/button contrast >=4.5; isolated headless Chrome exact1440 and390 viewport checks for TOC filtering/no-results/Escape/slash shortcut, theme toggle, clipboard success and denial, desktop sticky sidebar, mobile collapse, page overflow and guide/download/privacy layout. Desktop/mobile/light/dark/body screenshots were inspected (target/hantong-guide-design-qa). Initial draft QA found a decorative pseudo-element causing mobile overflow and a cross-realm probe timing issue; fixed CSS and probe event-loop synchronization, then exact viewport reruns passed. No Rust whole-suite rerun for static guide changes.

Publication recovered from GitHub service failures: two normal git pushes returned internal server error without updating gh-pages. Git Data API then published reviewed static-site tree through a no-force fast-forward only after checking exact remote parent and equality with local reviewed tree; authoritative published commitff335398d323c767c737868f6827bdb71c12e42b (local pre-publication commite161788fd7793f4a639198faf2dd08ef3dad1f51 retained on prior owned branch). Pages41810204015 build passed but first deploy returned502; reran only failed jobs, attempt2 succeeded. Owned site checkout now tracks site/beta22-zele-guide-20261104 at published commit, clean. Live index/download/CSS/JS/privacy/terms bytes match the verified source (canonical LF); Kakao-style crawler still receives the unchanged versioned share-image URL. Website https://tz1012.github.io/mail-hantong/ and download.html are published. Original root dirty changes/index and installed beta20 CorePID53436 remain preserved; no real account/provider/AI inference, credential handling, mail mutation or user-desktop input occurred.

2026-11-04 external-agent/desktop MCP connection update (post-beta22, local source only): user requested stronger external connectivity and proactive new-mail delivery; original requirements already include MCP-01 multi-host stdio and MCP-07/PUSH-08 receive subscriptions (M7-01/03/07/11 remain not_started). Implemented local convenience commands in owned linux-product-20261104 worktree: hantong-mcp config emits mcpServers JSON with absolute current_exe and empty args (external identity, no shell, no host-file writes); doctor probes existing bootstrap transport then authenticated external tool discovery, emits only state/stable errors/exposed tool names/action, no mail calls or account/profile/secret output. Bootstrap intentionally reports mail_ready=false, so diagnostic uses transport readiness before authenticated discovery. English/Korean help and EXTERNAL-MCP-GUIDE explain manual registration/review, fixed shared external profile, version-pinned config regeneration after updates, and published beta22 lacking these new commands. CLI-LAUNCH no longer implies external Trash is already enabled.

Fresh Windows verification: cargo test -p hantong-mcp --all-targets --locked --offline passed 20 unit + 3 actual-child-process integration tests (23 total). Two concurrent stdio child servers initialize/ping independently, reject direct send.execute without touching Core/profile, maintain protocol-only stdout, and terminate on EOF. Config JSON and Korean help tested through real binaries; doctor failure/no-mail-effects/discovery filtering tested with fake backend only. Fresh scoped Clippy all-targets -D warnings passed after the readiness correction. Traceability source hash refreshed for 341 criteria without promoting unverified ACs. Targeted format and diff checks recorded below. These tests are not real desktop-product interoperability, real account operations, Unix runtime revalidation or a whole-workspace run.

Concrete unapproved change proposal docs/contracts/proposals/external-mcp-mail-v3.md now specifies external single-message Trash request + existing terminal confirmation, opt-in Core receive tracking (60-second default), atomic sync/event ledger, bounded 7-day/10000-per-account retention, account/epoch-bound cursors, pending/ack, MCP resource updates and local CLI watch/notify/hook fallback. No bodies/subjects/addresses in notifications; consent/revocation rechecked before delivery; no automatic send/delete, arbitrary shell/URL callback or third-party message. Existing grants stay unchanged until explicit new v3 registration/consent. Per-host identities/HTTP excluded from this iteration. An MCP resource update alone is not evidence that a desktop host automatically wakes its model. Notification and external Trash implementation remain pending the concrete contract proposal adoption required by AGENTS section7; no such feature or human security review is marked complete. No new dependency, provider call, actual account/mail mutation, installed Core replacement, root reset/clean, tag/release/site publication. Original root dirty/untracked/index and installed beta20 runtime/accounts/keys preserved. Next: approve exact v3 change scope, then implement and verify receive-to-host behavior with fake provider + supporting host and fallback, followed by release validation.


2026-11-04 approved external MCP v3 implementation checkpoint (post-beta22): user confirmed the concrete two-item proposal. Implemented explicit v3 registration/replacement/profile publication and a distinct v3 review hash domain; frozen read3/v2 grants are unchanged. External send and single-message Gmail/IMAP Trash requests remain pending until Core-owned foreground terminal confirmation and current owner/epoch/account/data-transfer consent rechecks. Migration26?27 creates tracking/log/source/random-issued-cursor tables without granting or enabling any registration. Opt-in Core INBOX observations use a bounded Gmail IDs-only fingerprint or IMAP EXAMINE UIDVALIDITY/UIDNEXT, commit source+metadata event atomically, and recheck current consent, tracking and typed provider binding after I/O. No message content or provider writes are part of monitoring. Shared read slot/audit cancellation guards fail closed until a detached bounded read is reaped or Core restarts.

MCP resources/list/read/subscribe/unsubscribe now provide mail-hantong://events; a 5-second absolute polling deadline emits only standard URI update hints while stdin is idle or busy. Tracking defaults off, repeated subscribe preserves pending, unsubscribe stays local, and CLI watch/notify pending/ack is available to host-owned scheduling hooks. Seven-day/10000-per-account retention, exact enabled-scope/owner/epoch random persisted cursors, individual-event ack, reopen, expiry, consent revocation and disable/re-enable baselines are covered. Shared external identity means ack is shared across hosts. This is coalesced INBOX change detection, not exact new-message counts, bulk history sync, arbitrary webhook/shell execution, or proof that a real desktop app starts an LLM turn automatically.

Fresh verification at this checkpoint: Windows application79/policy11; MCP27 unit+3 real-child stdio tests; Store receive_events6, MCP registration16 and populated full v26?v27 migration1 passed; v3 approval domain1, Gmail IDs-only fake-provider1, Core cancellation lease1, contracts/client/server MCP5 passed. Windows and native Linux whole-workspace all-target check/Clippy -D warnings passed. Linux MCP23 unit+3 real-child stdio tests passed. Docs29 and architecture checks passed. A stale schema-v27 snapshot initially failed framework migration validation; regenerated from canonical current DDL and the actual encrypted populated migration rerun passed. Linux diagnostic rendering initially hit rustc1.95 ICE; short diagnostics exposed an E0716 temporary RefCell borrow in Unix terminal review, fixed by retaining the borrow binding; fresh native workspace checks passed without changing toolchain. No failed tests were removed or skipped. Remaining fresh native Core/schema tests and Intel/ARM Mac candidate CI are being run; earlier beta22 native results are not substituted. Human security review remains pending. No real provider/account/key/ACL access, running Core replacement, user desktop input or release/tag change occurred. Original D:/Coding/emailclient dirty/untracked/index state remains preserved. The updated guide explicitly distinguishes public beta22 from this unreleased source candidate.


2026-11-04 external MCP v3 verification continuation: implementation approval is confirmed by the user's reply ??????; no further actual-account consent is inferred. Product source is in C:/Users/cyary/.config/superpowers/worktrees/emailclient/linux-product-20261104, branch feature/linux-product-20261104, implementation9b01e9825483b36518e9b0fc1110a59f6a08a770 with refreshed traceability8152fd8 (canonical source hash99159f61710a06faf8d5a72e8c37e5840b7b44bd7566027788c692aeb2c82aa7). Original D:/Coding/emailclient HEAD/index/380 dirty+untracked paths are preserved; only this progress record is appended here. Existing beta20 CorePID53436 remains running from the same executable, SHA256 fdb9a8c9310358da280f9cf8c343f399b8085daf65a7e6716f4af16602d5f6f0. No actual account/provider/credential/ACL/mail write or desktop input occurred.

Implemented explicit v3 external MCP grant/terminal review and single-message Trash requests, metadata-only opt-in receive monitoring, durable source/event/cursor/ack Store schema27, resources/read/subscribe idle notifications, config/doctor and watch/notify fallback. Existing read3/v2 and built-in AI identities do not gain grants. Core polls ready Gmail/IMAP INBOX about60s, MCP polls local events about5s; hints contain no body/subject/address and never authorize writes or guarantee an LLM turn. Baseline, bounded retention, issued owner/epoch/scope cursors, consent revocation, multi-reader ack, restart, disable/re-enable and blocking-read cancellation are covered. Current guide/contracts remain explicitly post-beta22 candidate; public beta22 and site assets are unchanged. Security boundaries remain [?? ?? ??].

Fresh results: Windows application79/policy11; MCP27 unit+3 real-stdio child tests; Store receive_events6/MCP registration16/populated migration26?27 one; approval old/v2/v3 domains3; Secrets IDs-only fake provider1; Core cancellation1; contracts/client/server MCP5; docs29/CI config4; architecture, fmt, Windows+Linux whole-workspace all-target check/Clippy -D warnings passed. Native Linux MCP23+3, approval/Secrets/Core1 each and current/v25?26/v26?27 schema3 passed. No whole 1906-style test repetition or new actual provider/desktop-product test is claimed. Independent bounded reviews found no remaining CRITICAL/HIGH/MEDIUM blocker after removing the owner-free event writer and replacing account-ID-prefix adapter routing with stored provider metadata plus typed binding validation.

Native Mac candidate CI https://github.com/tz1012/mail-hantong/actions/runs/41823585069 uses8152fd8. Apple Silicon passed Clippy, new MCP26, schema3, v3/wire/Core/Secrets regressions and installed-product Keychain/encrypted database/start/stop/reopen smoke. Intel passed native integration checks and is packaging; final installed smoke verdict is still pending here. The first CI41823250527 stopped before native checks on a stale traceability hash after final contract whitespace trimming; refreshed the proof, cancelled the superseded run and started the current exact ref. No test was removed/skipped. No new release tag, installed-user binary replacement, main merge or site publication yet.


2026-11-04 beta23 release preparation: final review found a receive fairness bug before publication: the fixed MCP limit60/minute and foreground polling could repeatedly admit only the first accounts out of128. Added a persistent next-account position to resume after the last policy-admitted target; no authority or request budget was increased. Real ReadPolicy regression reserves12 foreground calls, processes128 targets over3 rate windows and verifies the budget stays60; Windows receive-worker2/2 passed, independent bounded review found no remaining blocker. Superseded unpublished Windows/Linux candidate packages are preserved under target/*-beta23-pre-fairness. Mac run41826312871 on the superseded guide/package source was cancelled before use; the completed earlier8152fd8 native runtime run41823585069 is evidence for the pre-fairness checkpoint only. Final delta verification and four fresh same-source packages follow before the beta23 release. Windows and Unix packages now include EXTERNAL-MCP-GUIDE.md in the verified manifest; Windows package tests12 and Unix archive tests5 passed. Updated release guides and site preparation preserve the existing beta22 downloads until the new release is public. No actual user runtime/account/store/ACL change occurred.


2026-11-04 beta.23 published ? external MCP and receive hints [?? ?? ??]: release https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.23 is public prerelease (published2026-11-04T12:28:36Z), immutable tag/target f76fc2a47ab36575a407bfae11f3c5645a5ee927, canonical source096279016be205595ea0b8983049d4ae1528fae46daf6b34ee6da0e8093c4da3. Ten assets include four native archives (Windows x64, Linux x64, Mac Intel/ARM), matching per-platform build metadata, committed-source ZIP and SHA256SUMS. All four manifests and executable hashes match; the source archive reproduces the frozen source hash; approved publisher asset bytes are unchanged across all four. Verified all ten GitHub upload digests before publishing, then downloaded all ten anonymously over HTTPS and matched every byte hash against the reviewed files. New v3 Trash requests and receive tracking require explicit external registration review; frozen v2 and built-in AI grants are not auto-expanded. Mailbox hints are bounded, metadata-only, opt-in and not a guarantee of exact new-mail counts or automatic LLM turns.

Final delta evidence: receive worker2/2 passed on Windows and Linux, including128 accounts sharing the60-RPM budget with foreground polling; final Core all-target Clippy -D warnings passed on both. Same-source native Mac CI https://github.com/tz1012/mail-hantong/actions/runs/41827738310 passed on Intel and Apple Silicon, including workspace Clippy, MCP stdio26, schema3, updated Core/Secrets/approval/wire regressions, installer checks, package build and installed-product Keychain/encrypted-DB/start/stop/reopen smoke. Earlier full-workspace check/Clippy, Windows MCP30, Store events6/registration16/populated migration1, application79/policy11, protocol5, approval domains3, docs29 and package tests12+5 remain separately identified evidence; no whole-suite count is claimed for this source. Final Windows Unicode-path isolated install/version/uninstall passed with existing installation pointer/User PATH/separate synthetic data/unknown files preserved. Final Linux installed package passed GUI/D-Bus-absent headless setup, protected synthetic input, encrypted DB, Core readiness, verified stop, wrong-unlock ciphertext preservation, correct reopen and non-TTY refusal. Public Unix bootstrap selected beta23, interactive hantong update succeeded, and synthetic data/unknown files remained unchanged.

Published guide/download pages at https://tz1012.github.io/mail-hantong/ and download.html now explain v3 config/doctor/register/review, supported resource hosts, watch/notify fallback, explicit write confirmation and limits. Reviewed site commit7f98df25f42a9726b8b2d702b086aeaf30b66537, Pages41829790436 succeeded. Node syntax, unique IDs/local links/anchors and all four platform URLs passed; live index/download/JS/CSS and the existing synthetic Kakao share image match reviewed bytes. Actual Kakao cached-card rendering was not checked and no message was sent. No main merge, dependency addition, actual provider login/send/Trash/LLM desktop conversation or actual-user program update occurred. Existing Windows CorePID53436 and executable hashfdb9a8c9310358da280f9cf8c343f399b8085daf65a7e6716f4af16602d5f6f0 remain unchanged; original D:/Coding/emailclient HEAD/index/dirty/untracked work is preserved. Progress was appended in both original root and the owned product worktree. Human security review, real external desktop interoperability/automatic wake behavior, macOS signing/notarization, remote HTTP and per-host identities remain outside this completion claim.


## 2026-11-04 ? ?? ???: ?? ???? ?????? ?? ?? ??

??? ??? ?? ? ??? ??? AI?, ? ??? ??????? ????. ?? stdio MCP ?? ? ???????? ?? ??? ?? ??/??? ?? ? opt-in ??? ?? ?? ? ??????CLI/TUI ??? ??? ??? ?????. ???? ???? OG ??? ?? ????? ???. ?? Zele ?? ?????? ????beta.23 ?? ??? ???? ??? ??? ?? ?? ????? ????. DESIGN.md? ?? ??? ????.

??: site commit 9501b45, gh-pages ?? fast-forward. Pages https://github.com/tz1012/mail-hantong/actions/runs/41834310062 success. 1440/390 ???????, ??????Escape?????????? ??/?????? overflow, Node ??, ?? metadata/????4? ??? ???? ? diff ?? ??. ?? index/download/CSS/JS? ?? commit bytes? ????(Windows ?? ?? CRLF? git LF ??? ??). ?? ????? ???? ??? ?? Rust ?????? ??? ???? ???. ?? ???? ?? ????? ?? ????? LLM ?? ??? ??? ???? ???. ?? ??????Core??? ??? ??? ???? ???.


## 2026-11-05 - beta.23 Unix Google callback/OFFLINE correction [human security review required]

User evidence is an OAuth callback followed by OFFLINE with no registered account, plus a separate unauthenticated HTTPS EOF observation. The actual failed provider/token/Keyring/account stage cannot be reconstructed from the discarded stderr; EOF remains a candidate for that machine, not a proven incident root cause. Source inspection and a verified test-CA Rustls duplex reproduction establish a real beta.23 defect: waiting for TLS close after a complete HTTP response can turn it into OFFLINE when close_notify is omitted. The corrected bounded reader returns only after exact Content-Length or strict chunked completion. Truncated, ambiguous, oversized and invalid frames still fail; no TLS certificate/hostname verification is disabled and no general EOF suppression or retry is added. Original strict close-delimited/trailer limits remain.

Added fixed endpoint/stage/kind/error diagnostics in Secrets and failure-stage diagnostics around Unix protected storage, account registration and profile/scope finalization. Diagnostic write failures do not change authorization results; no URL, account/request ID, code/token/key or provider response text is accepted. UNIX-INSTALL documents explicit foreground Core stderr capture because normal background detachment discards standard streams. Receipt/protocol/schema and dependencies are unchanged. Existing secret input, scopes, account identity and registration ordering are preserved.

Fresh validation: Linux Secrets Google tests51 passed (11 HTTP framing/TLS tests including verified-CA missing-close_notify reproduction and every prefix boundary), Unix Core Google3 passed, Windows Secrets Google49 passed with native Cargo exit0, Linux Secrets+Core all-target Clippy -D warnings and Windows Secrets all-target Clippy passed, CI config4 passed, fmt/diff/traceability checks passed. The first Linux verbose diagnostic renderer crashed in rustc1.95 while reporting transient warnings; short-format rerun passed. A Windows PowerShell stderr redirection initially reported shell error despite all49 tests passing; the subprocess-captured rerun returned native exit0. Independent bounded source review found no remaining blocker after documenting Mac install-root and incomplete instrumentation limits. No actual Google login, GNOME Keyring access, user data/keys/Core mutation or live mail operation was performed. Preparing beta.24 native artifacts; no deployment is claimed by this checkpoint.


## 2026-11-05 - IMAP protected-input diagnostics combined into beta.24 [human security review required]

The additional Linux report establishes only hidden-input failure with APPROVAL_REQUIRED, not missing authorization or successful Daum authentication. Confirmed the frontend collapsed all capture/staging SecretErrors into that code. The Unix frontend now retains stable invalid input / human step / expired / internal I/O or protection / binding failure categories, and attempts pending-review cancellation once without a SecretSubmit on capture failure. Expiry keeps precedence over cleanup errors; other cleanup failure codes remain visible. Secrets records fixed input versus protected-staging kinds, including empty input, TTY unavailability, input I/O, expiry before capture and expiry after input. Empty input uses the existing EmptySecret variant; credentials never leave Secrets and diagnostic output contains no value, size/hash, path, account ID or key. No dependency, protocol, grant or storage-format change. The existing blocking terminal reader still waits for Enter; staging and submission expiry checks remain enforced. Existing encrypted-stage cleanup after later submission/IPC failures is unchanged and remains a review/operational caveat.

Fresh focused results: Linux Secrets terminal_secret12 top-level tests plus its child cross-process encryption check passed; Windows corresponding8 passed, Linux Core frontend2 passed, Windows Secrets all-target Clippy passed, Linux Secrets+Core all-target Clippy passed, fmt/diff/CI-config4/traceability passed. Renaming the new tests with SEC08 IDs does not change behavior; final native CI will execute the exact final source. An advisor's cached6 results and unconfigured missing-Perl/OpenSSL Cargo attempt are not counted as current verification. Parent tests used the pinned configured native wrappers. Independent review found no authority widening or diagnostic credential leak. No app password was generated/revoked/read, no provider login occurred, and no real store/Core/installation was changed.

Google-only checkpoint source8583205 passed local Linux Google51/Core3 and Windows Google49 plus target Clippy. Mac native broad Google tests found pre-existing publisher fixture roots using macOS /var aliases; test-only canonicalization fixed that without weakening product symlink checks. Source d73668c then passed both Mac native CI jobs41955584165 (Google51/Core3, workspace Clippy, package and installed Keychain/encryptedDB/restart smoke). Those are Google checkpoint results, not final combined-source evidence. Superseded unpublished packages/source ZIPs are preserved. Combined final source hash a390a7885d2b1007631d30fe6e4663c0afdafad2623c4186262cb8e74e2bb406 will be packaged and verified on all four native targets before publication. Actual Google/Daum failures still need user-side retest; GNOME Keyring interoperability and human security review remain pending.
