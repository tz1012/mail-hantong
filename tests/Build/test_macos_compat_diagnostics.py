from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest
from unittest import mock


SCRIPTS = Path(__file__).parents[2] / "scripts"
SPEC = importlib.util.spec_from_file_location("macos_compat_diagnostics", SCRIPTS / "test_macos_keychain_upgrade.py")
assert SPEC and SPEC.loader
compat = importlib.util.module_from_spec(SPEC)
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    SPEC.loader.exec_module(compat)


class MacCompatibilityDiagnosticsTests(unittest.TestCase):
    def test_sec03_only_fixed_markers_are_reported(self):
        output = "\n".join([
            "synthetic-private-value-must-not-appear",
            "HANTONG_COMPAT_MARKER phase=upgrade step=read-old",
            "HANTONG_COMPAT_MARKER phase=upgrade step=write-new",
            "HANTONG_COMPAT_MARKER phase=upgrade step=done",
        ])
        diagnostic = compat._markers(output)
        self.assertIn("upgrade:read-old", diagnostic)
        self.assertIn("upgrade:write-new", diagnostic)
        self.assertIn("upgrade:done", diagnostic)
        self.assertNotIn("synthetic-private", diagnostic)

    def test_sec03_rejects_unknown_and_partial_marker_matches(self):
        output = "\n".join([
            "HANTONG_COMPAT_MARKER phase=unknown step=done",
            "HANTONG_COMPAT_MARKER phase=upgrade step=private-value",
            "HANTONG_COMPAT_MARKER phase=upgrade step=read-old-private-value",
            "HANTONG_COMPAT_MARKER phase=upgrade step=done private-value",
        ])
        self.assertEqual(compat._markers(output), "none")

    def test_sec03_diagnostic_output_is_bounded(self):
        output = "HANTONG_COMPAT_MARKER phase=upgrade step=read-old\n" * 200
        self.assertLessEqual(len(compat._markers(output)), 512)

    def test_sec03_timeout_reaps_group_and_preserves_only_fixed_evidence(self):
        keychain = Path("synthetic-keychain")
        process = mock.Mock()
        process.communicate.side_effect = [
            subprocess.TimeoutExpired("cargo", 900),
            ("running 1 test\nHANTONG_COMPAT_MARKER phase=upgrade step=read-old\nprivate-value", None),
        ]
        with mock.patch.object(compat, "_baseline", return_value=(keychain, (keychain,))), \
                mock.patch.object(compat.subprocess, "Popen", return_value=process), \
                mock.patch.object(compat, "_kill_process_group") as kill:
            with self.assertRaises(RuntimeError) as failure:
                compat.run_test(Path("old"), Path("target"), Path("root"), "upgrade", keychain)
        kill.assert_called_once_with(process)
        self.assertIn("upgrade:read-old", str(failure.exception))
        self.assertNotIn("private-value", str(failure.exception))
        process.communicate.assert_any_call(timeout=900)

    def test_sec03_zero_matching_tests_cannot_pass_the_gate(self):
        keychain = Path("synthetic-keychain")
        process = mock.Mock(returncode=0)
        process.communicate.return_value = ("running 0 tests\ntest result: ok", None)
        with mock.patch.object(compat, "_baseline", return_value=(keychain, (keychain,))), \
                mock.patch.object(compat.subprocess, "Popen", return_value=process):
            with self.assertRaisesRegex(RuntimeError, "exactly one"):
                compat.run_test(Path("old"), Path("target"), Path("root"), "upgrade", keychain)


if __name__ == "__main__":
    unittest.main()
