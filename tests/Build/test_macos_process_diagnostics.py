from __future__ import annotations

import contextlib
from io import StringIO
from pathlib import Path
import importlib.util
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).parents[2]
SPEC = importlib.util.spec_from_file_location("macos_compat_diagnostics", ROOT / "scripts" / "macos_compat_diagnostics.py")
assert SPEC and SPEC.loader
diagnostics = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(diagnostics)


class MacProcessDiagnosticsTests(unittest.TestCase):
    def test_sec03_exact_image_and_target_containment(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory).resolve()
            cargo = target / "cargo"
            cargo.touch()
            test = target / "hantong_secrets-abc"
            test.touch()
            self.assertTrue(diagnostics._allowed_image(str(cargo), {"cargo": cargo}, None))
            self.assertFalse(diagnostics._allowed_image(str(target / "other"), {"cargo": cargo}, None))
            self.assertTrue(diagnostics._allowed_image(str(test), {}, target))
            self.assertFalse(diagnostics._allowed_image(str(target.parent / test.name), {}, target))

    def test_sec03_invalid_phase_is_fixed_and_non_sensitive(self):
        output = StringIO()
        with contextlib.redirect_stdout(output):
            diagnostics.diagnose_process_tree(1, {}, phase="private-value")
        self.assertEqual(output.getvalue().strip(), "HANTONG_PROCESS_DIAG result=invalid-phase")
        self.assertNotIn("private", output.getvalue())

    def test_sec03_pid_start_identity_replacement_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory).resolve()
            cli = target / "cli"
            test = target / "hantong_secrets-test"
            cli.touch()
            test.touch()
            rows = {10: (1, "start-a", str(cli)), 11: (10, "start-a", str(test))}
            replaced = {**rows, 10: (1, "start-b", str(cli))}
            output = StringIO()
            with mock.patch.object(diagnostics, "_snapshot", side_effect=[rows, replaced]), mock.patch.object(diagnostics.subprocess, "run") as run, contextlib.redirect_stdout(output):
                diagnostics.diagnose_process_tree(10, {"cli": cli}, phase="upgrade", test_target=target)
            self.assertIn("result=identity-changed", output.getvalue())

    def test_sec03_sample_classification_uses_stack_symbols_not_private_headers(self):
        self.assertEqual(diagnostics._classify_sample(b"Process: keychain-private-value\nPath: Security::private\n"), "unknown")
        sample = b"    8 SecKeychainItemCopyContent (in Security)\n    4 mach_msg2_trap (in libsystem_kernel.dylib)\n"
        self.assertEqual(diagnostics._classify_sample(sample), "keychain,wait")

    def test_sec03_cargo_samples_actual_test_not_compiler_or_cargo(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory).resolve()
            cargo, test, rustc = (target / name for name in ("cargo", "hantong_secrets-test", "rustc"))
            for image in (cargo, test, rustc):
                image.touch()
            rows = {10: (1, "start", str(cargo)), 11: (10, "start", str(test)), 12: (10, "start", str(rustc))}
            calls = []
            def sample(command, **kwargs):
                calls.append(command)
                kwargs["stdout"].write(b"    3 SecKeychainFindGenericPassword (in Security)\nprivate-value")
                return mock.Mock(returncode=0)
            output = StringIO()
            with mock.patch.object(diagnostics, "_snapshot", return_value=rows), mock.patch.object(diagnostics.Path, "is_file", return_value=True), mock.patch.object(diagnostics.subprocess, "run", side_effect=sample), contextlib.redirect_stdout(output):
                diagnostics.diagnose_process_tree(10, {"cargo": cargo}, phase="upgrade", test_target=target)
            self.assertEqual([command[1] for command in calls], ["11"])
            self.assertIn("role=test result=sampled wait=keychain", output.getvalue())
            self.assertNotIn("private-value", output.getvalue())
            self.assertNotIn(str(target), output.getvalue())

    def test_sec03_product_samples_cli_and_exact_core_only(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory).resolve()
            cli, core, other = (target / name for name in ("cli", "core", "unrelated"))
            for image in (cli, core, other):
                image.touch()
            rows = {10: (1, "start", str(cli)), 11: (10, "start", str(core)), 12: (10, "start", str(other))}
            output = StringIO()
            with mock.patch.object(diagnostics, "_snapshot", return_value=rows), mock.patch.object(diagnostics.Path, "is_file", return_value=True), mock.patch.object(diagnostics.subprocess, "run", return_value=mock.Mock(returncode=0)) as sample, contextlib.redirect_stdout(output):
                diagnostics.diagnose_process_tree(10, {"cli": cli, "core": core}, phase="candidate")
            self.assertEqual([call.args[0][1] for call in sample.call_args_list], ["10", "11"])
            self.assertIn("role=cli", output.getvalue())
            self.assertIn("role=core", output.getvalue())



if __name__ == "__main__":
    unittest.main()
