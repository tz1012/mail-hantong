# Linux 제품 통합 계약 채택안 R2

상태: **채택 대기 제안 / 제품 경로 미변경 / 사람 보안 검토 완료 아님**.
독립 검토에서 발견한 첫 updater의 순환은 **E9의 1회 OS-Python 부트스트랩
예외 제안**으로 구체화했다. beta29에 없는 updater를 가정하지 않는다.
새 Python 전제·알고리즘은 기존 승인에 포함시키지 않고 아래 결정에 명시한다.
정확한 payload와 SHA는 구현 후 실제 사람 검토·native 검증 전에는 실행하지 않는다.
기준 제품/기록 commit: `7732cd12fbd508de99a47839ecb62c74b3c04696`.
채택 대상 전체 commit은 이번 수정본을 고정한 최종 응답 SHA이며, 아래 문서의
R2 조항을 함께 가리킨다. 이전 포괄 배포 승인을 새 계약 채택으로 간주하지 않는다.

## 정확한 문서·조항

| 문서 | 채택 대상 | 보장·운영 변화 |
| --- | --- | --- |
| `docs/adr/proposed/ADR-033-linux-sealed-release-identity.md` | R2 권고안 | Linux만 per-process sealed identity 선택. fs-verity/shared broker/legacy signal allowlist는 선택하지 않음. |
| `docs/contracts/proposals/linux-execution-identity-v2-r2.md` | E1–E9 전체 | FD197–201, 봉인 사본 측정 후 같은 객체 실행, 실제 peer image/FD·release 문맥·process instance 결속, stop-before-activate, 최초 OS-Python bootstrap 예외. |
| `docs/contracts/proposals/linux-static-pinned-delivery-20261107.md` | D1–D6 전체(R2) | 기존 Pages의 정확한 버전/전체 commit 경로, 독립 전달 pin·provenance, 고정 OS curl을 쓰는 Rust updater, 공유 피드 무변경. |

**왜 채택이 필요한가:** 기존 `peer_matches_current_image`의 프로세스 간 동일 inode
비교는 실행마다 inode가 다른 memfd와 양립하지 않는다. R2는 그 검사를 지우는 대신
각 프로세스 실제 image↔보유 FD inode 일치와 정확한 trusted release/root/code 비교로
교체한다. `current_exe` 기반 릴리스 탐색·relay caller·stop도 함께 바뀐다.
이는 AGENTS.md §7의 “계약을 바꿔야 하면 구현 전에 변경 제안을 보고하고 멈춘다”에
해당하므로, 기존 계약을 임의로 대체하는 제품 코드는 아직 작성/연결하지 않았다.
현재 가능한 독립 격리 시험과 통합 준비는 계속했다.

## 알아야 할 전제와 영향

- 해결 대상: 최종 봉인 사본 검증 뒤 선택된 한통 main ELF의 동일 inode 쓰기,
  truncate, writable mapping 변경과 실행 이미지/IPC/stop 문맥 불일치.
- 새 memfd 이름·argv·환경 경로·public JSON·같은 digest만으로 신뢰하지 않는다.
  root/pin/manifest/context는 자기의 독립 신뢰 문맥과 비교하며 inspection 거부는 거부다.
- 이미 실행 중인 최초 mutable bootstrap, active same-UID 사전 설치 트리/로컬 pin
  치환, loader/LD_PRELOAD·ptrace·root/kernel까지 해결한다고 주장하지 않는다.
  독립 expected digest와 신뢰 bootstrap이 없으면 배포/설치를 진행하지 않는다.
  최초 명령부터 active same-UID 공격에 견뎌야 한다면 별도 OS 보호 launcher 결정이
  필요하다. 이 한계 명시는 요청된 검증 후 쓰기 위험의 수용·면제가 아니다.
- 기존 OS root 소유 `/usr/bin/curl`·loader/library/CA를 다운로드 외부 전제로 사용한다.
  기존 Unix 설치의 curl 요구를 고정 경로/설정·proxy 환경 배제/redirect 금지로 좁힌다.
  한통 sealed-image 보장 대상으로 포장하지 않는다. 새 crate·키·자동 설치는 없다.
- E9는 **이미 설치된 root 소유 `/usr/bin/python3`를 최초 1회만 요구하는 새 결정**이다.
  `-I -S -c`의 정확한 검토 payload/독립 digest 입력으로 시작하고 mutable script를
  먼저 실행하지 않는다. 안전한 OS stdlib/import 경로를 검증하고 후보 Core를 봉인·
  재측정한 같은 FD로 실행한다. bootstrap Core는 IPC/메일/Secrets 서비스를 열지 않고
  E7 잠금·socket 부재 확인 후에만 활성화한다. Python/stdlib/최초 terminal 입력은
  E1 외부 신뢰 전제이며 누락/불안전이면 거부한다. 정상 v2 업데이트에는 사용하지 않는다.
- beta.29→v2 최초 전환은 **Core가 완전히 없는 오프라인 상태**에 한정한다. socket 부재와
  기존 socket/DB/key 잠금을 확보해야 한다. 새 helper의 구버전 signal, 자동 로그아웃·
  재부팅·stale socket 삭제는 없다. 필요한 사용 중단 창은 별도 사람 조작이지 이번
  문서 채택으로 OS 재부팅을 자동 실행하는 허가가 아니다.
- kernel 기능/FD 한도/`/proc` 검사 또는 noexec 정책이 허용하지 않으면 안전하게 실패한다.
  sysctl·ACL·권한을 낮추거나 mutable 파일·다른 interpreter로 우회하지 않는다.
- 새 주소를 만들지 않는다. D1 제안은 기존 공식 호스트의
  `/mail-hantong/artifacts/linux-x86_64/VERSION/FULL_COMMIT/`만 선택한다.
  채택 후에도 최종 gate 전 경로 생성·업로드·공유 Release/tag 변경은 하지 않는다.

## 최소 사용자 결정 문구

수정본 전체 SHA를 최종 응답의 값으로 지정하여 다음과 같이 확인하면 대상을 특정할 수 있다.

> 해당 SHA의 ADR-033 R2와 실행 정체성 E1–E9, 고정 전달 D1–D6을 Linux 구현 계약으로
> 채택합니다. 기존 OS Python3를 사용하는 최초 1회 bootstrap 예외, OS curl 전제,
> 기존 공식 Pages 고정 경로와 beta.29 오프라인 전환을 포함합니다.
> 새 차분의 실제 사람 보안 검토와 동일 최종 커밋의 필수 제품 검증 전에는 배포하지 마세요.

이 문구는 **아직 받은 승인이 아니다**. 일반 배포 승인은 이미 있으므로 다시 요청하지
않는다. 계약 채택도 실제 사람이 미래 코드를 검토했다는 기록은 아니다.
6322b9a의 기존 사용자 확인은 미변경 범위 코드에 대해 그대로 보존한다.

## 채택 후 구현·완료 순서

1. E1–E7/E9로 release lookup/launcher/peer/relay/stop/update/bootstrap을 함께 구현한다.
2. sealed image·context 위조/FD 수명/PID·exec 경쟁/noexec/오프라인 잠금/중단 설치와
   Windows·Mac 기존 피드 선택 거부·정상 회귀를 수행한다.
3. 동일 최종 SHA의 실제 Linux package/install/update/보존/Core restart/scope recovery,
   독립 검토 발견점 조치와 새 전체 차분 사람 검토를 기록한다. Mac 실패는 유지한다.
4. D6 통과 후 이미 받은 조건부 승인으로 정확한 정적 자산을 게시·재다운로드 검증한다.
5. 송파 고정 업데이트→정상 scope 확인→Gmail2개+Daum1개 각각 실제 목록·본문 조회.

송파 beta.29의 알려진 refresh CONFLICT와 이전 거부된 권한 변경은 반복하지 않는다.
자격증명·계정·금고·메일·권한 초기화/삭제나 자동 권한 부여는 어떤 단계에도 없다.
제품 보안 수정·배포·합성 성공만으로 세 계정 조회 완료를 선언하지 않는다.

## 이번 수정본의 신선한 검증과 미검증 범위

WSL Linux에서 합성 ELF 실행 8개와 기존 봉인 기작 4개, 총12개가 실패·제외 없이
통과했다(`python3 -m unittest discover -s tests/Build -p 'test_linux_sealed*.py' -v`).
실행 시험은 봉인 후 같은 FD 실행과 실제 `/proc/self/exe`↔상속 image FD inode 일치,
다른 객체 FD 교체 거부, 복사 중 변경 거부, 복사 후 원본 변경 격리, 봉인 후 쓰기·
truncate·쓰기 mmap 거부, 명시적 non-executable seal의 실행 거부, 자식 timeout/reap를
다룬다. fixture FD 번호는 시험 내부 값이며 E2 제품 map 구현을 증명하지 않는다.
unsealed 음성 시험은 의도적으로 fixture를 실행해 자식 검사 거부를 확인한다.
시스템 `vm.memfd_noexec=2` 설정 시험이나 제품의 pre-exec admission 성공 증거는 아니다.
Windows에서는 새8개가 Linux 전용으로 제외됐으며 이를 Windows 통과8개로 세지 않는다.

문서 시험29개, 새 Python compile, diff check 및 추적표 생성/검사가 통과했다.
제품 Rust·installer·update.sh는 이번에 바꾸지 않았다. 따라서 새 제품 IPC/relay/stop/
update나 최종 package/install/restart 회귀·실제 세 계정 조회는 아직 미검증이다.
앞선30102cfd의 Linux scope 회귀10개 결과를 이번 수정본의 전체 native gate로
재사용하지 않는다. canonical hash에는 현재 규칙상 C suffix가 포함되지 않으므로
새 합성 C fixture는 전체 Git commit 및 소스 archive로 결속하며 hash 규칙을 바꾸지 않았다.
