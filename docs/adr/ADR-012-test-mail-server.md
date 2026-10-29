# ADR-012 개발용 테스트 메일 서버

상태: 2026-10-29 연속 개발 위임에 따른 구현 결정. 제품 실행에는 Docker·WSL을 요구하지 않는다.

M0-07은 GreenMail standalone Docker 이미지 `docker.io/greenmail/standalone:2.1.15`를 사용한다. GreenMail 공식 저장소는 standalone 이미지가 Docker Hub에서 제공되며 기본 테스트 포트가 표준 메일 포트에 3000을 더한 값이라고 설명한다. [공식 standalone README](https://github.com/greenmail-mail-test/greenmail/tree/master/greenmail-docker/standalone), [공식 릴리스](https://github.com/greenmail-mail-test/greenmail/releases/tag/release-2.1.15).

대안으로 Dovecot·자체 SMTP 컨테이너를 구성할 수 있지만 인증·IMAP 동작을 별도로 조립해야 한다. GreenMail은 SMTP와 IMAP을 함께 제공하고, 테스트 설정 포트와 합성 사용자를 지정할 수 있어 M0 통합 시험의 범위를 작게 유지한다. 실제 서비스 호환성 시험을 대신하지 않는다.

Compose는 `restart: "no"`를 사용하고, 호스트 포트를 `127.0.0.1`에만 바인딩한다. 데이터 볼륨, 호스트 경로, Docker socket, 외부 네트워크, 환경 파일, 실제 주소를 사용하지 않는다. `tester:fixture-password@example.invalid`는 fixture 안에서만 쓰는 값이며 로그·제품 설정·배포 비밀이 아니다. SMTP relay나 인터넷 전송은 활성화하지 않는다.

서비스 healthcheck는 이미지에 존재하는 공식 테스트 포트의 TCP readiness를 확인하고 `docker compose up --wait`가 실패하면 시험을 시작하지 않는다. healthcheck 도구가 해당 이미지 변형에서 제공되지 않으면 통합 테스트 사전 검사에서 명시적으로 실패시켜 준비되지 않은 서버를 정상으로 처리하지 않는다.

실행 래퍼는 저장소 어디서든 Compose 파일의 절대 경로를 계산해 호출한다. `down`은 이 파일이 정의한 프로젝트만 대상으로 하며 임의 컨테이너나 호스트 파일을 삭제하지 않는다. Docker가 설치되지 않은 환경에서는 구성 정적 검사만 수행한다. M0-07 완료는 이 구성·래퍼·정적 시험의 완료이며 실제 컨테이너 기동 결과는 실행한 환경에서 별도 증거로 기록한다.
