#!/usr/bin/env python3
"""Dormant interactive adapter: no Keychain settings, OS input, or deletion."""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import time

import macos_interactive_artifacts as artifacts
from macos_compat_diagnostics import _image_path
from package_unix import check_macho
import prepare_macos_interactive_compat as prep

HostError, Cancelled, TimedOut = prep.IsolationError, KeyboardInterrupt, TimeoutError
TEST = "macos_keychain_compat::mac_keychain_compatibility_fixture"
MAX_OUTPUT = 2 * 1024**2
_require = prep.require


def _safe_hash(path, uid=None):
    info = path.lstat()
    _require(info.st_mode & 0o7022 == 0 and (uid is None or info.st_uid == uid), "image_mode")
    return hashlib.sha256(artifacts._pinned_bytes(path, 256 * 1024**2)).hexdigest()


@dataclass(frozen=True)
class Image:
    path: Path
    sha256: str
    identity: tuple


def _seal(path, session):
    _require(path.is_absolute(), "image_path")
    current = path
    while current != session.home:
        info = current.lstat()
        _require(info.st_uid == session.uid and not stat.S_ISLNK(info.st_mode)
                 and info.st_mode & 0o7022 == 0 and current != current.parent, "image_path")
        current = current.parent
    info = path.lstat()
    _require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_mode & 0o100, "image_type")
    data = artifacts._pinned_bytes(path, 256 * 1024**2)
    check_macho(data, "arm64" if session.architecture == "macos-aarch64" else "x86_64")
    _require(prep.snapshot(path.lstat()) == prep.snapshot(info), "image_changed")
    return Image(path, hashlib.sha256(data).hexdigest(), prep.snapshot(info))


def _image_json(image):
    return {"path": str(image.path), "sha256": image.sha256, "identity": list(image.identity)}


def _image_from(record, session):
    _require(isinstance(record, dict) and set(record) == {"path", "sha256", "identity"}, "image_binding")
    image = _seal(Path(record["path"]), session)
    _require(_image_json(image) == record, "image_changed")
    return image


def _write_once(path, record):
    payload = json.dumps(record, sort_keys=True, separators=(",", ":")).encode("ascii")
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), "wb") as stream:
        stream.write(payload)
        stream.flush()
        os.fsync(stream.fileno())
    fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)
    return hashlib.sha256(payload).hexdigest()


def _read_record(path, session):
    prep.private_path(path, session.home, session.uid, file=True)
    payload = artifacts._pinned_bytes(path, 65536)
    record = json.loads(payload, object_pairs_hook=artifacts._json_no_duplicates)
    _require(isinstance(record, dict), "host_record")
    return record, hashlib.sha256(payload).hexdigest()


class _OwnedRunner:
    """Retain image FDs; never signal a journal PID or supply terminal input."""
    def __init__(self, cancel):
        self.cancel, self.children = cancel, {}

    @staticmethod
    def _group_absent(pid):
        try:
            os.killpg(pid, 0)
        except ProcessLookupError:
            return True
        return False

    def start(self, argv, env, phase, *, image, cwd=None, capture=False):
        _require(argv[0] == str(image.path) and phase in prep.PHASES, "command")
        if self.cancel():
            raise KeyboardInterrupt
        fd = os.open(image.path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        child = None
        try:
            _require(prep.snapshot(os.fstat(fd)) == image.identity
                     and _safe_hash(image.path) == image.sha256, "image_changed")
            child = subprocess.Popen(argv, cwd=cwd, env=dict(env), stdin=subprocess.DEVNULL,
                                     stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
                                     stderr=subprocess.PIPE if capture else subprocess.DEVNULL,
                                     start_new_session=True, close_fds=True)
            self.children[child] = fd
            # A short-lived/unobservable image fails closed. This narrows, but
            # does not eliminate, the documented same-UID Mac pathname-exec gap.
            observed = _image_path(child.pid)
            _require(bool(observed) and Path(observed) == image.path, "process_image")
            _require(prep.snapshot(os.fstat(fd)) == image.identity
                     and prep.snapshot(image.path.lstat()) == image.identity
                     and _safe_hash(image.path) == image.sha256, "image_changed")
            return child
        except BaseException:
            if child is None:
                os.close(fd)
            else:
                self._stop(child)
            raise

    def _release(self, child):
        if not self._group_absent(child.pid):
            raise artifacts.UnreapedProcessError("owned_group_remains")
        child.wait(timeout=2)
        for stream in (child.stdout, child.stderr):
            if stream is not None:
                stream.close()
        fd = self.children.pop(child, None)
        if fd is not None:
            os.close(fd)

    def _stop(self, child):
        # Only the still-live original leader can authorize a group signal.
        # Exited leaders with unknown descendants require recovery, not a kill.
        try:
            if child.poll() is None:
                if os.getpgid(child.pid) != child.pid:
                    raise artifacts.UnreapedProcessError("owned_group_unproven")
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=2)
            deadline = time.monotonic() + 2
            while not self._group_absent(child.pid):
                if time.monotonic() >= deadline:
                    raise artifacts.UnreapedProcessError("owned_group_remains")
                time.sleep(0.02)
            self._release(child)
        except (OSError, subprocess.SubprocessError) as error:
            raise artifacts.UnreapedProcessError("owned_process_unreaped") from error

    def run(self, argv, env, timeout, phase, *, image, cwd=None):
        child = self.start(argv, env, phase, image=image, cwd=cwd, capture=True)
        selector, output = selectors.DefaultSelector(), {child.stdout: bytearray(), child.stderr: bytearray()}
        deadline = time.monotonic() + timeout
        try:
            for stream in output:
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ)
            while selector.get_map() or child.poll() is None:
                if self.cancel():
                    raise KeyboardInterrupt
                if time.monotonic() >= deadline:
                    raise TimeoutError
                for key, _ in selector.select(timeout=0.05):
                    block = os.read(key.fd, 65536)
                    if block:
                        output[key.fileobj].extend(block)
                        _require(sum(map(len, output.values())) <= MAX_OUTPUT, "process_output")
                    else:
                        selector.unregister(key.fileobj)
            _require(child.returncode == 0, "process_exit")
            result = bytes(output[child.stdout]) + b"\n" + bytes(output[child.stderr])
            self._release(child)
            return result
        except BaseException:
            self._stop(child)
            raise
        finally:
            selector.close()

    def wait(self, child, timeout):
        deadline = time.monotonic() + timeout
        while child.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError
            time.sleep(0.05)
        _require(child.returncode == 0, "core_exit")
        self._release(child)

    def stop_all(self):
        ok = True
        for child in tuple(self.children):
            try:
                self._stop(child)
            except (Exception, KeyboardInterrupt):
                ok = False
        return ok and not self.children


class NativeHost:
    def __init__(self, session, root, work, candidate_commit, candidate_sha, check_cancel=None):
        self.session, self.root, self.work = session, root, work
        self.candidate_commit, self.candidate_sha = candidate_commit, candidate_sha
        callback = check_cancel or (lambda: None)
        def checked_cancel():
            if callback():
                raise KeyboardInterrupt
        self.check_cancel = checked_cancel
        self.runner, self.state = _OwnedRunner(self.check_cancel), "new"
        self.manifest = self.manifest_hash = self.key_identity = None
        self.app_identity = self.db_identity = self.runtime_identity = None
        self.secret_root_identity = None
        self.core = self.core_image = None
        self.products = 0
        self.credential_phases = 0

    def _binding(self):
        return {"uid": self.session.uid, "root_identity": prep.identity(self.root.lstat()),
                "run_id": self.root.name, "architecture": self.session.architecture,
                "candidate_commit": self.candidate_commit, "candidate_sha256": self.candidate_sha,
                "synthetic_keychain_identity": prep.identity(prep.private_path(
                    self.root / "synthetic.keychain-db", self.session.home, self.session.uid, file=True)),
                "archives": {name: list(prep.snapshot(prep.private_path(
                    self.root / f"{name}-package.tar.gz", self.session.home, self.session.uid, file=True)))
                             for name in ("old", "candidate")}}

    def _runtime(self):
        base = Path(prep.command(["/usr/bin/getconf", "DARWIN_USER_TEMP_DIR"]).strip())
        _require(base.is_absolute() and base.stat().st_uid == self.session.uid
                 and base.stat().st_mode & 0o077 == 0, "runtime")
        return base / "mail-hantong"

    def _socket_absent(self):
        try:
            prep.absent(self._runtime() / "core-v2.sock")
            return True
        except (OSError, prep.IsolationError):
            return False

    def _source(self, name, commit):
        source = self.root / name
        tree = prep.source_revision(source, commit, self.session.home, self.session.uid)
        prep.absent(source / artifacts.MODULE_PATH)
        return {"commit": commit, "source_hash": prep.source_hash(source), "tree": tree,
                "lib_sha256": _safe_hash(source / "crates/hantong-secrets/src/lib.rs", self.session.uid)}

    def _sources(self):
        return {"old": self._source("old-source", prep.OLD_COMMIT),
                "candidate": self._source("candidate-source", self.candidate_commit)}

    def _verify_manifest(self, digest):
        record, actual = _read_record(self.work / "artifacts.json", self.session)
        _require(actual == digest and set(record) == {"schema", "binding", "sources", "images", "fixture_sha256"}
                 and record["schema"] == "hantong-interactive-artifacts-v1"
                 and record["binding"] == self._binding(), "artifact_receipt")
        _require(record["sources"] == self._sources(), "source_changed")
        expected = {f"{v}-{role}": self.work / f"{v}-product/bin" / ("hantong" if role == "cli" else "mail-hantong-core")
                    for v in ("old", "candidate") for role in ("cli", "core")}
        _require(isinstance(record["images"], dict)
                 and set(record["images"]) == set(expected) | {"seed", "upgrade", "reopen"}
                 and record["images"]["upgrade"] == record["images"]["reopen"], "artifact_receipt")
        for name, binding in record["images"].items():
            _require(isinstance(binding, dict) and isinstance(binding.get("path"), str), "artifact_path")
            path = Path(binding["path"])
            if name in expected:
                _require(path == expected[name], "artifact_path")
            else:
                target = self.work / ("old-target" if name == "seed" else "candidate-target") / "debug/deps"
                _require(path.parent == target and path.name.startswith("hantong_secrets-"), "artifact_path")
            _image_from(binding, self.session)
        return record

    def guard(self):
        _require(prep.collect_session() == self.session, "session_changed")
        prep.validate_session(self.session)
        prep.run_root(self.session, self.root.name)
        prep.private_path(self.work, self.session.home, self.session.uid)
        required = {"baseline.json", "synthetic.keychain-db", "old-package.tar.gz", "candidate-package.tar.gz",
                    "old-source", "candidate-source", "controller"}
        _require({p.name for p in self.root.iterdir()} == required, "root_entries")
        prep.load_baseline(self.session, self.root.name, self.root)
        key = self.root / "synthetic.keychain-db"
        identity = prep.identity(prep.private_path(key, self.session.home, self.session.uid, file=True))
        if self.key_identity is None:
            self.key_identity = identity
        _require(identity == self.key_identity, "keychain_replaced")
        _require(prep._baseline(self.session.home, self.session.uid) == (key, (key,)), "keychain_selection")
        if self.state == "new":
            prep.product_absent(self.session)
            self.state = "guarded"
        if self.manifest_hash:
            self.manifest = self._verify_manifest(self.manifest_hash)

    def _package_metadata(self, package, source):
        read = lambda p, n: json.loads(artifacts._pinned_bytes(p, n), object_pairs_hook=artifacts._json_no_duplicates)
        manifest = read(package / "release-manifest.json", 4 * 1024**2)
        _require(any(row["path"] == "BUILD-METADATA.json" for row in manifest["files"]), "package_source")
        metadata = read(package / "BUILD-METADATA.json", 65536)
        _require(metadata.get("format_version") == 1 and metadata.get("build_performed") is True
                 and metadata.get("source_relation") == "built_from_current_workspace"
                 and metadata.get("source_hash") == source["source_hash"], "package_source")

    def build(self):
        _require(self.state == "guarded", "state")
        self.check_cancel()
        sources = self._sources()
        fixture = self.root / "candidate-source/scripts/fixtures/macos_keychain_compat_module.rs"
        fixture_hash = _safe_hash(fixture, self.session.uid)
        arch = "aarch64" if self.session.architecture == "macos-aarch64" else "x86_64"
        tools = self.session.home / f".rustup/toolchains/1.95.0-{arch}-apple-darwin/bin"
        cargo, rustc = _seal(tools / "cargo", self.session), _seal(tools / "rustc", self.session)
        cargo_home = self.session.home / ".cargo"
        prep.private_path(cargo_home, self.session.home, self.session.uid)
        for name in ("config", "config.toml", "credentials", "credentials.toml"):
            prep.absent(cargo_home / name)
        env = prep.readonly_environment() | {"CARGO_HOME": str(cargo_home), "RUSTC": str(rustc.path)}
        images = {}
        for version in ("old", "candidate"):
            self.check_cancel()
            product = self.work / f"{version}-product"
            digest = prep.OLD_SHA[self.session.architecture] if version == "old" else self.candidate_sha
            artifacts.extract_verified_archive(self.root / f"{version}-package.tar.gz", product, digest)
            artifacts.verify_extracted_package(product, self.session.architecture)
            self._package_metadata(product, sources[version])
            for role in ("core", "cli"):
                path = product / "bin" / ("mail-hantong-core" if role == "core" else "hantong")
                images[f"{version}-{role}"] = _image_json(_seal(path, self.session))
            target = self.work / f"{version}-target"
            target.mkdir(mode=0o700)
            source = self.root / f"{version}-source"

            def compile_command(command, **kwargs):
                _require(command[:2] == ["cargo", "test"] and kwargs["cwd"] == source, "compile_command")
                _require(_seal(rustc.path, self.session) == rustc, "compiler_changed")
                output = self.runner.run([str(cargo.path), *command[1:]], kwargs["env"], kwargs["timeout"],
                                         "source-old-imap-smtp-seed" if version == "old"
                                         else "source-candidate-read-preserve-update", image=cargo, cwd=source)
                return subprocess.CompletedProcess(command, 0, output, b"")

            try:
                executable = artifacts.build_fixture(source, target, version, run_command=compile_command,
                                                    environment=env, fixture=fixture, cancel=self.check_cancel)
            except artifacts.ArtifactError as error:
                if str(error) == "cancelled":
                    raise KeyboardInterrupt from None
                if str(error) == "compile_timeout":
                    raise TimeoutError from None
                raise
            _require(_safe_hash(fixture, self.session.uid) == fixture_hash, "fixture_changed")
            after = self._source(f"{version}-source", sources[version]["commit"])
            _require(after["source_hash"] == sources[version]["source_hash"]
                     and after["lib_sha256"] == sources[version]["lib_sha256"], "source_restoration")
            sources[version] = after
            image = _image_json(_seal(executable, self.session))
            if version == "old":
                images["seed"] = image
            else:
                images["upgrade"] = images["reopen"] = image
        record = {"schema": "hantong-interactive-artifacts-v1", "binding": self._binding(),
                  "sources": sources, "images": images, "fixture_sha256": fixture_hash}
        self.manifest_hash = _write_once(self.work / "artifacts.json", record)
        self.manifest = self._verify_manifest(self.manifest_hash)
        self.state = "built"
        return {"source_restored": True, "artifact_receipt_sha256": self.manifest_hash}

    def _verified_image(self, name):
        _require(self.manifest is not None, "artifact_receipt")
        return _image_from(self.manifest["images"][name], self.session)

    def _verify_database(self):
        app = self.session.home / "Library/Application Support/mail-hantong"
        identity = prep.identity(prep.private_path(app, self.session.home, self.session.uid))
        if self.app_identity is None:
            self.app_identity = identity
        _require(identity == self.app_identity, "app_replaced")
        path = app / "data/mail-hantong.db"
        info = prep.private_path(path, self.session.home, self.session.uid, file=True)
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        try:
            _require(prep.snapshot(os.fstat(fd)) == prep.snapshot(info), "database_replaced")
            header = os.read(fd, 16)
            _require(len(header) == 16 and header != b"SQLite format 3\x00", "database_ciphertext")
            _require(prep.snapshot(os.fstat(fd)) == prep.snapshot(info)
                     and prep.snapshot(path.lstat()) == prep.snapshot(info), "database_replaced")
        finally:
            os.close(fd)
        if self.db_identity is None:
            self.db_identity = prep.identity(info)
        _require(prep.identity(info) == self.db_identity, "database_replaced")

    def _runtime_identity(self):
        root = self._runtime()
        info = root.lstat()
        _require(stat.S_ISDIR(info.st_mode) and info.st_uid == self.session.uid
                 and stat.S_IMODE(info.st_mode) == 0o700 and not root.is_symlink(), "runtime")
        _require({p.name for p in root.iterdir()} <= {"core-v2.sock", "core-v2.lock"}, "runtime_entries")
        prep.no_extended_acl(root)
        for path in root.iterdir():
            leaf = path.lstat()
            _require(leaf.st_uid == self.session.uid and stat.S_IMODE(leaf.st_mode) == 0o600
                     and (stat.S_ISSOCK(leaf.st_mode) if path.name.endswith(".sock")
                          else stat.S_ISREG(leaf.st_mode) and leaf.st_nlink == 1), "runtime_entry")
        return prep.identity(info)

    def _stop_core(self):
        child, image = self.core, self.core_image
        if child is None:
            return
        saved = self.runner.cancel
        self.runner.cancel = lambda: None
        try:
            self.runner.run([str(image.path), "unix-stop"], prep.readonly_environment(), 20,
                            "official-candidate-db-restart", image=image)
            self.runner.wait(child, 20)
            _require(self._socket_absent(), "socket_present")
            self.core = self.core_image = None
        except BaseException:
            self.runner._stop(child)
            self.core = self.core_image = None
            raise
        finally:
            self.runner.cancel = saved

    def product(self, version, iteration):
        _require(self.state == "built" and self.products < 3
                 and (version, iteration) == (("old", 1), ("candidate", 1), ("candidate", 2))[self.products], "phase")
        self.guard()
        _require(self._socket_absent() and self.core is None, "existing_core")
        if self.products == 0:
            app = self.session.home / "Library/Application Support/mail-hantong"
            prep.private_path(app.parent, self.session.home, self.session.uid)
            app.mkdir(mode=0o700)
            self.app_identity = prep.identity(prep.private_path(app, self.session.home, self.session.uid))
            self._runtime().mkdir(mode=0o700)
            self.runtime_identity = self._runtime_identity()
        else:
            self._verify_database()
            _require(self._runtime_identity() == self.runtime_identity, "runtime_replaced")
        core, cli = self._verified_image(f"{version}-core"), self._verified_image(f"{version}-cli")
        phase = "official-old-db-seed" if version == "old" else "official-candidate-db-read"
        self.core_image = core
        self.core = self.runner.start([str(core.path), "run"], prep.readonly_environment(), phase, image=core)
        deadline = time.monotonic() + 45
        try:
            while not (self._runtime() / "core-v2.sock").exists():
                self.check_cancel()
                _require(self.core.poll() is None, "core_exit")
                if time.monotonic() >= deadline:
                    raise TimeoutError
                time.sleep(0.05)
            runtime = self._runtime_identity()
            if self.runtime_identity is None:
                self.runtime_identity = runtime
            _require(runtime == self.runtime_identity, "runtime_replaced")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError
            output = self.runner.run([str(cli.path), "core", "status", "--json"], prep.readonly_environment(),
                                     remaining, phase, image=cli)
            status = json.loads(output, object_pairs_hook=artifacts._json_no_duplicates)
            expected = {"transport_ready": True, "mail_ready": False, "authenticated": False, "protocol_version": 1}
            _require(status.get("ok") is True and status.get("items") == [expected]
                     and status.get("completeness") == "complete" and status.get("account_results") == [], "readiness")
        finally:
            self._stop_core()
        self._verify_database()
        self.products += 1

    def credentials(self, phase):
        _require(self.state == "built" and self.products == 3 and self.credential_phases < 3
                 and phase == ("seed", "upgrade", "reopen")[self.credential_phases], "phase")
        self.guard()
        _require(self._socket_absent(), "socket_present")
        secret_root = self.work / "synthetic-secrets"
        if phase == "seed":
            secret_root.mkdir(mode=0o700)
        identity = prep.identity(prep.private_path(secret_root, self.session.home, self.session.uid))
        if phase == "seed":
            _require(self.secret_root_identity is None, "secret_root_reused")
            self.secret_root_identity = identity
        _require(identity == self.secret_root_identity, "secret_root_replaced")
        image = self._verified_image(phase)
        env = prep.readonly_environment() | {"HANTONG_COMPAT_ROOT": str(secret_root), "HANTONG_COMPAT_PHASE": phase}
        output = self.runner.run([str(image.path), "--exact", TEST, "--nocapture"], env, 900,
                                 {"seed": "source-old-imap-smtp-seed", "upgrade": "source-candidate-read-preserve-update",
                                  "reopen": "source-candidate-reopen"}[phase], image=image)
        text = output.decode("utf-8")
        _require(len(re.findall(r"^running 1 test$", text, re.MULTILINE)) == 1
                 and len(re.findall(r"^test " + re.escape(TEST) + r" \.\.\. ok$", text, re.MULTILINE)) == 1, "fixture_result")
        pair = lambda label: [f"{label}-selector", f"{label}-backend", f"{label}-returned"]
        expected = ["open", "opened"]
        if phase == "upgrade":
            expected += ["read-old", *pair("imap-old"), *pair("smtp-old"), "write-new", "read-new", *pair("imap-new"), *pair("smtp-old")]
        else:
            expected += ["read", *pair("imap-old" if phase == "seed" else "imap-new"), *pair("smtp-old")]
        markers = re.findall(r"^HANTONG_COMPAT_MARKER phase=([a-z]+) step=([a-z-]+)$", text, re.MULTILINE)
        _require(markers == [(phase, step) for step in expected + ["done"]], "fixture_result")
        self.guard()
        _require(prep.identity(prep.private_path(secret_root, self.session.home, self.session.uid))
                 == self.secret_root_identity, "secret_root_replaced")
        self.credential_phases += 1

    def shutdown(self):
        ok = True
        try:
            self._stop_core()
        except BaseException:
            ok = False
        ok = self.runner.stop_all() and ok
        try:
            ok = bool(ok and self.manifest_hash and self._socket_absent())
            if ok:
                self._verify_manifest(self.manifest_hash)
                if self.runtime_identity is not None:
                    _require(self._runtime_identity() == self.runtime_identity, "runtime_replaced")
                if self.secret_root_identity is not None:
                    _require(prep.identity(prep.private_path(self.work / "synthetic-secrets",
                        self.session.home, self.session.uid)) == self.secret_root_identity, "secret_root_replaced")
            record = {"schema": "hantong-interactive-shutdown-v1", "binding": self._binding(),
                      "artifact_receipt_sha256": self.manifest_hash, "shutdown_verified": ok,
                      "app_identity": self.app_identity, "db_identity": self.db_identity,
                      "runtime_identity": self.runtime_identity, "secret_root_identity": self.secret_root_identity}
            _write_once(self.work / "shutdown.json", record)
        except (Exception, KeyboardInterrupt):
            return False
        return ok

    def restored(self, expected_receipt_sha256):
        _require(prep.collect_session() == self.session, "session_changed")
        prep.validate_session(self.session)
        self._verify_manifest(expected_receipt_sha256)
        record, _ = _read_record(self.work / "shutdown.json", self.session)
        _require(set(record) == {"schema", "binding", "artifact_receipt_sha256", "shutdown_verified",
                                "app_identity", "db_identity", "runtime_identity", "secret_root_identity"}
                 and record["schema"] == "hantong-interactive-shutdown-v1"
                 and record["binding"] == self._binding() and record["shutdown_verified"] is True
                 and record["artifact_receipt_sha256"] == expected_receipt_sha256, "shutdown_unproven")
        _require(self._socket_absent(), "socket_present")
        if record["runtime_identity"] is not None:
            _require(self._runtime_identity() == record["runtime_identity"], "runtime_replaced")
        if record["secret_root_identity"] is not None:
            _require(prep.identity(prep.private_path(self.work / "synthetic-secrets",
                self.session.home, self.session.uid)) == record["secret_root_identity"], "secret_root_replaced")
        self.app_identity, self.db_identity = record["app_identity"], record["db_identity"]
        if self.app_identity is not None or self.db_identity is not None:
            _require(self.app_identity is not None and self.db_identity is not None, "database_binding")
            self._verify_database()
        expected = prep.load_baseline(self.session, self.root.name, self.root)
        _require(prep._baseline(self.session.home, self.session.uid) == expected, "restore_selection")
        return True
