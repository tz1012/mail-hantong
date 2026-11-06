# Mac 대화형 격리 호환성 시험: 준비·재현·복구

상태: **준비 전용 코드와 단위시험 구현 / 실제 Mac 운용 미실행 / 별도 사람 검토 필요 / 배포 보류**.

## 이번에 제공하는 정상 진입점

`scripts/prepare_macos_interactive_compat.py`는 CI와 별개인 읽기 전용 진입점이다. `baseline`, `prepared`, `restore-check` 세 단계만 있다. Keychain 생성·선택·잠금 해제·항목 읽기·변경·삭제, Cargo·Core 실행, OS 확인, 권한 변경을 제공하지 않는다. subprocess 허용 목록도 조회 명령으로 닫혀 있다.

`CI`, `GITHUB_ACTIONS`, `RUNNER_TEMP`, `GITHUB_WORKSPACE`, `HANTONG_TEST_KEYCHAIN`, `HANTONG_COMPAT_ROOT/PHASE`, Mac Cargo runner 환경이 있으면 거부한다. 기존 `run_macos_test.py`, `test_macos_keychain_upgrade.py`, `test_macos_cross_version_product.py`의 CI 제한을 삭제하거나 환경 변수 위장으로 넘지 않는다. 이 문서도 CI main을 로컬에서 우회 호출하라는 지시가 아니다.

현재 구현된 범위는 **사전검사와 실행 계획**이다. 실제 교차 버전 실행 controller는 아직 제공하지 않는다. 아래 단계별 실행 설계는 향후 검토 대상이며, 이번 명령을 성공시켰다고 실제 시험·OS 확인·복원·삭제가 승인되거나 완료된 것은 아니다. 새 executor가 필요하면 검증된 interactive context를 명시적으로 받고 각 단계 직전에 재검사하도록 별도 구현·검토한다. 기존 CI 함수의 guard를 끄거나 임의로 직접 호출하는 adapter는 허용하지 않는다.

## 환경 준비 조건 — 이번에는 실행하지 않음

Intel Mac과 Apple Silicon Mac 각각에서 별도 시험 기록을 만든다. 번역 실행을 Intel 실기 시험으로 계산하지 않는다. 현재 Windows 개발 환경에서는 이 Mac OS 검사를 실제 통과시킬 수 없다.

1. 관리자가 별도로 마련한 폐기 가능한 **표준 사용자 `hantong-compat`**, 실제 passwd home `/Users/hantong-compat`를 사용한다. 실제 업무 사용자에서 HOME/USER만 바꾸지 않는다. 운영자가 실제 계정·메일·비밀을 넣지 않은 전용 사용자임을 확인해야 한다. 이 선언은 기술적으로 증명되는 신원이나 OS 승인 증거가 아니다.
2. 해당 사용자의 GUI 데스크톱에 로그인한 기본 Terminal에서 진행한다. root/sudo, 관리자 그룹, 다른 console 소유자, GUI launchd domain 부재, SSH, stdin/stdout 비TTY는 거부한다. CI가 아니라는 환경 변수만으로 GUI 신분을 인정하지 않으며 UID·passwd·console·GUI 조회를 함께 확인한다. 같은 UID가 세션 환경을 위조하는 모든 경우를 이 준비 도구가 구별한다고 주장하지 않는다. 아키텍처는 Python 표준 ctypes에서 고정 OS libSystem의 `sysctlbyname("sysctl.proc_translated", ..., NULL, 0)` 읽기 조회로 Rosetta를 거부하고 native `arm64`/`x86_64`만 받아들인다. [Apple의 Rosetta 판별 절차](https://developer.apple.com/documentation/apple-silicon/about-the-rosetta-translation-environment)에 따라 Intel의 ENOENT는 native로 처리하되 다른 실패·잘못된 크기/값은 거부한다. Keychain API나 새 패키지 의존성은 추가하지 않는다. 실제 Mac의 이 OS 조회는 아직 검증하지 않았다.
3. home와 그 아래 `HantongCompat/<32자리 lowercase hex run-id>`를 소유자 전용0700으로 마련한다. 기존 디렉터리의 권한을 도구가 고치지 않는다. 파일은0600, 단일 hardlink, symlink·확장 ACL은 거부한다. 소스는 개발 checkout이나 다른 worktree가 아닌 run root 아래의 폐기 가능한 일반 clone 두 개다. `.git`도 run root 내부의 private 실제 디렉터리여야 한다. 이 사전 조건을 만드는 사용자·디렉터리·금고 운용은 이번 실행에 포함하지 않는다.
4. 준비 시작 시 실제 passwd home의 `Library/Application Support/mail-hantong`과 OS per-user runtime의 `mail-hantong`이 모두 없어야 한다. 기존 것이 있으면 보존하고 중단한다. 이동·초기화·삭제하지 않는다.
5. 전환 전 사용자 default/search Keychain의 **경로·파일 identity만** 기록한다. 내용을 열거나 비밀을 읽지 않는다. 이 전용 사용자의 home 밖 경로, 접근 불가 경로, private 조건을 충족하지 않는 경로도 거부한다. 기본 login 금고가 실제 사용자 금고인 환경에서는 진행하지 않는다.

## A. 전환 전 기준 기록

아직 `synthetic.keychain-db`를 만들거나 default/search를 바꾸기 **전**에 실행한다. run root는 준비된 **빈 디렉터리**여야 한다. `baseline.json`, 합성 금고, source/package 입력을 포함한 어떤 항목도 있으면 거부한다. 전용 root 바깥의 검토한 스크립트 경로에서 실행하고, source/package 복사본은 이 기준 기록 뒤에 마련한다. 기존 소스의 어느 CI 변수도 설정하지 않는다.

```sh
python3 scripts/prepare_macos_interactive_compat.py \
  --stage baseline --disposable-user-confirmed --run-id RUN_ID
```

`RUN_ID`는 운영자가 준비한 해당 32자리 hex 값으로 바꾼다. 다른 문자열·경로는 거부한다. 터미널 JSON을 전용 run root의 `baseline.json`에 **그대로** 보관하는 것은 운영자의 로컬 준비 단계다. 원본은 수정하거나 재생성해 실패를 덮지 않는다. 0600 private 파일이어야 한다. TTY가 필수이므로 위 명령의 stdout을 파이프로 감추거나 CI로 전달하지 않는다. 이 기록에는 전용 사용자의 로컬 경로·inode 등이 있으므로 대화·공개 로그로 보내지 않는다. DB·비밀번호·Keychain 비밀값은 들어 있지 않다.

이 baseline은 서명된 증거나 권한 토큰이 아니다. 같은 사용자가 바꿀 수 있는 로컬 준비 기록이며 실제 실행 controller의 OS 검증을 대체하지 않는다.

## B. 합성 금고 준비 후 검사

**후속 운용 승인·사람 검토 뒤** 운영자가 새 합성 전용 file Keychain을 해당 run root의 `synthetic.keychain-db`에 마련하고 잠금 해제 및 이 전용 사용자의 sole default/search로 선택해야 한다. 실제 사용자의 금고를 복사하거나 항목을 옮기지 않는다. 준비 도구는 생성·unlock·default/search 설정을 하지 않는다. 초기 상태는 새로 만든 합성 금고라는 운영자 provenance 기록으로 남긴다. 단순 파일명·POSIX 메타데이터로 금고 내용이 합성뿐임을 증명할 수 있다고 주장하지 않는다.

그 뒤 아래 파일·clone을 준비한다. 다운로드·설치·권한 변경도 이번 실행에서는 하지 않았다.

| run root 안 입력 | 검증 |
| --- | --- |
| `baseline.json` | 전환 전 기록, UID/run-id/root identity 및 원래 금고 identity 결속, 중복/알 수 없는 JSON 필드 거부 |
| `synthetic.keychain-db` | private regular file, 단일 link, 정확한 sole default/search 일치 |
| `old-package.tar.gz` | 공식 beta.29의 플랫폼별 고정 SHA-256 |
| `candidate-package.tar.gz` | 검토 기록에서 별도로 전달받은 후보 SHA-256 일치 |
| `old-source` | clean 일반 clone, HEAD=`5d1e4734dfe1475c951f8d052079212e1437dc50` |
| `candidate-source` | clean 일반 clone, 전달받은 정확한 후보 commit, canonical source hash도 결과에 기록 |

구버전 SHA-256:

- Intel: `dc46e62b04c229c7a1ba08587104eb3f274c15bae0b2d5d70b7513117f42bc7e`
- ARM: `b43ee9bae6b078ad6f21c6731c08c25bd67e1b8383fea34fa83df54c55ae74be`

```sh
python3 scripts/prepare_macos_interactive_compat.py \
  --stage prepared --disposable-user-confirmed --run-id RUN_ID \
  --candidate-commit CANDIDATE_COMMIT --candidate-sha256 CANDIDATE_ARCHIVE_SHA256
```

후보 commit/해시는 검토한 빌드 증거에서 확인한 값을 사용한다. 같은 파일을 지금 해시한 값만 입력하면 provenance를 검증한 것이 아니다. 현재 배포 가능한 새 Mac 후보가 통과했다는 뜻도 아니다. 이 단계의 root 직접 항목은 위 여섯 개만 허용하며 빠지거나 다른 항목이 있으면 거부한다. source worktree와 `.git` 전체의 메타데이터를 재귀 검사해 symlink·파일 hardlink·외부 소유자·특수 파일·group/world 쓰기·특수 mode를 거부한다. 공유 Git object·worktree·submodule·replace 참조를 거부하고, Git 실행 전 `.git/config`를 no-follow 열린 파일 identity에 결속해 읽어 기본 core 값·공개 origin HTTPS URL·정상 branch 설정만 허용한다. 외부 include·helper·core.worktree 등은 거부한다. 일반 원격 clone을 쓰며 local/shared clone이나 개발자 설정을 복사하지 않는다.

source는100,000개 노드, 파일당1GiB·전체8GiB 이하로 제한한다. ignored 파일도 포함해 clean 상태를 요구하고 source 전체 메타데이터 digest·HEAD·상태를 source hash 앞뒤에 비교한다. 입력 경로의 확장 ACL은 거부하지만 모든 source 내부 파일의 ACL이나 내용의 무해성을 재귀적으로 증명하는 도구는 아니다. 제품 디렉터리와 runtime 부재도 마지막에 다시 확인한다. 해시 중 file identity·크기·내용 변경을 발견하면 거부한다. 이 경로 검사는 같은 UID의 적대적 동시 변경을 원자적으로 차단하는 executor가 아니다. 이러한 잔여 조건을 위험 수용하지 않으며 실행 직전 별도 검증·실행 설계도 필요하다.

성공 결과는 `executed=false`, `execution_authorized=false`, `preflight-passed-execution-not-performed`다. 실제 읽기·호환성 성공으로 기록하지 않는다. 실패하면 닫힌 오류 종류만 출력한다. 금고 내용·명령 stderr·계정·메일·비밀번호를 출력하지 않는다.

## C. 향후 실제 실행 순서 — 별도 controller·Mac 환경·운용 검토 필요

다음은 정상 실행 경로의 설계다. 현재 prep 도구에는 `--run` 옵션이 없으며 아래 동작을 실행한 적도 없다. 무검토 shell 복사본이나 CI 환경 변수 위조로 대신하지 않는다. controller는 고정 phase와 검증된 context를 받고 arbitrary command/PID/경로를 받지 않아야 한다. 이번 준비 범위에서는 제품 API·저장 형식·계약 변경이 필요하지 않았다. 이 후속 controller가 금고 선택·복원·삭제 등을 실행하도록 만들 때에는 ADR-032 운용 범위 변경안을 먼저 보고·검토한다.

### 공식 바이너리: DB·DB 키

1. run root 아래 별도 old/candidate 설치 위치에 각각 해시 고정 아카이브를 설치한다. 안전한 archive member·manifest/hash/target 검증은 기존 패키지 규칙을 그대로 유지한다. 개인 PATH·실제 설치 채널은 변경하지 않는다.
2. 공식 old `hantong --lang en`으로 무계정 메뉴에 도달한 뒤 종료한다. 계정 연결·Google OAuth·메일·AI를 시작하지 않는다. 암호화 DB와 합성 금고의 `database-key`가 생성되어야 한다. DB 헤더16바이트의 plaintext SQLite 여부만 검사하고 DB 키를 추출하지 않는다.
3. old CLI `core status --json`과 정확한 old Core의 `unix-stop`을 사용한다. 호출이 성공하고 해당 owned process가 끝나며 socket이 없어져야 다음으로 간다. arbitrary PID kill이나 다른 실행 파일로 stop하지 않는다.
4. 후보의 첫 시작이 **동일한** DB·DB 키를 읽고 메뉴·status에 도달해야 한다. 기존45초 제한과 비어 있는 계정 상태를 유지한다. 종료·검증 뒤 같은 후보를 두 번째로 시작해 다시 읽기·status·종료를 확인한다. old/candidate가 같은 파일을 사용했음을 기록한다. OS 응답을 기다리다 제한시간이 지나면 실패이며 시간을 늘리거나 자동 재시도하지 않는다.

### 별도 소스 fixture: 합성 IMAP·SMTP

1. disposable old/candidate clone의 원본 `lib.rs` 바이트·source hash를 기록한다. 기존 `scripts/fixtures/macos_keychain_compat_module.rs`만 각각 시험 모듈로 임시 주입하고, 정상 제품 빌드·실제 개발 checkout은 변경하지 않는다. controller가 이 복원 책임을 갖는다.
2. old source에서 exact `macos_keychain_compat::mac_keychain_compatibility_fixture`, phase `seed`를 정확히1개 실행한다. 합성 IMAP/SMTP OAuth slot 생성 뒤 Secrets 내부에서 두 old 값을 비교해야 한다. provider에 연결하지 않는다. 실제 앱 비밀번호가 아니다.
3. 후보의 phase `upgrade`는 **두 old 값 읽기→IMAP만 새 합성 값으로 갱신→새 IMAP와 보존 SMTP 비교**를 모두 마쳐야 한다. 값 비교는 Secrets 내부에 두고 바이트를 외부로 내보내지 않는다.
4. 후보를 별도 프로세스로 다시 실행하는 phase `reopen`에서 새 IMAP와 기존 SMTP를 재비교한다. phase마다 정확히1개 시험, 원래900초 제한, `open/opened/.../done` 고정 마커를 요구한다. 기존 no-UI probe 분류는 이 조건을 대신하지 않는다.
5. 성공·실패·취소 모두 owned process group 종료를 확인한 뒤 주입 파일을 제거하고 `lib.rs` 원본 바이트·source hash를 복원·비교한다. 종료 확인에 실패하면 쓰기·정리하지 않고 보존한다. SIGKILL·전원 중단으로 finally가 안 돌아간 경우도 이후 복구 대상으로 남기며 새 seed로 덮지 않는다.

공식 바이너리 시험은 DB 키 호환성, 소스 fixture는 시험 바이너리의 합성 메일 slot 호환성이다. 소스 시험 통과를 공식 old 바이너리가 만든 메일 자격증명까지 검증한 것으로 확대하지 않는다. Intel과 ARM 각각 두 경로를 기록해야 한다.

## 예상 OS 요청 확인법

아래는 값이 아니라 코드의 고정 합성 selector다. 화면 문구는 OS 언어·버전에 따라 달라질 수 있다. 요청의 앱 이름만 보지 말고 해당 build의 실제 절대 경로·아키텍처·해시와 요청 시점·항목을 기록하고 대조한다. 원본 Keychain 내용이나 값 보기를 열지 않는다. OS가 세부 대상 확인 수단을 제공하지 않아 판별할 수 없으면 취소하고 실패로 남긴다.

| 경로 | 예상 실행 파일 | service / account |
| --- | --- | --- |
| 후보 DB 키 읽기 | 해당 후보 설치의 `bin/mail-hantong-core` | `mail-hantong.local-security.v1` / `database-key` |
| 후보 합성 IMAP | disposable 후보 target의 정확한 `hantong_secrets-*` | 같은 service / `mail-BASE32HEX-imap-oauth-refresh` |
| 후보 합성 SMTP | 동일한 후보 시험 실행 파일 | 같은 service / `mail-BASE32HEX-smtp-oauth-refresh` |

`BASE32HEX`는 ASCII `synthetic-upgrade-account`의 padding 없는 lowercase base32hex, 즉 `edsmst38clq6iopdelo6esj1chiiqob3cdnnarjk`다. 따라서 IMAP account 전체는 `mail-edsmst38clq6iopdelo6esj1chiiqob3cdnnarjk-imap-oauth-refresh`, SMTP account 전체는 같은 prefix 뒤 `-smtp-oauth-refresh`다. 코드의 `encoded_account()`·`slot.name()`과 대조해야 하며 평문 계정명을 selector로 오인하지 않는다. 서비스·선택자 확인을 위해 비밀을 출력하는 명령은 사용하지 않는다.

- old seed에서 예상하지 못한 항목 접근 요청, 다른 앱·항목·금고 요청, 일반 로그인·관리자 권한 요청, 금고 unlock 요청은 취소하고 실패한다. 합성 금고는 시험 전에 이미 준비·unlock되어 있어야 한다.
- 준비 전용 도구 실행 중에도 예상 밖 OS 창이나 개발 도구 설치·동의 요청이 나타나면 응답하지 않고 취소·중단한다. 사전검사 성공을 위해 시스템 설정이나 도구 설치를 자동 진행하지 않는다.
- 이번에는 어떤 요청에도 응답하지 않는다. 향후 별도 승인된 실제 시험에서 **실제 나타난** 예상 대상 요청에 한해서만 사람이 일회성 `Allow`를 직접 선택한다. `Always Allow`·접근 제어 편집·partition-list 변경·재서명은 금지한다. unlock을 위해 실제 사용자 비밀번호를 대화나 자동화에 넣지 않는다.
- 후보의 읽기·갱신·reopen마다 요청이 다시 나타날 수 있다. 일회 허용을 영속 허용으로 바꾸지 않는다. 요청이 전혀 없어도 원래 모든 값 비교·재시작 조건이 통과하면 그 관찰 사실을 기록한다. 요청이 안 보이면서 대기하면 시간 초과를 실패로 유지한다.

[Apple ACL 문서](https://developer.apple.com/documentation/security/access-control-lists)는 일회 허용과 지속 trusted-app 허용을 구분한다. 이 문서는 후자의 변경을 허가하지 않는다.

## 실패·취소·원상 복구·정리

1. 잘못된 요청은 OS의 취소/거부를 사람이 선택하는 별도 합성 시험으로 관찰한다. 현재는 mock 시험만 수행한다. 취소·거부·시간 초과·오류를 성공으로 바꾸거나 새 요청을 자동 재시도하지 않는다. 고정 단계·종류와 도달한 마커만 남긴다.
2. owned Core/test 프로세스의 실행 경로·시작 identity를 확인하고 정상 종료 절차를 먼저 적용한다. timeout 시 해당 controller가 생성한 process group만 회수한다. PID만으로 다른 프로세스를 종료하지 않는다. socket 부재만으로 모든 자식 종료를 증명하지 않는다.
3. 실패 시 합성 run root·금고·baseline·고정 진단은 보존한다. 원래 실제 사용자 데이터가 발견되면 즉시 중단하고 내용은 읽지 않는다. 자동 cleanup·재초기화는 하지 않는다.
4. 향후 명시적 복구 단계에서 전환 전 baseline의 정확한 default/search 순서와 파일 identity를 대상으로 복원한다. 복구 실패면 삭제하지 않는다. 원래 금고를 unlock하거나 항목 ACL을 고치는 복구는 아니다. 이 변경 작업은 현재 도구가 수행하지 않으며 이번 범위에서는 미실행이다.
5. 복원 작업 **이후** 읽기 전용 비교만 수행한다.

```sh
python3 scripts/prepare_macos_interactive_compat.py \
  --stage restore-check --disposable-user-confirmed --run-id RUN_ID
```

`selection_restored=true`는 경로·순서·original file identity 일치만 뜻한다. `process_shutdown_verified=false`, `cleanup_authorized=false`를 함께 반환한다. 실행 중인 시험이 없다는 증거나 cleanup 허가가 아니다. 원래 금고의 항목·값을 읽어 복원 여부를 검증하지 않는다.

6. 이후 별도로 승인된 cleanup에서 owned process 종료, 정확한 baseline 복원, source 원본 바이트 복원, 합성 대상 file identity가 모두 확인돼야 **그 run의 합성 대상만** 제거할 수 있다. 사용자 계정 삭제·전체 home 삭제·기존 앱 데이터 삭제·원래 Keychain 삭제는 정상 cleanup으로 취급하지 않는다. 이 도구에는 삭제 기능이 없다.

## 결과·검토·남은 일

환경 위장·root/admin·console/GUI/TTY·기존 데이터·unsafe path·ACL·hardlink·baseline 부재/변경·wrong commit/hash·변경 명령 거부는 OS/Keychain mock 단위시험으로 확인한다. mock 통과를 실제 Mac 증거로 표시하지 않는다. 이번에는 네 플랫폼 전체 Cargo/패키지 검사를 다시 실행하지 않는다. 제품 Rust·기존 CI 하네스는 변경하지 않았다.

새 사람 검토 대상: 준비 도구, 해당 Build 시험, 이 문서·계획·ADR-032 보완과 `95349fe`의 Secrets dependency/probe/harness를 연결해 읽는다. 확인 항목은 OS-derived session과 운영자 선언의 구분, 조회 명령 허용 목록, baseline 전후 결속, 값 비노출, 실패 보존, 실행/권한변경 기능 부재, cleanup 성공 과장 방지다. 고정 대상 커밋·파일과 실제 검증 결과는 PROGRESS에 기록한다. AI 검토나 “의존성 추가 승인”을 사람 검토 완료로 대체하지 않는다.

남은 작업: 실제 Intel·ARM GUI 시험 환경과 전용 사용자의 운용 준비, 별도 실행 controller의 범위 검토·구현, 정상/거부/취소/실패 복구 실제 시험, 교차 버전 값 보존·갱신·재시작 입증, 새 Secrets 사람 검토 및 기존 실행 파일 교체 경쟁 잔여 조건 해결. **배포 보류는 유지한다.**
