from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import stat
import signal
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).parents[2] / "scripts" / "run_macos_test.py"
SPEC = importlib.util.spec_from_file_location("run_macos_test", SCRIPT)
assert SPEC and SPEC.loader
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class FakeProcess:
    def __init__(self, code: int, args: list[str], env: dict[str, str], timeout: bool = False):
        self.returncode = code
        self.args = args
        self.env = env
        self.pid = 4321
        self.timeout = timeout

    def poll(self):
        return self.returncode

    def wait(self, timeout=None):
        if self.timeout and timeout == runner.CHILD_TIMEOUT:
            raise subprocess.TimeoutExpired(self.args, timeout)
        return self.returncode


class MacRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name).resolve(strict=True)
        self.runner_temp = root / "runner"
        self.workspace = root / "workspace"
        (self.workspace / "target" / "debug").mkdir(parents=True)
        self.runner_temp.mkdir()
        (self.runner_temp / "hantong-ci.keychain-db").touch()
        self.child = self.workspace / "target" / "debug" / "test-bin"
        self.child.write_bytes(b"synthetic")
        self.env = {
            "CI": "true", "GITHUB_ACTIONS": "true",
            "RUNNER_TEMP": str(self.runner_temp),
            "GITHUB_WORKSPACE": str(self.workspace),
            "HANTONG_TEST_KEYCHAIN": str(self.runner_temp / "hantong-ci.keychain-db"),
        }

    def tearDown(self):
        self.temp.cleanup()

    def _run(self, child_code=0, *, fail_cleanup=False, fail_restore=False, timeout=False, interrupt_setup=False, env=None, child=None):
        state = {"default": self.runner_temp / "hantong-ci.keychain-db", "list": [self.runner_temp / "hantong-ci.keychain-db"]}
        for path in state["list"]:
            path.touch()
        calls = []
        restore_count = [0]
        installed_handlers = {}
        signal_calls = []

        def security(args, **kwargs):
            calls.append(args)
            command = args[0]
            if command == "default-keychain" and "-s" not in args:
                return subprocess.CompletedProcess(args, 0, f'"{state["default"]}"\n', "")
            if command == "list-keychains" and "-s" not in args:
                return subprocess.CompletedProcess(args, 0, "".join(f'"{p}"\n' for p in state["list"]), "")
            if command == "create-keychain":
                Path(args[-1]).touch()
            elif command == "default-keychain" and "-s" in args:
                restore_count[0] += 1
                if restore_count[0] == 1:
                    state["default"] = Path(args[-1])
                if interrupt_setup and restore_count[0] == 1:
                    installed_handlers[runner.signal.SIGTERM](runner.signal.SIGTERM, None)
                if fail_restore and restore_count[0] == 2:
                    raise subprocess.CalledProcessError(1, args)
                state["default"] = Path(args[-1])
            elif command == "list-keychains" and "-s" in args:
                state["list"] = [Path(item) for item in args[args.index("-s") + 1:]]
            elif command == "delete-keychain":
                if fail_cleanup:
                    raise subprocess.CalledProcessError(1, args)
                Path(args[-1]).unlink()
            return subprocess.CompletedProcess(args, 0, "", "")

        def popen(args, **kwargs):
            process = FakeProcess(child_code, args, kwargs["env"], timeout=timeout)
            self.process = process
            return process

        selected = str(child or self.child)
        fake_fcntl = type("FakeFcntl", (), {"LOCK_EX": 2, "flock": staticmethod(lambda fd, mode: None)})
        def install_handler(signum, handler):
            signal_calls.append((signum, handler))
            self.signal_calls = signal_calls
            installed_handlers[signum] = handler
            return signal.SIG_DFL
        with mock.patch.dict(os.environ, self.env if env is None else env, clear=True), \
             mock.patch.dict(sys.modules, {"fcntl": fake_fcntl}), \
             mock.patch.object(runner, "_validate_lock"), \
             mock.patch.object(runner.os, "killpg", create=True), \
             mock.patch.object(runner.signal, "signal", side_effect=install_handler), \
             mock.patch.object(runner.platform, "system", return_value="Darwin"), \
             mock.patch.object(runner, "_security", side_effect=security), \
             mock.patch.object(runner.subprocess, "Popen", side_effect=popen):
            result = runner.main([selected, "--exact", "test_name"])
        return result, calls

    def test_refuses_outside_ci_before_security_mutation(self):
        env = dict(self.env, CI="false")
        with mock.patch.object(runner, "_security") as security, \
             mock.patch.dict(os.environ, env, clear=True), \
             mock.patch.object(runner.platform, "system", return_value="Darwin"):
            self.assertEqual(runner.main([str(self.child)]), 2)
        security.assert_not_called()

    def test_refuses_outside_target_before_mutation(self):
        outside = self.runner_temp / "outside"
        outside.touch()
        result, calls = self._run(child=outside)
        self.assertEqual(result, 2)
        self.assertEqual(calls, [])

    def test_refuses_symlinked_test_executable(self):
        linked = self.workspace / "target" / "debug" / "linked-test-bin"
        linked.write_bytes(b"synthetic")
        original = Path.is_symlink
        def fake_is_symlink(path):
            return path == linked or original(path)
        with mock.patch.object(Path, "is_symlink", fake_is_symlink):
            result, calls = self._run(child=linked)
        self.assertEqual(result, 2)
        self.assertEqual(calls, [])

    def test_lock_validation_uses_held_fd_metadata(self):
        bad = type("BadStat", (), {"st_mode": stat.S_IFREG | 0o644, "st_uid": 999})()
        with mock.patch.object(runner.os, "fstat", return_value=bad), mock.patch.object(runner, "_uid", return_value=123):
            with self.assertRaises(runner.RunnerError):
                runner._validate_lock(17)

    def test_lock_validation_rejects_nonregular_fd(self):
        bad = type("BadStat", (), {"st_mode": stat.S_IFDIR | 0o600, "st_uid": 123})()
        with mock.patch.object(runner.os, "fstat", return_value=bad), mock.patch.object(runner, "_uid", return_value=123):
            with self.assertRaises(runner.RunnerError):
                runner._validate_lock(17)

    def test_process_group_already_gone_does_not_fail(self):
        process = FakeProcess(7, ["test"], {})
        process.terminate = mock.Mock(side_effect=AssertionError("must not terminate exited leader"))
        with mock.patch.object(runner.os, "killpg", side_effect=ProcessLookupError, create=True):
            runner._terminate(process)

    def test_refuses_bad_baseline_before_keychain_mutation(self):
        state = {"default": self.runner_temp / "wrong.keychain-db", "list": [self.runner_temp / "wrong.keychain-db"]}
        state["default"].touch()
        def bad_security(args, **kwargs):
            if args[0] == "default-keychain":
                return subprocess.CompletedProcess(args, 0, f'"{state["default"]}"\n', "")
            return subprocess.CompletedProcess(args, 0, f'"{state["default"]}"\n', "")
        with mock.patch.object(runner, "_security", side_effect=bad_security) as security, \
             mock.patch.dict(os.environ, self.env, clear=True), \
             mock.patch.object(runner.platform, "system", return_value="Darwin"), \
             mock.patch.dict(sys.modules, {"fcntl": type("FakeFcntl", (), {"LOCK_EX": 2, "flock": staticmethod(lambda fd, mode: None)})}), \
             mock.patch.object(runner, "_validate_lock"):
            self.assertEqual(runner.main([str(self.child)]), 2)
        self.assertFalse(any(call.args[0][0] == "create-keychain" for call in security.call_args_list))

    def test_success_preserves_argv_and_restores_baseline(self):
        result, calls = self._run()
        self.assertEqual(result, 0)
        self.assertEqual(self.process.args[1:], ["--exact", "test_name"])
        self.assertEqual(self.process.env["HANTONG_TEST_KEYCHAIN"].startswith(str(self.runner_temp)), True)
        self.assertEqual(calls[-1][0], "delete-keychain")

    def test_child_nonzero_is_preserved_and_cleanup_runs(self):
        result, calls = self._run(child_code=7)
        self.assertEqual(result, 7)
        self.assertEqual(calls[-1][0], "delete-keychain")

    def test_cleanup_failure_fails_runner(self):
        result, _ = self._run(fail_cleanup=True)
        self.assertEqual(result, 2)
        self.assertGreaterEqual(len(self.signal_calls), 5)
        self.assertNotEqual(self.signal_calls[-1][1], signal.SIG_IGN)

    def test_restore_failure_fails_and_retains_temporary_keychain(self):
        result, calls = self._run(fail_restore=True)
        self.assertEqual(result, 2)
        own = Path(next(call[-1] for call in calls if call[0] == "create-keychain"))
        self.assertTrue(own.exists())

    def test_two_binaries_receive_distinct_keychains(self):
        second = self.workspace / "target" / "debug" / "other-test-bin"
        second.write_bytes(b"synthetic")
        first_result, first_calls = self._run()
        first_own = next(call[-1] for call in first_calls if call[0] == "create-keychain")
        second_result, second_calls = self._run(child=second)
        second_own = next(call[-1] for call in second_calls if call[0] == "create-keychain")
        self.assertEqual((first_result, second_result), (0, 0))
        self.assertNotEqual(first_own, second_own)

    def test_timeout_terminates_child_and_fails(self):
        result, _ = self._run(timeout=True)
        self.assertEqual(result, 2)

    def test_interrupt_during_keychain_setup_restores_and_cleans_own_keychain(self):
        result, calls = self._run(interrupt_setup=True)
        self.assertEqual(result, 2)
        self.assertEqual([call[0] for call in calls].count("delete-keychain"), 1)
        self.assertGreaterEqual([call[0] for call in calls].count("default-keychain"), 2)


if __name__ == "__main__":
    unittest.main()
