# M0-00 요건 분할 제안 (채택 전)

이 디렉터리는 v2.3 통합 요건서를 저장소 기준 문서로 전환하기 위한 검토 산출물이다.
사람이 검토·채택하기 전에는 `docs/requirements/`를 생성하거나 기준 문서로 간주하지 않는다.

원문: [mail-hantong 요건서 v2.3 개정안](../../latest/mail-hantong%20요건서%20v2.3%20개정안.docx)
실행 환경: Python 3.10 이상 (표준 라이브러리만 사용)
재생성: `python scripts/prepare_requirements.py`  ·  검사: `python scripts/prepare_requirements.py --check`

프로젝트 루트에서 실행한다. 생성 파일을 직접 편집하면 --check가 실패한다. 재생성은 기존 생성 매니페스트를 확인하며 다른 문서가 있는 폴더를 덮어쓰지 않는다.

- 원본 색인: `170개 요건`
- 분할된 수용 기준: `341개`

## 검증

- [검증 결과](validation.json)
- [원문 대응표](traceability.csv)
- [Word 전체 텍스트 동반본](source-content.md)

## 도메인 파일

- [ACC](acc.md)
- [AI](ai.md)
- [APR](apr.md)
- [BAK](bak.md)
- [CLI](cli.md)
- [EXT](ext.md)
- [HOST](host.md)
- [INST](inst.md)
- [KEY](key.md)
- [MAIL](mail.md)
- [MCP](mcp.md)
- [OPS](ops.md)
- [PUSH](push.md)
- [REM](rem.md)
- [SEC](sec.md)
- [SETUP](setup.md)
- [SYNC](sync.md)

검토용 수용기준 후보: AC 식별자는 원문 문장 경계에 부여했다. 설명·원문 전체·단계·조건·예외를 함께 해석한다. 복합 항목과 단계별 적용 범위는 검토가 남아 있다. AC 개수는 독립 시험 수 또는 구현 완료 수가 아니다.

원문 설명과 수용 기준 전체를 함께 보존한다. traceability.csv의 source_stage는 원문의 요건 단계이며 AC별 적용 확정이 아니다. 모든 review_status는 review_required다.
검증은 문서 보존을 확인한다. 제품·보안·호환성 시험 통과를 뜻하지 않는다.

## 특히 검토할 조건과 적용 범위

- [INST-08](inst.md): P1의 제공 조건과 미충족 시 비활성 동작을 함께 검토한다.
- [MAIL-15](mail.md): 상시 호스트 정시 실행은 조건부다. 원문 stage=1을 모든 AC에 일괄 적용하지 않는다.
- [MAIL-17](mail.md): 보존 이동 → 비어 있음 확인 → 삭제 순서와 경합 시 거부를 함께 검토한다.
- [SYNC-07](sync.md): 불명 상태 확인·사람 재시도 승인과 자동 재발송 금지를 함께 검토한다.
- [APR-02](apr.md): 휴대폰 서명 조건은 2단계 휴대폰 경로다. 각 거부 사례의 채널 적용성을 검토한다.
- [CLI-10](cli.md): 중복 알림 억제와 미확인 알림 재노출을 클라이언트별 커서·ack 문맥에서 검토한다.
- [BAK-04](bak.md): 보관함과 서버 업로드, 앱 알림과 외부 부작용을 구분한다. 비활성화는 P0 면제가 아니다.
- [OPS-02](ops.md): 언어·테마·배율·대비 대상의 시험 조합을 검토한다. 문장 수는 시험 수가 아니다.
