#!/usr/bin/env python3
"""Generate conservative requirement-to-test traceability reports."""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE_FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "tests/Corpus/index.json", "scripts/native-tools.json", "scripts/check-all", "scripts/traceability", "scripts/test-mail-server", "packaging/licenses/index.json")
SOURCE_SUFFIXES = {".rs", ".py", ".toml", ".txt", ".eml", ".mbox", ".ps1", ".sh", ".cert", ".fullchain", ".key", ".manifest"}
RESULTS = {"unimplemented", "not_run", "passed", "failed", "review_required", "not_applicable"}
REQUIRED_EVIDENCE = {"result", "test_id", "test_kind", "environment", "command", "exit_code", "log_path", "log_sha256", "evidence", "reviewer"}


class ValidationError(ValueError):
    pass


def source_hash(root: Path) -> str:
    digest = hashlib.sha256()
    paths = []
    for name in SOURCE_FILES:
        path = root / name
        if path.is_file(): paths.append(path)
    for directory in (root / "crates", root / "apps", root / "tests", root / "scripts", root / "packaging"):
        if directory.is_dir():
            paths.extend(p for p in directory.rglob("*") if p.is_file() and not p.is_symlink() and p.suffix.lower() in SOURCE_SUFFIXES and p.name not in {"traceability.json", "execution.json"} and "__pycache__" not in p.parts and p.parent.name not in {"target", "artifacts", ".local"})
    contracts = root / "docs/contracts"
    if contracts.is_dir():
        paths.extend(p for p in contracts.rglob("*") if p.is_file() and p.suffix.lower() in {".md", ".json", ".sql"})
    for path in sorted(paths, key=lambda p: p.relative_to(root).as_posix()):
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise ValidationError("source hash cannot include links outside the source tree")
        relative = path.relative_to(root).as_posix().encode()
        digest.update(len(relative).to_bytes(4, "big")); digest.update(relative)
        data = path.read_bytes(); digest.update(len(data).to_bytes(8, "big")); digest.update(data)
    return digest.hexdigest()


def read_source(root: Path) -> list[dict[str, str]]:
    path = root / "docs/requirements/traceability.csv"
    if not path.is_file(): raise ValidationError("canonical traceability.csv is missing")
    with path.open(encoding="utf-8-sig", newline="") as handle: rows = list(csv.DictReader(handle))
    if not rows: raise ValidationError("canonical traceability.csv is empty")
    ids = [row.get("criterion_id", "") for row in rows]
    if any(not value for value in ids) or len(ids) != len(set(ids)): raise ValidationError("canonical criterion IDs are missing or duplicated")
    return rows


def read_card_status(root: Path) -> dict[str, dict[str, str]]:
    path = root / "docs/latest/card-status.csv"
    if not path.is_file() or path.is_symlink(): return {}
    with path.open(encoding="utf-8-sig", newline="") as handle:
        return {row.get("card_id", ""): row for row in csv.DictReader(handle) if row.get("card_id")}


def safe_evidence_path(root: Path, value: object) -> Path:
    if not isinstance(value, str) or not value or "\\" in value or Path(value).is_absolute() or ".." in Path(value).parts:
        raise ValidationError("evidence path must be a relative path inside the repository")
    candidate = root / value
    if candidate.is_symlink(): raise ValidationError(f"evidence path is a symlink: {value}")
    path = candidate.resolve()
    try: path.relative_to(root.resolve())
    except ValueError as exc: raise ValidationError("evidence path escapes repository") from exc
    if not path.is_file(): raise ValidationError(f"evidence file is missing: {value}")
    return path


def load_json_no_duplicates(path: Path) -> dict:
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result: raise ValidationError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    try:
        value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ValidationError(f"invalid JSON evidence: {path}") from exc
    if not isinstance(value, dict): raise ValidationError(f"JSON evidence must be an object: {path}")
    return value


def validate_evidence(root: Path, data: dict) -> None:
    if not isinstance(data, dict) or data.get("schema_version") != 1 or data.get("source_hash") != source_hash(root):
        raise ValidationError("traceability evidence is stale or has an unsupported schema")
    source_ids = {row["criterion_id"] for row in read_source(root)}
    results = data.get("results")
    if not isinstance(results, dict): raise ValidationError("traceability results must be an object")
    unknown = set(results) - source_ids
    if unknown: raise ValidationError("unknown criterion IDs: " + ", ".join(sorted(unknown)))
    for criterion_id, item in results.items():
        if not isinstance(item, dict): raise ValidationError(f"invalid evidence result: {criterion_id}")
        result = item.get("result", "not_run")
        if result not in RESULTS - {"unimplemented"}: raise ValidationError(f"invalid evidence result: {criterion_id}")
        if result == "not_run":
            if item.get("test_id") and not isinstance(item["test_id"], str): raise ValidationError(f"invalid test_id: {criterion_id}")
            continue
        if result == "not_applicable":
            raise ValidationError(f"not_applicable is reserved until authoritative review evidence exists: {criterion_id}")
        if not REQUIRED_EVIDENCE <= set(item): raise ValidationError(f"incomplete execution metadata: {criterion_id}")
        if not item.get("test_id") or not item.get("test_kind") or not item.get("environment") or not item.get("reviewer"):
            raise ValidationError(f"execution metadata is incomplete: {criterion_id}")
        if not isinstance(item["command"], list) or not item["command"] or any(not isinstance(x, str) or not x for x in item["command"]): raise ValidationError(f"command must be a non-empty list: {criterion_id}")
        if not isinstance(item["exit_code"], int) or (result == "passed" and item["exit_code"] != 0) or (result == "failed" and item["exit_code"] == 0): raise ValidationError(f"exit_code does not match result: {criterion_id}")
        log = safe_evidence_path(root, item["log_path"])
        if hashlib.sha256(log.read_bytes()).hexdigest() != item["log_sha256"]: raise ValidationError(f"log hash mismatch: {criterion_id}")
        artifact = safe_evidence_path(root, item["evidence"])
        execution = load_json_no_duplicates(artifact)
        if execution.get("schema_version") != 1 or execution.get("criterion_id") != criterion_id or execution.get("source_hash") != source_hash(root) or execution.get("test_id") != item["test_id"] or execution.get("result") != result or execution.get("environment") != item["environment"] or execution.get("command") != item["command"] or execution.get("exit_code") != item["exit_code"] or execution.get("log_path") != item["log_path"] or execution.get("log_sha256") != item["log_sha256"]:
            raise ValidationError(f"structured execution evidence mismatch: {criterion_id}")


def build_rows(root: Path, evidence: dict) -> list[dict[str, str]]:
    rows = []
    cards = read_card_status(root)
    for source in read_source(root):
        item = evidence.get(source["criterion_id"], {}) if isinstance(evidence, dict) else {}
        result = item.get("result", "unimplemented")
        rows.append({
            "criterion_id": source["criterion_id"], "requirement_id": source["requirement_id"],
            "source_acceptance": source["source_acceptance"], "priority": source["priority"], "source_stage": source["source_stage"],
            "stage_applicability": item.get("stage_applicability", "review_required"),
            "configuration": item.get("configuration", "unmapped"), "provider": item.get("provider", "unmapped"), "card": item.get("card", source.get("stage1_card_ids", "")), "card_status": "; ".join(f"{card}:{cards.get(card, {}).get('status', 'unknown')}" for card in source.get("stage1_card_ids", "").split(",") if card),
            "test_id": item.get("test_id", ""), "test_kind": item.get("test_kind", ""), "environment": item.get("environment", ""),
            "result": result if result not in {"", "unimplemented"} else ("not_run" if item.get("test_id") else "unimplemented"), "evidence": item.get("evidence", item.get("log_path", "")), "reviewer": item.get("reviewer", ""), "approved_scope": item.get("approved_scope", ""),
        })
    return rows


FIELDS = ["criterion_id", "requirement_id", "source_acceptance", "priority", "source_stage", "stage_applicability", "configuration", "provider", "card", "card_status", "test_id", "test_kind", "environment", "result", "evidence", "reviewer", "approved_scope"]


def render_csv(rows: list[dict[str, str]]) -> str:
    import io
    output = io.StringIO(newline="")
    writer = csv.DictWriter(output, fieldnames=FIELDS, lineterminator="\n"); writer.writeheader(); writer.writerows(rows)
    return output.getvalue()


def render_md(rows: list[dict[str, str]], digest: str) -> str:
    counts = {status: sum(row["result"] == status for row in rows) for status in sorted(RESULTS)}
    lines = ["# 요건 추적표", "", "이 보고서는 실행 증거가 있는 경우에만 `passed` 또는 `failed`를 기록합니다. 태그나 문서 연결만으로 통과를 표시하지 않습니다.", "", f"소스 해시: `{digest}`", "", "| 상태 | 개수 |", "| --- | ---: |"]
    lines.extend(f"| `{key}` | {counts[key]} |" for key in sorted(counts))
    lines += ["", "상세 행은 [traceability.csv](traceability.csv)에서 확인합니다. `not_applicable`는 권위 있는 사람 검토 증거가 연결되기 전까지 예약 상태이며, 실행 증거 파일은 저장소 내부의 상대 경로만 허용합니다.", ""]
    return "\n".join(lines)


def evidence_data(root: Path) -> dict:
    path = root / "tests/traceability.json"
    if not path.is_file(): return {"schema_version": 1, "source_hash": source_hash(root), "results": {}}
    data = load_json_no_duplicates(path)
    # The checked-in empty template is intentionally source-agnostic; generation binds it
    # to the current product source hash. Any non-empty evidence must be hash-bound.
    if data.get("schema_version") == 1 and data.get("source_hash") == "" and data.get("results") == {}:
        data["source_hash"] = source_hash(root)
    validate_evidence(root, data)
    return data


def check_outputs(root: Path, data: dict) -> None:
    rows = build_rows(root, data.get("results", {})); expected_csv = render_csv(rows); expected_md = render_md(rows, source_hash(root))
    for path, expected in ((root / "docs/traceability.csv", expected_csv), (root / "docs/traceability.md", expected_md)):
        if not path.is_file() or path.read_text(encoding="utf-8") != expected: raise ValidationError(f"traceability output is stale: {path}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(); parser.add_argument("--root", type=Path, default=ROOT); parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv); root = args.root.resolve()
    try:
        data = evidence_data(root); rows = build_rows(root, data.get("results", {})); digest = source_hash(root)
        if args.check: check_outputs(root, data)
        else:
            (root / "docs/traceability.csv").write_text(render_csv(rows), encoding="utf-8", newline="")
            (root / "docs/traceability.md").write_text(render_md(rows, digest), encoding="utf-8", newline="")
        print(json.dumps({"valid": True, "criterion_count": len(rows), "source_hash": digest, "check": args.check}, ensure_ascii=False, sort_keys=True)); return 0
    except ValidationError as exc:
        print(json.dumps({"valid": False, "error": str(exc)}, ensure_ascii=False, sort_keys=True)); return 1


if __name__ == "__main__": raise SystemExit(main())
