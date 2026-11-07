from __future__ import annotations

import contextlib
import importlib
from io import StringIO
from pathlib import Path
import sys
import types
import unittest
from unittest import mock

SCRIPTS = Path(__file__).parents[2] / "scripts"
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]), mock.patch.dict(
    sys.modules, {"pwd": mock.Mock()} if sys.platform == "win32" else {}
):
    smoke = importlib.import_module("test_macos_installed_product")
    compat = importlib.import_module("test_macos_cross_version_product")


class MacProductStartupDiagnosticsTests(unittest.TestCase):
    def run_smoke(self, process, times, output):
        fake_pty = types.SimpleNamespace(openpty=lambda: (100, 101))
        with mock.patch.dict(sys.modules, {"pty": fake_pty}), \
                mock.patch.object(smoke.subprocess, "Popen", return_value=process), \
                mock.patch.object(smoke.time, "monotonic", side_effect=times), \
                mock.patch("select.select", return_value=([100], [], [])), \
                mock.patch.object(smoke.os, "read", return_value=output), \
                mock.patch.object(smoke.os, "write") as write, \
                mock.patch.object(smoke.os, "close") as close:
            try:
                smoke.run_cli(Path("synthetic-cli"), {})
            finally:
                self.assertEqual(close.call_args_list, [mock.call(101), mock.call(100)])
            return write.call_args_list

    def test_sec03_timeout_keeps_45_second_bound_reaps_and_never_exposes_output(self):
        process = mock.Mock(returncode=None)
        process.poll.return_value = None
        with self.assertRaises(smoke.CliStartupFailure) as caught:
            self.run_smoke(process, [0, 1, 46, 46], b"PRIVATE_SYNTHETIC_VALUE")
        self.assertEqual(caught.exception.kind, "menu-timeout")
        self.assertNotIn("PRIVATE", str(caught.exception))
        process.terminate.assert_called_once_with()
        process.wait.assert_called_once_with(timeout=5)

    def test_sec03_process_exit_without_menu_still_fails(self):
        process = mock.Mock(returncode=2)
        process.poll.return_value = 2
        with self.assertRaises(smoke.CliStartupFailure) as caught:
            self.run_smoke(process, [0], b"")
        self.assertEqual(caught.exception.kind, "menu-process-exit")
        process.terminate.assert_not_called()

    def test_sec03_menu_exit_success_requires_exact_exit_prompt_and_zero_exit(self):
        process = mock.Mock(returncode=0)
        process.poll.side_effect = [None, 0, 0, 0]
        self.assertEqual(self.run_smoke(process, [0, 1], b"1) Connect  2) Exit"), [mock.call(100, b"2\n")])
        process.terminate.assert_not_called()

    def test_sec03_output_bound_remains_fail_closed(self):
        process = mock.Mock(returncode=None)
        process.poll.return_value = None
        with self.assertRaises(smoke.CliStartupFailure) as caught:
            self.run_smoke(process, [0, 1], b"x" * (256 * 1024 + 1))
        self.assertEqual(caught.exception.kind, "output-limit")
        process.terminate.assert_called_once_with()

    def test_sec03_cross_version_logs_only_fixed_phase_and_classification(self):
        for kind in ("output-limit", "menu-timeout", "menu-process-exit", "exit-timeout", "exit-failed"):
            error = smoke.CliStartupFailure(kind)
            output = StringIO()
            with mock.patch.object(compat, "run_cli", side_effect=error), contextlib.redirect_stdout(output):
                with self.assertRaises(smoke.CliStartupFailure) as caught:
                    compat.run_cli_with_diagnostics(Path("PRIVATE-cli"), Path("PRIVATE-core"), {}, "old")
            self.assertIs(caught.exception, error)
            self.assertEqual(output.getvalue(), f"HANTONG_PRODUCT_MARKER phase=old step=cli-{kind}\n")

    def test_sec03_os_error_and_unknown_diagnostic_cannot_leak_values(self):
        output = StringIO()
        with mock.patch.object(compat, "run_cli", side_effect=OSError("PRIVATE_VALUE")), contextlib.redirect_stdout(output):
            with self.assertRaises(OSError):
                compat.run_cli_with_diagnostics(Path("cli"), Path("core"), {}, "candidate")
        self.assertEqual(output.getvalue(), "HANTONG_PRODUCT_MARKER phase=candidate step=cli-os-error\n")
        with self.assertRaisesRegex(ValueError, "unknown CLI startup classification"):
            smoke.CliStartupFailure("PRIVATE_VALUE")
        with mock.patch.object(compat, "run_cli") as run:
            with self.assertRaisesRegex(RuntimeError, "invalid CLI startup phase"):
                compat.run_cli_with_diagnostics(Path("cli"), Path("core"), {}, "PRIVATE_VALUE")
        run.assert_not_called()

    def test_sec03_finished_marker_does_not_change_success_or_observer_binding(self):
        output = StringIO()
        with mock.patch.object(compat, "run_cli") as run, contextlib.redirect_stdout(output):
            compat.run_cli_with_diagnostics(Path("cli"), Path("core"), {}, "candidate")
        self.assertEqual(output.getvalue(), "HANTONG_PRODUCT_MARKER phase=candidate step=cli-finished\n")
        with mock.patch.object(compat, "diagnose_process_tree") as diagnose:
            run.call_args.kwargs["observer"](123)
        diagnose.assert_called_once_with(123, {"cli": Path("cli"), "core": Path("core")}, phase="candidate")


if __name__ == "__main__":
    unittest.main()
