"""M0-00 fidelity, rejection, reproducibility, and output safety checks."""
import csv
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/prepare_requirements.py"
INDEX = ROOT / "docs/latest/requirements-index.csv"
SOURCE = ROOT / "docs/latest/mail-hantong 요건서 v2.3 개정안.docx"
sys.path.insert(0, str(ROOT / "scripts"))
import prepare_requirements as prep


def snapshot(folder):
    return {p.relative_to(folder).as_posix(): p.read_bytes()
            for p in folder.rglob("*") if p.is_file()}


class PrepareRequirementsTests(unittest.TestCase):
    def setUp(self):
        self.rows = prep.read_rows(INDEX)
        self.source_rows, _, _ = prep.docx_rows(SOURCE)

    def run_cli(self, out, *args):
        return subprocess.run(
            [sys.executable, "-X", "utf8", str(SCRIPT), "--out", str(out), *args],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8")

    def test_M000_csv_and_word_match_all_170_requirements(self):
        _, differences = prep.compare_source(self.rows, self.source_rows)
        self.assertEqual(differences, [])
        self.assertEqual(len(self.rows), 170)
        self.assertEqual(len(self.source_rows), 170)

    def test_M000_source_field_mismatches_are_rejected(self):
        for field in ("title", "description", "acceptance", "priority", "stage"):
            with self.subTest(field=field):
                changed = [dict(row) for row in self.source_rows]
                changed[0][field] += " altered"
                _, differences = prep.compare_source(self.rows, changed)
                self.assertTrue(any(d["field"] == field for d in differences))

    def test_M000_missing_and_substituted_ids_are_rejected(self):
        for changed in (self.source_rows[1:],
                        [dict(self.source_rows[0], requirement_id="INST-99")] + self.source_rows[1:]):
            with self.subTest(count=len(changed)):
                _, differences = prep.compare_source(self.rows, changed)
                self.assertTrue(any(d["field"] == "id" for d in differences))

    def test_M000_duplicate_csv_ids_are_invalid(self):
        rows = self.rows[:-1] + [dict(self.rows[0])]
        self.assertFalse(prep.validate(rows)["valid"])

    def test_M000_duplicate_word_ids_are_rejected(self):
        _, differences = prep.compare_source(self.rows, self.source_rows + [self.source_rows[0]])
        self.assertTrue(differences, "duplicate Word rows must not collapse silently")

    def test_M000_blank_acceptance_is_invalid(self):
        self.rows[0]["acceptance"] = " \n "
        self.assertFalse(prep.validate(self.rows)["valid"])

    def test_M000_malformed_id_is_invalid(self):
        self.rows[0]["requirement_id"] = "inst-001"
        self.assertFalse(prep.validate(self.rows)["valid"])

    def test_M000_lowercase_sentence_and_decimal_boundaries(self):
        self.assertEqual(prep.split_acceptance("멈추지 않는다. dry-run은 변경하지 않는다."),
                         ["멈추지 않는다.", "dry-run은 변경하지 않는다."])
        self.assertEqual(prep.split_acceptance("버전 2.3과 4.5:1을 유지한다."),
                         ["버전 2.3과 4.5:1을 유지한다."])

    def test_M000_every_acceptance_recombines_without_text_loss(self):
        for row in self.rows:
            with self.subTest(id=row["requirement_id"]):
                self.assertEqual(prep.normalize(" ".join(prep.split_acceptance(row["acceptance"]))),
                                 prep.normalize(row["acceptance"]))

    def test_M000_word_paragraphs_do_not_run_together(self):
        cell = ET.fromstring('<w:tc xmlns:w="' + prep.NS["w"] + '">'
                             '<w:p><w:r><w:t>alpha</w:t></w:r></w:p>'
                             '<w:p><w:r><w:t>beta</w:t></w:r></w:p></w:tc>')
        self.assertEqual(prep.xml_text(cell, preserve_breaks=True).strip(), "alpha\nbeta")

    def test_M000_mismatch_fails_before_creating_output(self):
        self.rows[0]["description"] = "altered"
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "proposal"
            with self.assertRaises(ValueError):
                prep.write_package(self.rows, SOURCE, out)
            self.assertFalse(out.exists())

    def test_M000_canonical_requirements_output_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "docs" / "requirements"
            with self.assertRaises(ValueError):
                prep.write_package(self.rows, SOURCE, out)
            self.assertFalse(out.exists())

    def test_M000_output_cannot_contain_its_input(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            source = out / SOURCE.name
            shutil.copyfile(SOURCE, source)
            before = snapshot(out)
            with self.assertRaises(ValueError):
                prep.write_package(self.rows, source, out)
            self.assertEqual(snapshot(out), before)

    def test_M000_generated_trace_links_and_original_context(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "proposal"
            result = self.run_cli(out)
            self.assertEqual(result.returncode, 0, result.stderr)
            with (out / "traceability.csv").open(encoding="utf-8", newline="") as handle:
                trace = list(csv.DictReader(handle))
            self.assertEqual({r["requirement_id"] for r in trace}, {r["requirement_id"] for r in self.rows})
            self.assertEqual(len({r["criterion_id"] for r in trace}), len(trace))
            source = (out / "source-content.md").read_text(encoding="utf-8")
            for row in trace:
                self.assertIn("source_table", row)
                self.assertIn("source_row", row)
                self.assertIn("source_stage", row)
                self.assertIn("source_anchor", row)
                self.assertIn(f'id="{row["source_anchor"]}"', source)
                self.assertEqual(row["review_status"], "review_required")
            cli = (out / "cli.md").read_text(encoding="utf-8")
            self.assertIn("&lt;group&gt;", cli)
            self.assertIn("원문 수용 기준 전체", cli)

    def test_M000_embedded_images_are_preserved_byte_for_byte(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "proposal"
            self.assertEqual(self.run_cli(out).returncode, 0)
            text = (out / "source-content.md").read_text(encoding="utf-8")
            with zipfile.ZipFile(SOURCE) as archive:
                images = [name for name in archive.namelist() if name.startswith("word/media/") and not name.endswith("/")]
                self.assertEqual(len(images), 2)
                for name in images:
                    self.assertEqual((out / "assets" / Path(name).name).read_bytes(), archive.read(name))
                    self.assertIn(f"(assets/{Path(name).name})", text)
                links = ET.fromstring(archive.read("word/_rels/document.xml.rels"))
                for link in links:
                    if link.attrib.get("Type", "").endswith("/hyperlink"):
                        self.assertIn(link.attrib["Target"], text)

    def test_M000_generated_text_uses_portable_lf(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "proposal"
            self.assertEqual(self.run_cli(out).returncode, 0)
            for path in out.iterdir():
                if path.suffix in {".md", ".json", ".csv"}:
                    self.assertFalse(b"\r\n" in path.read_bytes(), path.name)

    def test_M000_generation_succeeds_with_ascii_pipe_encoding(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "proposal"
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--out", str(out)], cwd=ROOT,
                env={**os.environ, "PYTHONIOENCODING": "ascii"},
                capture_output=True, text=True, encoding="ascii")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["requirement_count"], 170)

    def test_M000_generation_is_reproducible_and_has_no_machine_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "proposal"
            self.assertEqual(self.run_cli(out).returncode, 0)
            before = snapshot(out)
            self.assertEqual(self.run_cli(out).returncode, 0)
            self.assertEqual(snapshot(out), before)
            report = json.loads((out / "validation.json").read_text(encoding="utf-8"))
            self.assertFalse(Path(report["source_doc"]).is_absolute())
            self.assertEqual(report["stage1_p0_count"], 144)
            self.assertEqual(self.run_cli(out, "--check").returncode, 0)

    def test_M000_check_rejects_missing_changed_and_extra_files_without_writes(self):
        for change in ("missing", "changed", "extra"):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp) / "proposal"
                self.assertEqual(self.run_cli(out).returncode, 0)
                if change == "missing":
                    (out / "cli.md").unlink()
                elif change == "changed":
                    (out / "cli.md").write_text("altered", encoding="utf-8")
                else:
                    (out / "unexpected.txt").write_text("retain", encoding="utf-8")
                before = snapshot(out)
                self.assertNotEqual(self.run_cli(out, "--check").returncode, 0)
                self.assertEqual(snapshot(out), before)

    def test_M000_generation_rejects_unknown_existing_files_without_writes(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "proposal"
            out.mkdir()
            (out / "handwritten.txt").write_text("retain", encoding="utf-8")
            before = snapshot(out)
            self.assertNotEqual(self.run_cli(out).returncode, 0)
            self.assertEqual(snapshot(out), before)

    def test_M000_generation_does_not_overwrite_an_unowned_readme(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "handwritten"
            out.mkdir()
            (out / "README.md").write_text("User-owned instructions", encoding="utf-8")
            before = snapshot(out)
            self.assertNotEqual(self.run_cli(out).returncode, 0)
            self.assertEqual(snapshot(out), before)


if __name__ == "__main__":
    unittest.main()
