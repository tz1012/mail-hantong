# 개발용 IMAP/SMTP 테스트 서버

이 폴더는 제품 실행과 분리된 통합 시험 환경이다. [GreenMail 공식 standalone 이미지](https://github.com/greenmail-mail-test/greenmail/tree/master/greenmail-docker/standalone)의 2.1.15 릴리스를 사용한다. 공식 문서는 standalone 이미지의 테스트 포트가 기본 포트에 3000을 더한 값이라고 설명한다. 이 구성은 IMAP `3143`과 SMTP `3025`를 호스트의 loopback 포트 `13143`과 `13025`에만 매핑한다.

계정 `tester:fixture-password@example.invalid`는 합성 시험 자격증명이다. 실제 계정, 실제 수신자, 외부 SMTP 전달, OAuth, 비밀 저장소를 사용하지 않는다. Compose 파일에 호스트 볼륨·Docker socket·환경 파일을 연결하지 않는다.

```powershell
./scripts/test-mail-server.ps1 up
./scripts/test-mail-server.ps1 status
./scripts/test-mail-server.ps1 down
```

POSIX 셸에서는 `./scripts/test-mail-server up` 등을 사용한다. 명령은 현재 작업 폴더와 무관하게 이 Compose 파일을 정확히 지정한다. `up`은 컨테이너의 healthcheck가 준비될 때까지 기다리며, `status`는 readiness를 확인한다. Docker가 없거나 실행되지 않으면 명령은 오류를 반환한다.

제품 실행에는 Docker·WSL이 필요하지 않다. 이 환경은 개발자의 IMAP·SMTP 프로토콜 통합 테스트에서만 사용한다. 이 저장소에서는 Docker가 설치되지 않은 환경에서 서버를 실행하지 않았으며, 구성 정적 검사만 수행한다.
