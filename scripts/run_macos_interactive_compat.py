#!/usr/bin/env python3
"""Explicit interactive Mac synthetic compatibility controller.

No Keychain configuration, OS responses, permission repair, or deletion.
Actual execution requires a separately reviewed controller and a dedicated GUI
test user; unit tests inject a fake host and never enter the Mac backend.
"""
from __future__ import annotations

import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import sys

import prepare_macos_interactive_compat as prep

PHASES = ("build", "product-old", "product-candidate-upgrade", "product-candidate-restart",
          "credential-seed", "credential-upgrade", "credential-reopen")
OUTCOMES = {"not_started", "running", "passed", "denied", "failed", "cancelled", "timed_out"}
KEYS = {"schema", "run_id", "uid", "root_identity", "candidate_commit", "candidate_sha256",
        "status", "outcome", "phase", "completed", "shutdown_verified", "cleanup_eligible",
        "artifact_receipt_sha256", "source_restored"}


def initial_record(session, root: Path, run_id: str, commit: str, digest: str) -> dict:
    return {"schema": "hantong-interactive-controller-v1", "run_id": run_id,
            "uid": session.uid, "root_identity": prep.identity(root.lstat()),
            "candidate_commit": commit, "candidate_sha256": digest,
            "status": "not_started", "outcome": "not_started", "phase": None,
            "completed": [], "shutdown_verified": False, "cleanup_eligible": False,
            "artifact_receipt_sha256": None, "source_restored": False}


def validate_record(record: dict) -> None:
    prep.require(isinstance(record, dict) and set(record) == KEYS, "controller_record")
    prep.require(record["schema"] == "hantong-interactive-controller-v1"
                 and record["outcome"] in OUTCOMES
                 and record["status"] in {"not_started", "running", "restore_required", "recovery_required", "complete"},
                 "controller_record")
    prep.require(isinstance(record["run_id"], str) and re.fullmatch(r"[0-9a-f]{32}", record["run_id"]) is not None
                 and isinstance(record["candidate_commit"], str)
                 and re.fullmatch(r"[0-9a-f]{40}", record["candidate_commit"]) is not None
                 and isinstance(record["candidate_sha256"], str)
                 and re.fullmatch(r"[0-9a-f]{64}", record["candidate_sha256"]) is not None
                 and type(record["uid"]) is int and record["uid"] >= 501
                 and isinstance(record["root_identity"], list) and len(record["root_identity"]) == 5
                 and all(type(v) is int for v in record["root_identity"]), "controller_record")
    prep.require(isinstance(record["completed"], list)
                 and record["completed"] == list(PHASES[:len(record["completed"])])
                 and record["phase"] in (None, *PHASES)
                 and type(record["shutdown_verified"]) is bool
                 and type(record["cleanup_eligible"]) is bool
                 and type(record["source_restored"]) is bool, "controller_record")
    digest = record["artifact_receipt_sha256"]
    prep.require(digest is None or (isinstance(digest, str) and re.fullmatch(r"[0-9a-f]{64}", digest) is not None), "controller_record")
    prep.require(record["source_restored"] == (digest is not None), "controller_record")
    n = len(record["completed"])
    possible_phase = {PHASES[n - 1]} if n else {None}
    if n < len(PHASES):
        possible_phase.add(PHASES[n])
    prep.require(record["phase"] in possible_phase, "controller_record")
    if record["status"] == "not_started":
        prep.require(record["outcome"] == "not_started" and record["phase"] is None and n == 0
                     and not record["shutdown_verified"] and not record["source_restored"], "controller_record")
    elif record["status"] == "running":
        prep.require(record["outcome"] == "running" and not record["shutdown_verified"], "controller_record")
    else:
        prep.require(record["outcome"] in OUTCOMES - {"not_started", "running"}, "controller_record")
        prep.require(record["shutdown_verified"] == (record["status"] != "recovery_required"), "controller_record")
    if record["outcome"] == "passed":
        prep.require(record["completed"] == list(PHASES) and record["source_restored"], "controller_record")
    if record["cleanup_eligible"] or record["status"] == "complete":
        prep.require(record["cleanup_eligible"] and record["status"] == "complete"
                     and record["shutdown_verified"] and record["source_restored"]
                     and record["outcome"] not in {"running", "not_started"},
                     "controller_record")


def execute(host, record: dict, save, check_cancel, terminal_save=None) -> dict:
    """Single use; failure never advances to a later compatibility phase."""
    validate_record(record)
    prep.require(record["status"] == "not_started" and not record["completed"], "run_used")
    record["status"], record["outcome"] = "running", "running"
    save(record)
    try:
        for phase in PHASES:
            check_cancel()
            host.guard()
            record["phase"] = phase
            save(record)
            if phase == "build":
                proof = host.build()
                prep.require(isinstance(proof, dict) and proof.get("source_restored") is True
                             and isinstance(proof.get("artifact_receipt_sha256"), str)
                             and re.fullmatch(r"[0-9a-f]{64}", proof["artifact_receipt_sha256"]) is not None,
                             "source_restoration_unproven")
                record["source_restored"] = True
                record["artifact_receipt_sha256"] = proof["artifact_receipt_sha256"]
                save(record)
            elif phase == "product-old":
                host.product("old", 1)
            elif phase.startswith("product-candidate"):
                host.product("candidate", 1 if phase.endswith("upgrade") else 2)
            else:
                host.credentials(phase.removeprefix("credential-"))
            check_cancel()
            host.guard()
            record["completed"].append(phase)
            save(record)
        record["outcome"] = "passed"
    except KeyboardInterrupt:
        record["outcome"] = "cancelled"
    except TimeoutError:
        record["outcome"] = "timed_out"
    except prep.IsolationError:
        record["outcome"] = "denied"
    except Exception:
        record["outcome"] = "failed"
    finally:
        # Shutdown ignores the cancellation marker so it can reap owned children.
        try:
            record["shutdown_verified"] = host.shutdown() is True
        except (Exception, KeyboardInterrupt):
            record["shutdown_verified"] = False
        record["status"] = "restore_required" if record["shutdown_verified"] else "recovery_required"
        record["cleanup_eligible"] = False
        (terminal_save or save)(record)
    return record


def finalize(host, record: dict, save) -> dict:
    validate_record(record)
    prep.require(record["status"] == "restore_required" and record["shutdown_verified"], "shutdown_unproven")
    prep.require(record["source_restored"] and record["artifact_receipt_sha256"] is not None, "source_restoration_unproven")
    prep.require(host.restored(record["artifact_receipt_sha256"]) is True, "restoration_unproven")
    # Eligibility is evidence only: preserve every file, DB, Keychain and log.
    record["status"], record["cleanup_eligible"] = "complete", True
    save(record)
    return record


class Records:
    """Private compare-before-replace journal; never trust a recorded PID."""

    def __init__(self, session, root: Path, run_id: str):
        self.session, self.root, self.run_id = session, root, run_id
        self.work = root / "controller"
        self.path = self.work / "execution-state.json"
        self.seal = None

    def guard(self):
        prep.run_root(self.session, self.run_id)
        prep.private_path(self.work, self.session.home, self.session.uid)

    def create(self):
        # mkdir is exclusive, even if an earlier attempt failed before journaling.
        self.work.mkdir(mode=0o700)
        self.guard()
        self._sync_directory(self.root)

    def _sync_directory(self, path=None):
        fd = os.open(self.work if path is None else path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)

    def read(self):
        self.guard()
        before = prep.private_path(self.path, self.session.home, self.session.uid, file=True)
        prep.require(0 < before.st_size <= 16384, "controller_record")
        with os.fdopen(os.open(self.path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as stream:
            prep.require(prep.snapshot(os.fstat(stream.fileno())) == prep.snapshot(before), "replaced")
            data = stream.read(16385)
            prep.require(len(data) == before.st_size and prep.snapshot(os.fstat(stream.fileno())) == prep.snapshot(before), "replaced")
        prep.require(prep.snapshot(self.path.lstat()) == prep.snapshot(before), "replaced")

        def unique(pairs):
            result = {}
            for key, value in pairs:
                prep.require(key not in result, "controller_record")
                result[key] = value
            return result

        record = json.loads(data, object_pairs_hook=unique)
        validate_record(record)
        prep.require(record["uid"] == self.session.uid and record["run_id"] == self.run_id
                     and record["root_identity"] == prep.identity(self.root.lstat()), "controller_binding")
        self.seal = prep.snapshot(before)
        return record

    def save(self, record):
        self.guard()
        validate_record(record)
        data = json.dumps(record, sort_keys=True).encode()
        if self.seal is None:
            flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
            with os.fdopen(os.open(self.path, flags, 0o600), "wb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
        else:
            prep.require(prep.snapshot(self.path.lstat()) == self.seal, "replaced")
            temporary = self.work / "state-next.json"
            with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), "wb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            prep.require(prep.snapshot(self.path.lstat()) == self.seal, "replaced")
            os.replace(temporary, self.path)
        self.seal = prep.snapshot(self.path.lstat())
        self._sync_directory()

    @contextlib.contextmanager
    def lock(self, name="controller.lock"):
        import fcntl
        self.guard()
        prep.require(name in {"controller.lock", "transition.lock"}, "controller_lock")
        path = self.work / name
        fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            info = os.fstat(fd)
            prep.require(stat.S_ISREG(info.st_mode) and info.st_uid == self.session.uid
                         and info.st_nlink == 1 and stat.S_IMODE(info.st_mode) == 0o600, "controller_lock")
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield
        finally:
            os.close(fd)

    def cancel(self):
        with self.lock("transition.lock"):
            record = self.read()
            prep.require(record["status"] == "running", "run_not_running")
            path = self.work / "cancel-requested"
            with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), "wb") as stream:
                stream.write(b"cancel\n")
                stream.flush()
                os.fsync(stream.fileno())
            self._sync_directory()
        # A cooperative marker never signals a PID from this or another run.

    def finish(self, record):
        with self.lock("transition.lock"):
            try:
                self.check_cancel()
            except KeyboardInterrupt:
                if record["outcome"] == "passed":
                    record["outcome"] = "cancelled"
            self.save(record)

    def check_cancel(self):
        path = self.work / "cancel-requested"
        try:
            path.lstat()
        except FileNotFoundError:
            return
        prep.private_path(path, self.session.home, self.session.uid, file=True)
        raise KeyboardInterrupt


def make_host(session, root, records, record):
    from macos_interactive_host import NativeHost
    return NativeHost(session, root, records.work, record["candidate_commit"],
                      record["candidate_sha256"], records.check_cancel)


def start(session, root, records, record):
    # Save before imports, locks or host construction; every ordinary failure
    # after exclusive creation leaves a typed, non-resumable recovery record.
    records.save(record)
    previous = signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        with records.lock():
            host = make_host(session, root, records, record)
            return execute(host, record, records.save, records.check_cancel, records.finish)
    except (Exception, KeyboardInterrupt) as error:
        record["status"] = "recovery_required"
        record["outcome"] = "cancelled" if isinstance(error, KeyboardInterrupt) else "failed"
        record["shutdown_verified"] = record["cleanup_eligible"] = False
        records.save(record)
        raise
    finally:
        signal.signal(signal.SIGTERM, previous)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--action", choices=("start", "cancel", "finalize"), required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--candidate-commit")
    parser.add_argument("--candidate-sha256")
    parser.add_argument("--disposable-user-confirmed", action="store_true")
    parser.add_argument("--execution-reviewed", action="store_true",
                        help="Operator attestation only; does not replace the separately recorded human review")
    args = parser.parse_args()
    try:
        prep.require(args.disposable_user_confirmed, "operator_attestation")
        if args.action != "start":
            prep.require(args.candidate_commit is None and args.candidate_sha256 is None
                         and not args.execution_reviewed, "action_arguments")
        session = prep.collect_session()
        root = prep.run_root(session, args.run_id)
        records = Records(session, root, args.run_id)
        if args.action == "cancel":
            records.cancel()
            print("HANTONG_INTERACTIVE_CONTROL result=cancel_requested")
            return 0
        if args.action == "start":
            prep.require(args.execution_reviewed, "human_review_required")
            prep.prepare(session, args.run_id, args.candidate_commit or "", args.candidate_sha256 or "")
            records.create()
            record = initial_record(session, root, args.run_id, args.candidate_commit, args.candidate_sha256)
            result = start(session, root, records, record)
        else:
            with records.lock():
                record = records.read()
                host = make_host(session, root, records, record)
                result = finalize(host, record, records.save)
        print(json.dumps(result, sort_keys=True))
        if args.action == "finalize":
            return 0 if result["status"] == "complete" and result["cleanup_eligible"] else 2
        return 0 if result["outcome"] == "passed" else 2
    except KeyboardInterrupt:
        print("HANTONG_INTERACTIVE_CONTROL result=cancelled", file=sys.stderr)
        return 130
    except Exception:
        # Native and subprocess details may contain local paths or credential data.
        print("HANTONG_INTERACTIVE_CONTROL result=denied", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
