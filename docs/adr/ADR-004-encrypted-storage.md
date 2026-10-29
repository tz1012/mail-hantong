# ADR-004 암호화 로컬 저장소

상태: 2026-10-29 연속 개발 위임에 따른 구현 결정. **사람 보안 검토 필요**. M1-04 구현 전 기준이며 이 문서만으로 제품 완료나 보안 승인을 뜻하지 않는다.

## 결정

Windows 1단계의 로컬 메일 저장소는 SQLCipher를 포함한 `rusqlite 0.41.1`의 `bundled-sqlcipher-vendored-openssl` 기능을 사용한다. `backup` 기능은 암호화 온라인 백업에만 사용한다. `trace` 기능은 비밀값이 SQL 추적에 남을 수 있으므로 켜지 않는다. 정확한 의존성·네이티브 빌드 결과는 `Cargo.lock`에 고정한다.

DB 키는 일반 Store나 설정 파일에서 만들지 않는다. Secrets 경계가 Windows DPAPI CurrentUser로 보호하는 별도 내부 DB 키를 생성하고, OS 난수 32바이트를 보관한다. 키 원문을 반환하는 getter나 외부 closure는 제공하지 않는다. Secrets 내부에서만 32바이트를 ASCII `x'<64자리 hex>'` 67바이트 버퍼로 변환해 SQLCipher의 `sqlite3_key_v2()`에 전달하고, 키와 변환 버퍼를 즉시 지운다. `rusqlite::Connection`과 키 설정·검증이 끝난 상태를 감싼 불투명 `KeyedConnection`만 Store에 넘긴다. Store는 키, DPAPI 핸들, 원시 SQLite 연결을 외부 호출자에게 반환하지 않는다.

현재 DB 파일은 고정된 사용자 앱 데이터 루트와 고정된 파일명만 사용한다. 경로는 요청 본문이나 메일 데이터에서 만들지 않는다. 루트와 모든 기존 조상 디렉터리는 Windows reparse point를 거부하고 열린 핸들을 보존한다. DB leaf와 임시·백업 파일도 같은 규칙을 따르며, SQLite를 여는 동안 leaf identity가 바뀌면 실패한다. 하나의 데이터 루트 lockfile을 `share_mode=0`으로 열어 KeyedConnection 수명 동안 유지하고, 동시 키 생성·마이그레이션을 막는다. 키 저장소는 데이터 DB와 별도 루트에 둔다. 같은 로그인 사용자 악성 프로세스와 관리자·커널 침해는 방어 범위가 아니다.

## 열기와 초기화

새 DB는 DPAPI 키를 먼저 원자적으로 보관한 뒤 새 파일을 배타적으로 생성하고, 키 설정 전에 다른 DB 연산을 수행하지 않는다. DB 파일도 DELETE 공유 없는 핸들로 고정하고 SQLite NOFOLLOW를 사용한다. `cipher_version`이 비어 있지 않은지 확인하고 `sqlite_master`를 읽어 키가 실제로 맞는지 검증한다. 그 다음 [db-schema.sql](../contracts/db-schema.sql)의 스키마를 트랜잭션으로 만들고 `PRAGMA user_version`과 `app_metadata.schema_version`을 일치시킨다. 초기화 중단으로 0바이트 DB가 남으면 다음 실행은 변경 없이 실패하며 자동 재생성하지 않는다. DB 생성 전 중단되어 키만 남은 경우 기존 키를 재사용한다.

기존 DB인데 키가 없거나 손상되면 새 키를 만들거나 DB를 지우고 재생성하지 않는다. 일반 SQLite/plaintext fallback, `cipher_migrate`의 자동 실행, 오류 원문 노출을 금지한다. 장기 primary 쓰기 연결은 키·암호화 무결성 확인 뒤 `foreign_keys=ON`, `temp_store=MEMORY`, `trusted_schema=OFF`, `journal_mode=DELETE`, `synchronous=EXTRA`를 명시하고 `1`, `2`, `0`, `delete`, `3`을 재조회해 일치해야 한다. 불일치·설정 실패·commit 오류면 내부 오류로 실패하며 감사 start 또는 후속 외부 효과를 시작하지 않는다. 읽기 전용 백업 검증 연결에는 쓰기 PRAGMA를 적용하지 않는다. 임시 파일·검색 색인·첨부 캐시는 같은 암호화·보존 정책을 따라야 한다.

`EXTRA`는 DELETE rollback journal에서 `FULL`보다 한 단계 높은 SQLite 설정이며, SQLite는 `FULL`만으로는 전원 장애 직전 마지막 commit의 내구성을 보장하지 않는다고 설명한다. 이 결정은 SQLite/VFS의 flush 및 저장장치가 그 요청을 올바르게 수행한다는 전제만 채택한다. Windows의 `FlushFileBuffers`와 저장장치 캐시는 물리 전원 차단 생존의 증거가 아니므로, 이 ADR은 설정 검증과 fail-closed 순서를 보장할 뿐 전원 장애 증명을 주장하지 않는다. 근거: [SQLite synchronous](https://www.sqlite.org/pragma.html#synchronous), [SQLite atomic commit](https://www.sqlite.org/atomiccommit.html), [Windows FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers).

## 마이그레이션과 백업

마이그레이션은 유지보수 잠금, 스키마 버전 확인, 같은 키로 만든 별도 암호화 온라인 백업, 백업 재개방·무결성 확인, 백업 파일 동기화와 원자 publish 순서로 진행한다. 백업이 검증되기 전에는 원본 트랜잭션을 시작하지 않는다. 이후 `BEGIN IMMEDIATE` 안에서 스키마와 `user_version`을 함께 갱신한다. 실패하면 원본을 롤백하고 검증된 백업과 기존 키를 보존한다. 더 높은 버전은 `VERSION_MISMATCH`로 변경 없이 거부한다. 키 회전은 이 카드 범위에서 지원하지 않으며 미지원 오류를 반환한다.

## 대안과 한계

시스템 SQLite·시스템 OpenSSL·검증되지 않은 사전 빌드 DLL은 ABI·암호화 설정·공급망을 고정하지 못하므로 선택하지 않는다. vendored OpenSSL 빌드는 Windows MSVC, Perl, NASM이 필요하며 도구가 준비되지 않으면 암호화 DB 기능을 평문으로 낮추지 않고 빌드를 막는다. SQLCipher와 OpenSSL의 제3자 고지·라이선스는 제품 배포 자료에서 별도로 반영한다.

공식 근거: [rusqlite 0.41.1 features](https://docs.rs/crate/rusqlite/0.41.1/source/Cargo.toml.orig), [SQLCipher API](https://www.zetetic.net/sqlcipher/sqlcipher-api/), [SQLite online backup](https://www.sqlite.org/backup.html), [SQLite atomic commit](https://www.sqlite.org/atomiccommit.html).

## 검토 경계

이 ADR과 [database.md](../contracts/database.md), SQL 스키마, Secrets·Store의 연결은 `SEC-01`, `SEC-02`, `M1-03`, `M1-04`에 영향을 준다. 키 수명·FFI·파일 경로·백업·복구는 사람 보안 검토 대상이다. 2026-10-30 Windows에서 SQLCipher 4.19.0 community와 vendored OpenSSL의 실제 빌드·실행, 암호화·키 거부·백업·rollback 시험을 통과했다. 도구 준비 명령과 기록은 [진행 문서](../latest/PROGRESS.md)에 남긴다. 실제 다른 Windows 사용자, 로그온 세션, 손상 디스크, 깨끗한 설치는 별도 증거가 필요하다.
