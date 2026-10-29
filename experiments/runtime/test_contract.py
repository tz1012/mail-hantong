"""OPS-01 experiment behavior checks, not product acceptance tests."""

from pathlib import Path
import os
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
PROGRAMS = {
    "rust": Path(os.environ.get("RUNTIME_RUST_EXE", ROOT / "rust/target/release/hantong-runtime-rust.exe")),
    "dotnet": Path(os.environ.get("RUNTIME_DOTNET_EXE", ROOT / "dotnet/bin/publish/hantong-runtime-dotnet.exe")),
}
ROWS = (
    "7\t1\t300\t0\t회의 invoice\ta@example.invalid\talpha\n"
    "3\t1\t100\t1\tInvoice\tb@example.invalid\tplain\n"
    "9\t2\t900\t0\tinvoice\tc@example.invalid\t회의\n"
    "2\t1\t300\t1\tnotes\td@example.invalid\t회의\n"
    "6\t5\t600\t0\tother\te@example.invalid\ttext\n"
    "5\t1\t200\t1\tinvoice\tf@example.invalid\tdaily\n"
)


class OPS01RuntimeContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.cache = Path(self.temp.name) / "cache.tsv"
        self.body = Path(self.temp.name) / "body.txt"
        self.cache.write_bytes(ROWS.encode("utf-8"))
        self.body.write_bytes("Hello, 한통!\n".encode("utf-8"))

    def invoke(self, executable, commands=None, cache=None):
        self.assertTrue(executable.is_file(), f"Prototype not built: {executable}")
        args = [str(executable), "--ping"] if commands is None else [str(executable), "--serve", str(cache or self.cache), str(self.body)]
        return subprocess.run(args, input=commands, capture_output=True, encoding="utf-8", timeout=10)

    def check_commands(self, commands, expected):
        for name, executable in PROGRAMS.items():
            with self.subTest(runtime=name):
                result = self.invoke(executable, commands)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                self.assertEqual(result.stdout.splitlines(), ["READY\t6", *expected])

    def test_OPS01_ping_without_cache(self):
        for name, executable in PROGRAMS.items():
            with self.subTest(runtime=name):
                result = self.invoke(executable)
                self.assertEqual((result.returncode, result.stdout.strip(), result.stderr), (0, "PONG", ""))

    def test_OPS01_account_scoped_order_and_empty_account_results(self):
        self.check_commands("LIST\t1\nLIST\t2\nLIST\t5\nLIST\t3\nQUIT\n", ["LIST\t4\t7,2,5,3", "LIST\t1\t9", "LIST\t1\t6", "LIST\t0\t", "BYE"])

    def test_OPS01_unicode_case_sensitive_search_and_missing_query(self):
        self.check_commands("SEARCH\t1\tinvoice\nSEARCH\t1\t회의\nSEARCH\t1\tInvoice\nSEARCH\t1\tmissing\nQUIT\n", ["SEARCH\t2\t12", "SEARCH\t2\t9", "SEARCH\t1\t3", "SEARCH\t0\t0", "BYE"])

    def test_OPS01_body_bytes_and_repeated_requests(self):
        data = self.body.read_bytes()
        response = f"BODY\t{len(data)}\t{sum(data)}"
        self.check_commands("BODY\nBODY\nPING\nQUIT\n", [response, response, "PONG", "BYE"])

    def test_OPS01_invalid_commands_do_not_expand_account_scope(self):
        commands = ["LIST\t", "LIST\t0", "LIST\t6", "LIST\t-1", "LIST\t01", "LIST\t 1", "LIST\t+1", "SEARCH\t\tinvoice", "SEARCH\t1\t", "UNKNOWN", "LIST\t1\textra"]
        self.check_commands("\n".join(commands) + "\nPING\nQUIT\n", [*["ERR"] * len(commands), "PONG", "BYE"])

    def test_OPS01_malformed_cache_is_rejected_without_content_leak(self):
        self.cache.write_text("SECRET_SYNTHETIC_SENTINEL\n", encoding="utf-8")
        for name, executable in PROGRAMS.items():
            with self.subTest(runtime=name):
                result = self.invoke(executable, "QUIT\n")
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("READY", result.stdout)
                self.assertNotIn("SECRET_SYNTHETIC_SENTINEL", result.stdout + result.stderr)

    def test_OPS01_list_limits_to_100_matching_records(self):
        self.cache.write_bytes("".join(f"{i}\t1\t{i}\t0\tsubject\ta@example.invalid\tsnippet\n" for i in range(1, 206)).encode("utf-8"))
        expected = "LIST\t100\t" + ",".join(str(i) for i in range(205, 105, -1))
        for name, executable in PROGRAMS.items():
            with self.subTest(runtime=name):
                result = self.invoke(executable, "LIST\t1\nQUIT\n")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines(), ["READY\t205", expected, "BYE"])


if __name__ == "__main__":
    unittest.main()
