# 제안: Unix 로컬 실행·인증 v2

상태: 2026-11-04 사용자 구현 승인 / 사람 보안 검토 필요.
Windows 기존 계약을 대체하지 않으며, 구현과 native 검증 전 Unix 지원 완료를 뜻하지 않는다.
결정안은 [ADR-030](../../adr/proposed/ADR-030-unix-cli-runtime.md)을 따른다.

## Windows 호환

Windows protocol_version=1, SID/로그온 SID, transcript 인코딩, CNG 키 이름,
공개키·서명·프로필 JSON·DB schema는 유지한다. Unix도 JSON-RPC protocol_version=1을
유지하며 인증 envelope에만 auth_version=2를 추가한다. Windows envelope는 변경하지 않는다.
Unix는 auth_version=2만 허용하며 v1로 폴백하지 않는다. IPC 메일 도구·권한·확인 정책은 동일하다.

## Unix peer와 transcript

별도의 `UnixPeerContextV2`를 둔다. 필드는 platform(linux=1, macos=2),
effective_uid(u32), effective_gid(u32), peer_pid(u32), session_id(u32), process_instance([u8;32])다.
PID/session은 0이 아니어야 한다. 정수는 little-endian이며 peer 필드는 49바이트다.
peer 정보는 JSON 주장을 신뢰하지
않고 연결에서 커널이 관측한 값으로 만든다.

인증 canonical bytes는 다음 순서로 고정한다. 이 제안을 구현할 때 독립 golden fixture를 둔다.

1. ASCII domain `mail-hantong/ipc-auth/v2` 뒤 NUL.
2. role(u8: server=1/client=2), auth_version(u16:2).
3. registration_id(16), profile_epoch(u64), issued_at_ms(i64), expires_at_ms(i64).
4. server_instance(32), server_nonce(32), client_nonce(32), server_public_key_hash(32), client_public_key_hash(32).
5. server peer(49), client peer(49), v1과 같은 canonical account list와 tool list.

고정 부분은 326바이트이며 account/tool list의 길이 구분 데이터를 뒤에 붙인다.
전체 상한은 기존과 같은 32768바이트다. 필드 길이·순서 불일치, v1 downgrade는 인증 실패다.
scope list는 각각 u16 항목 수와 항목별 u16 ASCII 바이트 길이+값이며,
v1과 같은 정렬·중복·문자·개수 검증을 적용한다. 빈 두 목록을 포함한 최소 길이는330바이트다.
서버/클라이언트 session_id가 같아야 한다고 가정하지 않는다. 각자 관측한 실제
peer와 서명된 context가 일치해야 한다.
begin의 정확한 필드는 protocol_version,auth_version,registration_id,profile_epoch,
client_nonce,scope다. challenge는 protocol_version,auth_version,transcript_hex,
server_signature_hex이며 ready는 protocol_version,auth_version,authenticated(true)다.
auth.finish는 기존과 같이 signature_hex만 받는다. 중복·추가·null·float 필드는 거부한다.

Linux process_instance는 boot ID, SO_PEERCRED PID, `/proc/PID/stat` start time의
길이 구분 canonical hash다. macOS는 boot-session UUID, LOCAL_PEERPID,
OS process start/unique token을 사용한다. PID 수명과 이미지 확인을 인증 전후
재검사하며, 안정적인 증거를 얻지 못하면 실패한다. helper는 같은 Core 이미지와
release hash까지 검증한다.

## 경로와 파일

- Linux 영속 루트: 검증한 XDG_DATA_HOME 아래 `mail-hantong`, 없으면 검증한
  OS passwd home의 `.local/share/mail-hantong`. data/secrets는 그 아래에 두고,
  profiles는 XDG_CONFIG_HOME 또는 passwd home의 `.config` 아래 `mail-hantong/profiles`에 둔다.
- macOS 영속 루트: OS가 확인한 사용자 home의 `Library/Application Support/mail-hantong`.
- data/secrets/profiles는 분리한다. secrets 폴더는 잠금·메타데이터만 담으며 비밀 바이트를 저장하지 않는다.
  소유 UID·mode·regular file/directory·symlink 금지를
  handle 기준으로 검사한다. 임의 ACL/mode를 자동 수리하지 않는다.
- Linux socket: 검증한 XDG_RUNTIME_DIR(현재 euid 소유,0700)의 앱 전용 폴더.
  macOS socket: OS per-user 임시 루트의 앱 전용0700 폴더.
- socket0600, 보호된 배타 잠금. stale socket은 잠금·소유자·파일 종류 검증 후에만 제거한다.
  Linux `/tmp` 임의 경로 대체와 요청 인수로 socket 루트를 바꾸는 기능은 금지한다.

## OS 비밀 항목과 서명

service=`mail-hantong.local-security.v1`. selector는 database-key,
server-signing-<64 lower hex>, client-signing-<64 lower hex>,
mail-<base32hex account_id>-<stable slot>, google-<base32hex account_id>-v<positive version>,
model-<base32hex account_id> 중 하나다. account_id는 원래 ASCII 바이트를 padding 없이 인코딩한다.
selector 상한384바이트, PKCS#8 상한4096바이트, DB 키32바이트, 일반 비밀65536바이트다.
Linux 항목 label은 고정 문자열 `mail-hantong protected item`이다. macOS Keychain
store는 label modifier를 지원하지 않으므로 고정 service 이름을 사용한 OS 기본 표시를 유지한다.
selector/label에 이메일·메일 내용·비밀값·사용자 입력 경로를 넣지 않는다.

`hantong-contracts`의 최소 `OpaqueSigner` 인터페이스는 공개키72바이트와 서명64바이트만 반환한다.
Windows 구현은 Platform CNG, Unix 구현은 Secrets에 둬 양방향 crate 의존을 만들지 않는다.
개인키·DB 키·메일 자격 증명은 이 인터페이스를 통해 반환하지 않는다.
OS 저장소 항목은 프로필/계정/version과 일치해야 한다. 잠김·누락·변조는 실패이며
기존 DB가 있는 상태의 새 키 생성이나 자동 pin 교체는 금지한다.

## 지원 판정

터미널 demo·cross-compile·stub 검사는 메일 지원 완료가 아니다. OS별 실제 실행,
OS 저장소, 인증, SQLCipher migration, 합성 제공자 메일 흐름, 설치/업데이트/제거가
모두 검증돼야 한다. Linux headless/SSH의 암호화 금고는 별도 제안과 검증이 필요하다.
