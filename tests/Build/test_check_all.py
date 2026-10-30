import os
import sys
import unittest
from contextlib import redirect_stdout, redirect_stderr
from io import StringIO
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import check_all


class CheckAllTests(unittest.TestCase):
    def setUp(self):
        environment = mock.patch.object(check_all,"build_environment",return_value={})
        environment.start()
        self.addCleanup(environment.stop)
        for stream in (redirect_stdout(StringIO()), redirect_stderr(StringIO())):
            stream.__enter__()
            self.addCleanup(stream.__exit__, None, None, None)

    def test_M001_build_runner_uses_repository_root_and_expected_steps(self):
        completed = []

        def fake_run(command, *, cwd, env, check, timeout):
            self.assertEqual(timeout, 1800 if command[:2] in (["cargo", "build"], ["cargo", "clippy"]) else 600)
            completed.append((command, cwd, env.get("HANTONG_CHECK_ALL"), check))
            return mock.Mock(returncode=0)

        with mock.patch.object(check_all, "command_exists", return_value=True), mock.patch.object(
            check_all.subprocess, "run", side_effect=fake_run
        ):
            self.assertEqual(check_all.run_checks(ROOT), 0)

        commands = [item[0] for item in completed]
        self.assertEqual(completed[0][1], ROOT)
        self.assertTrue(all(item[3] is False for item in completed))
        self.assertIn([sys.executable, "-X", "utf8", "-m", "unittest", "discover", "-s", "tests/Docs", "-q"], commands)
        self.assertIn([sys.executable, "-X", "utf8", "-m", "unittest", "discover", "-s", "tests/Build", "-q"], commands)
        self.assertIn([sys.executable, "-X", "utf8", "scripts/check_requirements.py"], commands)
        self.assertIn([sys.executable, "-X", "utf8", "scripts/prepare_requirements.py", "--check"], commands)
        self.assertIn([sys.executable, "-X", "utf8", "scripts/check_architecture.py"], commands)
        self.assertIn([sys.executable, "-X", "utf8", "scripts/traceability.py", "--check"], commands)
        self.assertIn(["cargo", "build", "--workspace", "--all-targets", "--locked", "--offline"], commands)
        self.assertIn(["cargo", "test", "--workspace", "--all-targets", "--locked", "--offline"], commands)
        self.assertIn(["cargo", "fmt", "--all", "--", "--check"], commands)
        self.assertIn(["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"], commands)

    def test_M001_build_runner_preserves_first_failure_code(self):
        calls = 0

        def fake_run(command, *, cwd, env, check, timeout):
            nonlocal calls
            calls += 1
            return mock.Mock(returncode=17 if calls == 2 else 0)

        with mock.patch.object(check_all, "command_exists", return_value=True), mock.patch.object(
            check_all.subprocess, "run", side_effect=fake_run
        ):
            self.assertEqual(check_all.run_checks(ROOT), 17)
        self.assertEqual(calls, 2)

    def test_M001_missing_cargo_fails_before_commands(self):
        with mock.patch.object(check_all, "command_exists", return_value=False), mock.patch.object(
            check_all.subprocess, "run"
        ) as run:
            self.assertNotEqual(check_all.run_checks(ROOT), 0)
        run.assert_not_called()

    def test_M001_timeout_stops_checks_with_failure(self):
        with mock.patch.object(check_all, "command_exists", return_value=True), mock.patch.object(
            check_all.subprocess, "run", side_effect=check_all.subprocess.TimeoutExpired("cargo", 600)
        ) as run:
            self.assertEqual(check_all.run_checks(ROOT), 124)
        self.assertEqual(run.call_count, 1)

    def test_M104_missing_native_tools_stops_before_commands(self):
        with mock.patch.object(check_all,"command_exists",return_value=True), mock.patch.object(check_all,"build_environment",side_effect=ValueError("private")), mock.patch.object(check_all.subprocess,"run") as run:
            self.assertEqual(check_all.run_checks(ROOT),2)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
