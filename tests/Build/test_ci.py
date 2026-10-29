import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "check.yml"


class ContinuousIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text(encoding="utf-8")

    def test_M002_matrix_covers_both_required_platforms_without_fail_fast(self):
        self.assertIn("ubuntu-24.04", self.text)
        self.assertIn("windows-2025", self.text)
        self.assertRegex(self.text, r"fail-fast:\s*false")
        self.assertIn("name: CI required", self.text)
        self.assertRegex(self.text, r"if:\s*\$\{\{\s*always\(\)\s*\}\}")
        self.assertIn("needs: check", self.text)
        self.assertIn('MATRIX_RESULT: ${{ needs.check.result }}', self.text)

    def test_M002_workflow_uses_pinned_official_actions_and_read_only_permissions(self):
        self.assertIn("actions/checkout@v6.1.0", self.text)
        self.assertIn("actions/setup-python@v7.1.0", self.text)
        self.assertIn("persist-credentials: false", self.text)
        self.assertRegex(self.text, r"permissions:\s*\n\s+contents:\s*read")
        self.assertNotRegex(self.text, r"secrets\.")
        self.assertNotIn("continue-on-error", self.text)

    def test_M002_runs_fetch_before_shared_offline_checks(self):
        fetch = self.text.index("cargo fetch --locked")
        checks = self.text.index("python scripts/check_all.py")
        self.assertLess(fetch, checks)
        self.assertIn("rustup toolchain install 1.95.0", self.text)
        self.assertIn("--component rustfmt", self.text)
        self.assertIn("--component clippy", self.text)

    def test_M104_windows_native_tools_are_prepared_before_checks(self):
        self.assertIn("if: runner.os == 'Windows'", self.text)
        self.assertLess(self.text.index("python scripts/prepare_native_tools.py"), self.text.index("python scripts/check_all.py"))


if __name__ == "__main__":
    unittest.main()
