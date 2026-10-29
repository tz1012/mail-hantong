import hashlib
import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CORPUS = ROOT / "tests" / "Corpus"


class CorpusQualityTests(unittest.TestCase):
    def load_index(self):
        with (CORPUS / "index.json").open(encoding="utf-8") as handle:
            return json.load(handle)

    def test_M009_has_twenty_distinct_untrusted_injection_messages(self):
        index = self.load_index()
        entries = index["injection_messages"]
        self.assertEqual(len(entries), 20)
        self.assertEqual(len({entry["id"] for entry in entries}), 20)
        vectors = {vector for entry in entries for vector in entry["vectors"]}
        required = {
            "approval_claim", "secret_request", "cross_account_request", "subject_instruction",
            "quoted_instruction", "html_instruction", "attachment_instruction", "imported_message",
        }
        self.assertTrue(required <= vectors)
        for entry in entries:
            path = (CORPUS / entry["file"]).resolve()
            self.assertTrue(path.is_file())
            self.assertTrue(path.is_relative_to(CORPUS.resolve()))
            raw = path.read_bytes()
            self.assertEqual(hashlib.sha256(raw).hexdigest(), entry["sha256"])
            self.assertIn("X-Hantong-Fixture: UNTRUSTED-DATA", raw.decode("utf-8"))
            self.assertNotRegex(raw.decode("utf-8"), r"@(gmail|outlook|naver|daum)\.(com|net|co\.kr)")

    def test_M009_preserves_euc_kr_fixture_without_overwriting_source(self):
        index = self.load_index()
        path = CORPUS / index["euc_kr"]["file"]
        raw = path.read_bytes()
        self.assertIn("제목".encode("euc-kr"), raw)
        decoded = raw.decode("euc-kr")
        self.assertIn("한통 테스트", decoded)
        self.assertEqual(hashlib.sha256(raw).hexdigest(), index["euc_kr"]["sha256"])

    def test_M009_damaged_mbox_is_bounded_and_explicitly_malformed(self):
        index = self.load_index()
        path = CORPUS / index["damaged_mbox"]["file"]
        raw = path.read_bytes()
        self.assertLess(len(raw), 64 * 1024)
        self.assertIn(b"From ", raw)
        self.assertIn(b"X-Hantong-Malformed: missing-body-separator", raw)
        self.assertEqual(hashlib.sha256(raw).hexdigest(), index["damaged_mbox"]["sha256"])


if __name__ == "__main__":
    unittest.main()
