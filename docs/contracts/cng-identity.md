# Windows CNG 등록 키 primitive v1

`hantong-platform`의 Windows 전용 `ClientSigningKey`는 현재 사용자 Microsoft Software Key Storage Provider에 P-256 ECDSA 키를 생성하거나 명시적으로 재개방한다. 이름은 `mail-hantong-client-` 접두사와 CNG 난수 32바이트의 소문자 hex로 구성하며, 외부 입력은 이 형식만 허용한다. 생성 시 `NCRYPT_ALLOW_SIGNING_FLAG`와 export policy 0을 설정하고 `NCRYPT_SILENT_FLAG`로 finalize/open한다. 개인 키 바이트는 API와 로그에 노출하지 않는다.

공개 키 wire 값은 정확히 72바이트 `BCRYPT_ECCPUBLIC_BLOB`(P-256 public magic, key size 32, X/Y 각 32바이트)다. 인증 transcript는 최대 32 KiB에서 CNG SHA-256으로 해시한다. CNG ECDSA 서명은 정확히 64바이트이며 DER로 해석하지 않는다. malformed blob·길이·이름과 서명 불일치는 거부한다. 이 primitive는 등록·승인·세션 권한을 생성하지 않는다. IPC 인증 활성화 전에는 별도 transcript/철회 계약이 필요하다.

키를 생성하거나 재개방할 때 알고리즘·signing-only usage·export policy 0을 다시 확인한다. `NCryptOpenKey`의 문서화된 `NTE_BAD_KEYSET`만 키 없음(`NOT_FOUND`)으로 분류하고, 권한·provider·손상·기타 오류는 `INTERNAL_ERROR`로 보존해 생성 fallback을 실행하지 않는다. public blob은 정렬된 메모리라고 가정하지 않고 bytes로 header를 읽으며, CNG import의 좌표 검증을 통과해야 한다. 소비형 `delete`는 명시적으로 열린 해당 키만 제거하며 정상 인증의 Drop은 키를 지우지 않는다. 테스트는 자신이 만든 키만 정리한다. provider는 key handle 뒤에 닫고, 실패한 delete 뒤에도 handle은 해제한다.

시험용 private export는 실제로 실패하고 출력 buffer도 비어 있어야 한다. Microsoft Software KSP의 오류 값은 정책과 Windows 구현에 따라 다를 수 있으므로 NTE_PERM 하나만을 계약으로 정하지 않는다. 공식 근거: [NCryptExportKey](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptexportkey), [Export Policy 속성](https://learn.microsoft.com/en-us/windows/win32/seccng/key-storage-property-identifiers).

코어 서버 신원은 `ServerSigningKey`의 `mail-hantong-server-` + 32바이트 식별자 hex 네임스페이스를 사용한다. 클라이언트 키 API는 이 이름을 열 수 없고, 서버 API는 클라이언트 네임스페이스를 열 수 없다. 서버 식별자는 먼저 암호화 DB의 `creating` 행에 기록한 뒤 `create(id)`를 호출하고 공개키를 검증해 `active`로 전환해야 한다. 기존 이름은 `CONFLICT`, 없는 이름의 `open(id)`는 `NOT_FOUND`이며 자동으로 다른 서버 키를 만들지 않는다.

공식 근거: [NCryptOpenKey](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptopenkey), [NCryptCreatePersistedKey](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptcreatepersistedkey), [NCryptSignHash](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptsignhash), [BCryptVerifySignature](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptverifysignature), [BCRYPT_ECCKEY_BLOB](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/ns-bcrypt-bcrypt_ecckey_blob).
