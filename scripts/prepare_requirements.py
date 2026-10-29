#!/usr/bin/env python3
"""Prepare the review-only M0-00 requirements package from the latest packet.

The generated files are deliberately kept under docs/proposals/M0-00.  This
tool never writes docs/requirements, because that directory becomes canonical
only after human adoption of the proposal.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import html
import json
import re
import sys
import tempfile
import zipfile
from collections import Counter, defaultdict
from pathlib import Path
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
NS = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}
ID_RE = re.compile(r"^[A-Z]+-\d{2}$")
REVIEW_NOTES = {
    "INST-08": "P1의 제공 조건과 미충족 시 비활성 동작을 함께 검토한다.",
    "MAIL-15": "상시 호스트 정시 실행은 조건부다. 원문 stage=1을 모든 AC에 일괄 적용하지 않는다.",
    "MAIL-17": "보존 이동 → 비어 있음 확인 → 삭제 순서와 경합 시 거부를 함께 검토한다.",
    "SYNC-07": "불명 상태 확인·사람 재시도 승인과 자동 재발송 금지를 함께 검토한다.",
    "APR-02": "휴대폰 서명 조건은 2단계 휴대폰 경로다. 각 거부 사례의 채널 적용성을 검토한다.",
    "CLI-10": "중복 알림 억제와 미확인 알림 재노출을 클라이언트별 커서·ack 문맥에서 검토한다.",
    "BAK-04": "보관함과 서버 업로드, 앱 알림과 외부 부작용을 구분한다. 비활성화는 P0 면제가 아니다.",
    "OPS-02": "언어·테마·배율·대비 대상의 시험 조합을 검토한다. 문장 수는 시험 수가 아니다.",
}
REVIEW_CONTEXT = (
    "검토용 수용기준 후보: AC 식별자는 원문 문장 경계에 부여했다. "
    "설명·원문 전체·단계·조건·예외를 함께 해석한다. 복합 항목과 단계별 적용 범위는 "
    "검토가 남아 있다. AC 개수는 독립 시험 수 또는 구현 완료 수가 아니다."
)


def read_rows(index: Path) -> list[dict[str, str]]:
    with index.open(encoding="utf-8-sig", newline="") as handle:
        rows = list(csv.DictReader(handle))
    required = {"requirement_id", "title", "description", "acceptance", "priority", "stage", "stage1_card_ids"}
    if not rows or not required <= set(rows[0]):
        raise ValueError("requirements-index.csv is missing required columns")
    return rows


def split_acceptance(text: str) -> list[str]:
    """Split only at explicit sentence/list boundaries; preserve source text."""
    text = " ".join(text.split())
    if not text:
        return []
    # Whitespace after punctuation distinguishes sentences from 2.3 / 4.5:1.
    # Commas and conjunctions stay intact to retain conditions and alternatives.
    parts = re.split(r"(?<=[.!?。？！])\s+(?=[A-Za-z가-힣0-9•\-])", text)
    parts = [p.strip() for p in parts if p.strip()]
    return parts or [text]


def normalize(text: str) -> str:
    return " ".join(text.replace("\u00a0", " ").split())


def xml_text(node: ET.Element, preserve_breaks: bool = False) -> str:
    chunks: list[str] = []
    def visit(item: ET.Element) -> None:
        tag = item.tag.rsplit("}", 1)[-1]
        if tag == "t":
            chunks.append(item.text or "")
        elif preserve_breaks and tag in {"br", "cr"}:
            chunks.append("\n")
        elif preserve_breaks and tag == "tab":
            chunks.append("\t")
        for child in item:
            visit(child)
        if preserve_breaks and tag == "p":
            chunks.append("\n")
    visit(node)
    return "".join(chunks)


def docx_rows(path: Path) -> tuple[list[dict[str, str]], list[tuple[str, str, list[str]]], int]:
    """Extract five-column requirement rows, all visible blocks, and image count."""
    with zipfile.ZipFile(path) as archive:
        root = ET.fromstring(archive.read("word/document.xml"))
        rows_out: list[dict[str, str]] = []
        blocks: list[tuple[str, str, list[str]]] = []
        rels: dict[str, str] = {}
        try:
            rel_root = ET.fromstring(archive.read("word/_rels/document.xml.rels"))
            for rel in rel_root:
                rid = rel.attrib.get("Id")
                target = rel.attrib.get("Target", "")
                if rid and target.startswith("media/"):
                    rels[rid] = Path(target).name
        except KeyError:
            pass

        def images_in(node: ET.Element) -> list[str]:
            return [rels[rid] for blip in node.iter() if (rid := blip.attrib.get("{http://schemas.openxmlformats.org/officeDocument/2006/relationships}embed")) in rels]
        body = root.find("w:body", NS)
        if body is None:
            return rows_out, blocks, 0
        table_number = 0
        for child_index, child in enumerate(list(body)):
            tag = child.tag.rsplit("}", 1)[-1]
            if tag == "p":
                text = xml_text(child, preserve_breaks=True).strip()
                embedded = images_in(child)
                if text or embedded:
                    blocks.append((f"P{child_index + 1}", text or "[이미지]", embedded))
            elif tag == "tbl":
                table_number += 1
                table_rows = child.findall("w:tr", NS)
                five_columns = bool(table_rows and len(table_rows[0].findall("w:tc", NS)) == 5)
                for row_number, row in enumerate(child.findall("w:tr", NS), 1):
                    cells: list[str] = []
                    for cell in row.findall("w:tc", NS):
                        cells.append(xml_text(cell, preserve_breaks=True).strip())
                    anchor = f"T{table_number}-R{row_number}"
                    blocks.append((anchor, "[표] " + " | ".join(cells), images_in(row)))
                    if five_columns and cells and cells[0].strip():
                        match = re.match(r"^([A-Z]+-\d{2})(?:\s+|$)(.*)$", cells[0], re.S)
                        if match and len(cells) == 5:
                            rows_out.append({"requirement_id": match.group(1), "title": match.group(2), "description": cells[1], "acceptance": cells[2], "priority": cells[3], "stage": cells[4], "table_number": str(table_number), "row_number": str(row_number)})
        images = sum(1 for name in archive.namelist() if name.startswith("word/media/"))
        return rows_out, blocks, images


def validate(rows: list[dict[str, str]]) -> dict:
    ids = [row["requirement_id"] for row in rows]
    duplicates = sorted(k for k, v in Counter(ids).items() if v > 1)
    malformed = sorted(k for k in ids if not ID_RE.fullmatch(k))
    empty = sorted(row["requirement_id"] for row in rows if not normalize(row["acceptance"]))
    ac_count = sum(len(split_acceptance(row["acceptance"])) for row in rows)
    return {
        "requirement_count": len(rows),
        "unique_requirement_count": len(set(ids)),
        "duplicate_requirement_ids": duplicates,
        "malformed_requirement_ids": malformed,
        "empty_acceptance_ids": empty,
        "acceptance_criterion_count": ac_count,
        "priorities": dict(sorted(Counter(row["priority"] for row in rows).items())),
        "stages": dict(sorted(Counter(row["stage"] for row in rows).items())),
        "valid": len(rows) == 170 and not duplicates and not malformed and not empty,
    }


def compare_source(rows: list[dict[str, str]], source_rows: list[dict[str, str]]) -> tuple[dict[str, dict[str, str]], list[dict[str, str]]]:
    """Compare CSV values with the five-column Word requirement tables."""
    csv_by_id = {row["requirement_id"]: row for row in rows}
    doc_by_id = {row["requirement_id"]: row for row in source_rows}
    mismatches: list[dict[str, str]] = []
    for side, values in (("csv", rows), ("docx", source_rows)):
        for rid, count in Counter(r["requirement_id"] for r in values).items():
            if count > 1:
                mismatches.append({"requirement_id": rid, "field": "duplicate_id", "source": side})
    for rid in sorted(set(csv_by_id) | set(doc_by_id)):
        if rid not in csv_by_id or rid not in doc_by_id:
            mismatches.append({"requirement_id": rid, "field": "id", "csv": rid if rid in csv_by_id else "<missing>", "docx": rid if rid in doc_by_id else "<missing>"})
            continue
        csv_row, doc_row = csv_by_id[rid], doc_by_id[rid]
        values = {"title": (csv_row["title"].removeprefix(rid).strip(), doc_row["title"]), "description": (csv_row["description"], doc_row["description"]), "acceptance": (csv_row["acceptance"], doc_row["acceptance"]), "priority": (csv_row["priority"], doc_row["priority"]), "stage": (csv_row["stage"], doc_row["stage"])}
        for field, (csv_value, doc_value) in values.items():
            if normalize(csv_value) != normalize(doc_value):
                mismatches.append({"requirement_id": rid, "field": field, "csv": csv_value, "docx": doc_value})
    locations = {rid: {"table_number": row["table_number"], "row_number": row["row_number"], "anchor": f"T{row['table_number']}-R{row['row_number']}-{rid}"} for rid, row in doc_by_id.items()}
    return locations, mismatches


def protect_output(source_doc: Path, out: Path) -> None:
    resolved = out.resolve()
    if source_doc.resolve().is_relative_to(resolved):
        raise ValueError("output must not contain the source document")
    parts = [p.lower() for p in resolved.parts]
    if any(parts[i:i + 2] == ["docs", "requirements"] for i in range(len(parts) - 1)):
        raise ValueError("canonical docs/requirements is not an allowed proposal output")
    if out.exists() and not out.is_dir():
        raise ValueError("output must be a directory")
    if out.exists() and any(p.is_symlink() for p in out.rglob("*")):
        raise ValueError("output must not contain symbolic links")


def write_package(rows: list[dict[str, str]], source_doc: Path, out: Path) -> None:
    protect_output(source_doc, out)
    if not validate(rows)["valid"]:
        raise ValueError("invalid CSV requirement set")
    source_rows, source_blocks, image_count = docx_rows(source_doc)
    locations, mismatches = compare_source(rows, source_rows)
    if not validate(source_rows)["valid"]:
        raise ValueError("invalid Word requirement set")
    if mismatches:
        raise ValueError("CSV/DOCX requirement mismatch: " + json.dumps(mismatches[:5], ensure_ascii=False))
    domains: dict[str, list[dict[str, str]]] = defaultdict(list)
    for row in rows:
        domains[row["requirement_id"].split("-", 1)[0]].append(row)
    with zipfile.ZipFile(source_doc) as archive:
        image_names = [n for n in archive.namelist() if n.startswith("word/media/") and not n.endswith("/")]
        source_links = [(r.attrib["Id"], r.attrib["Target"]) for r in
                        ET.fromstring(archive.read("word/_rels/document.xml.rels"))
                        if r.attrib.get("Type", "").endswith("/hyperlink")]
    expected_files = {"README.md", "traceability.csv", "source-content.md", "validation.json"}
    expected_files.update(f"{domain.lower()}.md" for domain in domains)
    expected_files.update("assets/" + Path(name).name for name in image_names)
    existing_files = {p.relative_to(out).as_posix() for p in out.rglob("*") if p.is_file()}
    if existing_files:
        manifest_path = out / "validation.json"
        if not manifest_path.is_file():
            raise ValueError("nonempty output lacks a generator ownership manifest")
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if (not isinstance(manifest, dict)
                or manifest.get("generator") != "mail-hantong/M0-00"
                or not isinstance(manifest.get("generated_files"), list)
                or not all(isinstance(name, str) for name in manifest["generated_files"])):
            raise ValueError("output is not an existing M0-00 generated package")
        if existing_files - set(manifest["generated_files"]):
            raise ValueError("output contains files outside the generator ownership manifest")
    if existing_files - expected_files:
        raise ValueError("output contains files not owned by this generator: " + ", ".join(sorted(existing_files - expected_files)))
    out.mkdir(parents=True, exist_ok=True)
    index_lines = [
        "# M0-00 요건 분할 제안 (채택 전)", "",
        "이 디렉터리는 v2.3 통합 요건서를 저장소 기준 문서로 전환하기 위한 검토 산출물이다.",
        "사람이 검토·채택하기 전에는 `docs/requirements/`를 생성하거나 기준 문서로 간주하지 않는다.", "",
        "원문: [mail-hantong 요건서 v2.3 개정안](../../latest/mail-hantong%20요건서%20v2.3%20개정안.docx)",
        "실행 환경: Python 3.10 이상 (표준 라이브러리만 사용)",
        "재생성: `python scripts/prepare_requirements.py`  ·  검사: `python scripts/prepare_requirements.py --check`", "",
        "프로젝트 루트에서 실행한다. 생성 파일을 직접 편집하면 --check가 실패한다. 재생성은 기존 생성 매니페스트를 확인하며 다른 문서가 있는 폴더를 덮어쓰지 않는다.", "",
        f"- 원본 색인: `{len(rows)}개 요건`", f"- 분할된 수용 기준: `{sum(len(split_acceptance(r['acceptance'])) for r in rows)}개`", "",
        "## 검증", "", "- [검증 결과](validation.json)", "- [원문 대응표](traceability.csv)",
        "- [Word 전체 텍스트 동반본](source-content.md)", "",
        "## 도메인 파일", "",
        "\n".join(f"- [{domain}]({domain.lower()}.md)" for domain in sorted(domains)), "",
        REVIEW_CONTEXT, "",
        "원문 설명과 수용 기준 전체를 함께 보존한다. traceability.csv의 source_stage는 원문의 요건 단계이며 AC별 적용 확정이 아니다. 모든 review_status는 review_required다.",
        "검증은 문서 보존을 확인한다. 제품·보안·호환성 시험 통과를 뜻하지 않는다.", "",
        "## 특히 검토할 조건과 적용 범위", "",
        "\n".join(f"- [{rid}]({rid.split('-')[0].lower()}.md): {note}" for rid, note in REVIEW_NOTES.items()),
    ]
    (out / "README.md").write_text("\n".join(index_lines) + "\n", encoding="utf-8", newline="\n")
    trace_rows = []
    for domain, items in sorted(domains.items()):
        lines = [f"# {domain} 요건", "", "상태: 채택 전 검토 제안", ""]
        for row in items:
            rid = row["requirement_id"]
            criteria = split_acceptance(row["acceptance"])
            lines += [f"## {rid} {row['title'].removeprefix(rid).strip()}", "",
                      f"- 우선순위: `{row['priority']}`  ", f"- 단계: `{row['stage']}`  ",
                      f"- 작업 카드: `{row['stage1_card_ids'] or '—'}`  ", f"- 원문 위치: [{locations[rid]['anchor']}](source-content.md#{locations[rid]['anchor']})", "", "### 원문 설명", "",
                      html.escape(row["description"].strip(), quote=False) or "(없음)", "", "### 수용 기준 후보", ""]
            lines += [f"> 원문 수용 기준 전체: {html.escape(row['acceptance'].strip(), quote=False)}", "", REVIEW_CONTEXT, ""]
            if rid in REVIEW_NOTES:
                lines += [f"적용 범위 검토: {REVIEW_NOTES[rid]}", ""]
            for number, criterion in enumerate(criteria, 1):
                acid = f"{rid}-AC{number:02d}"
                lines.append(f"- **{acid}** {html.escape(criterion, quote=False)}")
                trace_rows.append({"requirement_id": rid, "criterion_id": acid,
                                   "source_acceptance": row["acceptance"], "split_acceptance": criterion,
                                   "priority": row["priority"], "source_stage": row["stage"],
                                   "stage1_card_ids": row["stage1_card_ids"],
                                   "source_doc": source_doc.name,
                                   "source_table": locations[rid]["table_number"],
                                   "source_row": locations[rid]["row_number"],
                                   "source_anchor": locations[rid]["anchor"],
                                   "review_status": "review_required",
                                   "review_note": REVIEW_NOTES.get(rid, "문맥·복합 항목·단계 적용 검토 필요")})
            lines += ["", "---", ""]
        (out / f"{domain.lower()}.md").write_text("\n".join(lines), encoding="utf-8", newline="\n")
    fields = list(trace_rows[0])
    with (out / "traceability.csv").open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        writer.writerows(trace_rows)
    validation = validate(rows)
    validation["generator"] = "mail-hantong/M0-00"
    validation["docx_requirement_count"] = len(source_rows)
    validation["csv_docx_mismatch_count"] = len(mismatches)
    validation["stage1_p0_count"] = sum(1 for row in rows if row["priority"] == "P0" and row["stage"].startswith("1"))
    validation["source_doc"] = source_doc.name
    validation["source_doc_sha256"] = hashlib.sha256(source_doc.read_bytes()).hexdigest()
    validation["source_block_count"] = len(source_blocks)
    validation["embedded_image_count"] = image_count
    validation["extracted_image_count"] = len(image_names)
    validation["referenced_image_count"] = sum(len(images) for _, _, images in source_blocks)
    validation["hyperlink_count"] = len(source_links)
    validation["review_status"] = "review_required"
    assets = out / "assets"
    assets.mkdir(exist_ok=True)
    with zipfile.ZipFile(source_doc) as archive:
        for name in archive.namelist():
            if name.startswith("word/media/") and not name.endswith("/"):
                (assets / Path(name).name).write_bytes(archive.read(name))
    source_lines = ["# Word 원문 동반본", "", f"원본: `{source_doc.name}`", f"SHA-256: `{validation['source_doc_sha256']}`", f"임베디드 이미지: `{image_count}`개 (assets/에 추출)", "", "아래는 Word 본문의 문단·표 행을 순서대로 추출한 검토용 텍스트다. 줄바꿈은 원문 XML의 줄바꿈을 보존한다.", ""]
    for i, (anchor, block, images) in enumerate(source_blocks, 1):
        req_match = re.match(r"\[표\] ([A-Z]+-\d{2})\s", block)
        aliases = f"<a id=\"{anchor}\"></a>"
        if req_match and req_match.group(1) in locations:
            aliases += f"<a id=\"{locations[req_match.group(1)]['anchor']}\"></a>"
        source_lines.append(f"{aliases}\n{i:04d}. {html.escape(block, quote=False).replace(chr(10), '<br>')}")
        for image in images:
            source_lines.append(f"\n![Word embedded image: {image}](assets/{image})")
    source_lines += ["", "## 원문 하이퍼링크 대상", ""]
    source_lines.extend(f"- [{rid}]({url})" for rid, url in source_links)
    (out / "source-content.md").write_text("\n".join(source_lines) + "\n", encoding="utf-8", newline="\n")
    validation["generated_files"] = sorted(expected_files)
    (out / "validation.json").write_text(json.dumps(validation, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def check_package(rows: list[dict[str, str]], source_doc: Path, out: Path) -> dict:
    if not out.is_dir():
        raise ValueError(f"package does not exist: {out}")
    with tempfile.TemporaryDirectory(prefix="m000-check-") as temp:
        expected = Path(temp) / "M0-00"
        write_package(rows, source_doc, expected)
        current_files = {p.relative_to(out).as_posix() for p in out.rglob("*") if p.is_file()}
        expected_files = {p.relative_to(expected).as_posix() for p in expected.rglob("*") if p.is_file()}
        extra = sorted(current_files - expected_files)
        missing = sorted(expected_files - current_files)
        changed = sorted(rel for rel in current_files & expected_files if (out / rel).read_bytes() != (expected / rel).read_bytes())
        report = {"valid": not (extra or missing or changed), "extra_files": extra, "missing_files": missing, "changed_files": changed}
        if not report["valid"]:
            raise ValueError("package is stale or modified: " + json.dumps(report, ensure_ascii=False))
        return report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--index", type=Path, default=ROOT / "docs/latest/requirements-index.csv")
    parser.add_argument("--source-doc", type=Path, default=ROOT / "docs/latest/mail-hantong 요건서 v2.3 개정안.docx")
    parser.add_argument("--out", type=Path, default=ROOT / "docs/proposals/M0-00")
    parser.add_argument("--check", action="store_true", help="check an existing package without modifying it")
    args = parser.parse_args()
    if args.index.resolve().is_relative_to(args.out.resolve()):
        raise ValueError("output must not contain the source index")
    rows = read_rows(args.index)
    report = validate(rows)
    if not report["valid"]:
        raise SystemExit(json.dumps(report, ensure_ascii=True))
    if args.check:
        print(json.dumps(check_package(rows, args.source_doc, args.out), ensure_ascii=True))
        return 0
    write_package(rows, args.source_doc, args.out)
    print(json.dumps(report, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    sys.stderr.reconfigure(errors="backslashreplace")
    try:
        raise SystemExit(main())
    except (ValueError, OSError, ET.ParseError, zipfile.BadZipFile) as error:
        print(f"M0-00: {error}", file=sys.stderr)
        raise SystemExit(1)
