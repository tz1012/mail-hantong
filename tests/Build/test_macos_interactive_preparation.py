from __future__ import annotations

import contextlib
from dataclasses import replace
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

SCRIPTS = Path(__file__).parents[2] / "scripts"
SPEC = importlib.util.spec_from_file_location("macos_interactive_preparation", SCRIPTS / "prepare_macos_interactive_compat.py")
assert SPEC and SPEC.loader
prep = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = prep
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    SPEC.loader.exec_module(prep)

# Simulated Darwin paths must have an absolute anchor on the test host too.
if os.name == "nt":
    prep.HOME = Path("C:/Users") / prep.USER

RUN = "a" * 32
COMMIT = "b" * 40
SHA = "c" * 64
ROOT = prep.HOME / "HantongCompat" / RUN
KC = ROOT / "synthetic.keychain-db"


def session():
    return prep.Session("Darwin", "macos-aarch64", 501, 501, (20, 12), 80,
                        prep.USER, prep.HOME, 501, True, True, False, False, True)


def info(mode=stat.S_IFDIR | 0o700, **changes):
    values = dict(st_mode=mode, st_uid=501, st_gid=20, st_dev=1, st_ino=2,
                  st_nlink=1, st_size=3, st_mtime_ns=4, st_ctime_ns=5)
    values.update(changes)
    return SimpleNamespace(**values)


class SessionTests(unittest.TestCase):
    def collect(self, *, extra_env=None, machine="arm64", gui=0):
        account = SimpleNamespace(pw_name=prep.USER, pw_dir=str(prep.HOME))
        environment = {"HOME": "/misleading", "USER": "not-the-os-user", **(extra_env or {})}
        with mock.patch.dict(sys.modules, {
            "pwd": SimpleNamespace(getpwuid=lambda uid: account),
            "grp": SimpleNamespace(getgrnam=lambda name: SimpleNamespace(gr_gid=80)),
        }), mock.patch.dict(prep.os.environ, environment, clear=True), \
             mock.patch.object(prep.os, "getuid", return_value=501, create=True), \
             mock.patch.object(prep.os, "geteuid", return_value=501, create=True), \
             mock.patch.object(prep.os, "getgid", return_value=20, create=True), \
             mock.patch.object(prep.os, "getegid", return_value=20, create=True), \
             mock.patch.object(prep.os, "getgroups", return_value=[20, 12], create=True), \
             mock.patch.object(prep.os, "stat", return_value=info()), \
             mock.patch.object(prep.platform, "system", return_value="Darwin"), \
             mock.patch.object(prep, "native_architecture", return_value="macos-aarch64" if machine == "arm64" else "macos-x86_64"), \
             mock.patch.object(prep, "command", return_value="20 12"), \
             mock.patch.object(prep.subprocess, "run", return_value=SimpleNamespace(returncode=gui)) as run, \
             mock.patch.object(prep.sys, "stdin", SimpleNamespace(isatty=lambda: True)), \
             mock.patch.object(prep.sys, "stdout", SimpleNamespace(isatty=lambda: True)):
            result = prep.collect_session()
            self.assertEqual(dict(prep.os.environ), environment)
            self.assertEqual(run.call_args.args[0], ["/bin/launchctl", "print", "gui/501"])
            return result

    def test_sec03_os_account_wins_over_home_and_user_environment(self):
        for machine in ("arm64", "x86_64"):
            result = self.collect(machine=machine)
            prep.validate_session(result)
            self.assertEqual(result.home, prep.HOME)
            self.assertEqual(result.username, prep.USER)

    def test_sec03_ci_and_cargo_runner_environment_is_rejected_even_if_empty(self):
        for name in ("CI", "GITHUB_ACTIONS", "HANTONG_TEST_KEYCHAIN", "HANTONG_COMPAT_ROOT",
                     "HANTONG_COMPAT_PHASE", "RUNNER_TEMP", "GITHUB_WORKSPACE",
                     "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER", "CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER"):
            with self.subTest(name=name), self.assertRaisesRegex(prep.IsolationError, "separate_lane"):
                prep.validate_session(self.collect(extra_env={name: ""}))

    def test_sec03_missing_gui_domain_is_rejected(self):
        with self.assertRaisesRegex(prep.IsolationError, "gui_domain"):
            prep.validate_session(self.collect(gui=1))

    def test_sec03_real_isolation_facts_accept_both_architectures(self):
        for arch in prep.OLD_SHA:
            prep.validate_session(replace(session(), architecture=arch))

    def test_sec03_rejects_wrong_os_identity_admin_console_and_terminal(self):
        cases = [("system", "Linux"), ("uid", 0), ("euid", 0), ("groups", (20, 80)),
                 ("admin_gid", -1), ("username", "actual-user"), ("home", Path("/Users/actual-user")),
                 ("console_uid", 502), ("tty_input", False), ("tty_output", False),
                 ("remote", True), ("ci", True), ("gui_domain", False), ("architecture", "other")]
        for key, value in cases:
            with self.subTest(key=key), self.assertRaises(prep.IsolationError):
                prep.validate_session(replace(session(), **{key: value}))

    def test_sec03_forged_environment_cannot_enable_non_mac(self):
        with mock.patch.object(prep.platform, "system", return_value="Windows"), \
             mock.patch.dict(prep.os.environ, {"CI": "true", "GITHUB_ACTIONS": "true", "USER": prep.USER}), \
             mock.patch.object(prep, "command") as command:
            with self.assertRaisesRegex(prep.IsolationError, "^platform$"):
                prep.collect_session()
            command.assert_not_called()

    def test_sec03_bad_session_never_queries_keychain(self):
        with mock.patch.object(prep, "_baseline") as baseline:
            with self.assertRaises(prep.IsolationError):
                prep.prepare(replace(session(), username="actual-user"), RUN, COMMIT, SHA)
            baseline.assert_not_called()


class PathTests(unittest.TestCase):
    def check_path(self, override=None, *, file=True):
        path = KC if file else ROOT
        def metadata(value):
            if override and value == override[0]:
                return override[1]
            return info(stat.S_IFREG | 0o600) if value == KC else info()
        with mock.patch.object(Path, "lstat", autospec=True, side_effect=metadata), \
             mock.patch.object(prep, "no_extended_acl"):
            return prep.private_path(path, prep.HOME, 501, file=file)

    def test_sec03_private_root_and_single_link_file_are_accepted(self):
        self.check_path()
        self.check_path(file=False)

    def test_sec03_rejects_symlink_owner_mode_type_and_hardlink(self):
        cases = [(KC, info(stat.S_IFLNK | 0o600)), (prep.HOME, info(st_uid=502)),
                 (ROOT, info(stat.S_IFDIR | 0o755)), (KC, info(stat.S_IFREG | 0o644)),
                 (ROOT.parent, info(stat.S_IFLNK | 0o700)), (KC, info(st_nlink=2, mode=stat.S_IFREG | 0o600)),
                 (KC, info(stat.S_IFIFO | 0o600))]
        for path, value in cases:
            with self.subTest(path=str(path), mode=value.st_mode), self.assertRaises(prep.IsolationError):
                self.check_path((path, value))

    def test_sec03_rejects_outside_or_parent_traversal_before_inspection(self):
        for path in (Path("/Users/actual-user/keychain"), prep.HOME / ".." / "actual-user", Path("relative")):
            with mock.patch.object(Path, "lstat") as metadata, self.assertRaises(prep.IsolationError):
                prep.private_path(path, prep.HOME, 501)
            metadata.assert_not_called()

    def test_sec03_unknown_or_extended_acl_is_rejected_without_repair(self):
        for output in ("", "drwx------+ 2 user staff 64 date directory\n"):
            with mock.patch.object(prep, "command", return_value=output) as command:
                with self.assertRaises(prep.IsolationError):
                    prep.no_extended_acl(ROOT)
                command.assert_called_once_with(["/bin/ls", "-lde", str(ROOT)])

    def test_sec03_existing_product_state_or_socket_is_not_removed(self):
        with mock.patch.object(Path, "lstat", return_value=info()), \
             mock.patch.object(Path, "unlink") as unlink, mock.patch.object(Path, "rmdir") as rmdir:
            with self.assertRaisesRegex(prep.IsolationError, "existing_product_state"):
                prep.absent(ROOT)
            unlink.assert_not_called()
            rmdir.assert_not_called()


class ArchiveTests(unittest.TestCase):
    def check_archive(self, expected=None, *, changed=False):
        data = b"synthetic-package"
        before = info(stat.S_IFREG | 0o600, st_size=len(data))
        class Stream(io.BytesIO):
            def fileno(self):
                return 123
        with mock.patch.object(prep, "private_path", return_value=before), \
             mock.patch.object(prep.os, "O_NOFOLLOW", 0x20000, create=True), \
             mock.patch.object(prep.os, "O_NONBLOCK", 0x4000, create=True), \
             mock.patch.object(prep.os, "open", return_value=123) as opened, \
             mock.patch.object(prep.os, "fdopen", return_value=Stream(data)), \
             mock.patch.object(prep.os, "fstat", return_value=before), \
             mock.patch.object(Path, "lstat", return_value=info(stat.S_IFREG | 0o600, st_size=len(data), st_ino=99) if changed else before):
            value = prep.archive_hash(ROOT / "old-package.tar.gz", expected or hashlib.sha256(data).hexdigest(), prep.HOME, 501)
            self.assertEqual(opened.call_args.args[1] & 0x20000, 0x20000)
            self.assertFalse(opened.call_args.args[1] & (prep.os.O_WRONLY | prep.os.O_RDWR | prep.os.O_CREAT))
            return value

    def test_sec03_archive_hash_is_read_only_and_no_follow(self):
        self.check_archive()

    def test_sec03_wrong_digest_and_post_read_replacement_are_denied(self):
        for kwargs in ({"expected": SHA}, {"changed": True}):
            with self.subTest(kwargs=kwargs), self.assertRaises(prep.IsolationError):
                self.check_archive(**kwargs)


class PreparationTests(unittest.TestCase):
    def prepare_with(self, *, baseline=None, run_id=RUN, revision=COMMIT, sha=SHA,
                     late_product=False, changed_source=False):
        with mock.patch.object(prep, "private_path", return_value=info()), \
             mock.patch.object(prep, "root_entries"), \
             mock.patch.object(prep, "load_baseline", return_value=(prep.HOME / "original.keychain-db", (prep.HOME / "original.keychain-db",))), \
             mock.patch.object(prep, "_baseline", side_effect=baseline or [(KC, (KC,)), (KC, (KC,))]), \
             mock.patch.object(prep, "absent", side_effect=[None, None, prep.IsolationError("existing_product_state")] if late_product else None) as absent, \
             mock.patch.object(prep, "source_revision", side_effect=[None, None, None, prep.IsolationError("source_dirty")] if changed_source else None) as source, \
             mock.patch.object(prep, "archive_hash", side_effect=lambda path, digest, home, uid: digest), \
             mock.patch.object(prep, "command", return_value=str(prep.HOME / "synthetic-runtime") + "\n"), \
             mock.patch.object(prep, "source_hash", return_value="d" * 64), \
             mock.patch.object(Path, "stat", return_value=info()), \
             mock.patch.object(Path, "lstat", return_value=info()), \
             mock.patch.object(Path, "is_dir", return_value=True):
            result = prep.prepare(session(), run_id, revision, sha)
            self.assertEqual(source.call_count, 4)
            self.assertEqual(absent.call_count, 4)
            return result

    def test_sec03_plan_never_claims_execution_or_authorization(self):
        result = self.prepare_with()
        self.assertIs(result["executed"], False)
        self.assertIs(result["execution_authorized"], False)
        self.assertEqual(result["phases"], list(prep.PHASES))
        self.assertEqual(result["old_archive_sha256"], prep.OLD_SHA["macos-aarch64"])

    def test_sec03_late_product_creation_or_source_mutation_denies_preparation(self):
        for kwargs, code in (({"late_product": True}, "existing_product_state"), ({"changed_source": True}, "source_dirty")):
            with self.subTest(kwargs=kwargs), self.assertRaisesRegex(prep.IsolationError, code):
                self.prepare_with(**kwargs)

    def test_sec03_wrong_or_changed_keychain_selection_fails(self):
        other = Path("/private/other.keychain-db")
        for baselines in ([(other, (other,))], [(KC, (KC, other))], [(KC, (KC,)), (other, (other,))]):
            with self.subTest(baselines=baselines), self.assertRaisesRegex(prep.IsolationError, "keychain_selection"):
                self.prepare_with(baseline=baselines)

    def test_sec03_malformed_identifiers_are_rejected(self):
        for kwargs in ({"run_id": "../outside"}, {"revision": "main"}, {"sha": "not-a-hash"}):
            with self.subTest(kwargs=kwargs), self.assertRaises(prep.IsolationError):
                self.prepare_with(**kwargs)

    def test_sec03_source_commit_root_and_clean_state_are_required(self):
        source = ROOT / "candidate-source"
        for outputs in (["/other", COMMIT, ""], [str(source), "0" * 40, ""], [str(source), COMMIT, " M lib.rs"]):
            with mock.patch.object(prep, "private_path"), \
                 mock.patch.object(prep, "source_tree", return_value=("stable", info())), \
                 mock.patch.object(prep, "git_config_file"), \
                 mock.patch.object(prep, "command", side_effect=outputs), self.assertRaises(prep.IsolationError):
                prep.source_revision(source, COMMIT, prep.HOME, 501)

    def test_sec03_command_uses_no_shell_and_removes_git_environment_overrides(self):
        with mock.patch.dict(prep.os.environ, {"GIT_DIR": "/outside", "GIT_CONFIG_COUNT": "1"}), \
             mock.patch.object(prep.subprocess, "run", return_value=SimpleNamespace(stdout="fixed")) as run:
            self.assertEqual(prep.command(["/usr/bin/id", "-G", prep.USER]), "fixed")
            kwargs = run.call_args.kwargs
            self.assertNotIn("shell", kwargs)
            self.assertNotIn("GIT_DIR", kwargs["env"])
            self.assertNotIn("GIT_CONFIG_COUNT", kwargs["env"])
            self.assertEqual(kwargs["env"]["GIT_CONFIG_GLOBAL"], prep.os.devnull)
            self.assertEqual(set(kwargs["env"]), {"PATH", "HOME", "LC_ALL", "GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL"})

    def test_sec03_modifying_commands_and_product_or_test_launches_are_denied(self):
        commands = [["/usr/bin/security", verb, "synthetic"] for verb in (
            "create-keychain", "unlock-keychain", "delete-keychain", "set-keychain-settings",
            "set-key-partition-list", "add-trusted-app")]
        commands += [["/usr/bin/security", "default-keychain", "-d", "user", "-s", str(KC)],
                     ["/usr/bin/security", "list-keychains", "-d", "user", "-s", str(KC)],
                     ["cargo", "test"], ["/usr/bin/codesign", "-s", "-"], ["/bin/chmod", "700", str(ROOT)],
                     ["/private/mail-hantong-core", "run"]]
        for argv in commands:
            with self.subTest(argv=argv), mock.patch.object(prep.subprocess, "run") as run:
                with self.assertRaisesRegex(prep.IsolationError, "command_denied"):
                    prep.command(argv)
                run.assert_not_called()

    def test_sec03_cli_hides_subprocess_and_keychain_observation_errors(self):
        args = ["prepare", "--stage", "prepared", "--disposable-user-confirmed", "--run-id", RUN, "--candidate-commit", COMMIT, "--candidate-sha256", SHA]
        for error in (subprocess.CalledProcessError(1, ["private-value"], stderr="private-value"),
                      OSError("private-value"), AttributeError("private-value"), UnicodeError("private-value")):
            output = io.StringIO()
            with mock.patch.object(sys, "argv", args), mock.patch.object(prep, "collect_session", side_effect=error), contextlib.redirect_stderr(output):
                self.assertEqual(prep.main(), 2)
            self.assertEqual(output.getvalue().strip(), "HANTONG_INTERACTIVE_PREP result=denied reason=observation_failed")

    def test_sec03_preparation_cancel_preserves_state_and_returns_fixed_result(self):
        args = ["prepare", "--stage", "baseline", "--disposable-user-confirmed", "--run-id", RUN]
        output = io.StringIO()
        with mock.patch.object(sys, "argv", args), mock.patch.object(prep, "collect_session", side_effect=KeyboardInterrupt), \
             mock.patch.object(prep, "capture_baseline") as capture, contextlib.redirect_stderr(output):
            self.assertEqual(prep.main(), 130)
            capture.assert_not_called()
        self.assertEqual(output.getvalue().strip(), "HANTONG_INTERACTIVE_PREP result=cancelled")

    def test_sec03_no_run_or_permission_changing_option_is_exposed(self):
        with mock.patch.object(sys, "argv", ["prepare", "--run"]), \
             mock.patch.object(prep, "collect_session") as collect, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                prep.main()
            collect.assert_not_called()

    def test_sec03_operator_attestation_is_required_before_os_queries(self):
        with mock.patch.object(sys, "argv", ["prepare", "--stage", "baseline", "--run-id", RUN]), \
             mock.patch.object(prep, "collect_session") as collect, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(prep.main(), 2)
            collect.assert_not_called()


class BaselineTests(unittest.TestCase):
    original = prep.HOME / "Library/Keychains/login.keychain-db"

    def record(self):
        return {"schema": "hantong-interactive-mac-baseline-v1", "uid": 501,
                "root_identity": prep.identity(info()), "run_id": RUN,
                "default": str(self.original), "search": [str(self.original)],
                "keys": [{"path": str(self.original), "identity": prep.identity(info(stat.S_IFREG | 0o600))}],
                "operator_attestation": "disposable-no-real-data"}

    def load(self, record=None, raw=None):
        data = raw if raw is not None else json.dumps(record or self.record()).encode()
        file_info = info(stat.S_IFREG | 0o600, st_size=len(data))
        class Stream(io.BytesIO):
            def fileno(self):
                return 123
        def metadata(path):
            return file_info if path.name == "baseline.json" else info()
        with mock.patch.object(prep, "private_path", side_effect=lambda path, *a, **kw: file_info if path.name == "baseline.json" else info(stat.S_IFREG | 0o600)), \
             mock.patch.object(prep.os, "O_NOFOLLOW", 0x20000, create=True), \
             mock.patch.object(prep.os, "O_NONBLOCK", 0x4000, create=True), \
             mock.patch.object(prep.os, "open", return_value=123), \
             mock.patch.object(prep.os, "fdopen", return_value=Stream(data)), \
             mock.patch.object(prep.os, "fstat", return_value=file_info), \
             mock.patch.object(Path, "lstat", autospec=True, side_effect=metadata):
            return prep.load_baseline(session(), RUN, ROOT)

    def test_sec03_pre_switch_record_is_bound_to_user_root_and_original_keychains(self):
        self.assertEqual(self.load(), (self.original, (self.original,)))

    def test_sec03_stale_or_wrong_user_root_run_keychain_records_are_rejected(self):
        cases = [("uid", 502), ("run_id", "f" * 32), ("root_identity", [99] * 5),
                 ("operator_attestation", "not-confirmed"), ("schema", "other"),
                 ("default", str(KC)), ("search", []), ("keys", [])]
        for key, value in cases:
            record = self.record()
            record[key] = value
            with self.subTest(key=key), self.assertRaises(prep.IsolationError):
                self.load(record)
        record = self.record()
        record["keys"][0]["identity"][1] += 1
        with self.assertRaisesRegex(prep.IsolationError, "baseline_binding"):
            self.load(record)

    def test_sec03_duplicate_unknown_or_malformed_record_fields_are_rejected(self):
        for data in (b'{"schema":"x","schema":"x"}', b'[]', b'not json', b'\xff'):
            with self.subTest(data=data), self.assertRaises(prep.IsolationError):
                self.load(raw=data)
        record = self.record()
        record["unapproved"] = "private-value"
        with self.assertRaises(prep.IsolationError):
            self.load(record)

    def test_sec03_baseline_capture_is_before_synthetic_setup_and_stable(self):
        expected = (self.original, (self.original,))
        with mock.patch.object(prep, "run_root", return_value=ROOT), \
             mock.patch.object(prep, "root_entries"), \
             mock.patch.object(prep, "absent") as absent, mock.patch.object(prep, "product_absent"), \
             mock.patch.object(prep, "_baseline", return_value=expected), \
             mock.patch.object(prep, "private_path", return_value=info(stat.S_IFREG | 0o600)), \
             mock.patch.object(Path, "lstat", return_value=info()):
            result = prep.capture_baseline(session(), RUN)
            self.assertEqual(result, self.record())
            self.assertEqual(absent.call_args_list, [mock.call(KC), mock.call(ROOT / "baseline.json")])

    def test_sec03_missing_pre_switch_record_never_produces_a_prepared_plan(self):
        with mock.patch.object(prep, "run_root", return_value=ROOT), \
             mock.patch.object(prep, "root_entries"), \
             mock.patch.object(prep, "load_baseline", side_effect=FileNotFoundError), \
             mock.patch.object(prep, "_baseline") as baseline:
            with self.assertRaises(FileNotFoundError):
                prep.prepare(session(), RUN, COMMIT, SHA)
            baseline.assert_not_called()

    def test_sec03_restore_check_is_not_cleanup_permission_and_never_changes_selection(self):
        expected = (self.original, (self.original,))
        with mock.patch.object(prep, "run_root", return_value=ROOT), \
             mock.patch.object(prep, "load_baseline", return_value=expected), \
             mock.patch.object(prep, "product_absent"), mock.patch.object(prep, "_baseline", return_value=expected):
            result = prep.verify_restoration(session(), RUN)
            self.assertTrue(result["selection_restored"])
            self.assertFalse(result["cleanup_authorized"])
            self.assertFalse(result["process_shutdown_verified"])
        with mock.patch.object(prep, "run_root", return_value=ROOT), \
             mock.patch.object(prep, "load_baseline", return_value=expected), \
             mock.patch.object(prep, "product_absent"), mock.patch.object(prep, "_baseline", return_value=(KC, (KC,))):
            with self.assertRaisesRegex(prep.IsolationError, "restore_selection"):
                prep.verify_restoration(session(), RUN)

    def test_sec03_selection_probe_has_only_read_only_security_verbs(self):
        value = f'    "{self.original.as_posix()}"\n'
        with mock.patch.object(prep, "command", return_value=value) as command, \
             mock.patch.object(prep, "private_path"):
            self.assertEqual(prep._baseline(prep.HOME, 501), (self.original, (self.original,)))
            self.assertEqual(command.call_args_list, [
                mock.call(["/usr/bin/security", "default-keychain", "-d", "user"]),
                mock.call(["/usr/bin/security", "list-keychains", "-d", "user"])])

    def test_sec03_selection_rejects_duplicates_empty_or_ambiguous_default(self):
        value = f'"{self.original.as_posix()}"\n'
        for outputs in (["", value], [value + value, value], [value, value + value]):
            with mock.patch.object(prep, "command", side_effect=outputs), \
                 mock.patch.object(prep, "private_path"), self.assertRaises(prep.IsolationError):
                prep._baseline(prep.HOME, 501)


class NativeArchitectureTests(unittest.TestCase):
    def query(self, *, machine="x86_64", result=0, error=0, translated=0, size=4):
        def query(name, value, length, new_value, new_size):
            self.assertEqual(name, b"sysctl.proc_translated")
            self.assertIs(new_value, None)
            self.assertEqual(new_size, 0)
            prep.ctypes.cast(value, prep.ctypes.POINTER(prep.ctypes.c_int))[0] = translated
            prep.ctypes.cast(length, prep.ctypes.POINTER(prep.ctypes.c_size_t))[0] = size
            prep.ctypes.set_errno(error)
            return result
        library = SimpleNamespace(sysctlbyname=mock.Mock(side_effect=query))
        with mock.patch.object(prep.platform, "system", return_value="Darwin"), \
             mock.patch.object(prep.platform, "machine", return_value=machine), \
             mock.patch.object(prep.ctypes, "CDLL", return_value=library) as load:
            answer = prep.native_architecture()
            load.assert_called_once_with("/usr/lib/libSystem.B.dylib", use_errno=True)
            return answer

    def test_sec03_native_intel_absent_translation_oid_and_arm_are_supported(self):
        self.assertEqual(self.query(result=-1, error=prep.errno.ENOENT), "macos-x86_64")
        self.assertEqual(self.query(machine="arm64"), "macos-aarch64")
        self.assertEqual(self.query(), "macos-x86_64")

    def test_sec03_rosetta_unknown_error_size_value_or_machine_are_denied(self):
        for kwargs in ({"translated": 1}, {"result": -1, "error": prep.errno.EPERM},
                       {"result": 1}, {"size": 1}, {"translated": 2}, {"machine": "other"}):
            with self.subTest(kwargs=kwargs), self.assertRaises(prep.IsolationError):
                self.query(**kwargs)


class RootEntriesTests(unittest.TestCase):
    names = ("baseline.json", "synthetic.keychain-db", "old-source", "candidate-source",
             "old-package.tar.gz", "candidate-package.tar.gz")

    def test_sec03_baseline_requires_empty_root_and_prepared_exact_inputs(self):
        for stage, names in (("baseline", ()), ("prepared", self.names)):
            with mock.patch.object(Path, "iterdir", return_value=iter(ROOT / name for name in names)), \
                 mock.patch.object(prep, "private_path") as check:
                prep.root_entries(ROOT, session(), stage)
                self.assertEqual(check.call_count, len(names))

    def test_sec03_unexpected_missing_or_unsafe_entry_is_rejected(self):
        for stage, names in (("baseline", ("actual-data",)), ("prepared", self.names + ("extra",)),
                             ("prepared", self.names[:-1])):
            with mock.patch.object(Path, "iterdir", return_value=iter(ROOT / name for name in names)), \
                 self.assertRaisesRegex(prep.IsolationError, "root_entries"):
                prep.root_entries(ROOT, session(), stage)
        with mock.patch.object(Path, "iterdir", return_value=iter(ROOT / name for name in self.names)), \
             mock.patch.object(prep, "private_path", side_effect=prep.IsolationError("symlink")), \
             self.assertRaisesRegex(prep.IsolationError, "symlink"):
            prep.root_entries(ROOT, session(), "prepared")


class SourceIsolationTests(unittest.TestCase):
    source = ROOT / "candidate-source"
    valid_config = '[core]\nrepositoryformatversion = 0\nbare = false\nfilemode = true\n'

    def tree(self, *, change=None, alternate=False):
        paths = {"": [".git", "lib.rs"], ".git": ["config", "objects"], ".git/objects": []}
        if alternate:
            paths[".git/objects"] = ["info"]
            paths[".git/objects/info"] = ["alternates"]
        def scan(directory):
            relative = directory.relative_to(self.source).as_posix()
            relative = "" if relative == "." else relative
            entries = []
            for name in paths[relative]:
                key = (directory / name).relative_to(self.source).as_posix()
                value = info(stat.S_IFDIR | 0o755) if key in paths else info(stat.S_IFREG | 0o644)
                if change and key == change[0]:
                    value = change[1]
                entries.append(SimpleNamespace(name=name, stat=lambda *, follow_symlinks, value=value: value))
            return contextlib.nullcontext(iter(entries))
        with mock.patch.object(prep.os, "scandir", side_effect=scan):
            return prep.source_tree(self.source, 501)

    def test_sec03_source_tree_metadata_is_bound_without_skipping_links(self):
        before = self.tree()[0]
        self.assertEqual(len(before), 64)
        self.assertNotEqual(self.tree(change=("lib.rs", info(stat.S_IFREG | 0o644, st_mtime_ns=99)))[0], before)

    def test_sec03_nested_symlink_hardlink_owner_write_access_and_external_objects_are_denied(self):
        cases = [("lib.rs", info(stat.S_IFLNK | 0o644)), (".git/config", info(stat.S_IFLNK | 0o644)),
                 ("lib.rs", info(stat.S_IFREG | 0o644, st_nlink=2)),
                 (".git/objects", info(stat.S_IFDIR | 0o777)),
                 (".git/config", info(stat.S_IFREG | 0o644, st_uid=502)),
                 ("lib.rs", info(stat.S_IFIFO | 0o600))]
        for change in cases:
            with self.subTest(change=change[0]), self.assertRaises(prep.IsolationError):
                self.tree(change=change)
        with self.assertRaisesRegex(prep.IsolationError, "source_reference"):
            self.tree(alternate=True)

    def test_sec03_git_config_allows_plain_public_clone_not_external_includes_helpers_or_worktree(self):
        prep.validate_git_config(self.valid_config)
        prep.validate_git_config(self.valid_config + '[remote "origin"]\nurl = https://github.com/tz1012/mail-hantong.git\nfetch = +refs/heads/*:refs/remotes/origin/*\n')
        for extra in ('[include]\npath = /outside/config\n', '[filter "custom"]\nclean = private-command\n',
                      '[core]\nworktree = /outside\n', '[extensions]\nworktreeConfig = true\n',
                      '[credential]\nhelper = private-command\n', '[DEFAULT]\nworktree = /outside\n'):
            with self.subTest(extra=extra.splitlines()[0]), self.assertRaisesRegex(prep.IsolationError, "git_config"):
                prep.validate_git_config(self.valid_config + extra)

    def test_sec03_source_metadata_change_during_git_queries_is_rejected(self):
        with mock.patch.object(prep, "private_path"), \
             mock.patch.object(prep, "source_tree", side_effect=[("before", info()), ("after", info())]), \
             mock.patch.object(prep, "git_config_file"), \
             mock.patch.object(prep, "command", side_effect=[str(self.source), COMMIT, ""]):
            with self.assertRaisesRegex(prep.IsolationError, "source_changed"):
                prep.source_revision(self.source, COMMIT, prep.HOME, 501)

    def test_sec03_git_config_read_is_pinned_and_replacement_is_denied(self):
        data = self.valid_config.encode()
        before = info(stat.S_IFREG | 0o644, st_size=len(data))
        class Stream(io.BytesIO):
            def fileno(self):
                return 123
        for changed in (False, True):
            with mock.patch.object(prep.os, "O_NOFOLLOW", 0x20000, create=True), \
                 mock.patch.object(prep.os, "O_NONBLOCK", 0x4000, create=True), \
                 mock.patch.object(prep.os, "open", return_value=123) as opened, \
                 mock.patch.object(prep.os, "fdopen", return_value=Stream(data)), \
                 mock.patch.object(prep.os, "fstat", return_value=before), \
                 mock.patch.object(Path, "lstat", return_value=info(stat.S_IFREG | 0o644, st_size=len(data), st_ino=99) if changed else before):
                if changed:
                    with self.assertRaisesRegex(prep.IsolationError, "replaced"):
                        prep.git_config_file(self.source, before)
                else:
                    prep.git_config_file(self.source, before)
                self.assertEqual(opened.call_args.args[1] & 0x20000, 0x20000)


if __name__ == "__main__":
    unittest.main()
