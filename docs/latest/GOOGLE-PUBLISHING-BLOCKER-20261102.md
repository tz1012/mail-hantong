> 해결됨 (2026-11-02): Branding 저장 및 In production 게시 완료. 아래는 당시 차단 이력이다. 최신 결과는 [게시 기록](../local-verification/2026-11-02/google-publishing/PRODUCTION-RESULT.ko.md)을 따른다.

# Google 운영 모드 게시 재점검 (2026-11-02)

결과: **게시 미완료 — Google UI의 Branding 구성 요구로 차단됨**. 도구 승인 거부가 아니며 게시 변경은 실행하지 않았다. 대상은 기존 프로젝트 `mail-hantong-test-20261030`, 앱 `mail-hantong 테스트`이다.

## 현재 콘솔 증거

- Audience: Publishing status **Testing**, User type External. Publish app disabled.
- 차단 안내: `To publish your app, you must complete your configuration on the Branding page.`
- 기존 테스트 사용자 두 명 유지. 새 사용자·scope·credential 변경 없음.
- Branding: 앱 이름, 지원 이메일, 개발자 연락처가 저장돼 있다. 홈페이지·개인정보처리방침·약관 URL 및 Authorized domains는 비어 있다. Save/Discard disabled로 미저장 편집 상태가 아니다.
- Information and summary: 현재 Testing이므로 verification이 필요 없다고 표시한다. 이는 In production 게시 완료를 뜻하지 않는다.
- UI는 누락 필드별 오류를 표시하지 않았다. 특정 URL 하나만 추가하면 게시가 가능하다고 단정하지 않는다. disabled 버튼 강제 클릭·숨은 API 호출·가짜 URL 입력은 하지 않았다.

## 기존 자료 조사

원본 README가 가리키는 공개 저장소 https://github.com/tz1012/mail-hantong 과 공개 릴리스는 실제 존재한다. 저장소의 README는 기능 소개 자료로 활용할 수 있지만, 기존 별도 홈페이지·개인정보처리방침·이용약관 URL은 원본 문서 검색에서 찾지 못했다. GitHub 저장소 URL 자체를 소유 검증된 전용 홈페이지라고 가정하지 않았다. 기존 교회/무관한 도메인은 사용하지 않았다.

## 공식 문서와 구분

- [Audience](https://support.google.com/cloud/answer/15549945): Publish app 이후 In production이며 검증은 별도 조건이다. Testing의 Gmail offline 승인/refresh token은 7일 만료 대상이다. 이미 받은 토큰이 상태 변경만으로 자동 영구화된다고 보장하지 않는다.
- [Branding](https://support.google.com/cloud/answer/15549049): 외부 운영 앱에 홈페이지·개인정보처리방침·약관 링크를 요구하며 홈페이지와 정책은 소유 검증 도메인 및 실제 데이터 이용 내용과 맞아야 한다.
- [개인용 검증 예외](https://support.google.com/cloud/answer/13464323): 개인용 100명 미만 앱은 검증 심사가 필수가 아닐 수 있지만, 현재 콘솔의 구성 차단을 제거하거나 In production 상태로 바꾸는 증거는 아니다.

## 사용자에게 한 번에 확인할 다음 범위

새 공개 자료가 필요하다. 제안은 기존 공개 GitHub 프로젝트의 별도 웹사이트 파일만으로 홈페이지·개인정보처리방침·이용 안내/약관 3개를 준비하고, 검토된 내용만 승인된 호스팅에 공개한 뒤 같은 Google 앱의 URL/Authorized domain을 설정하는 것이다. GitHub Pages의 `https://tz1012.github.io/mail-hantong/`는 후보 주소일 뿐, 현재 사용 가능·소유 검증·설정 권한은 아직 확인하지 않았고 생성하지 않았다. 구매·유료 호스팅이나 기존 도메인 재사용은 포함하지 않는다.

공개할 내용은 앱의 실제 기능, Google에서 접근하는 데이터와 목적, PC 암호화 캐시와 보호 토큰 저장, Google 인증/갱신 전송, 사용자 지정 외부 AI 사용 시의 별도 동의·전달 범위, 권한 철회·로컬 데이터 관리 방법, 현재 개발판 한계를 실제 구현과 대조한 문안이다. 이메일·비밀값·토큰·메일 원문·개발 로그를 공개하지 않는다. 문의 경로는 새 개인 연락처 공개 대신 기존 공개 GitHub 문의 경로를 우선 검토한다. 법적 약속을 임의로 확정하지 않고 정확한 최종 문안을 먼저 검토할 수 있게 준비해야 한다.

필요한 추가 승인 범위: 위 3개 웹 문서의 실제 최종 문안과 공개 위치, 그 웹 파일만의 공개 배포/필요한 제한적 커밋·푸시 또는 호스팅 업로드, Google의 해당 Authorized domain·Branding URL 등록. 기존 코드 전체·시험 산출물 업로드, 새 scope/credential, 유료 서비스, 법적 동의 제출, Google 검증 심사 신청은 별도이며 포함하지 않는다. 게시 전에는 소유·공개 접근성·링크·내용을 확인하고, 마지막 성공 판단은 동일 프로젝트 콘솔의 In production 표시로 한다.

이번 점검 중 코드/보안 정책/토큰/메일/런타임 변경은 없었다. 기존 1,699개 검사 및 양방향 첨부 시험 성공 상태를 보존했다.
