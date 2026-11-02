# 설치부터 이용까지: 실제 적용 전에 필요한 두 결정

상태: 구현 검증 중 / 사람 보안 검토 필요. 이 문서는 승인 증거가 아니다.

2026-11-02 후속 상태: 사용자가 "오케이 승인할게. 둘다"라고 명시적으로 승인했다.
아래 두 제안에 대한 실행 권한은 부여됐으며 승인 사실은 ADR-018과 PROGRESS에도
기록한다. 사용자 추가 질문에 따라 위험 0을 보장하지 않으며, 앱 사칭·한도 남용
위험과 개인 토큰 보호를 구분해 설명했다. 이 승인은 실제 보안 코드 검토 완료
증거가 아니다. 아래 제안 본문은 승인 대상의 기록으로 보존한다.

실행 결과: 기존 저장소 복구는 완료했다. 설치된 preview.5에서 상호 인증과
기존 Gmail 두 계정 ready를 확인했고 양쪽 원본은 보존했다. 재로그인·재동의·
메일 작업은 하지 않았다. 게시자 설정은 Console에서 재다운로드가 불가능해
원래 저장한 Desktop JSON 경로를 확인 중이다. 실제 배포 자산 포함·공개는
아직 수행하지 않았다. 상세 결과와 백업 위치는 PROGRESS 최신 항목에 기록했다.

후속 실행 결과: 사용자가 신규 클라이언트 생성을 별도로 허용했다. 배포 전용
Desktop 클라이언트 v2의 JSON을 보호 경로에 보관하고 실제 설정 포함 preview.6을
빌드·설치했다. 첫 생성분의 도구 출력 노출 사고와 삭제·대체 조치는 PROGRESS에
기록했다. 새 설치의 설정 무결성·시작과 기존 두 계정 인증은 통과했다.
새 클라이언트의 실제 로그인/동의 시험과 보안 코드 사람 검토·공개 게시는 남아 있다.
이 문서의 이전 단락은 당시 상태이며 최신 판정은 PROGRESS의 마지막 항목을 따른다.

## 1. 게시자 Google Desktop 앱 설정의 배포

일반 사용자는 Google Cloud 프로젝트나 앱 자격증명을 만들지 않는다. 게시자가
소유한 Desktop OAuth 앱 설정을 설치 패키지의 데이터 자산으로 제공하고,
프로그램의 Secrets 모듈만 이 값을 사용한다. 사용자에게 보이는 과정은
Google 브라우저 로그인·동의와 기존 Windows 보호 승인뿐이다.

검토할 구현은 `docs/contracts/publisher-google-configuration.md`,
`crates/hantong-secrets/src/google/publisher_config.rs`,
`apps/mail-hantong-core/src/setup_google.rs`, `scripts/package_cli.py`다.
외부 파일을 선택적으로 패키징하고, manifest 소유·크기·SHA-256·Desktop 형식·
고정 Google endpoint를 검사한다. 소스·로그·테스트·커밋에 실제 값을 넣지 않는다.
현재 작업에서는 합성 값만 시험했으며 실제 자산은 준비·추출·포함하지 않았다.

현재 ADR-018의 **“No publisher secret is shipped.”**와 직접 충돌한다.
현재 앱은 과거 실제 시험에서 secret 없이 token 교환에 실패했으므로 client ID만
넣으면 된다고 가정하거나 필수 secret을 제거하지 않는다. Desktop 앱에 배포된
client secret은 추출될 수 있으므로 기밀성을 보장할 수 없다. 숨김/암호화와
함께 키를 제공하는 방법으로 이 사실을 감추지 않는다. 계정 토큰은 계속 Windows
보호 저장소에만 보관하고 PKCE/state/loopback·사람 승인을 유지한다.

근거: [Google Desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app),
[RFC 8252 §8.4](https://datatracker.ietf.org/doc/html/rfc8252#section-8.4).
문서의 secret optional 표기만으로 이 프로젝트의 실제 secret-missing 관측을
무시하지 않는다.

필요한 결정: 소스에서 제외한 **게시자 Desktop 앱 설정 데이터 자산을 실제
설치 패키지에 포함·배포**하도록 ADR-018의 해당 제한을 변경할지 여부.
실제 값은 채팅이나 CLI 인수로 받지 않으며 기존 계정의 보호 번들에서 추출하지
않는다. 승인 이후에도 게시자 제공 경로와 실제 파일은 보호된 취급으로 준비한다.
Google 검증 심사 제출/통과는 이 결정이나 In production 전환과 별개다.

## 2. 기존 개발 환경 데이터의 명시적 복구

기존 계정 상태는 Codex 패키지의 LocalCache/Local/mail-hantong 아래에 있고,
일반 LocalAppData/mail-hantong에는 별도의 data/secrets만 존재한다. 경로·
디렉터리 존재 메타데이터만 확인했으며 파일 내용은 직접 읽지 않았다.
기존 두 계정과 key/pin을 버리고 다시 연결하는 방법은 복구로 취급하지 않는다.

새 구현은 KF_FLAG_NO_PACKAGE_REDIRECTION으로 실행 환경에 무관한 일반
LocalAppData를 사용한다. 기존 데이터는 자동 합치거나 덮어쓰지 않는다.
현재 일반 루트의 unsafe ACL을 loader에서 허용하는 우회도 만들지 않는다.

제안하는 실제 복구 순서:

1. 정확히 관찰한 실행 바이너리·인수를 보존하고 해당 코어만 잠시 중지한다.
2. 양쪽 기존 루트를 각각 보존한다. 실제 토큰/키 평문을 열거나 내보내지 않는다.
3. Secrets/Store의 보호된 경로에서 암호화 상태와 공개 등록 프로필의 일관성을
   검증하고 기존 정상 상태를 일반 루트로 복구한다. 대상의 기존 파일은 보존된
   별도 백업으로 남기며 덮어쓰지 않는다. 실패 시 기존 상태로 돌아간다.
4. 같은 계정·등록·권한으로 core status / clients verify / 계정 ready를 확인한다.
   재로그인·재동의·새 grant·메일 발송은 복구에 포함하지 않는다.

필요한 결정: **기존 계정 저장 상태를 보존 백업한 뒤 일반 설치 경로로 복구하는
실제 데이터 작업**을 허용할지 여부. 이 문서는 실제 이동을 실행하지 않았고,
아직 구현되지 않은 범용 자동 migration 기능을 완료로 표시하지 않는다.

근거: AGENTS.md §5 실제 계정/비밀값 경계, §6 사람 보안 검토,
ADR-018의 게시자 secret 배포 제한, ADR-026 제안의 자동 migration 금지.
