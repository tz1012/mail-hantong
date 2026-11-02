# ADR-026: 패키지 실행과 무관한 Windows 사용자 데이터 루트

- 상태: 제안
- 날짜: 2026-11-02
- 관련 요건: INST-01, INST-03, SEC-02

## 결정

Windows의 `LocalAppData`는 `SHGetKnownFolderPath`로 조회할 때
`KF_FLAG_NO_PACKAGE_REDIRECTION`을 항상 전달한다. 제품은 이 결과 아래에
`mail-hantong\data`, `mail-hantong\secrets`, `mail-hantong\profiles`를 둔다.
환경 변수, 실행 파일 위치, 패키지 이름으로 이 루트를 재정의하지 않는다.

## 근거

Windows 패키지 호스트는 Known Folder 경로를 패키지 전용 위치로 리디렉션할 수
있다. Microsoft 문서는 `KF_FLAG_NO_PACKAGE_REDIRECTION`을 사용하면 Known
Folder API가 패키지 리디렉션을 적용하지 않는다고 정의한다.

- [KNOWN_FOLDER_FLAG](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ne-shlobj_core-known_folder_flag)
- [SHGetKnownFolderPath](https://learn.microsoft.com/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath)

이 플래그를 사용하지 않으면 같은 Windows 사용자라도 패키지 실행과 일반 실행이
서로 다른 저장소를 선택할 수 있다. 이 결정은 두 실행 경로가 같은 계정·비밀·프로필
저장을 사용하도록 경로 선택만 고정한다.

## 범위와 안전 경계

이 변경은 OS 경로 조회의 플래그만 바꾼다. 디렉터리를 만들거나 열지 않고, 기존
데이터를 이동·병합·삭제하지 않으며 ACL도 변경하지 않는다. 설치·업데이트 시 기존
패키지 전용 저장소를 자동으로 마이그레이션하지 않는다. 마이그레이션이 필요하면
별도의 사람 검토와 명시적 복구 절차가 필요하다.

테스트는 실제 사용자 저장소에 쓰지 않으며, 주입 가능한 Known Folder resolver가
패키지 경로와 안정 경로를 구별하고 실제 호출 플래그가 정확한지 확인한다.

## 사람 검토 필요

이 결정은 사용자 데이터 루트와 비밀 저장소의 경로에 영향을 주므로 적용 전
사람 보안 검토가 필요하다. [사람 검토 필요]
