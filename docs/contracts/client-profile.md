# 로컬 CLI 공개 프로필 v1

상태: 2026-10-30 등록 UI 연결을 위한 구현 계약. 사람 보안 검토 필요. 이 문서는 데이터 형식과 배포 순서를 정하며 프로필을 파싱한 것만으로 권한을 만들지 않는다.

## 선택과 형식

초기 제품은 `default` 선택자 하나만 제공한다. CLI의 `--client`는 선택자이며 경로·등록 ID·키 이름·서버 pin을 직접 받을 수 없다. 환경 변수·unauthenticated readiness·네트워크 응답으로 프로필을 만들거나 pin을 갱신하지 않는다.

파일 내용은 최대 32KiB UTF-8 JSON object다. BOM, trailing JSON, 중복/알 수 없는 field, 잘못된 타입/상한은 거부한다. 필드는 전부 필수이고 null은 허용하지 않는다.

| 필드 | 값 |
| --- | --- |
| schema_version | 정수 1 |
| profile | 문자열 `default` |
| registration_id | 16 bytes의 소문자 hex, 전부 0 금지 |
| client_key_id | 32 bytes의 소문자 hex, 전부 0 금지 |
| client_public_key | CNG P-256 ECDSA public blob 72 bytes의 소문자 hex |
| server_public_key | pinned CNG P-256 ECDSA public blob 72 bytes의 소문자 hex |
| profile_epoch | 1..i64::MAX 정수 |
| account_ids | ASCII account ID 최대 128개, 중복 금지, 빈 배열은 권한 0개 |
| tool_ids | 정확히 `["mail.accounts.list"]` |
| read_only | true |
| requests_per_minute | 정수 60 |

ID와 scope는 기존 auth 계약을 재사용한다. 공개 blob은 길이와 ECS1/32-byte 좌표 header를 파싱 단계에서 검사하며 실제 P-256 점 검증은 CNG import/auth 경계가 수행한다. parser가 cryptographic validation 또는 신뢰를 부여한다고 주장하지 않는다. client_key_id에서 고정 `mail-hantong-client-<hex>` 선택자를 계산한다. 개인 키/비밀번호/token/승인 결과/nonce는 프로필에 없다. Debug/오류는 경로·ID·키·scope 원문을 숨긴다.

parser/encoder는 `hantong-ipc-client`의 순수 데이터 API다. 생성자/JSON bytes는 검증되지 않은 공개 데이터이며 이 타입으로 사람이 승인했다는 증거를 만들 수 없다. 실제 인증은 신뢰된 고정 경로의 파일을 읽고 기존 CNG 키의 공개값·서버 pin·등록 epoch를 대조한 뒤 상호 서명을 수행한다.

## 경로·발행과 복구

client_key_id는 `cng-identity.md`의 registration_id 도메인 hash로 계산하며 loader가 일치 여부를 대조한다. 개인 키는 무작위로 생성되고 이 hash는 공개 selector일 뿐이다.

파일 I/O는 드라이브 루트부터 각 조상을 reparse 없이 열고 SHARE_DELETE를 제외해 유지한다. 제품 루트·profiles 폴더·파일은 현재 사용자/SYSTEM/Administrators 소유자와 실제 allow-list DACL만 허용한다. 기존 디렉터리는 같은 allow-list를 만족하는 상속 ACE도 허용하며 DACL을 변경하지 않는다. 프로필 파일은 상속 보호된 DACL이 필수이고 새 디렉터리/파일도 보호된 DACL로 만든다. 다른 주체 권한이 있는 경로/파일을 자동으로 고치지 않는다. loader는 어떠한 ACL·폴더·파일도 수정하지 않는다.

이 구분은 기존 디렉터리의 DACL 보호를 설정하면 Windows가 자식 파일의 상속 metadata를 바꿀 수 있음을 격리 fixture에서 확인해 정했다. 기존 파일 보존을 위해 디렉터리 권한은 현재 실제 allow-list로 검증하고, 파일에는 상속 보호를 요구한다. 이후 부모 ACL에 다른 주체 권한이 생기면 다음 loader/publisher 검증이 실패한다. 같은 사용자·관리자에 의한 ACL 변경은 기존 잔여 위험 범위이며 이를 완전히 격리한다고 주장하지 않는다.

publisher는 열린 임시 파일 handle에서 쓰기·flush 후 `SetFileInformationByHandle(FileRenameInfo)`로 고정 절대 목적지에 no-replace rename한다. 모든 조상 handle을 유지하고 파일을 닫은 뒤 경로로 rename하지 않는다. RootDirectory 상대 rename은 이 Windows 환경에서 ERROR_INVALID_PARAMETER로 실패해 사용하지 않는다. 기존 파일은 canonical bytes가 정확히 같을 때만 재사용한다. 반환하는 불투명 PublishedProfile guard는 파일·조상 handle을 Store active CAS까지 유지하며 승인 증거는 아니다. 파일에는 SHARE_WRITE/SHARE_DELETE를 허용하지 않는다. 다른 형식의 동등한 JSON도 자동 덮어쓰지 않는다.

경로는 OS Known Folder로 구한 LocalAppData 아래 고정 `mail-hantong/profiles/default.json`이다. publisher/loader가 조상 reparse·경로 교체·권한을 검증하고 열린 handle로 수명을 고정해야 한다. 임시 파일은 loader가 읽지 않는다. 일반 CLI가 파일을 생성·복구·덮어쓰지 않는다. 경로 I/O 구현은 별도 보호 경계이며 순수 codec 완료를 배포 완료로 기록하지 않는다.

코어는 신뢰 창의 일회성 사람 확인과 현재 요청 결속을 검증한 뒤 Store에 approved request + provisioning registration을 같은 트랜잭션으로 저장한다. 기존 CNG 키/공개값을 검증하고 고정 경로에 원자적으로 프로필을 발행한 후에만 registration을 active로 CAS 전환한다. 기존 파일이 정확히 같은 프로필이면 복구에 재사용할 수 있지만 불일치하면 자동 덮어쓰기/재pin하지 않는다. 파일만 있고 active 등록이 없으면 메일 접근은 생기지 않는다.

소프트웨어 CNG와 같은 사용자 파일 ACL은 같은 사용자 악성코드에 대한 완전한 격리를 보장하지 않는다. 등록 허용은 서버의 영속 grant와 현재 epoch를 매 요청 재검증하며 프로필의 선언 자체를 권한으로 쓰지 않는다. 첫 UI 구현은 default/read-only/accounts.list에 한정하고, 다른 프로필·쓰기 권한은 후속 계약에서 추가한다.
