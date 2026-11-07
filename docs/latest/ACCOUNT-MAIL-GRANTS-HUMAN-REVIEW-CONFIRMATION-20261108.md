# 계정별 메일 권한 v2 — 사용자 보안 검토 완료 확인

수신일: 2026-11-08. 이 문서는 사용자의 실제 확인을 기록하며 에이전트가 대신 수행한 사람 검토 보고서가 아니다.

## 실제 확인과 정확한 대상

사용자는 다음 전체 커밋과 범위 문서·전체 차분을 명시한 질문에 답했다.

- 검토 대상: `3a21411c2f7d9930b8b55faf86459f3637bc991b`
- 비교 기준: `c22e1bf1507430fd88e3fa1625d96ed09b2e3c40`
- 범위 문서: 해당 대상 커밋의 `docs/latest/ACCOUNT-MAIL-GRANTS-REVIEW-20261108.md`
- 질문: “새 후보 3a21411c2f7d9930b8b55faf86459f3637bc991b 전체의 실제 사람 보안 검토가 완료됐나요?”
- 사용자 확인 원문:

> 모두 완료야

검토 완료 확인자는 이 대화의 사용자다. 별도의 실명·개별 발견 사항·항목별 판정·검토 시각은 제공되지 않았으므로 만들어 넣지 않는다. 이 응답을 위 대상 전체에 대한 실제 사람 보안 검토 완료 확인으로 접수한다. 더 이상 해당 대상의 사람 확인이 전혀 없다고 표시하지 않는다. 이미 주어진 조건부 공식 배포 승인도 유지한다.

## 이후 변경과 검증의 구분

현재 검사·패키지 대상 제품 커밋은 `40e574358890f82a78a7e74bee9caa73e9810f4d`다. 위 확인을 미래 코드나 payload/digest의 검토로 확대하지 않는다.

3a21411 이후 제품 소스 차이는 Secrets 두 파일의 다음 네 cfg 선언 변경이다.

1. `terminal_secret.rs`의 시험용 tokio trait import: 기존 IMAP 시험과 같은 Windows/Linux test 범위.
2. 같은 파일의 새 `consume_terminal_imap_probe_with_stream`: 같은 Windows/Linux test 범위.
3. `imap_probe/windows.rs`의 새 `probe_with_credential_stream`: 같은 Windows/Linux test 범위.
4. `read_masked_terminal_line` 기본 시간제한 wrapper: 유일한 caller인 Linux headless vault와 같은 Linux 범위. Mac 보호 입력이 사용하는 공유 `read_masked_terminal_line_until`은 계속 Mac에서 제공된다.

이는 네이티브 Mac Clippy/시험 빌드 오류 수정을 위한 변경이다. 기존 실행 가능한 시험을 삭제하거나 끄지 않았고, 제품 자격증명·TLS·권한 검사·공유 입력의 내용을 바꾸지 않았다. 별도 자동 독립 검토의 cfg 일관성 PASS는 실제 사람 검토와 구분한다. 이 좁은 후속 차분까지 사람이 읽었다는 개별 확인은 아직 없으며, 위 3a 확인을 해당 차분에 소급 적용하지 않는다.

## 남은 실제 배포 조건

40e57435의 Linux 전체 검사와 Linux 패키지·격리 설치/업데이트/Core 재시작 gate는 통과했다. Mac ARM·Intel 전체 검사도 통과했다. Windows 전체 검사는 조회 시 진행 중이다. Windows 같은 제품 커밋의 후보 패키지·격리 beta29 업데이트·해시/구버전/합성 sentinel 보존은 통과했다.

Mac ARM 패키지의 공식 beta29 DB·Keychain 호환성 gate는 실패했고, 별도 합성 자격증명 호환성 gate는 진행 중이다. 첫 gate의 완료 로그가 아직 제공되지 않아 세부 실패 단계는 확정하지 않는다. 실패를 승인으로 면제하거나 읽기 거부를 호환성 성공으로 계산하지 않는다. 설치 제품의 합성 전체 메일 흐름 증거와 송파 Gmail 두 개·다음 한 개의 실제 목록/본문 조회도 완료되지 않았다.

이 문서는 검사 성공·공식 배포·송파 적용·계정 권한 부여 완료를 뜻하지 않는다. 실제 계정·금고·메일·키·ACL 및 기존 c22 private packet을 보존한다.
