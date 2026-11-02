# Google 연결과 Gmail 읽기·보내기

Windows CLI의 베타 후보 기능입니다. **보안 코드 사람 검토 필요.** 실제 Google 연결·재시작 및 제한 안의 실제 본문 조회를 확인했습니다. 실제 발송 검증과 게시 여부는 [진행 기록](PROGRESS.md)을 기준으로 확인하세요. 기존 preview.4에는 아래 새 시작 흐름이 포함되지 않습니다.

게시자 설정이 포함된 설치본은 `hantong --lang ko init`으로 시작합니다. Google Cloud 프로젝트나 앱 비밀값을 직접 입력할 필요가 없습니다. 연결 전에 읽기 또는 읽기·작성·보내기를 선택하고 Google에서 동의하면 작업 메뉴로 이어집니다. 추가 한통 계정 승인이나 Windows Hello는 요구하지 않습니다. 이미 연결한 개인 읽기 계정은 작업 메뉴의 `4) 메일 작성 및 보내기 활성화`로 같은 계정의 Google 보내기 권한을 추가합니다. 다른 계정·혼합 권한을 자동 확대하지 않습니다.

실제 발송은 별개입니다. 저장된 초안의 수신자·본문·첨부를 네이티브 발송 확인창에서 검토하고 사용자가 보내기를 누르면 한 번 전송됩니다. 이 개인 Gmail 경로에는 Windows Hello가 없습니다. Google 권한이나 CLI 명령만으로 발송을 승인하지 않습니다. 결과가 `unknown`이면 자동 재발송하지 않습니다. MCP·외부 AI 접근도 이 연결로 자동 활성화되지 않습니다.

작업 메뉴의 `1) 받은편지함 보기`에서 메일 번호를 선택하면 안전한 텍스트 본문을 읽을 수 있습니다. `5) 메일 작성`에서는 수신자·제목·본문과 선택 첨부를 입력합니다. 본문은 단독 줄의 `.`으로 끝내고, 완성된 초안에서 발송 확인창을 열지 선택합니다. 발송 확인창에서 내용을 검토하고 보내기를 누르면 결과가 표시됩니다. 메뉴만으로 사용할 수 있으며 계정 ID·초안 ID·해시를 직접 입력할 필요가 없습니다. 아직 배포 전 검증 중인 기능이므로 공개 설치본의 지원 여부는 진행 기록을 확인하세요.

아래 `--client-id` 절차는 고급 개발·앱 운영 설정입니다. 일반 이용에는 필요하지 않으며 이 고급 경로의 보호 입력·승인 절차는 유지합니다.

## 앱 설정은 최초 한 번

Google Cloud 프로젝트의 Gmail API, OAuth 동의 화면·테스트 사용자, **Desktop app** 클라이언트 설정은 계정마다 새로 만들지 않습니다. 테스트 앱은 허용된 테스트 계정만 사용하세요. 일반 배포에는 Google 정책·검증 요건을 별도로 충족해야 합니다.

코어 실행 후 첫 계정을 연결합니다. 명령에는 공개 client ID만 넣습니다. 비밀값·토큰은 채팅, 파일, 로그나 명령행에 넣지 마세요.

```powershell
mail-hantong-core run
# 다른 터미널에서
hantong login google --client-id YOUR_DESKTOP_CLIENT_ID.apps.googleusercontent.com --name Personal --lang ko
```

한통의 보호 입력창에서 앱 비밀값을 입력하고 본인 확인을 합니다. 이어서 Google에서 계정을 선택하고 읽기 전용 동의를 합니다. 돌아온 한통 보호창에서 정확한 계정을 확인하고 Windows Hello를 진행합니다. CLI 등록이 없으면 선택 계정 하나로 등록을 안내하고, 필요한 읽기 권한을 별도의 보호창에서 확인합니다. 기존 등록이나 다른 계정의 권한을 자동 대체하지 않습니다.

첫 연결에 성공하면 보호 저장된 동일 Desktop 앱 설정을 다음 계정에 재사용합니다. 계정별 토큰과 권한은 분리합니다. 첫 연결을 저장 전에 취소했거나 기존 저장 형식에 앱 자격증명이 없으면 보호 입력이 다시 필요합니다. 잘못되거나 손상된 보호 파일은 자동 우회하지 않습니다. `--client-id`를 다시 명시하면 앱 설정을 의도적으로 다시 입력하는 경로이므로, 일반 계정 추가에는 생략하세요.

## 계정 추가·목록·재연결

```powershell
hantong login google --name Work --lang ko
hantong gmail accounts --lang ko
hantong gmail accounts --json
hantong login google --reauthorize-account ACCOUNT_ID --lang ko
```

목록은 현재 CLI에 허용된 계정과 연결 상태를 표시합니다. 미등록 계정 전체를 공개하는 목록이 아닙니다. 추가할 때 Google 계정별 동의와 본인 확인은 유지합니다. 기존 CLI가 읽기 권한이면 기존 계정들을 유지하면서 선택한 계정 하나만 읽기 권한에 추가하도록 안내합니다. 쓰기 권한이 있는 다른 프로필이나 충돌하는 승인 요청은 자동 변경하지 않습니다.

재연결은 지정한 계정과 Google 로그인 계정이 같아야 합니다. Google에서 refresh token을 돌려주지 않을 때만 명시적으로 `--force-consent true`를 사용하세요. 연결 성공이 자동 메일 읽기나 동기화를 뜻하지는 않습니다.

## 중단한 단계 이어가기

각 입력·브라우저·계정 확인 단계는 준비된 시점부터 최대 300초입니다. 터미널에 남은 시간을 표시하며 상태 조회로 시한을 늘리지 않습니다. 콜백 화면은 흰 화면 대신 한통으로 돌아가라는 안내를 표시합니다. 콜백 수신만으로 연결 성공을 주장하지 않습니다.

```powershell
# 터미널을 닫았지만 기존 Google 요청이 아직 유효할 때
hantong login google --resume-request REQUEST_ID --lang ko
# Google 연결은 끝났고 로컬 등록/읽기 권한 승인만 남았을 때
hantong login google --continue-account ACCOUNT_ID --lang ko
```

재개용 요청 ID와 계정 ID는 터미널에 안내합니다. `--resume-request`는 새 Google 로그인을 열지 않습니다. `--continue-account`는 Google 로그인·메일 동기화 없이 로컬 권한만 확인합니다. 연결 전 요청이 만료됐으면 새 로그인을 시작하되 이미 저장된 앱 설정을 재사용합니다. 이전 콜백 URL을 새로고침하거나 재사용하지 마세요. 코어 재시작 후에는 메모리에만 있던 미완료 Google 요청을 재개할 수 없습니다.

로컬 인증 또는 보호 파일 검사 실패는 재등록·권한 요청으로 자동 전환하지 않습니다. `hantong clients verify`로 확인하고 보고된 오류를 해결하세요. Windows Hello는 사용자가 직접 진행합니다. 운영체제가 포커스 이동을 거절하면 작업 표시줄에서 한통 보호창을 선택하세요.

## 명시적으로 메일 읽기

```powershell
hantong gmail labels --account ACCOUNT_ID --json
hantong gmail sync --account ACCOUNT_ID --label-id INBOX --limit 1 --json
hantong gmail status --account ACCOUNT_ID --execution-id EXECUTION_ID --json
hantong gmail list --account ACCOUNT_ID --label-id INBOX --limit 1 --json
hantong gmail read --account ACCOUNT_ID --message-id MESSAGE_ID
```

목록과 본문은 암호화된 로컬 캐시를 읽습니다. 서버 자료를 가져오려면 명시적 동기화가 필요합니다. 동기화 완료 상태는 `completed`입니다. 다음 동기화의 `sync.next_page_token`은 `--page-token`, 목록의 `next_cursor`는 `--after`에 사용합니다. 두 커서는 서로 바꿔 쓰지 않습니다. 검색은 로컬 캐시 범위입니다.

자동화 재시도는 `--expected-version`, `--operation-id`, `--idempotency-key`를 고정해 같은 작업이 중복 실행되지 않게 합니다. Interactive `login google`과 재개 명령은 터미널·대화형 Windows 데스크톱이 필요하며 `--json`/`--no-input`에서는 실행하지 않습니다. 영어가 기본이며 한국어는 `--lang ko`를 지정합니다.

읽기는 `openid email gmail.readonly`, 읽기·보내기는 여기에 `gmail.send`를 추가합니다. 일반 흐름은 `gmail.modify`나 전체 메일 권한을 요청하지 않습니다. 본문은 HTML을 안전한 텍스트로 변환한 뒤 최대 64KiB를 표시하고, 초과하면 일부 표시 안내를 붙입니다. 1MiB MIME 파싱·2MiB 제공자 응답 등 별도 안전 한도를 넘거나 지원하지 않는 형식은 명시적 오류를 반환합니다. HTML 실행·외부 이미지·자동 첨부 다운로드는 하지 않습니다. MCP에는 별도 등록·계정별 범위·필요한 외부 AI 동의가 필요합니다.

기존 보호 저장소·백업을 보존하세요. 앱 비밀값을 포함한 저장 형식은 이전 preview.4 원본 바이너리와 호환되지 않을 수 있습니다. 검토용 UX 빌드를 되돌릴 때는 작업 전 보존한 동일 진단/읽기 버전 바이너리와 해시를 사용하고 보호 파일을 직접 편집하지 마세요.


## Gmail 발송/첨부 경로 — 사람 검토 필요

`hantong login google --access-mode readonly|read-send|modify`로 계정별 Google 권한 모드를
선택한다. 기본은 readonly다. 기존 등록/동의를 건너뛰지 않는다. 두 계정 송수신만
시험할 때 Google 최소 권한은 각 계정의 readonly + send다. 초안의 Google 서버
저장·읽음/라벨 변경에는 modify가 필요하며 이 권한 자체는 휴지통 이동과 초안 영구
삭제도 허용한다. 앱 삭제 경로는 제공하지 않는다. 현재 원격 초안 저장 및 라벨 변경은
전송 계층만 준비되어 있고 CLI/승인 흐름은 아직 공개하지 않았다.

`clients refresh-gmail-send <쉼표로 구분한 Google 계정 ID>`는 선택한 1~4개 계정에만
기존 B8 + 로컬 초안 생성/발송 요청/실행 C11을 요청한다. 기존 계정이 선택에서 빠지면
충돌로 중단한다. 자동 만료 없는 권한 변경이므로 네이티브 검토와 Windows Hello가
필요하다. Google 권한과 로컬 권한은 별개이며 실제 발송마다 메시지·첨부 확인이 남는다.

`draft create`의 `--attachment-file`과 `--attachment-type text/plain`은 파일을 한 번
읽어 로컬 암호화 초안에 저장한다. 이 초안은 Gmail 서버 초안이 아니다. `send`는 기존
초안 ID/버전/해시/작업 ID/중복 방지 키를 사용하고 네이티브 승인 후 1회만 전송한다.
결과가 unknown이면 자동 재전송하지 말고 양 계정에서 상태를 확인한다.

수신 내용 검증은 `gmail sync --limit 1 --verify-attachments true`로 명시한다.
파일명·형식·크기와 실제 다운로드 바이트의 SHA-256만 암호화 캐시에 남긴다.
첨부 원문은 응답에 포함하지 않는다. 첨부는 최대 8개/총 1 MiB이며 기존 IPC 1 MiB
프레임 제한은 JSON 및 hex 크기까지 포함한다. CLI 첨부 업로드는 현재 파일 1개다.

베타 후보의 현재 DB는 schema 23이며 코어가 암호화 백업 후 지원 버전의 마이그레이션을 수행합니다. 이전 바이너리를 새 DB에 바로 연결해 되돌리지 마세요. 복구는 해당 버전의 바이너리·데이터·키를 함께 보존한 백업 기준으로 진행합니다. 마이그레이션 자체는 계정·권한·발송을 생성하지 않습니다.
