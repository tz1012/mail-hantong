# Unix 실행 경쟁 완화·Mac 교차 버전 검증

상태: **Mac 실패 원인 진단 계속 / 공식 배포 보류 / 새 보안 변경 사람 검토 필요**. 아래의 기존 자동 검증은 종료됐으며, 사용자 후속 지시에 따라 두 대기 지점을 구분하는 진단과 재시험을 이어간다. 이 문서는 위험 수용 기록이 아니다.

## 고정 제품 소스

- 새 제품 코드: `9c7f91eea0933c8d957be33d3e3b98267918ee22`.
- 제품 소스 해시: `eb4747adfdf9498b90a26e45ae55ccc9f57c2dc62f86b5696e958d40afe23eb1`.
- 사용자 확인이 전달된 이전 대상: `6322b9a1a5f27f7e0ef32a65554ef20a79db0be6`. [확인 원문 기록](HUMAN-SECURITY-REVIEW-CONFIRMATION-20261106.md)은 보존하며 새 변경을 검토한 것으로 확대하지 않는다.
- 새 검토 범위: `git diff 6322b9a1a5f27f7e0ef32a65554ef20a79db0be6..9c7f91eea0933c8d957be33d3e3b98267918ee22`. 보고 문서 커밋은 제품 코드와 별개다.

## 구현

`crates/hantong-platform/src/unix_release.rs`는 manifest에서 실제 해시 검증한 파일 핸들·원래 스냅샷·digest를 유지한다. `VerifiedCoreCommand`는 원시 Command를 외부에 노출하지 않고 spawn/status/output마다 부모에서 동일 파일의 전체 해시를 다시 검사한다. 재검사에서 검출한 크기·내용·모드·소유자·inode·디렉터리 교체는 실행을 거부한다. 마지막 검사 이후의 모든 변조를 차단한다는 뜻은 아니다. 자식은 미리 준비된 경로와 스냅샷으로 fstat/lstat 등 async-signal-safe 검사만 수행한다.

Linux 바이너리는 보유한 FD의 `/proc/self/fd`로 실행한다. 마지막 경로 검사 후 교체가 일어나도 교체된 프로그램을 실행하지 않는다. 해당 경로를 사용할 수 없으면 실패하며 원래 패키지 경로로 폴백하지 않는다. Mac 바이너리는 마지막 경로 검사를 추가했지만 pathname exec는 유지된다. Mac 원자적 실행 경쟁이 해결됐다고 표시하지 않는다.

Core 시작·터미널 검토, relay, MCP 검토, TUI→CLI, AI `/tui`, 업데이트 스크립트와 Core stop에 적용했다. 허용 asset은 Core·CLI·TUI·update.sh의 고정 상대 경로뿐이다. 업데이트 스크립트는 고정 OS `/bin/sh`가 검증한 공개 FD를 읽는다. 스크립트 FD는 0 위치로 되돌리고 그 공개 FD에 한해서만 자식의 CLOEXEC을 해제한다. 비밀 FD·자격증명·ACL·등록·승인 범위를 변경하지 않는다. TUI는 패키지 검증보다 먼저 잘못된 입력을 거부한다.

## 실행 경쟁 회귀시험

로컬 Linux `unix_release` 15개와 TUI 45개가 통과했다. 기존 잘못된 입력·접근의 거부 시험은 유지했다. 새 시험은 인자·파이프, manifest digest 기준 보존, 실행 전 교체·symlink·동일 크기 쓰기·모드·bin 교체 거부, 16회 실제 경쟁, 마지막 검사 후 강제 교체, script FD 실행 및 CLI/TUI asset을 다룬다. 강제 경쟁 시험은 스케줄러가 우연히 경쟁 창을 맞추기를 기다리지 않고 자식의 마지막 검사가 끝난 뒤 교체를 삽입한다.

최초 같은 크기 쓰기 시험이 실패해 스냅샷 검사에 더해 부모의 재해시를 적용했다. Mac 첫 실행은 없는 `/bin/false` 시험 경로로 3개가 실패했고 `/usr/bin/false`로 정렬했다. 이후 전체 검사에서 기존 TUI 입력 거부 순서 시험이 실패해 실제 검증 순서를 수정했다. 시험 assertion·제한·거부 조건은 유지했으며 실패·취소 기록은 PROGRESS에 남긴다.

후속 Linux 전체 검사에서는 기존 프로필 게시 복구 시험이 `CONFLICT`로 실패했다. `unix_profile_publication.rs`의 게시 객체가 잠금 파일을 닫아도, 병렬 fork가 같은 open-file-description을 상속하면 exec까지 잠금이 남을 수 있었다. 복제 FD를 유지하는 결정적 회귀가 수정 전 같은 충돌로 실패했다. 소유 객체의 Drop에서 잠금을 명시적으로 해제해 이 지연을 제거했다. 해제 실패는 close까지 기존 잠금을 유지하는 방향으로 남는다. 회귀는 소유자 생존 중 배타성, drop 후 복구, 오래된 FD 종료 후에도 새 소유자 잠금 유지, 게시 바이트와 백업 정리를 검사한다. 기존 회귀를 삭제·직렬화·재시도 처리하지 않았다. 수정 후 로컬 Linux Platform58개와 all-target Clippy `-D warnings`가 통과했다.

## Mac 호환성 증거의 두 범위

1. **공식 beta.29 바이너리→후보 바이너리**: 공개 Mac Intel/ARM 아카이브 SHA-256을 고정 검증한다. 실제 구버전 CLI/Core가 격리된 CI Keychain에 DB 키와 암호화 DB를 만들고, 후보가 같은 금고·DB를 열어 두 번 실행·종료하는지 검사한다. 기존 앱 루트·socket이 있으면 거부하며, 살아 있는 socket이 남으면 데이터 정리를 거부한다. 공식 구버전이 메일 자격증명까지 생성했다는 시험은 아니다.
2. **고정 beta.29 소스→후보 소스**: beta.29 전체 커밋을 확인한 뒤 동일한 Secrets 내부 unit fixture만 임시로 추가한다. 합성 IMAP·SMTP 자격증명을 만들고 후보가 IMAP old→new와 SMTP 보존 값을 Secrets 안에서 실제 비교하며 재시작 후 다시 검사한다. 값은 로그·외부 API로 내보내지 않는다. 시험이 끝나면 lib.rs 원본 바이트를 복원하고 주입한 모듈을 제거한다. 제품 빌드에는 포함되지 않는다. 실제 공식 바이너리의 메일 Keychain item identity 증거와 구분한다.

두 하네스는 GitHub Mac 격리 환경으로 제한하고 정확한 CI Keychain 기본값·검색 목록을 검사한다. 실제 사용자 금고·비밀번호·계정·메일을 사용하지 않는다. 소스 fixture는 단계별 900초 제한과 정확히 1개 시험 실행을 요구하며, 시간 초과 시 소유한 프로세스 그룹을 회수한다. 기존 Keychain ACL을 넓히거나 접근을 자동 승인하지 않는다.

초기 호환성 하네스는 시간 초과 때 진행 증거가 사라졌다. 이 공백을 보완해 Secrets fixture는 `open/opened/read-old/write-new/read-new/done` 등 고정 표식만 출력하며 부모는 유한 허용 목록과 완전 일치하는 표식만 최대512자로 기록한다. 공식 바이너리 시험은 `old/candidate`와 반복번호·install/start/status/stop을 구분한다. 실패는 고정 종류만 출력하고 원래 exception·stderr·Cargo 원문을 노출하지 않는다. `markers=none`은 표식 증거가 없다는 뜻이며 컴파일 실패나 Keychain 실패로 확정하지 않는다. 이 필터·timeout 회수·0시험 거부의 독립 회귀5개가 통과했고, 독립 읽기 전용 검토에서 진단 변경의 추가 결함은 발견되지 않았다.

진단 보완 커밋 `2c7ad7a`의 실제 Mac 두 아키텍처 로그에서는 다음이 직접 확인됐다. 공식 beta.29의 `old install/start-1/status-1/stop-1`이 완료됐고 후보는 `candidate start-1` 후 제한시간 내 초기 화면을 완료하지 못했다. 별도 소스 시험의 `seed:open/opened/read/done`은 성공했지만 후보 `upgrade`는 `fixture_started=true`, `upgrade:open/opened/read-old`에서900초 시간 초과했다. 따라서 후보 시험 바이너리가 시작되지 않은 문제는 아니며, 저장소를 연 뒤 기존 합성 자격증명 읽기를 마치지 못했다. `write-new`·`done`·`reopen` 성공은 입증되지 않았다. 잠금·ACL·서명 요구사항 중 어느 원인인지는 이 표식만으로 확정하지 않는다.

현재 backend는 `apple-native-keyring-store 1.0.2`의 legacy `SecKeychain` 경로다. Apple은 file-based Keychain의 ACL과 코드 서명 요구사항에 따라 OS 상호작용이 필요할 수 있다고 설명한다. 이것은 가능한 조건에 대한 공식 설명이며 현재 timeout의 원인을 확정한 증거가 아니다. [Apple Keychain 구현 구분](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains), [ACL 설명](https://developer.apple.com/documentation/security/access-control-lists), [코드 서명과 Keychain](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/AboutCS/AboutCS.html).

## 최종 자동 검증

- [최종 네 플랫폼 전체 검사](https://github.com/tz1012/mail-hantong/actions/runs/42206703903): Windows·Linux·Mac Intel·Mac ARM 및 CI required 모두 성공. `2c7ad7a`의 Linux 전체 검사 실패는 위 잠금 수정의 계기이며 성공으로 바꾸어 기록하지 않는다.
- [최종 Linux scope·패키지·설치 gate](https://github.com/tz1012/mail-hantong/actions/runs/42206703892): 성공. 내려받은 metadata 소스 해시·SHA256SUMS와 아카이브 내부 manifest 565개 파일의 크기·해시가 모두 일치한다.
- [최종 Mac 두 아키텍처 패키지·호환성 gate](https://github.com/tz1012/mail-hantong/actions/runs/42206731004): Intel·ARM 모두 실패로 종료. 양쪽 모두 공식 구버전 install/start/status/stop과 후보 install은 성공했으나 후보 start-1이 완료되지 않았다. 별도 소스 시험 seed는 성공했고 후보 upgrade는 `fixture_started=true`, `upgrade:open/opened/read-old`에서900초 시간 초과했다. 원인을 Keychain 잠금·ACL·비밀번호 문제로 확정하지 않는다. 후보의 갱신·reopen 성공은 입증되지 않았다. CI 내부 패키지 빌드는 성공했지만 설치 smoke와 자산 upload는 필수 호환성 실패로 실행되지 않았으며 Mac 패키지를 배포 자산으로 채택하지 않는다.
- Windows 최종 후보 빌드 exit 0. 직전 `438af22` 패키지로 격리 한글 경로 설치·제거·beta.29 업그레이드 통과. 최종 ZIP은 그 패키지와 모든 항목을 비교해 `BUILD-METADATA.json`, `release-manifest.json` 두 파일 외의 모든 바이트가 같음을 확인했다. 최종 manifest의 모든 레코드와 소스 해시도 검증했다. 메타데이터만 바뀐 최종 ZIP 자체를 다시 설치했다고 표시하지 않는다. 미지 파일·별도 합성 데이터·실제 설치 포인터·User PATH 보존을 확인했으며 실제 계정 업그레이드 시험은 아니다.
- 최종 Rust 실행 합계: Windows 1,981, Linux 1,233, 각 Mac 1,092, 실패·ignore 0. 고유 시험 개수가 아니라 자식/필터 실행을 포함한 합계다. 각 플랫폼 Docs29개 통과. Build158개 중 Unix의 기존 Windows 전용 조건부45개, Windows의 기존 prebuild TUI 실행 파일 부재 조건부1개 제외를 실제 실행으로 계산하지 않는다. 새 skip·ignore·시험 삭제·제한시간 증가는 없다. 이 전체 검사 성공이 별도 Mac 필수 호환성 gate 성공을 의미하지 않는다.

보류된 Windows beta.30 ZIP: `0a9479fa5085daae0f1e172ccfd79295623069e28db7663c20eac5dc7ac26335`.
보류된 Linux beta.30 tar.gz: `e44450b685d1bda7a93ba40e255c3cefe780e4b4e851076a581c8b31c3c3a4f4`.
고정 커밋 소스 ZIP: `c48636a23bf8f8d4b40279117c1ebe06794f61e7462a0fa6116de0936495ccae`.
이 값은 공식 공개 자산 체크섬이라는 뜻이 아니다.

최종 CI의 모든 run head는 위 제품 커밋 `9c7f91e`와 일치한다. Windows·Linux metadata와 고정 커밋 소스 ZIP을 추출해 같은 canonical source hash를 재현했다. Mac 자산은 실패한 gate로 인해 내려받아 검증한 공개 후보가 없으며, 네 플랫폼 배포 자산이 모두 준비됐다고 표시하지 않는다. 공식 beta.30 release 조회는 HTTP404였고 현재 공개 최신 버전은 beta.29다.

## 남은 기술·검토 조건

- **Mac 바이너리의 마지막 경로 경쟁은 남는다.** 공개 exec/spawn API는 경로를 받으며 Linux 기법을 그대로 적용하지 않았다. Apple XNU는 exec에서 namei로 얻은 vnode를 직접 읽고, `/dev/fd`의 특수 복제는 vn_open 경로이므로 Mac 바이너리 실행의 대체 수단으로 주장하지 않는다. [Apple exec 코드](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_exec.c#L2395-L2446), [fdesc 구현](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/miscfs/devfs/devfs_fdesc_support.c#L341-L399).
- **Linux도 최종 검사 뒤 원래 inode에 쓰는 공격을 완전히 막지는 못한다.** 열린 FD 실행은 rename 교체를 막지만 파일 내용의 동시 변경 자체를 막지 않는다는 OS 한계를 유지한다. [Linux fexecve 설명](https://man7.org/linux/man-pages/man3/fexecve.3.html). 봉인된 실행 이미지와 현재 package/image 결속을 함께 다루는 설계는 이번에 구현하거나 승인 처리하지 않았다.
- 독립 AI 검토는 놓친 sibling 실행 경로를 찾아 수정했으며, 이후 새 실행 경로 결함은 찾지 못했다. 위 두 한계는 남는다고 보고했다. AI 검토를 AGENTS §6의 사람 검토로 대신하지 않는다.
- AGENTS §6에 따라 새 Platform 실행 경계와 호출부는 새 대상 커밋으로 사람 검토가 필요하다. 기존 `6322b9a` 확인 원문은 보존하지만 새로운 결과·위험 수용을 만들어 넣지 않는다. 별도 OS 권한·서명·키 접근 설계가 필요하면 AGENTS §7의 구체적 제안 절차를 따른다.

새 사람 검토에서 확인할 항목은 정확한 manifest 파일·digest 유지, 모든 sibling 실행 경로의 보호 wrapper 사용, 자식의 async-signal-safe 처리, 공개 script FD만 상속하는지, Linux FD 실행의 폴백 부재와 남은 같은-inode 쓰기 한계, Mac pathname 실행 한계, 게시 잠금의 소유자 수명·명시적 해제·복구 배타성, 합성 Secrets 시험의 값 비노출·원본 바이트 복원·Keychain ACL 보존, CI 커밋과 패키지 metadata의 일치다. 필요한 기록은 실제 검토 원문과 대상 전체 커밋·확인 범위이며, 검토하지 않은 세부 결과나 위험 수용은 대신 작성하지 않는다. 사람 검토 완료 확인만으로 실패한 필수 시험이나 알려진 기술적 공백을 통과 처리하지 않는다.

근거는 [AGENTS.md](../../AGENTS.md) §6 ‘사람 검토가 필요한 영역’의 “아래 경로를 바꾸는 변경은 테스트가 통과해도 에이전트가 완료로 표시하지 않는다”와 현재 Rust 대응 목록의 `hantong-platform`·IPC 인증·Secrets 경계다. §9도 해당 영역에 `[사람 검토 필요]` 표시를 요구한다. 별도 계약 변경에는 §7의 “계약을 바꿔야 하면 구현 전에 변경 제안을 보고하고 멈춘다”가 적용된다. 사용자 배포 승인은 접수된 상태이며, 이 조항을 배포 승인이 없다는 뜻으로 바꾸거나 추가 검토자 자격을 요구하지 않는다.

### 현재 환경에서 입증하지 못한 범위와 후속 조건

- Mac 후보가 이전 바이너리의 DB 키를 읽고 재시작까지 완료했다는 증거와, 기존 합성 메일 자격증명의 갱신·재개방 성공 증거가 없다. 격리된 실제 Mac CI에서 시험했지만 실패했다. 현재 작업 호스트는 Windows이며 해당 Mac CI의 대화형 화면은 제공되지 않아 OS 요청 창 유무·종류를 관찰하거나 사람이 직접 응답하는 시험은 수행하지 못했다. 시간 초과를 OS 승인 부족으로 확정하지 않는다.
- 다음 진단은 기존 selector 생성·보호 읽기 호출의 세부 단계와 짧은 시점의 프로세스 스택, old/candidate의 실제 코드 서명 요구사항을 비교해야 한다. OS 상호작용이 관찰된다면 비관리자 시험 사용자·합성 금고만 있는 폐기 가능한 Mac 격리 환경에서 정상 사용자 단계를 검증해야 한다. 영구 신뢰 추가·ACL 확대·실제 사용자 금고 사용으로 성공을 만들지 않는다.
- legacy Keychain의 no-UI/OSStatus 진단은 별도 선택지다. 현재 `security-framework`는 전이 의존성이고 Secrets가 직접 호출할 수 있는 API는 노출돼 있지 않다. 이를 이미 실행했다거나 원인이 확인됐다고 표시하지 않는다. 직접 의존성·backend 노출·FFI를 추가한다면 해당 변경을 명시해 검토해야 하며 이번 제품에 임의로 추가하지 않았다.
- 같은 UID의 쓰기까지 막는 Linux 봉인 이미지 실행은 현재 설치 이미지·IPC 피어 결속과 함께 설계해야 한다. Mac 역시 현재 경로 검사만으로 원자적 실행을 보장하지 않는다. 다른 설치 권한·서명 신뢰·프로세스 이미지 식별을 도입하는 변경을 기존 계약에 자동 승인하거나 사용자 권한을 높여 우회하지 않았다.

공식 배포의 재개 조건은 남은 실행 경계에 대한 구현·계약 정합성과 새 변경의 사람 검토, 실제 Mac 호환성 실패 해결 및 해당 제품 커밋의 필수 검증 통과다. 기존 배포 승인은 유지하며 위험 수용으로 조건을 대체하지 않는다.

현재 공식 배포·송파 업데이트·실제 계정 연결 시험은 진행하지 않았다. 기존 데이터·금고·메일·인증정보·권한과 이전 승인 차단을 보존한다. 자동 승인·메일 발송·휴지통 이동·자격증명 재생성은 없다.
