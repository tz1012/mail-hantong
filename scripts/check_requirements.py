#!/usr/bin/env python3
"""Read-only integrity check for the adopted docs/requirements baseline."""
from __future__ import annotations

import argparse
import csv
import hashlib
import html
import json
import re
import sys
from datetime import datetime
from pathlib import Path
from urllib.parse import unquote

import prepare_requirements as prep

ID_RE = re.compile(r"^[A-Z]+-\d{2}$")
AC_RE = re.compile(r"^\s*-\s*\*\*([A-Z]+-\d{2}-AC\d{2})\*\*\s+(.+?)\s*$", re.MULTILINE)
REQUIRED_ADOPTION = {"schema_version", "status", "scope", "approved_by", "approval_recorded_at_utc", "evidence", "requirement_count", "acceptance_criterion_count", "domain_files", "source_doc", "source_doc_sha256", "source_index", "source_index_sha256", "files"}
INDEX_FIELDS = {"requirement_id", "title", "description", "acceptance", "priority", "stage", "stage1_card_ids"}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def safe_relative(value: object) -> bool:
    if not isinstance(value, str) or not value or "\\" in value:
        return False
    path = Path(value)
    return not path.is_absolute() and ".." not in path.parts and not any(part == "" for part in path.parts)


def contained(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except (OSError, ValueError):
        return False


def managed_path(base: Path, name: str, root: Path) -> Path | None:
    if not safe_relative(name):
        return None
    path = base / Path(name)
    if path.is_symlink() or not contained(path, root):
        return None
    return path


def local_links(path: Path, text: str, errors: list[str]) -> None:
    for target in re.findall(r"\[[^\]]*\]\((<[^>]+>|[^)\s]+)\)", text):
        target = target.strip("<>")
        if re.match(r"^[A-Za-z][A-Za-z0-9+.-]*:", target) or target.startswith("#"):
            continue
        target_path, _, fragment = target.partition("#")
        resolved = path.parent / unquote(target_path)
        if resolved.is_symlink() or not contained(resolved, path.parents[2]) or not resolved.is_file():
            errors.append(f"broken link: {path.relative_to(path.parents[2])}: {target}")
        elif fragment and resolved.suffix.lower() in {".md", ".markdown"}:
            anchors = {f"#{x}" for x in re.findall(r"id=[\"']([^\"']+)", resolved.read_text(encoding="utf-8-sig"))}
            headings = {"#" + re.sub(r"[^a-z0-9 -]", "", line.lower()).strip().replace(" ", "-") for line in resolved.read_text(encoding="utf-8-sig").splitlines() if line.startswith("#")}
            if "#" + fragment not in anchors and "#" + fragment.lower() not in headings:
                errors.append(f"broken anchor: {path}: {target}")


def validate_baseline(root: Path) -> dict:
    errors: list[str] = []
    canonical = root / "docs" / "requirements"
    adoption_path = canonical / "adoption.json"
    result = {"valid": False, "errors": errors}
    try:
        if (not canonical.is_dir() or adoption_path.is_symlink()
                or not contained(adoption_path, root) or not adoption_path.is_file()):
            errors.append("adopted docs/requirements/adoption.json is missing")
            return result
        adoption = json.loads(adoption_path.read_text(encoding="utf-8-sig"))
        if not REQUIRED_ADOPTION <= set(adoption): errors.append("adoption metadata is missing required fields")
        if adoption.get("schema_version") != 1 or adoption.get("status") != "adopted" or adoption.get("scope") != "requirements_baseline_only":
            errors.append("adoption metadata status/scope is not adopted requirements_baseline_only")
        timestamp = adoption.get("approval_recorded_at_utc")
        timestamp_ok = isinstance(timestamp, str) and timestamp.endswith("Z")
        try:
            if timestamp_ok: datetime.fromisoformat(timestamp[:-1] + "+00:00")
        except ValueError:
            timestamp_ok = False
        evidence = adoption.get("evidence", {})
        if adoption.get("approved_by") != "Andrew Cho (user)" or evidence.get("user_response") != "네" or evidence.get("approved_proposal") != "docs/proposals/M0-00/README.md" or not timestamp_ok:
            errors.append("approval evidence is missing")
        managed = adoption.get("files", {})
        if not isinstance(managed, dict): errors.append("files manifest is not an object"); managed = {}
        for name in managed:
            if managed_path(canonical, name, root) is None: errors.append(f"unsafe manifest path: {name}")
        actual = {}
        for path in canonical.rglob("*"):
            if path == adoption_path:
                continue
            if path.is_symlink():
                errors.append(f"symlink is not allowed: {path.relative_to(canonical)}")
            elif path.is_file():
                actual[path.relative_to(canonical).as_posix()] = path
        if set(managed) != set(actual):
            errors.append("canonical file inventory differs from adoption manifest")
        for name, expected in managed.items():
            if name in actual and (not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected) or digest(actual[name]) != expected):
                errors.append(f"file hash mismatch: {name}")
        domain_files = adoption.get("domain_files", [])
        if (not isinstance(domain_files, list)
                or any(not isinstance(x, str) for x in domain_files)
                or len(domain_files) != len(set(domain_files))
                or any(not safe_relative(x) for x in domain_files)
                or any(Path(x).suffix.lower() not in {".md", ".markdown"} for x in domain_files)):
            errors.append("invalid domain_files")
        else:
            for name in domain_files:
                if name not in actual: errors.append(f"missing domain file: {name}")
        index = managed_path(root, adoption.get("source_index", ""), root) or root / "__invalid__"
        source = managed_path(root, adoption.get("source_doc", ""), root) or root / "__invalid__"
        if not index.is_file() or digest(index) != adoption.get("source_index_sha256"): errors.append("source index hash/path mismatch")
        if not source.is_file() or digest(source) != adoption.get("source_doc_sha256"): errors.append("source document hash/path mismatch")
        rows = []
        if index.is_file():
            with index.open(encoding="utf-8-sig", newline="") as handle: rows = list(csv.DictReader(handle))
            if rows and not INDEX_FIELDS <= set(rows[0]): errors.append("source index fields are incomplete")
        ids = [r.get("requirement_id") for r in rows]
        if len(ids) != len(set(ids)) or any(not ID_RE.fullmatch(x or "") for x in ids): errors.append("source index IDs are invalid or duplicated")
        headings = []
        acs = []
        for name, path in actual.items():
            if path.suffix.lower() not in {".md", ".markdown"}: continue
            text = path.read_text(encoding="utf-8-sig")
            local_links(path, text, errors)
            if name not in domain_files:
                continue
            headings.extend(re.findall(r"^##\s+([A-Z]+-\d{2})\s+(.+?)\s*$", text, re.M))
            acs.extend((match.group(1), match.group(2)) for match in AC_RE.finditer(text))
        heading_ids = [rid for rid, _ in headings]
        if set(heading_ids) != set(ids) or len(heading_ids) != len(ids): errors.append("canonical requirement headings do not match source index")
        index_by_id = {row.get("requirement_id"): row for row in rows}
        for rid, title in headings:
            expected_title = index_by_id.get(rid, {}).get("title", "")
            if expected_title.startswith(rid + " "):
                expected_title = expected_title[len(rid) + 1:]
            if expected_title != title:
                errors.append(f"requirement title mismatch: {rid}")
        trace = canonical / "traceability.csv"
        trace_rows = []
        if trace.is_symlink() or not contained(trace, root):
            errors.append("traceability.csv path is unsafe")
        elif not trace.is_file():
            errors.append("traceability.csv is missing")
        else:
            with trace.open(encoding="utf-8-sig", newline="") as handle: trace_rows = list(csv.DictReader(handle))
            required_trace = {"requirement_id", "criterion_id", "source_acceptance", "split_acceptance", "priority", "source_stage", "source_anchor"}
            if trace_rows and not required_trace <= set(trace_rows[0]): errors.append("traceability fields are incomplete")
            trace_ids = [r.get("criterion_id") for r in trace_rows]
            if len(trace_ids) != len(set(trace_ids)) or any(not re.fullmatch(r"[A-Z]+-\d{2}-AC\d{2}", x or "") for x in trace_ids): errors.append("traceability criterion IDs are invalid or duplicated")
            source_content_path = canonical / "source-content.md"
            if source_content_path.is_symlink() or not contained(source_content_path, root):
                errors.append("source-content.md path is unsafe")
                source_content = ""
            else:
                source_content = source_content_path.read_text(encoding="utf-8-sig") if source_content_path.is_file() else ""
            trace_by_id = {}
            for row in trace_rows:
                rid, acid = row.get("requirement_id", ""), row.get("criterion_id", "")
                if not acid.startswith(rid + "-"): errors.append(f"criterion owner mismatch: {acid}")
                if rid not in index_by_id: errors.append(f"unknown trace requirement: {rid}")
                trace_by_id.setdefault(rid, []).append(row)
                if row.get("source_anchor") and f'id="{row["source_anchor"]}"' not in source_content: errors.append(f"missing source anchor: {row.get('source_anchor')}")
                owner = index_by_id.get(rid)
                if owner:
                    if (row.get("priority") != owner.get("priority")
                            or row.get("source_stage") != owner.get("stage")):
                        errors.append(f"traceability source fields mismatch: {acid}")
                    if prep.normalize(row.get("source_acceptance", "")) != prep.normalize(owner.get("acceptance", "")):
                        errors.append(f"traceability source acceptance mismatch: {acid}")
            trace_acs = [row.get("criterion_id") for row in trace_rows]
            if set(trace_acs) != {acid for acid, _ in acs} or len(trace_acs) != len(acs): errors.append("canonical AC text does not match traceability")
            for acid, text in acs:
                row = next((item for item in trace_rows if item.get("criterion_id") == acid), None)
                if row and prep.normalize(html.unescape(text)) != prep.normalize(row.get("split_acceptance", "")):
                    errors.append(f"AC text mismatch: {acid}")
            for rid, owner in index_by_id.items():
                if rid not in trace_by_id:
                    errors.append(f"missing traceability rows: {rid}")
                expected = prep.normalize(owner.get("acceptance", ""))
                actual = prep.normalize(" ".join(row.get("split_acceptance", "") for row in trace_by_id.get(rid, [])))
                if actual != expected: errors.append(f"acceptance recombination mismatch: {rid}")
        result.update({"requirement_count": len(ids), "acceptance_criterion_count": len(trace_rows), "domain_file_count": len(domain_files) if isinstance(domain_files, list) else 0})
        if adoption.get("requirement_count") != len(ids) or adoption.get("acceptance_criterion_count") != len(trace_rows): errors.append("adoption counts differ from canonical files")
    except Exception as exc:
        errors.append(f"validation error: {type(exc).__name__}")
    result["valid"] = not errors
    return result


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args(argv)
    result = validate_baseline(args.root.resolve())
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0 if result["valid"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
