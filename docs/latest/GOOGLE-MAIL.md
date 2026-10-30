# Google 계정 연결과 Gmail 읽기

Windows CLI 소스에서 제공하는 다중 계정 읽기 기능이다. Google 계정마다 브라우저에서 로그인하고, 로컬 보호 화면에서 계정을 확인한다. 개인 Gmail과 관리자가 허용한 Google Workspace 계정을 같은 방식으로 연결한다. 이메일 주소가 아닌 Google 고유 계정 ID로 구분하므로, 다른 계정의 토큰이나 메일을 섞지 않는다.

## 최초 설정

현재 저장소에는 배포자 Google OAuth 클라이언트 ID가 등록되어 있지 않다. 따라서 설치만으로 Google 로그인이 바로 준비되지는 않는다. 사용자가 소유한 Google Cloud 프로젝트에서 Gmail API를 활성화하고 OAuth 동의 화면·대상 사용자를 설정한 뒤, **Desktop app** 유형의 OAuth 클라이언트를 준비해야 한다. 테스트 모드라면 로그인할 계정을 테스트 사용자로 허용한다. CLI에는 공개 `client_id`만 전달한다. 비밀번호·토큰·client_secret을 명령에 넣지 않는다.

일반 공개 배포에는 Google의 Gmail 제한 범위 검증 및 조직별 관리자 정책 확인이 별도로 필요하다. 테스트 모드의 외부 앱은 사용자 수와 인증 만료에 제한이 있다. 이 개발에서는 실제 Google 프로젝트 생성·사용자 로그인·메일 가져오기를 실행하지 않았다.

코어를 실행한 뒤 첫 계정을 연결한다:

```powershell
mail-hantong-core run
# 다른 터미널
hantong login google --client-id YOUR_DESKTOP_CLIENT_ID.apps.googleusercontent.com --name Personal
```

브라우저에서 Google 계정과 읽기 권한을 선택한다. 이후 보호 화면에서 정확한 이메일·Google 계정·클라이언트 설정을 확인한다. Windows 본인 확인을 마치면 계정을 저장한다. CLI 등록이나 계정 범위 갱신이 필요하면 별도 보호 화면이 이어진다. 계정 연결만으로 기존 CLI·에이전트에 권한을 추가하지 않는다. 허용 후 받은편지함의 첫 32개 메일을 가져와 목록을 표시한다.

두 번째 계정부터는 저장된 공개 클라이언트 ID를 재사용한다. 매번 Google 계정 선택 화면을 표시한다:

```powershell
hantong login google --name Work
hantong accounts list --json
```

같은 Google 계정을 다시 연결하면 기존 로컬 ID와 저장된 메일을 유지하고 인증 연결을 교체한다. 다른 계정은 별도로 추가한다. 계정 수를 제품 설정으로 제한하지 않는다. 한 번에 처리하는 네트워크 작업 수는 제한하며 계정별 실패를 격리한다.

## 메일 목록·본문·추가 페이지

`ACCOUNT_ID`는 계정 목록의 `google_...` 값을 사용한다. 조회는 암호화된 로컬 캐시를 읽으며, 새 메일 가져오기는 명시적인 동기화 요청으로 수행한다:

```powershell
hantong gmail labels --account ACCOUNT_ID --json
hantong gmail sync --account ACCOUNT_ID --label-id INBOX --limit 32 --json
hantong gmail status --account ACCOUNT_ID --execution-id EXECUTION_ID --json
hantong gmail list --account ACCOUNT_ID --label-id INBOX --limit 32 --json
hantong gmail read --account ACCOUNT_ID --message-id MESSAGE_ID
```

동기화 상태가 `completed`이면 저장이 끝났다. `sync.next_page_token`이 있으면 다음 동기화에 `--page-token TOKEN`을 전달한다. 로컬 목록의 `next_cursor`는 `gmail list --after CURSOR`로 사용한다. 두 커서는 서로 바꿔 쓰지 않는다. `--query`는 가져온 메일의 제목·발신자·요약에서 검색하며 전체 Gmail 서버 검색이 아니다.

자동화에서 동일 동기화 요청을 재시도하려면 `--expected-version`, `--operation-id`, `--idempotency-key`를 고정한다. 버전은 라벨/목록 응답의 `sync_version`으로 확인한다. 이미 접수한 동일 요청은 원래 실행 ID를 반환하며 네트워크 작업을 중복 실행하지 않는다. 코어 재시작으로 중단된 작업은 자동 재실행하지 않는다.

토큰이 만료되거나 취소되면 해당 계정만 다시 연결한다. Google이 refresh token을 반환하지 않는 경우 명시적으로 동의를 다시 요청한다:

```powershell
hantong login google --reauthorize-account ACCOUNT_ID --force-consent true
```

정확히 지정한 계정과 다른 Google 계정으로 로그인하면 교체를 거부한다. 이미 가져온 메일은 인증 만료 후에도 로컬 권한 범위에서 읽을 수 있다.

## 범위와 제한

- English is the default; add `--lang ko` for Korean. `--json` provides structured output for explicit setup/status/read commands. Interactive `login google` requires a terminal and an interactive Windows desktop.
- 요청 범위는 `openid email gmail.readonly`다. 라벨·메타데이터·최대 64KiB의 텍스트 본문을 읽는다. HTML은 실행하지 않으며 외부 이미지와 첨부파일을 열지 않는다. 지원하지 않거나 너무 큰 본문은 메타데이터와 안정적 오류로 표시한다.
- 이 Google 연결은 Gmail 수정·삭제·발송 권한을 제공하지 않는다. 기존 별도 SMTP 발송 경로의 사람 승인 원칙은 유지한다.
- MCP는 동일한 공통 메일 도구의 `gmail_*` 모드를 조회할 수 있지만 독립 등록·정확한 계정 권한·계정별 외부 AI 동의를 요구한다. 기존 등록에 새 계정을 자동 추가하지 않는다. 현재 full9 MCP 등록의 새 계정 범위 갱신은 미지원이며, 연결할 계정들을 준비한 뒤 최초 MCP 등록에서 명시적으로 허용해야 한다.
- 실제 제공자 호환성과 사람 보안 검토는 아직 남아 있다. `preview.4`에 추가한 기능이며 `preview.3` 다운로드에는 포함되지 않는다. 배포·검증 상태는 [진행 기록](https://github.com/tz1012/mail-hantong/blob/main/docs/latest/PROGRESS.md)을 확인한다.

Google 공식 안내: [Desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app), [Gmail 권한](https://developers.google.com/workspace/gmail/api/auth/scopes), [제한 범위 검증](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification).
