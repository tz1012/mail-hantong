# mail-hantong 개발 기준 요건

상태: **2026-10-29 사용자 승인으로 채택한 Markdown 기준 문서**.

사용자가 원문 170개 요건을 보존한 Markdown 분할본을 개발 기준으로 채택하는 질문에 “네”라고 답했다. 이 폴더가 앞으로 요건을 읽고 검토하는 기준 경로다. 문서 채택은 제품 기능·보안·호환성 시험 완료를 뜻하지 않는다.

- 요건 **170개**, 원문의 1단계 P0 **144개**, 문장별 수용기준 식별자 **341개**.
- [원문 요건 색인](../latest/requirements-index.csv), [AC 원문 대응표](traceability.csv), [Word 전체 텍스트 동반본](source-content.md).
- [승인·무결성 기록](adoption.json), [채택 전 검토 패키지](../proposals/M0-00/README.md), [당시 원문 검증 결과](../proposals/M0-00/validation.json).
- [원본 Word](<../latest/mail-hantong 요건서 v2.3 개정안.docx>)는 보관 자료이며 기준 문서에 자동으로 덮어쓰지 않는다.
- 작업 지시와 현재 상태는 계속 [최신 문서 모음](../latest/README.md)과 [단일 진행 기록](../latest/PROGRESS.md)에서 확인한다.

<a id="current-addenda"></a>
## 현재 적용 보완과 해석

1. 요건의 설명·수용기준 전체·조건·예외·원문 문맥을 함께 읽는다. AC 식별자는 추적을 위한 문장별 번호이며 독립 시험 341개를 모두 정의하거나 통과했다는 뜻이 아니다. 구체적인 시험 조합·채널·단계 적용성은 해당 구현 카드에서 검토한다.
2. traceability.csv의 `source_stage`는 원문의 단계 값이다. `review_status=review_required`와 검토 메모는 개별 AC의 문맥·적용성 검토 항목을 보존한 것이며 문서 채택 여부와 별개다. 문서 채택 상태는 adoption.json에 기록했다. 이후 제품 구현·시험 결과는 별도 증거로 연결한다.
3. 이미 승인된 [FEATURES.md](../latest/FEATURES.md)의 보완을 함께 적용한다. 특히 OPS-01의 웜 캐시 앱 시작 목표는 원문 §13.1의 3초에서 **1초 이내**로 강화되었다. 원문 동반본의 옛 수치는 변경 이력으로 보존한다. 기준은 4코어급 CPU·16GB·SSD·5계정·10만 건이며 GUI 목록을 실제 사용할 수 있어야 한다.
4. Windows 우선, Mac 코어·CLI·최소 승인 도우미 후 전체 GUI 제공 방향도 FEATURES.md를 따른다. 정확한 Mac 출시 단계·일정·지원 OS/CPU는 미정이며 이번 채택으로 모든 Mac 기능을 1단계에 넣지 않는다.
5. 코어·CLI는 [ADR-001의 Rust 결정](../adr/ADR-001-core-runtime.md), 라이선스는 [ADR-016](../adr/ADR-016-license.md)을 따른다. GUI·crate·IPC·DB·보안 계약의 미결 항목을 이번 채택으로 승인하지 않는다.
6. MAIL-15 상시 호스트, APR-02 휴대폰, INST-08 P1 조건 등은 원문에 적힌 조건부 단계와 함께 적용한다. 원문의 단계 문자열을 모든 AC의 확정 구현 단계로 복제하지 않는다. 나머지 검토 메모도 각 영역 문서와 대응표에 보존했다.

## 영역별 기준 문서

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

## 검증과 변경 관리

```powershell
python -X utf8 scripts/check_requirements.py
python -X utf8 scripts/prepare_requirements.py --check
python -X utf8 -m unittest discover -s tests/Docs -v
```

첫 명령은 채택된 문서의 누락·중복·내용/해시·원문 대응·링크를 읽기 전용으로 검사한다. 두 번째 명령은 보관된 제안 패키지를 검증하며 이 폴더를 재생성하지 않는다. 세 번째는 문서 도구의 시험이다. 어느 결과도 제품의 메일·보안 동작 검증을 대신하지 않는다.

변경은 AGENTS.md의 승인 절차를 따른다. 승인된 요건 변경 시 원문 대응·색인·채택 버전·무결성 기록과 관련 안내를 함께 갱신한다. 검사를 통과시키려고 승인 없이 adoption.json의 해시만 바꾸지 않는다. 해시는 손상 탐지 수단이며 사람 승인 증거를 대신하지 않는다.

승인 기록 시각은 adoption.json에 저장한 반영 시각이다. 대화 메시지의 정확한 송신 시각을 추정한 값이 아니다.
