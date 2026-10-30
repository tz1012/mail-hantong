# 암호화 데이터베이스 계약 v1

상태: M1-04 구현 기준 및 v12 UID 공개 작업·명시 권한 갱신 확장, **사람 보안 검토 필요**. 코어의 암호화 저장소 연결은 구현됐으며 메일 API 활성화와는 별개다. `SEC-01`, `SEC-02`와 [ADR-004](../adr/ADR-004-encrypted-storage.md)를 함께 적용한다.

## 소유권과 공개 경계

Secrets가 DB 키를 생성·복호화·설정·지우고 `KeyedConnection`을 소유한다. Store 내부는 `KeyedConnection::with_connection`의 수명 제한 `&rusqlite::Connection`으로 고정 repository 쿼리를 실행한다. 이 내부 연결 접근은 클라이언트나 도구 API에 노출하지 않는다. mutable Connection, 소유권 이전, 빌린 statement 반환, DB 키 바이트, SQLCipher 키 문자열, DPAPI 평문, 외부 키 callback은 제공하지 않는다. Store가 외부에 제공하는 것은 검증된 타입과 안정적 오류뿐이다. 로그·Debug·오류에는 키·SQL·DB 경로·계정 값·메일 원문을 넣지 않는다.

지원 플랫폼은 Windows 1단계뿐이다. Linux/macOS는 평문 저장소로 대체하지 않고 `CAPABILITY_UNSUPPORTED` 또는 내부 미지원 오류를 반환한다. 실제 계정·메일·네트워크는 사용하지 않는다.

## 파일과 열기

DB 위치·파일명·백업 위치는 설치 구성에서 결정한 상수다. 사용자 입력으로 경로를 만들지 않는다. 루트·조상·leaf·임시 파일은 reparse/symlink·junction을 거부하고 조상 핸들을 열린 채 유지한다. DB leaf도 DELETE 공유 없는 핸들로 연결 수명 동안 고정하며 SQLite는 NOFOLLOW로 연다. 데이터 루트의 고정 lockfile을 `share_mode=0`으로 KeyedConnection 전체 수명 동안 유지해 동시 초기화와 migration을 직렬화한다. 키 루트는 DB 루트와 분리한다. lockfile은 크래시 후 남아도 열린 핸들이 닫히면 다음 실행에서 재사용할 수 있으며, 파일 존재만으로 잠금 상태를 판단하지 않는다.

열기 순서는 다음과 같다.

1. 고정 루트의 안전성을 확인하고, 새 DB인지 기존 DB인지 구분한다.
2. 새 DB면 Secrets 내부에서 OS 난수 32바이트 키를 만들고 DPAPI에 먼저 저장한다. 기존 DB에 키가 없으면 새로 만들지 않는다.
3. 첫 DB 연산으로 `sqlite3_key_v2()`를 호출한다. 원시 32바이트는 Secrets 안에서만 67바이트 `x'<64 hex>'` BLOB literal로 변환한다.
4. `cipher_version`이 비어 있지 않은지 확인한 뒤 `SELECT count(*) FROM sqlite_master`를 실행한다.
5. 암호화 무결성을 확인한 뒤, 장기 primary 쓰기 연결에서 `foreign_keys=ON`, `temp_store=MEMORY`, `trusted_schema=OFF`, `journal_mode=DELETE`, `synchronous=EXTRA`를 명시적으로 설정하고 각각 `1`, `2`, `0`, `delete`, `3`인지 다시 읽어 확인한다. 설정·재조회·이후 commit 중 어느 하나라도 실패하거나 불일치하면 안정적인 내부 오류로 열기 또는 쓰기를 실패시키며, Store 트랜잭션과 후속 효과를 시작하지 않는다. 읽기 전용 백업 검증 연결에는 쓰기 PRAGMA를 적용하지 않는다.
6. 이 검사를 통과한 불투명 연결을 Store가 받아 `PRAGMA user_version`과 `app_metadata.schema_version`을 비교한 뒤, 초기 스키마 트랜잭션 또는 지원하는 마이그레이션을 수행한다. 기존 0바이트 DB는 실패한 초기화로 취급해 자동 재생성하지 않는다.

잘못된 키, 손상, 파일 교체, SQLCipher 미지원은 외부에서 구별하지 않는 안정 오류로 반환한다. plaintext SQLite fallback, 자동 키 재생성, 원본 자동 삭제는 없다.

데이터 루트와 키 루트는 열린 핸들의 볼륨 번호·128비트 파일 ID를 조상 목록까지 대조해 같은 디렉터리나 서로의 하위 디렉터리가 아님을 확인한다. 키 루트도 별도 lockfile로 잠가 다른 데이터 루트의 동시 키 초기화를 거부한다. 기존 journal/WAL/SHM이나 그 존재 확인 오류가 있으면 현재 구현은 변경 없이 거부한다. 중단된 트랜잭션의 자동 복구는 검증된 복구 경로를 추가하기 전까지 활성화하지 않는다. 파일 식별 근거는 [Windows FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)다.

`DELETE + EXTRA`는 감사 start commit을 외부 효과보다 먼저 둘 때 요구하는 SQLite rollback-journal 설정이다. SQLite는 `FULL`이 rollback mode에서 마지막 commit의 전원 장애 내구성을 반드시 보장하지 않는다고 명시하며 `EXTRA`를 권장한다. 다만 이 설정은 OS/VFS와 저장장치가 flush 요청을 올바르게 이행한다는 가정 위에서만 동작한다. Windows의 `FlushFileBuffers`도 장치·하드웨어 캐시의 절대적 전원 장애 증명을 제공하지 않으므로, 이 계약은 설정·반환 성공·실패 시 효과 미시작을 검증할 뿐 실제 전원 차단 내구성을 주장하지 않는다. [SQLite synchronous](https://www.sqlite.org/pragma.html#synchronous), [SQLite atomic commit](https://www.sqlite.org/atomiccommit.html), [Windows FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)를 따른다.

## 초기 물리 스키마 범위

M1-04의 v1은 `app_metadata`, `settings`, `accounts` 세 테이블로 시작했다. v2는 [서버 신원·등록 저장 계약](registration-store.md)의 5개 테이블, v3는 [감사 기록 계약](audit.md)의 bounded append-only 감사 테이블을 추가했다. v4는 [메일 연결 설정 저장](mail-connection-store.md)의 endpoint 설정·관측·중복 방지 테이블, v5는 [설정 저널](imap-setup-journal.md)의 신규 요청·효과 이력 두 테이블을 추가했고, v6는 [명시 재개](imap-setup-resume.md)의 부모·자식 연결과 버전 결속을 추가했다. v7은 [클라이언트 범위 갱신 계약](client-scope-refresh.md)의 전용 `client_scope_refreshes` 이력 표를 추가했다. v8은 [IMAP 폴더 관측 저장](imap-folder-store.md)의 catalog·operation 두 표를 추가한다. v10은 [폴더 갱신 공개 도구](imap-folder-tools.md)의 durable admission ledger를 추가한다. 지원하는 이전은 v1→v2→v3→v4→v5→v6→v7→v8→v9→v10→v11→v12 순서의 migration ladder이며 각 단계의 검증된 백업 이력은 그대로 보존한다. 최초 등록 request/key/pin은 보존하고 범위 갱신만 v7 전용 이력으로 기록한다. Message, Thread, Folder, Tag, Draft, Outbox, Approval, ScheduledSend 등은 해당 기능 카드에서 별도 마이그레이션한다. `accounts`에는 토큰·비밀번호·키를 넣지 않고 Secrets 참조만 후속 계약으로 연결한다.

`app_metadata`는 singleton 1행, `database_format='mail-hantong'`, 양의 `schema_version`, UTC epoch milliseconds를 가진다. 초기화 코드는 암호화 연결 검증 후 bound parameter로 이 행을 삽입한다. `settings`는 key/value JSON과 `version>=1`을 저장한다. 설정 변경은 `UPDATE ... WHERE setting_key=? AND version=?`의 CAS로만 수행하며 영향 행이 0이면 `CONFLICT`이고 기존 값을 보존한다. `accounts`는 기준 모델의 `account_id`, `provider`, `provider_subject`, `primary_address`, `display_name`, `color`, `account_number`, `status`, `capabilities`, `granted_scopes`, `settings_version`을 저장한다. provider는 `gmail|graph|imap|archive`만 허용하고, archive 계정은 동기화·발송하지 않는다. 배열 필드는 표준 JSON 배열이어야 하며 `provider_subject`가 있는 경우 공급자·주체 조합은 계정 내에서 유일해야 한다.

## 백업과 마이그레이션

SQLite online backup API로 같은 키의 암호화 목적지를 만들고 재개방·`cipher_integrity_check`·스키마 버전을 검증한 뒤 동기화·원자 publish한다. OS 파일 복사로 live WAL/DB를 백업하지 않는다. 백업 실패는 원본을 변경하지 않고 종료한다. 백업이 검증된 뒤에만 `BEGIN IMMEDIATE`로 migration을 수행한다. migration 실패 시 원본·기존 키·검증된 백업을 모두 보존한다. v4 메일 설정 전용 operation 표는 `operation_id` PK와 `UNIQUE(account_id,idempotency_key)`를 가진다. 향후 다른 변경 원장에도 같은 범위 규칙을 적용한다. v4와 v5 DDL은 별도 고정 파일로 보존한다. 3→4와 4→5 경로는 각각 해당 버전의 DDL을 검증하며 5→6은 별도 v5 reader로 전체 기존 저널을 검증한 뒤 백업한다. 6→7은 v6 DDL을 고정 보존하고 전용 `client_scope_refreshes` 표를 추가·검증하는 migration으로 수행한다. v6의 원본 등록 request/key/pin과 기존 grant는 보존하며 범위 갱신 이력만 새 표에 기록한다. 7→8은 v7 DDL을 고정 보존하고 기존 행을 변경하지 않은 채 관측 표 두 개만 추가한다. 각 단계는 실제 전체 연결 검증을 한 트랜잭션으로 수행하며 실패하면 기존 행과 검증된 백업을 보존한다.

## 필수 시험 매트릭스

- 키 없는 DB 복사본에서 sentinel 본문을 읽을 수 없음; DB·journal·wal·shm·backup 원시 검색에도 sentinel 없음.
- `temp_store=MEMORY`로 임시 데이터가 파일에 남지 않음.
- 빈 `cipher_version`, plaintext SQLite, 잘못된 키와 손상 DB가 모두 같은 비공개 오류로 실패하고 재생성·fallback하지 않음.
- raw key 32바이트가 Secrets 내부에서만 정확히 67바이트로 변환되고 모든 버퍼가 지워짐.
- 기존 DB의 키 누락, 루트·조상·leaf reparse, 조상 교체가 변경 없이 거부됨.
- 신규 v8과 v1→v2→v3→v4→v5→v6→v7→v8 이전, metadata/user_version 일치, pragma 설정, settings CAS, account·감사·메일 설정·setup·범위 갱신 이력 제약이 통과함.
- 백업 실패 전 원본 무변경, migration 실패 rollback, 검증된 백업·키 보존, newer version 변경 없음.
- 오류·로그·Debug에 키·SQL·경로·설정값·계정값·메일 원문이 없음.

이 계약은 DB 연결·스키마 경계만 정의한다. 발송 승인, 계정 등록, 공급자 동기화, 검색 엔진과 인증 세션은 각 계약을 통과해야 한다.

## v10 폴더 갱신 작업 원장

`folder_refresh_requests`는 audit execution과 정확한 계정·endpoint·catalog 세대를 결속한다. admitted/running/succeeded/failed 상태를 DDL과 strict reader가 함께 확인하며, operation과 계정별 idempotency key는 중복 접수를 막는다. 시작되지 않은 작업은 시작 시 자동 재개하지 않는다. v9 audit 행·child 행·capacity·AUTOINCREMENT high-water는 `009-to-010.sql`로 보존한다. migration 실패와 백업 충돌은 원본을 그대로 둔다.

## v9 계정·도구 범위 이력

v8 DDL은 `migrations/schema-v8.sql`로 보존한다. 8→9는 [범위 갱신 v2](client-scope-refresh.md)에 따라 기존 ledger를 재구축하며 모든 v1 행의 원래 값/hash를 보존하고 명시 legacy 도구와 hash_version1을 추가한다. v7/v8은 전용 legacy reader로 검증한 후 이전한다. 새 fresh/schema9 reader는 v1·v2 hash/history를 구분하고 전체 계정/도구 범위를 검증한다. 실패/백업 충돌은 기존 자료를 보존한다. 현재 변경의 검증은 진행 중이다.


## v11 UID 페이지 저장

v10 DDL은 `migrations/schema-v10.sql`로 고정하며 `010-to-011.sql`은 [UID 페이지 계약](imap-uid-page.md)의 operation/cursor/message 세 표와 operation별 메시지 조회 index를 추가한다. 기존 감사·folder job을 변경하거나 재개하지 않는다. 메타데이터와 UID coverage cursor 및 과거 receipt는 한 Immediate transaction에 저장한다. 데이터는 관측 자료이며 메시지 변경/발송 권한이 아니다.

시작 시 DDL·FK·표별 개수 한도를 검사한다. 각 operation/cursor/message의 canonical JSON·hash·키/원본 snapshot 관계는 조회·재실행·신규 적용 때 검증한다. 최대100만 메시지 JSON을 시작 때 일괄 decode하지 않으며 미조회 행까지 검증했다고 주장하지 않는다. 이 slice는 본문·기존 메일 상태 대조·UIDVALIDITY reset이나 공개 sync 도구를 추가하지 않는다.


## v12 UID 작업과 권한 이력 보존

[공개 UID 계약](imap-uid-tools.md), [영속 작업](imap-uid-jobs.md), [명시 scope 갱신](client-scope-refresh.md)을 적용한다. v11 DDL은 `migrations/schema-v11.sql`로 고정한다. v11→v12는 기존 audit parent/children과 folder job FK를 함께 재구성해 새 audit tool을 허용하고, scope 이력의 CHECK만 확장하며 빈 UID 작업 원장을 추가한다. 기존 scope v1/v2의 값·hash·phase는 그대로 보존한다. audit sequence high-water와 한 번도 사용하지 않은 sequence의 부재도 유지한다. 기존 UID page/cursor/message와 미완료 folder job을 삭제·재실행하지 않는다.

보호된 backup과 전체 DDL/FK 검증, 원자 commit 규칙은 동일하다. 과거 v9/v10/v11은 frozen v1/v2 scope validator, v12는 v1/v2/v3 validator를 사용한다. schema migration을 권한 확인으로 간주하지 않으며 새 도구 권한은 별도의 native 확인을 통과해야 한다. 실패 시 원본 v11 행과 DDL로 rollback하고 기존 backup 충돌은 덮어쓰지 않는다.

## v13 기존 UID 관측 overlay

[기존 UID 대조 계약](imap-uid-reconcile.md)에 따라 head·request·message observation 표 세 개와 조회 index만 추가한다. v12 DDL은 `migrations/schema-v12.sql`로 보존하고 `012-to-013.sql`을 기존 ladder 다음에 실행한다. 원본 UID page/message/cursor, 미완료 작업, 감사 sequence와 scope v1/v2/v3 이력은 변경하지 않는다. 새 scope나 audit tool은 추가하지 않는다. 보호된 v12 backup→원자 DDL/metadata/version→전체 schema/FK 검증 순서를 유지하며 실패 시 원본과 검증된 backup을 보존한다.

## v15 로컬 메타데이터 ID와 명시 권한

[검색 계약](imap-metadata-search.md)과 [scope v4 계약](client-metadata-scope-refresh.md)에 따라 frozen `schema-v14.sql`과 `014-to-015.sql`을 사용한다. 열 개 audit FK closure 표와 scope ledger를 함께 재구축하고 새로운 `imap_message_identities` 표를 추가한다. 보호된 암호화 backup 뒤 단일 Immediate transaction에서 모든 기존 행/JSON/hash·원본 scope v1~v3·미완료 작업·audit capacity/sequence를 보존하며 자동 재개하거나 권한을 확대하지 않는다. 미사용 audit sequence의 행 부재도 보존한다. 같은 transaction에서 모든 불변 IMAP locator의 ID를 streaming backfill하고 삽입값을 검증한다. 잘못된 locator/충돌/누락/변조/DDL 실패는 원본 v14로 rollback한다.

schema12~14는 frozen max3 scope validator, schema15는 max4 validator를 사용한다. mapping은 새 append/recovery와 함께 원자 삽입하며 조회·재실행·startup에서 자동 생성하지 않는다. startup은 mapping/base bijection·FK·count를 검증하고 개별 hash는 선택된 source read에서 검증한다. 전체 메시지 JSON을 startup에 일괄 decode했다고 주장하지 않는다. 실행 증거와 남은 검토는 [PROGRESS](../latest/PROGRESS.md)에 기록한다.
