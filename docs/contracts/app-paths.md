# 앱 데이터 경로 계약 v1

`hantong-platform::app_directories()`는 현재 Windows 사용자의 `FOLDERID_LocalAppData`를 운영체제 Known Folder API로 조회하고, 그 아래 고정된 `mail-hantong/data`와 `mail-hantong/secrets` 두 경로를 계산한다. `LOCALAPPDATA` 환경 변수, CLI 인자, 메일 데이터로 경로를 덮어쓸 수 없다.

이 함수는 경로를 발견만 하며 디렉터리·파일을 만들거나 읽지 않는다. 반환 경로의 reparse point·junction·조상 교체 검사는 `hantong-secrets`와 Store의 파일 핸들이 담당한다. 데이터와 비밀 루트는 항상 형제 디렉터리다. Known Folder API 실패, UNC·device·상대·parent 경로는 `CAPABILITY_UNSUPPORTED`로 거부한다. 비 Windows 제품 빌드는 평문 경로로 대체하지 않고 같은 오류를 반환한다.

`AppDirectories`의 `Debug` 출력은 실제 경로를 포함하지 않는다. 제품 설치·초기화 단계가 실제 디렉터리를 만들 때 이 계약의 반환값만 사용하고, 외부 요청에서 임의 루트를 받지 않는다.

공식 근거: [SHGetKnownFolderPath](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath). 반환 메모리는 성공 여부와 관계없이 `CoTaskMemFree`로 해제한다.

코어 `run`은 이 경로의 암호화 DB·스키마·저장 설정을 검증한 뒤 IPC를 열고, 종료까지 Store와 파일 잠금을 유지한다. 초기 설정의 시간대는 `UTC`이며 설정 마법사에서 사용자가 선택한 IANA 시간대로 변경한다. 기존 설정은 시작 시 덮어쓰지 않는다. 초기화 오류가 있으면 준비 이벤트나 IPC를 열지 않는다. `--help`와 `--version`은 경로 발견·저장소 초기화를 실행하지 않는다.

서버 신원은 DB v2에서 추가한 공개 metadata(현재 v3에서도 유지)와 현재 사용자 CNG의 서버 전용 키로 연결한다. 키 생성 전 creating 행을 기록하며 active 신원의 키 누락·공개 키 불일치 때 자동 교체하지 않는다. CNG 키 handle도 코어 종료까지 유지한다. 이 초기화가 클라이언트를 자동 등록하거나 승인하지 않는다.
