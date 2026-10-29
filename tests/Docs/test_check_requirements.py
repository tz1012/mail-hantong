import csv
import hashlib
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from unittest import mock
from pathlib import Path

import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import check_requirements as checker


class CheckRequirementsTests(unittest.TestCase):
    def make_fixture(self):
        temp = tempfile.TemporaryDirectory()
        root = Path(temp.name)
        canonical = root / "docs/requirements"
        latest = root / "docs/latest"
        canonical.mkdir(parents=True)
        latest.mkdir(parents=True)
        index = latest / "requirements-index.csv"
        rows = [
            {"requirement_id":"ACC-01","title":"Accounts","description":"d","acceptance":"First criterion. Second criterion.","priority":"P0","stage":"1","stage1_card_ids":"M1"},
            {"requirement_id":"MAIL-02","title":"Mail","description":"d","acceptance":"Only criterion.","priority":"P1","stage":"2","stage1_card_ids":""},
        ]
        with index.open("w", encoding="utf-8", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=rows[0].keys(), lineterminator="\n")
            writer.writeheader(); writer.writerows(rows)
        source = latest / "source.docx"; source.write_bytes(b"source")
        (canonical / "source-content.md").write_text('<a id="T1"></a>\n', encoding="utf-8")
        (canonical / "README.md").write_text("# Adopted\n\n[ACC](acc.md) [MAIL](mail.md)\n", encoding="utf-8")
        (canonical / "acc.md").write_text("# ACC\n\n## ACC-01 Accounts\n\n- **ACC-01-AC01** First criterion.\n- **ACC-01-AC02** Second criterion.\n\n", encoding="utf-8")
        (canonical / "mail.md").write_text("# MAIL\n\n## MAIL-02 Mail\n\n- **MAIL-02-AC01** Only criterion.\n\n", encoding="utf-8")
        (canonical / "traceability.csv").write_text(
            "requirement_id,criterion_id,source_acceptance,split_acceptance,priority,source_stage,source_anchor\n"
            "ACC-01,ACC-01-AC01,First criterion. Second criterion.,First criterion.,P0,1,T1\n"
            "ACC-01,ACC-01-AC02,First criterion. Second criterion.,Second criterion.,P0,1,T1\n"
            "MAIL-02,MAIL-02-AC01,Only criterion.,Only criterion.,P1,2,T1\n", encoding="utf-8")
        files = {}
        for path in sorted(p for p in canonical.rglob("*") if p.is_file()):
            files[path.relative_to(canonical).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
        adoption = {
            "schema_version": 1, "status": "adopted", "scope": "requirements_baseline_only",
            "approved_by": "Andrew Cho (user)", "approval_recorded_at_utc": "2026-10-29T00:00:00Z",
            "evidence": {"user_response":"네", "approved_proposal":"docs/proposals/M0-00/README.md"},
            "requirement_count": 2, "acceptance_criterion_count": 3,
            "domain_files": ["acc.md", "mail.md"], "source_doc":"docs/latest/source.docx",
            "source_doc_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "source_index":"docs/latest/requirements-index.csv",
            "source_index_sha256": hashlib.sha256(index.read_bytes()).hexdigest(), "files": files,
        }
        (canonical / "adoption.json").write_text(json.dumps(adoption), encoding="utf-8")
        return temp, root

    def test_M000_valid_adopted_baseline(self):
        temp, root = self.make_fixture(); self.addCleanup(temp.cleanup)
        result = checker.validate_baseline(root)
        self.assertTrue(result["valid"], result["errors"])
        self.assertEqual(result["requirement_count"], 2)

    def test_M000_rejects_tamper_missing_extra_and_bad_anchor(self):
        for mode in ("tamper", "missing", "extra", "nested-adoption", "anchor"):
            with self.subTest(mode=mode):
                temp, root = self.make_fixture(); self.addCleanup(temp.cleanup)
                canonical = root / "docs/requirements"
                if mode == "tamper": (canonical / "acc.md").write_text("changed", encoding="utf-8")
                if mode == "missing": (canonical / "acc.md").unlink()
                if mode == "extra": (canonical / "extra.md").write_text("x", encoding="utf-8")
                if mode == "nested-adoption":
                    (canonical / "nested").mkdir()
                    (canonical / "nested" / "adoption.json").write_text("{}", encoding="utf-8")
                if mode == "anchor": (canonical / "source-content.md").write_text("", encoding="utf-8")
                self.assertFalse(checker.validate_baseline(root)["valid"])

    def test_M000_rejects_unsafe_manifest_path_and_unadopted_status(self):
        temp, root = self.make_fixture(); self.addCleanup(temp.cleanup)
        adoption = root / "docs/requirements/adoption.json"
        payload = json.loads(adoption.read_text(encoding="utf-8")); payload["files"]["../escape"] = "0" * 64; adoption.write_text(json.dumps(payload), encoding="utf-8")
        self.assertFalse(checker.validate_baseline(root)["valid"])
        payload["files"].pop("../escape"); payload["status"] = "review_required"; adoption.write_text(json.dumps(payload), encoding="utf-8")
        self.assertFalse(checker.validate_baseline(root)["valid"])

    def test_M000_cli_returns_json_and_nonzero_for_missing_baseline(self):
        with tempfile.TemporaryDirectory() as directory, redirect_stdout(StringIO()) as output:
            result = checker.main(["--root", directory])
        self.assertEqual(result, 1)
        self.assertFalse(json.loads(output.getvalue())["valid"])

    def test_M000_semantic_mismatch_is_not_only_a_hash_failure(self):
        temp, root = self.make_fixture(); self.addCleanup(temp.cleanup)
        canonical = root / "docs/requirements"
        adoption_path = canonical / "adoption.json"
        payload = json.loads(adoption_path.read_text(encoding="utf-8"))
        path = canonical / "mail.md"
        path.write_text("# MAIL\n\n## MAIL-02 Mail\n\n- **MAIL-02-AC01** Wrong criterion.\n\n", encoding="utf-8")
        payload["files"]["mail.md"] = hashlib.sha256(path.read_bytes()).hexdigest()
        adoption_path.write_text(json.dumps(payload), encoding="utf-8")
        result = checker.validate_baseline(root)
        self.assertFalse(result["valid"])
        self.assertTrue(any("AC text mismatch" in error for error in result["errors"]))

    def test_M000_rejects_duplicate_and_title_mismatch_with_refreshed_hash(self):
        for change in ("duplicate", "title"):
            with self.subTest(change=change):
                temp, root = self.make_fixture(); self.addCleanup(temp.cleanup)
                canonical = root / "docs/requirements"
                path = canonical / "acc.md"
                text = path.read_text(encoding="utf-8")
                text += "\n## ACC-01 Accounts\n" if change == "duplicate" else ""
                text = text.replace("## ACC-01 Accounts", "## ACC-01 Wrong", 1) if change == "title" else text
                path.write_text(text, encoding="utf-8")
                adoption_path = canonical / "adoption.json"
                payload = json.loads(adoption_path.read_text(encoding="utf-8"))
                payload["files"]["acc.md"] = hashlib.sha256(path.read_bytes()).hexdigest()
                adoption_path.write_text(json.dumps(payload), encoding="utf-8")
                self.assertFalse(checker.validate_baseline(root)["valid"])

    def test_M000_rejects_broken_link_and_anchor_with_refreshed_hash(self):
        temp, root = self.make_fixture(); self.addCleanup(temp.cleanup)
        canonical = root / "docs/requirements"
        readme = canonical / "README.md"
        source = canonical / "source-content.md"
        readme.write_text("# Adopted\n\n[missing](missing.md#nope)\n", encoding="utf-8")
        source.write_text("", encoding="utf-8")
        payload_path = canonical / "adoption.json"
        payload = json.loads(payload_path.read_text(encoding="utf-8"))
        payload["files"]["README.md"] = hashlib.sha256(readme.read_bytes()).hexdigest()
        payload["files"]["source-content.md"] = hashlib.sha256(source.read_bytes()).hexdigest()
        payload_path.write_text(json.dumps(payload), encoding="utf-8")
        result = checker.validate_baseline(root)
        self.assertFalse(result["valid"])
        self.assertTrue(any("broken link" in error for error in result["errors"]))
        self.assertTrue(any("missing source anchor" in error for error in result["errors"]))

    def test_M000_rejects_symlinked_manifest_path(self):
        temp, root = self.make_fixture(); self.addCleanup(temp.cleanup)
        canonical = root / "docs/requirements"
        link = canonical / "outside.md"
        try:
            link.symlink_to(root / "outside-source.md")
        except (OSError, NotImplementedError):
            with mock.patch.object(checker, "contained", return_value=False):
                self.assertIsNone(checker.managed_path(canonical, "outside.md", root))
                payload = json.loads((canonical / "adoption.json").read_text(encoding="utf-8"))
                payload["files"]["outside.md"] = "0" * 64
                (canonical / "adoption.json").write_text(json.dumps(payload), encoding="utf-8")
            self.assertFalse(checker.validate_baseline(root)["valid"])
            return
        (root / "outside-source.md").write_text("outside", encoding="utf-8")
        payload = json.loads((canonical / "adoption.json").read_text(encoding="utf-8"))
        payload["files"]["outside.md"] = hashlib.sha256((root / "outside-source.md").read_bytes()).hexdigest()
        (canonical / "adoption.json").write_text(json.dumps(payload), encoding="utf-8")
        result = checker.validate_baseline(root)
        self.assertFalse(result["valid"])
        self.assertTrue(any("symlink" in error or "inventory" in error for error in result["errors"]))


if __name__ == "__main__":
    unittest.main()
