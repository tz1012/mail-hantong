from __future__ import annotations

import copy
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

SCRIPTS = Path(__file__).parents[2] / "scripts"
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    SPEC = importlib.util.spec_from_file_location("interactive_controller", SCRIPTS / "run_macos_interactive_compat.py")
    control = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(control)


def record():
    return {"schema": "hantong-interactive-controller-v1", "run_id": "a" * 32, "uid": 501,
            "root_identity": [1, 2, 501, 20, 16832], "candidate_commit": "b" * 40,
            "candidate_sha256": "c" * 64, "status": "not_started", "outcome": "not_started",
            "phase": None, "completed": [], "shutdown_verified": False, "cleanup_eligible": False,
            "artifact_receipt_sha256": None, "source_restored": False}


class Host:
    def __init__(self, fail=None, exception=None, shutdown=True, restored=True):
        self.events = []
        self.fail, self.exception = fail, exception
        self.shutdown_ok, self.restored_ok = shutdown, restored

    def event(self, value):
        self.events.append(value)
        if value == self.fail:
            raise self.exception

    def guard(self): self.event("guard")
    def build(self):
        self.event("build")
        return {"source_restored": True, "artifact_receipt_sha256": "d" * 64}
    def product(self, version, iteration): self.event(f"product:{version}:{iteration}")
    def credentials(self, phase): self.event(f"credentials:{phase}")
    def shutdown(self):
        self.event("shutdown")
        return self.shutdown_ok
    def restored(self, expected):
        assert expected == "d" * 64
        self.event("restored")
        return self.restored_ok


class ControllerTests(unittest.TestCase):
    def run_fake(self, host, cancel=lambda: None):
        snapshots = []
        current = record()
        result = control.execute(host, current, lambda r: snapshots.append(copy.deepcopy(r)), cancel)
        return result, snapshots

    def test_inst01_exact_phase_order_and_pass_does_not_delete_or_restore_keychain(self):
        host = Host()
        result, saved = self.run_fake(host)
        self.assertEqual([e for e in host.events if e != "guard"], [
            "build", "product:old:1", "product:candidate:1", "product:candidate:2",
            "credentials:seed", "credentials:upgrade", "credentials:reopen", "shutdown"])
        self.assertEqual(result["outcome"], "passed")
        self.assertEqual(result["status"], "restore_required")
        self.assertFalse(result["cleanup_eligible"])
        self.assertTrue(saved[-1]["shutdown_verified"])
        self.assertEqual(saved[0]["completed"], [])

    def test_sec03_cancel_timeout_denial_and_failure_at_every_phase_are_terminal(self):
        events = ["build", "product:old:1", "product:candidate:1", "product:candidate:2",
                  "credentials:seed", "credentials:upgrade", "credentials:reopen"]
        for index, phase in enumerate(events):
            for exception, outcome in [(KeyboardInterrupt(), "cancelled"), (TimeoutError(), "timed_out"),
                                       (control.prep.IsolationError("denied"), "denied"),
                                       (RuntimeError("DO-NOT-EXPOSE-SECRET"), "failed")]:
                with self.subTest(phase=phase, outcome=outcome):
                    host = Host(phase, exception)
                    result, saved = self.run_fake(host)
                    self.assertEqual(result["outcome"], outcome)
                    self.assertEqual(result["completed"], list(control.PHASES[:index]))
                    self.assertEqual(host.events.count("shutdown"), 1)
                    self.assertEqual(host.events[-1], "shutdown")
                    self.assertNotIn("DO-NOT-EXPOSE-SECRET", json.dumps(saved))
                    self.assertNotIn(events[index + 1] if index + 1 < len(events) else "unused", host.events)

    def test_sec03_initial_guard_denies_before_any_execution(self):
        host = Host("guard", control.prep.IsolationError("owner"))
        result, _ = self.run_fake(host)
        self.assertEqual(host.events, ["guard", "shutdown"])
        self.assertEqual(result["outcome"], "denied")

    def test_sec03_real_host_cancel_and_timeout_types_keep_their_classification(self):
        with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
            import macos_interactive_host as native
        for error, expected in ((native.Cancelled(), "cancelled"), (native.TimedOut(), "timed_out")):
            result, _ = self.run_fake(Host("credentials:upgrade", error))
            self.assertEqual(result["outcome"], expected)

    def test_sec03_cancel_marker_before_first_phase_still_reaps_owned_processes(self):
        host = Host()
        def cancel(): raise KeyboardInterrupt
        result, _ = self.run_fake(host, cancel)
        self.assertEqual(host.events, ["shutdown"])
        self.assertEqual(result["outcome"], "cancelled")

    def test_sec03_shutdown_failure_prevents_finalize(self):
        host = Host(shutdown=False)
        result, _ = self.run_fake(host)
        self.assertEqual(result["status"], "recovery_required")
        with self.assertRaises(control.prep.IsolationError):
            control.finalize(host, result, lambda _: self.fail("must not save"))
        self.assertNotIn("restored", host.events)

    def test_sec03_shutdown_exception_never_looks_clean(self):
        host = Host("shutdown", RuntimeError("private"))
        result, _ = self.run_fake(host)
        self.assertFalse(result["shutdown_verified"])
        self.assertEqual(result["status"], "recovery_required")

    def test_sec03_restore_refusal_preserves_state(self):
        host = Host(restored=False)
        result, _ = self.run_fake(host)
        before = copy.deepcopy(result)
        with self.assertRaises(control.prep.IsolationError):
            control.finalize(host, result, lambda _: self.fail("must not save"))
        self.assertEqual(result, before)

    def test_inst01_finalize_is_read_only_and_retains_failed_outcome(self):
        host = Host("credentials:upgrade", RuntimeError("private"))
        result, _ = self.run_fake(host)
        host.events.clear()
        saved = []
        control.finalize(host, result, lambda r: saved.append(copy.deepcopy(r)))
        self.assertEqual(host.events, ["restored"])
        self.assertTrue(saved[0]["cleanup_eligible"])
        self.assertEqual(saved[0]["outcome"], "failed")

    def test_sec03_used_run_and_double_finalize_rejected(self):
        host = Host()
        result, _ = self.run_fake(host)
        count = len(host.events)
        with self.assertRaises(control.prep.IsolationError):
            control.execute(host, result, lambda _: None, lambda: None)
        self.assertEqual(len(host.events), count)
        control.finalize(host, result, lambda _: None)
        with self.assertRaises(control.prep.IsolationError):
            control.finalize(host, result, lambda _: None)

    def test_sec03_corrupt_phase_order_or_fake_success_refused(self):
        for changes in [{"completed": ["credential-reopen"]}, {"outcome": "passed"},
                        {"cleanup_eligible": True}, {"status": "complete"}, {"untrusted": "secret"}]:
            with self.subTest(changes=changes):
                value = record() | changes
                with self.assertRaises(control.prep.IsolationError): control.validate_record(value)

    def test_sec03_impossible_state_cross_products_are_rejected(self):
        for status in ("restore_required", "recovery_required", "complete"):
            for outcome in ("not_started", "running"):
                with self.subTest(status=status, outcome=outcome):
                    value = record() | {"status": status, "outcome": outcome,
                                        "shutdown_verified": status != "recovery_required"}
                    with self.assertRaises(control.prep.IsolationError): control.validate_record(value)
        for changes in ({"status": "running", "outcome": "running", "phase": "credential-reopen"},
                        {"source_restored": True}, {"artifact_receipt_sha256": "d" * 64},
                        {"status": "restore_required", "outcome": "failed", "shutdown_verified": False}):
            with self.assertRaises(control.prep.IsolationError): control.validate_record(record() | changes)

    def test_sec03_failed_or_missing_build_restoration_proof_stops_before_product(self):
        for proof in ({}, {"source_restored": False}, {"source_restored": True, "artifact_receipt_sha256": "wrong"}):
            host = Host()
            host.build = mock.Mock(return_value=proof)
            result, _ = self.run_fake(host)
            self.assertEqual(result["outcome"], "denied")
            self.assertEqual(result["completed"], [])
            self.assertFalse(any(event.startswith("product:") for event in host.events))

    def test_sec03_constructor_import_lock_and_cancel_failures_leave_typed_recovery_record(self):
        for error in (ImportError("private"), RuntimeError("private"), KeyboardInterrupt()):
            current, saved = record(), []
            records = SimpleNamespace(save=lambda r: saved.append(copy.deepcopy(r)),
                                      lock=lambda: contextlib.nullcontext())
            with mock.patch.object(control, "make_host", side_effect=error), \
                 mock.patch.object(control.signal, "signal", return_value=None):
                with self.assertRaises(type(error)): control.start(None, None, records, current)
            self.assertEqual(saved[0]["status"], "not_started")
            self.assertEqual(saved[-1]["status"], "recovery_required")
            self.assertFalse(saved[-1]["shutdown_verified"])
            control.validate_record(saved[-1])
        records.lock = mock.Mock(side_effect=OSError("private"))
        current = record()
        with mock.patch.object(control.signal, "signal", return_value=None):
            with self.assertRaises(OSError): control.start(None, None, records, current)
        self.assertEqual(current["status"], "recovery_required")

    def test_sec03_directory_fsync_is_bounded_and_closes_descriptor(self):
        store = control.Records(None, Path("root"), "a" * 32)
        with mock.patch.object(control.os, "open", return_value=17) as opened, \
             mock.patch.object(control.os, "O_DIRECTORY", 0, create=True), \
             mock.patch.object(control.os, "O_NOFOLLOW", 0, create=True), \
             mock.patch.object(control.os, "fsync") as sync, mock.patch.object(control.os, "close") as close:
            store._sync_directory()
            opened.assert_called_once()
            sync.assert_called_once_with(17)
            close.assert_called_once_with(17)


class RecordTests(unittest.TestCase):
    def setUp(self):
        self.sync = mock.patch.object(control.Records, "_sync_directory").start()
        self.addCleanup(mock.patch.stopall)

    def test_sec03_constructor_failure_is_persisted_to_private_journal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            session = SimpleNamespace(home=root, uid=501)
            store = control.Records(session, root, "a" * 32)
            current = record() | {"root_identity": control.prep.identity(root.lstat())}
            with mock.patch.object(store, "guard"), \
                 mock.patch.object(store, "lock", side_effect=lambda *_: contextlib.nullcontext()), \
                 mock.patch.object(control.os, "O_NOFOLLOW", 0, create=True), \
                 mock.patch.object(control.signal, "signal", return_value=None), \
                 mock.patch.object(control, "make_host", side_effect=RuntimeError("private")):
                store.create()
                self.sync.assert_called_once_with(root)
                with self.assertRaises(RuntimeError): control.start(session, root, store, current)
                persisted = json.loads(store.path.read_text(encoding="utf-8"))
                self.assertEqual(persisted["status"], "recovery_required")
                self.assertEqual(persisted["outcome"], "failed")
                self.assertGreaterEqual(self.sync.call_count, 3)
                control.validate_record(persisted)

    def test_sec03_journal_duplicate_unknown_wrong_binding_and_size_are_denied(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            store = control.Records(SimpleNamespace(home=root, uid=501), root, "a" * 32)
            store.work.mkdir()
            value = record() | {"root_identity": control.prep.identity(root.lstat())}
            cases = [json.dumps(value)[:-1] + ',"uid":501}', json.dumps(value | {"extra": "private"}),
                     json.dumps(value | {"uid": 502}), json.dumps(value | {"run_id": "d" * 32}),
                     json.dumps(value | {"root_identity": [0] * 5}), "x" * 16385]
            with mock.patch.object(store, "guard"), \
                 mock.patch.object(control.prep, "private_path", side_effect=lambda p, *_a, **_k: p.lstat()), \
                 mock.patch.object(control.os, "fstat", side_effect=lambda _: store.path.lstat()), \
                 mock.patch.object(control.os, "O_NOFOLLOW", 0, create=True), \
                 mock.patch.object(control.os, "O_NONBLOCK", 0, create=True):
                for text in cases:
                    with self.subTest(case=text[:30]):
                        store.path.write_text(text, encoding="utf-8")
                        with self.assertRaises(control.prep.IsolationError): store.read()
                store.path.write_text(json.dumps(value), encoding="utf-8")
                self.assertEqual(store.read(), value)

    def test_sec03_private_journal_exclusive_run_cancel_single_use_and_compare_replace(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            session = SimpleNamespace(home=root, uid=501)
            store = control.Records(session, root, "a" * 32)
            with mock.patch.object(store, "guard"), mock.patch.object(store, "lock", side_effect=lambda *_: contextlib.nullcontext()), \
                 mock.patch.object(control.prep, "run_root"), \
                 mock.patch.object(control.os, "O_NOFOLLOW", 0, create=True):
                store.create()
                with self.assertRaises(FileExistsError): store.create()
                current = record()
                current["root_identity"] = control.prep.identity(root.lstat())
                store.save(current)
                store.read = mock.Mock(return_value=current | {"status": "running"})
                store.cancel()
                with self.assertRaises(FileExistsError): store.cancel()
                with mock.patch.object(control.prep, "private_path"):
                    with self.assertRaises(KeyboardInterrupt): store.check_cancel()
                store.path.write_text("tampered", encoding="utf-8")
                with self.assertRaises(control.prep.IsolationError): store.save(current)
                self.assertEqual(store.path.read_text(), "tampered")

    def test_sec03_cancel_refuses_completed_run_and_never_signals_pid(self):
        store = control.Records(SimpleNamespace(), Path("unused"), "a" * 32)
        with mock.patch.object(store, "read", return_value=record() | {"status": "restore_required"}), \
             mock.patch.object(store, "lock", side_effect=lambda *_: contextlib.nullcontext()), \
             mock.patch.object(control.os, "kill", create=True) as kill:
            with self.assertRaises(control.prep.IsolationError): store.cancel()
            kill.assert_not_called()

    def test_sec03_cancel_and_terminal_commit_share_lock_and_cancel_wins_before_commit(self):
        store = control.Records(None, Path("root"), "a" * 32)
        order = []
        @contextlib.contextmanager
        def locked(name):
            self.assertEqual(name, "transition.lock")
            order.append("lock")
            yield
            order.append("unlock")
        current, _ = ControllerTests().run_fake(Host())
        def cancelled():
            order.append("check")
            raise KeyboardInterrupt
        with mock.patch.object(store, "lock", side_effect=locked), \
             mock.patch.object(store, "check_cancel", side_effect=cancelled), \
             mock.patch.object(store, "save", side_effect=lambda _: order.append("save")):
            store.finish(current)
        self.assertEqual(order, ["lock", "check", "save", "unlock"])
        self.assertEqual(current["outcome"], "cancelled")

    def test_inst01_fresh_process_finalize_keeps_failed_outcome_but_returns_zero(self):
        current, _ = ControllerTests().run_fake(Host("credentials:upgrade", RuntimeError("private")))
        records = mock.Mock()
        records.lock.side_effect = lambda: contextlib.nullcontext()
        records.read.return_value = current
        with mock.patch.object(sys, "argv", ["controller", "--action", "finalize", "--run-id", "a" * 32,
                                             "--disposable-user-confirmed"]), \
             mock.patch.object(control.prep, "collect_session"), mock.patch.object(control.prep, "run_root"), \
             mock.patch.object(control, "Records", return_value=records), \
             mock.patch.object(control, "make_host", return_value=Host()), \
             mock.patch.object(sys, "stdout", new_callable=io.StringIO) as output:
            self.assertEqual(control.main(), 0)
        self.assertEqual(json.loads(output.getvalue())["outcome"], "failed")

    def test_sec03_real_entry_is_platform_denied_without_host_import(self):
        with mock.patch.object(sys, "argv", ["controller", "--action", "start", "--run-id", "a" * 32,
                                             "--disposable-user-confirmed", "--execution-reviewed"]), \
             mock.patch.object(control.prep, "collect_session", side_effect=control.prep.IsolationError("platform")), \
             mock.patch.object(sys, "stderr", new_callable=io.StringIO) as output:
            self.assertEqual(control.main(), 2)
            self.assertEqual(output.getvalue(), "HANTONG_INTERACTIVE_CONTROL result=denied\n")


if __name__ == "__main__": unittest.main()
