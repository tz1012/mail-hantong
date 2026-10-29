# IMAP 폴더 관측 저장 v1

상태: 2026-10-30 M2-03/EXT-06의 v8 선행 구현 계약. 연속 개발 위임에 따라 작성하며 **사람 검토 필요**. [폴더 탐색](imap-folder-discovery.md)의 완전한 snapshot만 저장한다. 새 공개 도구·Core 자동 접속·UID 동기화·메일 삭제는 이 범위에 없다.

## 저장과 의미

v7 DDL을 `migrations/schema-v7.sql`로 고정하고 `007-to-008.sql`로 두 표를 추가한다. `imap_mailbox_catalogs`는 계정별 최신 bounded JSON 관측 한 행, `imap_mailbox_catalog_operations`는 원자 적용의 중복 방지 이력이다. 기존 계정·설정·감사·등록·scope/setup 저널은 변경하지 않는다. v7 검증→암호화 백업→DDL/metadata/user_version 변경→v8 검증을 한 migration 단계로 수행하며 실패하면 rollback한다.

catalog는 폴더의 최근 관측 자료다. UIDVALIDITY·UID·message·cursor·로컬 초안·사용자 지정 역할 매핑과 분리한다. 새 snapshot에 폴더가 없어도 메일이나 대기 작업을 삭제하지 않는다. SPECIAL-USE 역할 완전성은 false이고 표시명으로 역할을 추정하지 않는다. 현재 연결이나 쓰기 권한을 증명하는 데이터가 아니다.

## Domain storage codec

`ImapMailboxSnapshot::to_storage_json()`과 `from_storage_json(bytes)`는 외부 의존 추가 없이 Domain의 기존 serde/serde_json을 사용한다. 최대 JSON bytes는 8MiB다. 이름·descriptor·개수·wire 총량의 기존 한도도 모두 유지한다. `ImapMailboxName`은 생성 때의 encoding을 보존하며 snapshot은 서로 다른 encoding의 이름 혼합을 거부한다. 빈 snapshot은 이름 encoding이 없어도 유효하다.

고정 JSON 필드 순서는 `schema_version:1`, `account_id`, `settings_version`, `capabilities`(정렬된 고정 ID), `special_use_complete:false`, `mailboxes`(canonical wire 순)다. 각 mailbox의 순서는 `wire_hex`(소문자 hex), `display_name`, `encoding`(`modified_utf7|utf8`), `delimiter`(한 문자 또는 null), `no_select`, `non_existent`, `children`(`unknown|has_children|has_no_children`), `roles`(Domain enum 순서의 고정 소문자 이름)다.

decode는 unknown/duplicate field·enum·범위·중복 mailbox·비정규 ID/역할/목록 순서를 거부한다. 모든 wire 이름을 다시 decode하여 display와 일치함을 확인한다. INBOX만 wire는 정확히 `INBOX`, display는 ASCII 대소문자 변형을 허용한다. 다른 이름에는 정규화·손실 복구를 하지 않는다. 재직렬화 bytes가 입력과 정확히 같아야 하며 공백·키 순서·대문자 hex 같은 비정규 저장도 거부한다. 이 codec은 신뢰할 수 없는 저장 자료의 검증이지 권한 발급이 아니다. Debug/오류에는 이름과 원문을 넣지 않는다.

## Store API와 결속

`imap_catalog_payload_hash(expected_account_version, endpoint, snapshot, expected_generation)`는 아래 canonical payload의 SHA-256을 반환한다. `Store::replace_imap_mailbox_snapshot(expected_account_version, endpoint, snapshot, &MailMutation)`는 같은 hash를 재계산한다. account_id는 snapshot의 명시된 값이며 빈 목록을 전체 계정으로 해석하지 않는다. `MailMutation.expected_version`은 catalog generation이다. 최초는 0, 성공은 +1이며 i64 범위를 넘지 않는다. endpoint 버전은 snapshot.settings_version과 같아야 한다.

endpoint hash는 `mail-hantong.imap-catalog-endpoint.v1\0` + account_id의 u32 big-endian 길이와 UTF-8 bytes + Domain endpoint의 canonical JSON bytes를 SHA-256한다. payload hash는 `mail-hantong.imap-catalog-payload.v1\0` + expected_account_version(u64 BE) + expected_generation(u64 BE) + endpoint_hash(32 bytes) + snapshot JSON byte 길이(u64 BE) + canonical JSON bytes를 SHA-256한다. snapshot_hash는 canonical JSON bytes 자체의 SHA-256이다. 비밀값은 포함하지 않는다. Windows SHA-256 구현은 기존 platform 함수를 사용한다.

Immediate transaction 안에서 exact operation replay를 먼저 확인한다. 동일 operation_id와 account/idempotency_key, expected/result generation, account version, endpoint version/hash, snapshot hash, payload hash가 모두 일치할 때만 과거 결과 generation을 반환하고 현재 catalog를 쓰지 않는다. 동일 ID/key의 다른 요청은 CONFLICT다. 이미 더 최신 catalog가 있어도 replay는 이전 성공의 기록일 뿐이며 되돌리거나 현재 결속을 보증하지 않는다. 기존 operation 행 손상은 INTERNAL_ERROR다.

새 적용은 계정 provider=imap, 알려진 상태=ready, 정확한 account settings_version, 현재 IMAP endpoint 전체 필드/버전, 현재 catalog generation을 같은 transaction 안에서 대조한다. account 미존재는 NOT_FOUND, 다른 알려진 provider는 CAPABILITY_UNSUPPORTED, 정상 자료의 stale/version/state 불일치는 CONFLICT다. 손상된 현재 행은 INTERNAL_ERROR다. snapshot의 endpoint 숫자 버전만으로 적용하지 않는다. 기존 catalog가 다른 endpoint에 결속되어 있어도 검증된 generation을 CAS하여 새 endpoint의 목록으로 교체할 수 있다.

operation 표는 전역 100,000행 상한이며 exact replay를 제외한 초과는 QUOTA_EXCEEDED다. 자동 pruning은 하지 않는다. 같은 account의 stored_at 시각보다 UTC가 뒤로 가면 INTERNAL_ERROR다. operation 삽입과 catalog 교체는 같은 transaction으로 commit하며 중간 실패 시 둘 다 보존/rollback한다. 실패·조회·재개는 네트워크 효과를 실행하지 않는다.

`Store::load_imap_mailbox_snapshot(account_id)`는 `Option<StoredImapMailboxCatalog>`를 반환한다. getter는 `generation`, `account_version`, `endpoint_version`, `stored_at_utc_ms`, `snapshot`, `binding_current`다. catalog 미존재는 None, 손상은 INTERNAL_ERROR다. hash·고정 JSON·last_operation 참조·계정/endpoint version·시각·operation 결과의 연결을 다시 검증한다. `binding_current`는 현재 ready 계정의 version/정확한 endpoint hash와 저장 결속의 일치만 뜻하며 네트워크 최신성이나 승인/삭제 권한이 아니다. false여도 과거 목록과 generation을 보존해 새 refresh의 CAS 입력으로 쓸 수 있다. Debug는 식별자·이름·hash를 가린다.

모든 비Windows Store/hash API는 기존 CAPABILITY_UNSUPPORTED로 실패하며 평문 대체 경로를 만들지 않는다. 공개 CLI 오류·종료 코드는 추가하지 않는다.

## 검증 순서

Domain strict roundtrip·mixed-case INBOX·UTF8/UTF7·unknown/duplicate/순서/한도/손상 시험부터 작성한다. Store는 최초/교체/reopen, 정확한 replay와 최신 세대 보존, 다른 account/endpoint/버전/상태/세대 거부, hash/JSON/operation 연결 손상, quota, 시간 역행, INSERT/UPDATE/commit 실패 rollback을 시험한다. migration은 v7 기존 행과 DDL 보존, 백업 충돌 전 무변경, 실제 DDL 실패 rollback, v1→v8 순차 이전을 검사한다. 실제 사용자 DB·계정·메일·Hello는 사용하지 않는다.
