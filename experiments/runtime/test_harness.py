"""Independent checks for experiment data and statistics, not product tests."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

MODULE = Path(__file__).with_name("benchmark.py")


class OPS01HarnessTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(MODULE.is_file(), "Benchmark harness has not been implemented")
        spec = importlib.util.spec_from_file_location("runtime_benchmark", MODULE)
        self.bench = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.bench)

    def test_OPS01_nearest_rank_p95_and_median(self):
        summary = self.bench.summarize(list(range(1, 21)))
        self.assertEqual(summary["p95"], 19)
        self.assertEqual(summary["median"], 10.5)
        self.assertEqual(summary["samples"], 20)
        with self.assertRaises(ValueError):
            self.bench.summarize([])

    def test_OPS01_fixture_is_deterministic_and_covers_five_accounts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = self.bench.generate_fixture(root / "first", 100)
            second = self.bench.generate_fixture(root / "second", 100)
            self.assertEqual(first["cache_sha256"], second["cache_sha256"])
            self.assertEqual(first["body_sha256"], second["body_sha256"])
            lines = (root / "first/cache.tsv").read_text(encoding="utf-8").splitlines()
            fields = [line.split("\t") for line in lines]
            self.assertEqual(len(fields), 100)
            self.assertEqual({int(row[1]) for row in fields}, {1, 2, 3, 4, 5})
            self.assertEqual({int(row[0]) for row in fields}, set(range(1, 101)))
            self.assertNotEqual([int(row[0]) for row in fields], list(range(100, 0, -1)))
            body = (root / "first/body.txt").read_bytes()
            self.assertEqual(len(body), 128 * 1024)
            body.decode("utf-8")

    def test_OPS01_oracle_orders_ties_and_scopes_unicode_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "cache.tsv").write_text("2\t1\t10\t0\t회의\ta@example.invalid\tx\n9\t2\t20\t0\t회의\tb@example.invalid\tx\n3\t1\t10\t0\tHi\tc@example.invalid\tx\n", encoding="utf-8")
            (root / "body.txt").write_bytes(b"AB")
            expected = self.bench.make_oracle(root)
            self.assertEqual(expected("LIST\t1"), "LIST\t2\t3,2")
            self.assertEqual(expected("SEARCH\t1\t회의"), "SEARCH\t1\t2")
            self.assertEqual(expected("SEARCH\t1\thi"), "SEARCH\t0\t0")
            self.assertEqual(expected("BODY"), "BODY\t2\t131")


if __name__ == "__main__":
    unittest.main()
