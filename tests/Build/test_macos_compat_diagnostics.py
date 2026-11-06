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

    def test_sec03_backend_boundaries_distinguish_fixed_slots_only(self):
        output = "\n".join([
            "HANTONG_COMPAT_MARKER phase=upgrade step=imap-old-selector",
            "HANTONG_COMPAT_MARKER phase=upgrade step=imap-old-backend",
            "HANTONG_COMPAT_MARKER phase=upgrade step=smtp-old-returned",
            "HANTONG_COMPAT_MARKER phase=upgrade step=private-selector",
            "HANTONG_COMPAT_MARKER phase=upgrade step=imap-old-backend secret",
        ])
        self.assertEqual(compat._markers(output), "upgrade:imap-old-selector,upgrade:imap-old-backend,upgrade:smtp-old-returned")

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
                mock.patch.object(compat, "_kill_process_group") as kill, \
                mock.patch.object(compat.subprocess, "run", return_value=mock.Mock(stdout="/toolchain/cargo")), \
                mock.patch.object(compat, "diagnose_process_tree") as diagnose:
            with self.assertRaises(RuntimeError) as failure:
                compat.run_test(Path("old"), Path("target"), Path("root"), "upgrade", keychain)
        kill.assert_called_once_with(process)
        diagnose.assert_called_once_with(process.pid, {"cargo": Path("/toolchain/cargo")}, phase="upgrade", test_target=Path("target"))
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

    def test_sec03_probe_markers_are_finite_and_non_sensitive(self):
        output = "\n".join([
            "HANTONG_KEYCHAIN_PROBE stage=read category=interaction_not_allowed",
            "HANTONG_KEYCHAIN_PROBE stage=private category=ok",
            "HANTONG_KEYCHAIN_PROBE stage=read category=ok private-value",
        ])
        self.assertEqual(compat._probe_markers(output), "read:interaction_not_allowed")

    def test_sec03_successful_probe_does_not_replace_failed_upgrade(self):
        calls = []
        def phase(*args):
            calls.append(args[3])
            if args[3] == "upgrade":
                raise RuntimeError("original upgrade failed")
        args = (Path("old"), Path("new"), Path("old-target"), Path("new-target"), Path("root"), Path("keychain"))
        with mock.patch.object(compat, "run_test", side_effect=phase), mock.patch.object(compat, "run_probe") as probe:
            with self.assertRaisesRegex(RuntimeError, "original upgrade failed"):
                compat.run_phases(*args)
        self.assertEqual(calls, ["seed", "upgrade"])
        probe.assert_called_once()

    def test_sec03_failed_probe_keeps_all_original_phases_and_fails_overall(self):
        args = (Path("old"), Path("new"), Path("old-target"), Path("new-target"), Path("root"), Path("keychain"))
        with mock.patch.object(compat, "run_test") as phase, mock.patch.object(compat, "run_probe", side_effect=RuntimeError("probe failed")):
            with self.assertRaisesRegex(RuntimeError, "probe failed"):
                compat.run_phases(*args)
        self.assertEqual([call.args[3] for call in phase.call_args_list], ["seed", "upgrade", "reopen"])

    def test_sec03_probe_rechecks_baseline_even_after_failure(self):
        keychain = Path("keychain")
        with mock.patch.object(compat, "run_test", side_effect=RuntimeError("probe failed")), mock.patch.object(compat, "_baseline", return_value=(Path("changed"), ())):
            with self.assertRaisesRegex(RuntimeError, "baseline changed"):
                compat.run_probe(Path("source"), Path("target"), Path("root"), keychain)

    def test_sec03_probe_requires_read_and_restoration_markers(self):
        keychain = Path("keychain")
        process = mock.Mock(returncode=0)
        process.communicate.return_value = ("running 1 test\nHANTONG_KEYCHAIN_PROBE stage=guard category=ready\nmac_keychain_read_probe ... ok", None)
        with mock.patch.object(compat, "_baseline", return_value=(keychain, (keychain,))), mock.patch.object(compat.subprocess, "Popen", return_value=process):
            with self.assertRaisesRegex(RuntimeError, "complete classified result"):
                compat.run_test(Path("source"), Path("target"), Path("root"), "probe", keychain)


    def test_sec03_classified_probe_uses_exact_probe_target_without_secret_output(self):
        keychain = Path("keychain")
        process = mock.Mock(returncode=0)
        process.communicate.return_value = ("running 1 test\n" + "\n".join([
            "HANTONG_KEYCHAIN_PROBE stage=guard category=ready",
            "HANTONG_KEYCHAIN_PROBE stage=store_open category=ready",
            "HANTONG_KEYCHAIN_PROBE stage=read category=interaction_not_allowed",
            "HANTONG_KEYCHAIN_PROBE stage=restore category=ok",
            "test macos_keychain_probe::mac_keychain_read_probe ... ok",
            "private-value",
        ]), None)
        from io import StringIO
        import contextlib
        output = StringIO()
        with mock.patch.object(compat, "_baseline", return_value=(keychain, (keychain,))), mock.patch.object(compat.subprocess, "Popen", return_value=process) as start, contextlib.redirect_stdout(output):
            compat.run_probe(Path("source"), Path("target"), Path("root"), keychain)
        self.assertIn(compat.PROBE_NAME, start.call_args.args[0])
        self.assertIn("read:interaction_not_allowed", output.getvalue())
        self.assertNotIn("private-value", output.getvalue())



if __name__ == "__main__":
    unittest.main()
