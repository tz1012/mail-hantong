import csv
import json
import tempfile
import hashlib
import unittest
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import traceability


class TraceabilityTests(unittest.TestCase):
    def fixture(self):
        temp = tempfile.TemporaryDirectory()
        root = Path(temp.name)
        (root / "docs/requirements").mkdir(parents=True)
        (root / "docs").mkdir(exist_ok=True)
        (root / "crates/a/src").mkdir(parents=True)
        (root / "crates/a/src/main.rs").write_text("fn main() {}\n", encoding="utf-8")
        rows = [{
            "requirement_id": "ACC-01", "criterion_id": "ACC-01-AC01",
            "source_acceptance": "criterion", "split_acceptance": "criterion",
            "priority": "P0", "source_stage": "1", "stage1_card_ids": "M1-10",
            "source_doc": "source.docx", "source_table": "1", "source_row": "1",
            "source_anchor": "T1", "review_status": "review_required", "review_note": "review",
        }]
        with (root / "docs/requirements/traceability.csv").open("w", encoding="utf-8", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=rows[0].keys()); writer.writeheader(); writer.writerows(rows)
        return temp, root

    def execution(self, root, result="passed"):
        log = root / "run.log"; log.write_text("real command output\n", encoding="utf-8")
        item = {"result": result, "test_id": "T", "test_kind": "unit", "environment": "fixture", "command": ["python", "-m", "test"], "exit_code": 0 if result == "passed" else 1, "log_path": "run.log", "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest(), "reviewer": "agent", "evidence": "execution.json"}
        artifact = dict(item); artifact.update(schema_version=1, criterion_id="ACC-01-AC01", source_hash=traceability.source_hash(root))
        (root / "execution.json").write_text(json.dumps(artifact), encoding="utf-8")
        return item

    def test_M004_empty_mapping_is_unimplemented(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        result = traceability.build_rows(root, {})
        self.assertEqual(result[0]["result"], "unimplemented")
        self.assertEqual(result[0]["test_id"], "")

    def test_M004_rejects_unknown_duplicate_and_stale_evidence(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        source_hash = traceability.source_hash(root)
        evidence = {"schema_version": 1, "source_hash": source_hash, "results": {"UNKNOWN-AC01": {"result": "passed"}, "ACC-01-AC01": self.execution(root)}}
        with self.assertRaises(traceability.ValidationError): traceability.validate_evidence(root, evidence)
        evidence["results"].pop("UNKNOWN-AC01")
        evidence["source_hash"] = "0" * 64
        with self.assertRaises(traceability.ValidationError): traceability.validate_evidence(root, evidence)

    def test_M004_verifiable_pass_requires_evidence_file_and_metadata(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        data = {"schema_version": 1, "source_hash": traceability.source_hash(root), "results": {"ACC-01-AC01": self.execution(root)}}
        traceability.validate_evidence(root, data)
        self.assertEqual(traceability.build_rows(root, data["results"])[0]["result"], "passed")

    def test_M004_not_applicable_is_reserved_until_authoritative_review(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        data = {"schema_version": 1, "source_hash": traceability.source_hash(root), "results": {"ACC-01-AC01": {"result": "not_applicable", "scope_decision": {"criterion_id": "ACC-01-AC01", "source_hash": traceability.source_hash(root), "approved_by": "agent", "reason": "later", "current_scope": "phase-2"}}}}
        with self.assertRaises(traceability.ValidationError): traceability.validate_evidence(root, data)

    def test_M004_runtime_artifacts_do_not_change_source_hash(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        before = traceability.source_hash(root)
        (root / "run.log").write_text("later", encoding="utf-8")
        (root / "execution.json").write_text("{}", encoding="utf-8")
        self.assertEqual(traceability.source_hash(root), before)
        (root / "crates/a/src/new.rs").write_text("fn changed() {}\n", encoding="utf-8")
        self.assertNotEqual(traceability.source_hash(root), before)

    def test_M004_corpus_manifest_changes_invalidate_evidence(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        corpus = root / "tests/Corpus"
        corpus.mkdir(parents=True)
        path = corpus / "index.json"
        path.write_text('{"fixtures": []}', encoding="utf-8")
        before = traceability.source_hash(root)
        path.write_text('{"fixtures": ["changed"]}', encoding="utf-8")
        self.assertNotEqual(traceability.source_hash(root), before)

    def test_M004_build_harness_and_native_manifest_invalidate_evidence(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        scripts = root / "scripts"
        scripts.mkdir()
        before = traceability.source_hash(root)
        (scripts / "check_all.py").write_text("raise SystemExit(1)", encoding="utf-8")
        after = traceability.source_hash(root)
        self.assertNotEqual(after, before)
        (scripts / "native-tools.json").write_text('{"tools": {}}', encoding="utf-8")
        self.assertNotEqual(traceability.source_hash(root), after)

    def test_M004_empty_template_still_requires_schema_one(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        (root / "tests").mkdir()
        (root / "tests/traceability.json").write_text(json.dumps({"schema_version": 0, "source_hash": "", "results": {}}), encoding="utf-8")
        with self.assertRaises(traceability.ValidationError): traceability.evidence_data(root)

    def test_M004_schema_and_contract_changes_invalidate_evidence(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        contracts = root / "docs/contracts"
        contracts.mkdir()
        before = traceability.source_hash(root)
        for name, content in [("db-schema.sql","CREATE TABLE t(x);"),("database.md","changed contract"),("tool.schema.json",'{}')]:
            (contracts / name).write_text(content,encoding="utf-8")
            after = traceability.source_hash(root)
            self.assertNotEqual(before,after)
            before = after

    def test_M004_check_detects_output_tamper_without_writing(self):
        temp, root = self.fixture(); self.addCleanup(temp.cleanup)
        output = root / "docs/traceability.csv"
        output.write_text("tampered\n", encoding="utf-8")
        with self.assertRaises(traceability.ValidationError): traceability.check_outputs(root, {})
        self.assertEqual(output.read_text(encoding="utf-8"), "tampered\n")


if __name__ == "__main__":
    unittest.main()
