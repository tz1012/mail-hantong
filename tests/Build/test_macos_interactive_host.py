from __future__ import annotations

import importlib.util
import copy
import json
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

SCRIPTS = Path(__file__).parents[2] / "scripts"
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("macos_interactive_host", SCRIPTS / "macos_interactive_host.py")
assert SPEC and SPEC.loader
host = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = host
SPEC.loader.exec_module(host)


def session():
    return host.prep.Session("Darwin", "macos-aarch64", 501, 501, (20,), 80,
                             "hantong-compat", Path("/Users/hantong-compat"), 501,
                             True, True, False, False, True)


class FakeProc:
    def __init__(self, returncode=None):
        self.pid, self.returncode = 123, returncode
        self.stdout = self.stderr = None

    def poll(self):
        return self.returncode

    def wait(self, timeout=None):
        self.returncode = 0
        return 0


class FakeStream:
    def __init__(self, fd):
        self.fd = fd

    def fileno(self):
        return self.fd


class FakeSelector:
    def __init__(self):
        self.entries = {}

    def register(self, stream, _events):
        self.entries[stream] = (mock.Mock(fileobj=stream, fd=stream.fileno()), _events)

    def unregister(self, stream):
        self.entries.pop(stream, None)

    def get_map(self):
        return self.entries

    def select(self, timeout=0):
        return list(self.entries.values())

    def close(self):
        pass


class HostTests(unittest.TestCase):
    def make_host(self):
        temp = tempfile.TemporaryDirectory()
        base = Path(temp.name) / "home"
        base.mkdir()
        root = base / ("a" * 32)
        root.mkdir()
        s = host.prep.Session("Darwin", "macos-aarch64", 501, 501, (20,), 80,
                              "hantong-compat", base, 501, True, True, False, False, True)
        (base / "Library/Application Support").mkdir(parents=True)
        value = host.NativeHost(s, root, root / "controller", "b" * 40, "c" * 64)
        self.addCleanup(temp.cleanup)
        return value

    def test_SEC03_guard_rejects_session_change_before_observation(self):
        value = self.make_host()
        with mock.patch.object(host.prep, "collect_session", return_value=mock.sentinel.other), \
             mock.patch.object(host.prep, "validate_session") as validate:
            with self.assertRaisesRegex(host.HostError, "session_changed"):
                value.guard()
        validate.assert_not_called()

    def test_SEC03_guard_rejects_non_synthetic_selection(self):
        value = self.make_host()
        for name in ("baseline.json", "synthetic.keychain-db", "old-package.tar.gz", "candidate-package.tar.gz"):
            (value.root / name).touch()
        (value.root / "old-source").mkdir()
        (value.root / "candidate-source").mkdir()
        (value.root / "controller").mkdir()
        with mock.patch.object(host.prep, "collect_session", return_value=value.session), \
             mock.patch.object(host.prep, "validate_session"), mock.patch.object(host.prep, "run_root"), \
             mock.patch.object(host.prep, "private_path"), mock.patch.object(host.prep, "load_baseline"), \
             mock.patch.object(host.prep, "_baseline", return_value=(Path("/other.keychain"), ())):
            with self.assertRaisesRegex(host.HostError, "keychain_selection"):
                value.guard()

    def test_SEC03_build_returns_source_and_manifest_proof(self):
        value = self.make_host()
        value.state = "guarded"
        value.work.mkdir()
        image = value.root / "fixture"
        image.write_bytes(b"fixture")
        fake = mock.patch.multiple(host.artifacts, extract_verified_archive=mock.Mock(),
                                   verify_extracted_package=mock.Mock(return_value={"bin/hantong": "a" * 64,
                                                                                   "bin/mail-hantong-core": "a" * 64}),
                                   build_fixture=mock.Mock(return_value=image))
        sources = {"old": {"commit": host.prep.OLD_COMMIT, "source_hash": "x", "lib_sha256": "y", "tree": ()},
                   "candidate": {"commit": value.candidate_commit, "source_hash": "x", "lib_sha256": "y", "tree": ()}}
        img = host.Image(image, "a" * 64, (1,))
        calls = []
        def build_fixture(source, target, phase, **kwargs):
            calls.append((source, phase, kwargs["environment"], kwargs["fixture"]))
            kwargs["run_command"](["cargo", "test"], cwd=source, env=kwargs["environment"], timeout=1)
            return image
        with fake, mock.patch.object(host.os, "O_NOFOLLOW", 0, create=True), mock.patch.object(host.os, "O_DIRECTORY", 0, create=True), mock.patch.object(host.prep, "private_path"), mock.patch.object(value, "_sources", return_value=sources), \
             mock.patch.object(value, "_source", side_effect=lambda name, commit: sources["old" if name.startswith("old") else "candidate"]), \
             mock.patch.object(host.artifacts, "build_fixture", side_effect=build_fixture), \
             mock.patch.object(value, "_package_metadata"), mock.patch.object(host, "_seal", return_value=img), \
             mock.patch.object(host, "_safe_hash", return_value="a" * 64), \
             mock.patch.object(host, "_write_once", return_value="a" * 64), \
             mock.patch.object(value.runner, "run", return_value=b""), \
             mock.patch.object(value, "_verify_manifest", return_value={}):
            result = value.build()
        self.assertTrue(result["source_restored"])
        self.assertRegex(result["artifact_receipt_sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual([item[1] for item in calls], ["old", "candidate"])
        self.assertTrue(all("CARGO_HOME" in item[2] and "RUSTC" in item[2] for item in calls))
        self.assertTrue(all(item[3].name == "macos_keychain_compat_module.rs" for item in calls))

    def test_SEC03_product_order_stops_each_foreground_core(self):
        value = self.make_host()
        value.state = "built"
        image = host.Image(value.root / "image", "a" * 64, (1,))
        child = FakeProc()
        runtime = value.root / "runtime"
        status = json.dumps({"ok": True, "items": [{"transport_ready": True, "mail_ready": False,
                    "authenticated": False, "protocol_version": 1}], "completeness": "complete",
                    "account_results": []}).encode()
        def start(*args, **kwargs):
            runtime.mkdir(exist_ok=True)
            (runtime / "core-v2.sock").touch()
            return child
        with mock.patch.object(value, "guard"), mock.patch.object(host.prep, "private_path"), mock.patch.object(value, "_verified_image", return_value=image), \
             mock.patch.object(value, "_runtime", return_value=runtime), mock.patch.object(value, "_runtime_identity", return_value=(1,)), \
             mock.patch.object(value, "_socket_absent", return_value=True), mock.patch.object(value, "_verify_database"), \
             mock.patch.object(value, "_stop_core", side_effect=lambda: setattr(value, "core", None)) as stop, \
             mock.patch.object(value.runner, "start", side_effect=start), \
             mock.patch.object(value.runner, "run", return_value=status):
            (value.root / "core-v2.sock").touch()
            value.product("old", 1)
            value.product("candidate", 1)
            value.product("candidate", 2)
        self.assertEqual(stop.call_count, 3)

    def test_SEC03_credentials_binds_exact_root_phase_and_markers(self):
        value = self.make_host()
        value.state, value.products = "built", 3
        value.work.mkdir()
        image = host.Image(value.root / "fixture", "a" * 64, (1,))
        markers = ["open", "opened", "read", "imap-old-selector", "imap-old-backend", "imap-old-returned",
                   "smtp-old-selector", "smtp-old-backend", "smtp-old-returned", "done"]
        output = "\n".join(["running 1 test", "test " + host.TEST + " ... ok"] +
                             ["HANTONG_COMPAT_MARKER phase=seed step=" + step for step in markers]).encode()
        with mock.patch.object(value, "guard"), mock.patch.object(value, "_socket_absent", return_value=True), \
             mock.patch.object(host.prep, "private_path"), \
             mock.patch.object(value, "_verified_image", return_value=image), \
             mock.patch.object(host, "_safe_hash", return_value="a" * 64), \
             mock.patch.object(value.runner, "run", return_value=output) as run:
            value.credentials("seed")
        env = run.call_args.args[1]
        self.assertEqual(env["HANTONG_COMPAT_ROOT"], str(value.work / "synthetic-secrets"))
        self.assertEqual(env["HANTONG_COMPAT_PHASE"], "seed")

    def test_SEC03_credentials_accepts_all_three_exact_marker_sequences(self):
        value = self.make_host()
        value.state, value.products = "built", 3
        value.work.mkdir()
        image = host.Image(value.root / "fixture", "a" * 64, (1,))
        phases = {"seed": ["open", "opened", "read", "imap-old-selector", "imap-old-backend", "imap-old-returned",
                            "smtp-old-selector", "smtp-old-backend", "smtp-old-returned", "done"],
                  "upgrade": ["open", "opened", "read-old", "imap-old-selector", "imap-old-backend", "imap-old-returned",
                               "smtp-old-selector", "smtp-old-backend", "smtp-old-returned", "write-new", "read-new",
                               "imap-new-selector", "imap-new-backend", "imap-new-returned", "smtp-old-selector",
                               "smtp-old-backend", "smtp-old-returned", "done"],
                  "reopen": ["open", "opened", "read", "imap-new-selector", "imap-new-backend", "imap-new-returned",
                             "smtp-old-selector", "smtp-old-backend", "smtp-old-returned", "done"]}
        def output(phase):
            return ("running 1 test\ntest " + host.TEST + " ... ok\n" +
                    "\n".join(f"HANTONG_COMPAT_MARKER phase={phase} step={step}" for step in phases[phase])).encode()
        with mock.patch.object(value, "guard"), mock.patch.object(value, "_socket_absent", return_value=True), \
             mock.patch.object(host.prep, "private_path", side_effect=lambda p, *_a, **_k: p.lstat()), \
             mock.patch.object(value, "_verified_image", return_value=image), \
             mock.patch.object(host, "_safe_hash", return_value="a" * 64), \
             mock.patch.object(value.runner, "run", side_effect=lambda argv, env, timeout, phase, **kwargs: output(env["HANTONG_COMPAT_PHASE"])):
            for phase in phases:
                value.credentials(phase)
                self.assertEqual(host.prep.identity((value.work / "synthetic-secrets").lstat()), value.secret_root_identity)

    def test_SEC03_credentials_rejects_missing_duplicate_reordered_or_wrong_markers(self):
        cases = ["running 1 test\ntest " + host.TEST + " ... ok\n",
                 "running 1 test\ntest " + host.TEST + " ... ok\nHANTONG_COMPAT_MARKER phase=seed step=open\nHANTONG_COMPAT_MARKER phase=seed step=done\n",
                 "running 1 test\ntest " + host.TEST + " ... ok\nHANTONG_COMPAT_MARKER phase=upgrade step=open\n",
                 "running 1 test\ntest " + host.TEST + " ... ok\nHANTONG_COMPAT_MARKER phase=seed step=open\nHANTONG_COMPAT_MARKER phase=seed step=open\nHANTONG_COMPAT_MARKER phase=seed step=done\n"]
        for text in cases:
            value = self.make_host()
            value.state, value.products = "built", 3
            value.work.mkdir()
            image = host.Image(value.root / "fixture", "a" * 64, (1,))
            with mock.patch.object(value, "guard"), mock.patch.object(value, "_socket_absent", return_value=True), \
                 mock.patch.object(host.prep, "private_path", side_effect=lambda p, *_a, **_k: p.lstat()), \
                 mock.patch.object(value, "_verified_image", return_value=image), \
                 mock.patch.object(value.runner, "run", return_value=text.encode()):
                with self.assertRaisesRegex(host.HostError, "fixture_result"):
                    value.credentials("seed")

    def test_SEC03_replaced_secret_root_refuses_upgrade_reopen_shutdown_and_finalize(self):
        value = self.make_host()
        value.state, value.products = "built", 3
        value.work.mkdir()
        secret_root = value.work / "synthetic-secrets"
        secret_root.mkdir()
        original = host.prep.identity(secret_root.lstat())
        value.secret_root_identity = original
        preserved = value.work / "preserved-original-synthetic-secrets"
        secret_root.rename(preserved)
        secret_root.mkdir()
        digest, binding = "a" * 64, {"uid": 501}
        shutdown_record = {"schema": "hantong-interactive-shutdown-v1", "binding": binding,
                           "artifact_receipt_sha256": digest, "shutdown_verified": True,
                           "app_identity": None, "db_identity": None, "runtime_identity": None,
                           "secret_root_identity": original}
        with mock.patch.object(value, "guard"), mock.patch.object(value, "_socket_absent", return_value=True), \
             mock.patch.object(host.prep, "private_path", side_effect=lambda p, *_a, **_k: p.lstat()), \
             mock.patch.object(value.runner, "run") as run:
            for index, phase in ((1, "upgrade"), (2, "reopen")):
                value.credential_phases = index
                with self.assertRaisesRegex(host.HostError, "secret_root_replaced"): value.credentials(phase)
            run.assert_not_called()
            value.manifest_hash = digest
            with mock.patch.object(value, "_verify_manifest"), mock.patch.object(host, "_write_once") as write:
                self.assertFalse(value.shutdown())
                write.assert_not_called()
            with mock.patch.object(host.prep, "collect_session", return_value=value.session), \
                 mock.patch.object(host.prep, "validate_session"), mock.patch.object(value, "_verify_manifest"), \
                 mock.patch.object(value, "_binding", return_value=binding), \
                 mock.patch.object(host, "_read_record", return_value=(shutdown_record, "b" * 64)):
                with self.assertRaisesRegex(host.HostError, "secret_root_replaced"): value.restored(digest)
        self.assertTrue(preserved.is_dir())
        self.assertEqual(host.prep.identity(preserved.lstat()), original)

    def test_SEC03_runner_rejects_nonzero(self):
        runner = host._OwnedRunner(lambda: None)
        child = FakeProc(7)
        child.stdout = mock.Mock(); child.stderr = mock.Mock()
        child.stdout.fileno.return_value = 1; child.stderr.fileno.return_value = 2
        with mock.patch.object(runner, "start", return_value=child), mock.patch.object(runner, "_release"), \
             mock.patch.object(runner, "_stop"), \
             mock.patch.object(host.selectors.DefaultSelector, "get_map", return_value={}), \
             mock.patch.object(host.os, "set_blocking"), mock.patch.object(host.os, "killpg", create=True), \
             mock.patch.object(host.os, "O_NONBLOCK", 0, create=True):
            with self.assertRaisesRegex(host.HostError, "process_exit"):
                runner.run(["/private/image"], {}, 1, "source-candidate-reopen", image=mock.Mock(path=Path("/private/image")))

    def test_SEC03_runner_timeout_stops_owned_child_once(self):
        runner = host._OwnedRunner(lambda: None)
        child = FakeProc()
        child.stdout, child.stderr = FakeStream(1), FakeStream(2)
        with mock.patch.object(runner, "start", return_value=child), mock.patch.object(runner, "_stop") as stop, \
             mock.patch.object(host.selectors, "DefaultSelector", return_value=FakeSelector()), \
             mock.patch.object(host.os, "set_blocking"), mock.patch.object(host.time, "monotonic", side_effect=[0, 2]):
            with self.assertRaises(TimeoutError):
                runner.run(["/private/image"], {}, 1, "source-candidate-reopen", image=mock.Mock(path=Path("/private/image")))
        stop.assert_called_once_with(child)

    def test_SEC03_runner_drains_both_streams_and_bounds_output(self):
        runner = host._OwnedRunner(lambda: None)
        child = FakeProc(0)
        child.stdout, child.stderr = FakeStream(1), FakeStream(2)
        selector = FakeSelector()
        reads = {1: [b"stdout"], 2: [b"stderr"]}
        def read(fd, _size):
            values = reads[fd]
            return values.pop(0) if values else b""
        with mock.patch.object(runner, "start", return_value=child), mock.patch.object(runner, "_release"), \
             mock.patch.object(host.selectors, "DefaultSelector", return_value=selector), \
             mock.patch.object(host.os, "set_blocking"), mock.patch.object(host.os, "read", side_effect=read):
            output = runner.run(["/private/image"], {}, 1, "source-candidate-reopen", image=mock.Mock(path=Path("/private/image")))
        self.assertIn(b"stdout", output)
        self.assertIn(b"stderr", output)

    def test_SEC03_runner_output_limit_has_finite_error_without_payload(self):
        runner = host._OwnedRunner(lambda: None)
        child = FakeProc(0)
        child.stdout, child.stderr = FakeStream(1), FakeStream(2)
        selector = FakeSelector()
        with mock.patch.object(runner, "start", return_value=child), mock.patch.object(runner, "_stop") as stop, \
             mock.patch.object(host.selectors, "DefaultSelector", return_value=selector), \
             mock.patch.object(host.os, "set_blocking"), mock.patch.object(host.os, "read", return_value=b"x" * (host.MAX_OUTPUT + 1)), \
             mock.patch.object(host.os, "killpg", create=True):
            with self.assertRaisesRegex(host.HostError, "process_output") as error:
                runner.run(["/private/image"], {}, 1, "source-candidate-reopen", image=mock.Mock(path=Path("/private/image")))
        stop.assert_called_once_with(child)
        self.assertNotIn("x", str(error.exception))

    def test_SEC03_runner_rejects_wrong_group_without_signal(self):
        runner = host._OwnedRunner(lambda: None)
        child = FakeProc(None)
        runner.children[child] = 1
        with mock.patch.object(host.os, "getpgid", return_value=124, create=True), \
             mock.patch.object(host.os, "killpg", create=True) as kill:
            with self.assertRaisesRegex(host.artifacts.UnreapedProcessError, "owned_group_unproven"):
                runner._stop(child)
        kill.assert_not_called()

    def test_SEC03_wrong_kernel_image_is_stopped_and_never_accepted(self):
        value = self.make_host()
        image = host.Image(value.root / "image", "a" * 64, (1,))
        child = FakeProc()
        runner = host._OwnedRunner(lambda: None)
        with mock.patch.object(host.os, "open", return_value=77), \
             mock.patch.object(host.os, "O_NOFOLLOW", 0, create=True), \
             mock.patch.object(host.os, "O_NONBLOCK", 0, create=True), \
             mock.patch.object(host.os, "fstat"), mock.patch.object(host.prep, "snapshot", return_value=(1,)), \
             mock.patch.object(host, "_safe_hash", return_value=image.sha256), \
             mock.patch.object(host.subprocess, "Popen", return_value=child), \
             mock.patch.object(host, "_image_path", return_value=str(value.root / "replacement")), \
             mock.patch.object(runner, "_stop") as stop:
            with self.assertRaisesRegex(host.HostError, "process_image"):
                runner.start([str(image.path)], {}, "source-candidate-reopen", image=image)
        stop.assert_called_once_with(child)

    def test_SEC03_changed_image_hash_or_identity_refuses_spawn(self):
        value = self.make_host()
        image = host.Image(value.root / "image", "a" * 64, (1,))
        for identity, digest in (((2,), image.sha256), ((1,), "b" * 64)):
            with mock.patch.object(host.os, "open", return_value=77), mock.patch.object(host.os, "close") as close, \
                 mock.patch.object(host.os, "O_NOFOLLOW", 0, create=True), \
                 mock.patch.object(host.os, "O_NONBLOCK", 0, create=True), \
                 mock.patch.object(host.os, "fstat"), mock.patch.object(host.prep, "snapshot", return_value=identity), \
                 mock.patch.object(host, "_safe_hash", return_value=digest), \
                 mock.patch.object(host.subprocess, "Popen") as spawn:
                with self.assertRaisesRegex(host.HostError, "image_changed"):
                    host._OwnedRunner(lambda: None).start([str(image.path)], {}, "source-candidate-reopen", image=image)
                spawn.assert_not_called()
                close.assert_called_once_with(77)

    def test_SEC03_seal_checks_both_native_architectures_and_rejects_mismatch(self):
        value = self.make_host()
        info = SimpleNamespace(st_uid=501, st_mode=stat.S_IFREG | 0o755, st_nlink=1)
        for architecture, expected in (("macos-aarch64", "arm64"), ("macos-x86_64", "x86_64")):
            with mock.patch.object(Path, "lstat", return_value=info), \
                 mock.patch.object(host.artifacts, "_pinned_bytes", return_value=b"wrong native image"), \
                 mock.patch.object(host, "check_macho", side_effect=host.artifacts.PackageError("architecture")) as check:
                with self.assertRaises(host.artifacts.PackageError):
                    host._seal(value.root / "image", SimpleNamespace(home=value.session.home, uid=501, architecture=architecture))
                check.assert_called_once_with(b"wrong native image", expected)

    def test_SEC03_manifest_rechecks_sources_and_rejects_external_paths_before_open(self):
        value = self.make_host()
        digest, binding, sources = "a" * 64, {"uid": 501}, {"old": "fixed", "candidate": "fixed"}
        images = {f"{v}-{role}": {"path": str(value.work / f"{v}-product/bin" /
                    ("hantong" if role == "cli" else "mail-hantong-core"))}
                  for v in ("old", "candidate") for role in ("cli", "core")}
        images.update({name: {"path": str(value.work / ("old-target" if name == "seed" else "candidate-target") /
                                     "debug/deps/hantong_secrets-fixed")} for name in ("seed", "upgrade", "reopen")})
        original = {"schema": "hantong-interactive-artifacts-v1", "binding": binding,
                    "sources": sources, "images": images, "fixture_sha256": "b" * 64}
        cases = []
        changed = copy.deepcopy(original); changed["sources"]["candidate"] = "changed"; cases.append(changed)
        changed = copy.deepcopy(original); changed["binding"]["uid"] = 502; cases.append(changed)
        changed = copy.deepcopy(original); changed["images"]["seed"]["path"] = str(value.session.home / "unrelated"); cases.append(changed)
        changed = copy.deepcopy(original); changed["images"]["reopen"]["path"] += "changed"; cases.append(changed)
        for record in cases:
            opened = []
            with mock.patch.object(host, "_read_record", return_value=(record, digest)), \
                 mock.patch.object(value, "_binding", return_value=binding), \
                 mock.patch.object(value, "_sources", return_value=sources), \
                 mock.patch.object(host, "_image_from", side_effect=lambda r, _: opened.append(r["path"])):
                with self.assertRaises(host.HostError): value._verify_manifest(digest)
            self.assertNotIn(str(value.session.home / "unrelated"), opened)
        with mock.patch.object(host, "_read_record", return_value=(original, digest)), \
             mock.patch.object(value, "_binding", return_value=binding), \
             mock.patch.object(value, "_sources", return_value=sources), \
             mock.patch.object(host, "_image_from") as opened:
            self.assertEqual(value._verify_manifest(digest), original)
            self.assertEqual(opened.call_count, 7)

    def test_SEC03_runner_rejects_exited_leader_group_without_signal(self):
        runner = host._OwnedRunner(lambda: None)
        child = FakeProc(0)
        runner.children[child] = 1
        with mock.patch.object(runner, "_group_absent", return_value=False), mock.patch.object(host.os, "killpg", create=True) as kill:
            with self.assertRaisesRegex(host.artifacts.UnreapedProcessError, "owned_group_remains"):
                runner._stop(child)
        kill.assert_not_called()

    def test_SEC03_runner_kills_only_verified_live_group(self):
        runner = host._OwnedRunner(lambda: None)
        child = FakeProc(None)
        runner.children[child] = 1
        with mock.patch.object(host.os, "getpgid", return_value=child.pid, create=True), \
             mock.patch.object(host.os, "killpg", create=True) as kill, mock.patch.object(runner, "_group_absent", return_value=True), \
             mock.patch.object(runner, "_release"), mock.patch.object(host.signal, "SIGKILL", 9, create=True):
            runner._stop(child)
        kill.assert_called_once_with(child.pid, 9)

    def test_SEC03_cancel_and_timeout_do_not_succeed(self):
        runner = host._OwnedRunner(lambda: (_ for _ in ()).throw(KeyboardInterrupt))
        child = FakeProc()
        child.stdout = mock.Mock(); child.stderr = mock.Mock()
        child.stdout.fileno.return_value = 1; child.stderr.fileno.return_value = 2
        child.poll = lambda: None
        with mock.patch.object(runner, "start", return_value=child), mock.patch.object(runner, "_stop"), \
             mock.patch.object(host.selectors.DefaultSelector, "get_map", return_value={}), mock.patch.object(host.os, "set_blocking"), \
             mock.patch.object(host.time, "monotonic", side_effect=[0, 0]):
            with self.assertRaises(KeyboardInterrupt):
                runner.run(["/private/image"], {}, 1, "source-candidate-reopen", image=mock.Mock(path=Path("/private/image")))

    def test_SEC03_fresh_host_restored_requires_bound_shutdown_record(self):
        value = self.make_host()
        digest = "a" * 64
        binding = {"uid": value.session.uid, "root_identity": [1], "run_id": value.root.name,
                   "architecture": value.session.architecture, "candidate_commit": value.candidate_commit,
                   "candidate_sha256": value.candidate_sha}
        record = {"schema": "hantong-interactive-shutdown-v1", "binding": binding,
                  "artifact_receipt_sha256": digest, "shutdown_verified": True,
                  "app_identity": None, "db_identity": None, "runtime_identity": None, "secret_root_identity": None}
        with mock.patch.object(value, "_binding", return_value=binding), \
             mock.patch.object(host.prep, "collect_session", return_value=value.session), \
             mock.patch.object(host.prep, "validate_session"), mock.patch.object(value, "_verify_manifest"), \
             mock.patch.object(host, "_read_record", return_value=(record, "b" * 64)), \
             mock.patch.object(value, "_socket_absent", return_value=True), \
             mock.patch.object(host.prep, "load_baseline", return_value=("baseline",)), \
             mock.patch.object(host.prep, "_baseline", return_value=("baseline",)):
            self.assertTrue(value.restored(digest))

    def test_SEC03_restored_rejects_manifest_digest_source_socket_baseline_and_binding_changes(self):
        value = self.make_host()
        digest = "a" * 64
        binding = {"uid": value.session.uid, "root_identity": [1], "run_id": value.root.name,
                   "architecture": value.session.architecture, "candidate_commit": value.candidate_commit,
                   "candidate_sha256": value.candidate_sha}
        record = {"schema": "hantong-interactive-shutdown-v1", "binding": binding,
                  "artifact_receipt_sha256": digest, "shutdown_verified": True,
                  "app_identity": None, "db_identity": None, "runtime_identity": None, "secret_root_identity": None}
        def attempt(record_value=record, digest_value=digest, socket=False, baseline=False, verify=True):
            with mock.patch.object(value, "_binding", return_value=binding), \
                 mock.patch.object(host.prep, "collect_session", return_value=value.session), \
                 mock.patch.object(host.prep, "validate_session"), mock.patch.object(value, "_verify_manifest", side_effect=None if verify else host.HostError("source_changed")), \
                 mock.patch.object(host, "_read_record", return_value=(record_value, "b" * 64)), \
                 mock.patch.object(value, "_socket_absent", return_value=not socket), \
                 mock.patch.object(host.prep, "load_baseline", return_value=("new",) if baseline else ("baseline",)), \
                 mock.patch.object(host.prep, "_baseline", return_value=("baseline",)):
                return value.restored(digest_value)
        bad_digest = dict(record, artifact_receipt_sha256="b" * 64)
        with self.assertRaises(host.HostError): attempt(record_value=bad_digest)
        with self.assertRaises(host.HostError): attempt(verify=False)
        with self.assertRaises(host.HostError): attempt(socket=True)
        with self.assertRaises(host.HostError): attempt(baseline=True)

    def test_SEC03_shutdown_failure_does_not_claim_restored(self):
        value = self.make_host()
        value.state = "built"
        value.manifest_hash = "a" * 64
        value.runner.stop_all = mock.Mock(return_value=False)
        self.assertFalse(value.shutdown())


if __name__ == "__main__":
    unittest.main()
