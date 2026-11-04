# Linux headless 비밀 금고 계약 변경안

상태: 변경안 제시 후 사용자 구현 계속 지시(2026-11-04), 사람 보안 검토 필요. [ADR-031](../../adr/proposed/ADR-031-linux-headless-secrets.md)의
구현 승인 범위를 구체화한다. 현재 Unix OS 저장소 계약을 자동 대체하지 않는다.

## 저장 경계와 backend 고정

Secrets만 금고 암호문·복호화 항목·해제용 암호에 접근한다. Platform은 파일·TTY
handle 검증만 담당하고 CLI/MCP/IPC에는 비밀값·임의 서명·raw key API를 추가하지 않는다.
Windows/macOS와 기존 Linux OS 저장소는 그대로 둔다.

검증된 Linux secrets 루트에 `backend-v1.json`과 `headless-v1.age`를 사용한다.
metadata는 정확히 `format_version`(정수1), `backend`(`age-passphrase-v1`),
`vault_id`(CSPRNG32바이트의 소문자64hex)를 담으며4096바이트 이하이다.
추가·중복·null·다른 버전은 거부한다. 일반 사용자 입력으로 경로를 바꾸지 않는다.

새 backend 선택은 DB·server/client identity·기존 OS 비밀 항목이 없는 최초 초기화에만
허용한다. 기존 데이터가 있으면 별도 명시적 이전 계약 없이 backend를 변경하지 않는다.
DB가 있는데 metadata/금고가 없거나 OS 저장소를 읽을 수 없으면 새로 만들지 않는다.
기존 OS 비밀 항목 존재 여부를 확정할 수 없는 상황도 새 초기화로 간주하지 않는다.

## 암호문과 결속

암호화는 age0.12.1의 passphrase API와 표준 age 바이너리 포맷으로 수행한다.
plugin/SSH/CLI-common 기능과 외부 pinentry는 사용하지 않는다.
금고 암호문은16MiB 이하, 복호화 출력은8MiB 이하로 제한하고 상한 초과 전에 중단한다.
scrypt 작업량은 library의 명시적 최대 작업량 제한을 적용하며 상한을 초과한 파일에
자동으로 더 많은 CPU/메모리를 허용하지 않는다. 구현 전 해당 API로 생성·해제 양쪽의
자원 상한을 강제할 수 있음을 검증하며, 불가능하면 다른 방식으로 대체하지 않고 보고한다.

복호화된 내부 JSON은 정확히 `format_version`(1), `vault_id`(metadata와 같음),
`generation`(1부터 시작하는 양의u64), `items`를 담는다. 각 item은 기존 Unix
계약의 selector와 base64 비밀값으로 구성하며 중복 selector·잘못된 selector·추가 필드는
거부한다. DB 키32바이트, 서명키4096바이트 이하, 일반 비밀65536바이트 이하라는
기존 종류별 한도를 유지한다. 내부 JSON·base64도 비밀이며 Secrets의 보호 버퍼 밖으로
반환하거나 로그에 남기지 않는다. 전체 복호화·인증·schema 검증 전에는 항목을 사용하지 않는다.

파일·루트는 FD 기준 소유자·mode·종류·symlink·identity와 배타 잠금을 검사한다.
쓰기에는 expected generation CAS를 사용하며 새 암호문 전체를 동일 보호 디렉터리에
저장·fsync한 뒤 atomic rename과 directory fsync를 수행한다. 평문 임시 파일은 없다.
교체 실패·전원 중단·DB/금고 결속 불일치는 키 재생성 없이 실패한다. 백업도 암호문만
허용한다. 이전 암호문과 DB를 함께 되돌린 공격까지 탐지하는 외부 monotonic counter는
제공하지 않으며, 이 한계는 사용자 백업/복구 안내에 명시한다.

## 터미널 수명

Linux에 유효한 XDG_RUNTIME_DIR가 있으면 기존 소켓 경로를 유지한다. 환경변수가
아예 없는 headless 세션은 OS passwd home 아래 고정 `.local/state/mail-hantong/run`
디렉터리를0700으로 준비하고 기존0600 socket·배타 잠금·stale identity 검증을 사용한다.
있는 XDG_RUNTIME_DIR가 잘못된 경우 이 경로로 우회하지 않는다. HOME 주장이나
임의 `/tmp` 경로는 사용하지 않으며 CLI/Core가 같은 고정 경로를 계산한다.

인증 중계의 headless 확장: 별도 도우미가 잠금 해제 암호를 재입력하거나 저장하지
않도록 주 Core에 제한된 broker를 둔다. 커널이 관측한 동일 UID·Core 실행 이미지·
release hash·process instance가 일치하는 Core 도우미만 한 번의 고정 profile/RPC
envelope를 제출할 수 있다. 주 Core가 Secrets의 signer로 기존 auth-v2 loopback을
수행하며 기존 정책·scope/epoch·단일 claim 검사를 그대로 통과시킨다. signer·서명·
복호화 키를 반환하는 endpoint는 없다. 일반 CLI의 직접 broker 접근은 거부한다.
foreground terminal-review Core는 직접 broker에 연결해 Get/Confirm의 process
instance 결속을 유지하며, 매번 다른 자식의 PID를 같은 사람의 증거로 간주하지 않는다.
최초 등록 전에는 기존 계약의 pending 제출/상태와 typed terminal-review 경로만
허용하고 인증된 메일 요청이나 scope를 자동으로 부여하지 않는다.

금고 생성·잠금 해제는 실제 TTY에서 Core 소유 foreground helper/Secrets 입력으로
진행한다. 입력 echo와 line discipline을 복구하고 취소·EOF·시간 만료 시 보호 버퍼를
폐기한다. raw 암호를 CLI params·익명 로그·argv·env·일반 IPC에 실어 보내지 않는다.
생성 시 두 번 입력해 일치를 검사한다. 잠금 해제 후 암호와 복호화 항목은 Secrets 소유
프로세스 수명 안에만 유지하며 종료 시 폐기한다. 실행 중인 다른 Core에 비밀을 전달하는
새 unlock RPC는 추가하지 않는다. 잠금은 Core의 안전한 종료로 수행하고, 다음 시작은
다시 터미널 잠금 해제를 요구한다.

TTY 없는 서비스에서 암호가 필요한 경우 기존 `HUMAN_STEP_REQUIRED`로 종료하며
무인 시작 성공이나 메일 ready를 표시하지 않는다. 읽기 가능한 평문 파일이나 env로
해결하라고 안내하지 않는다. 이 제한은 화면 없는 SSH TTY 지원과 구분한다.
발송/Trash 확인·AI 제공자별 동의·scope/epoch·단일 claim·unknown 무재시도는 그대로다.

## 필수 검증

새 금고 생성→동일 키로 SQLCipher 재개방, 잘못된 암호/변조/절단/과대입력/KDF 비용
초과, selector/DB/identity/금고ID 불일치, missing key 무재생성, 동시쓰기 CAS·잠금,
symlink/타UID, 저장 중단 후 old/new 상태 보존, EOF/취소의 TTY 복원과 값 미노출을
합성 자료로 검증한다. 실제 GUI와 D-Bus 없는 Linux SSH 환경에서 설치·Core 시작·
CLI 연결·3계정 격리·메일 읽기/작성/확인·재시작과 업데이트를 별도로 검증한다.
금고만 성공한 결과를 Linux 메일 지원 완료로 표시하지 않는다.
