#!/usr/bin/env python3
"""Offline Linux R2 activation smoke using two verified real packages.

The old and candidate evidence directories must contain the package archive,
source archive, and linux-pinned-candidate.json produced by package_unix.py.
The candidate is never re-labelled or rebuilt.  This test uses only a private
install/XDG root and synthetic state; it does not fetch, publish, or contact a
provider.  It proves installer + sealed Core bootstrap/activation/restart, not
the public-origin download step.
"""
from __future__ import annotations

import argparse
from contextlib import ExitStack
import stat
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from linux_pinned_candidate import CandidateError, CandidateReport, verify_candidate  # noqa: E402
from linux_product_pty import console  # noqa: E402


def fail(message: str) -> "NoReturn":
    raise RuntimeError(message)


def one_file(directory: Path, pattern: str) -> Path:
    values = sorted(directory.glob(pattern))
    if len(values) != 1 or not values[0].is_file() or values[0].is_symlink():
        fail(f"expected exactly one regular {pattern}")
    return values[0]


def evidence(directory: Path) -> tuple[Path, Path, Path, CandidateReport]:
    directory = directory.resolve()
    if not directory.is_dir() or directory.is_symlink():
        fail("evidence directory is not a real directory")
    descriptor = one_file(directory, "linux-pinned-candidate.json")
    value = json.loads(descriptor.read_text(encoding="utf-8"))
    package = directory / value["package_filename"]
    source = directory / value["source_filename"]
    if not package.is_file() or package.is_symlink() or not source.is_file() or source.is_symlink():
        fail("candidate evidence is incomplete")
    try:
        report = verify_candidate(descriptor, package, source, "Linux", "x86_64")
    except (CandidateError, OSError, ValueError) as error:
        fail(f"candidate evidence rejected: {error}")
    return descriptor, package, source, report


def extract(package: Path, destination: Path, expected_sha: str) -> None:
    with package.open("rb") as stream:
        payload = stream.read(256 * 1024 * 1024 + 1)
    if len(payload) > 256 * 1024 * 1024 or hashlib.sha256(payload).hexdigest() != expected_sha:
        fail("package changed after evidence verification")
    destination.mkdir(mode=0o700, parents=True)
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as archive:
        members = archive.getmembers()
        for member in members:
            name = Path(member.name)
            if name.is_absolute() or ".." in name.parts or "\\" in member.name:
                fail("package contains an unsafe path")
            if not member.isfile() or name == Path("."):
                fail("package contains a non-regular entry")
        archive.extractall(destination, filter="data")
    if not (destination / "install.sh").is_file():
        fail("package installer is missing")


def pin(install: Path, descriptor: Path, report: CandidateReport) -> None:
    target = install / "release-pins" / report.version / report.commit
    target.mkdir(mode=0o700, parents=True)
    for parent in (install / "release-pins", install / "release-pins" / report.version, target):
        os.chmod(parent, 0o700)
    descriptor_bytes = descriptor.read_bytes()
    values = {
        "format_version": 1,
        "target": "linux-x86_64",
        "version": report.version,
        "commit": report.commit,
        "descriptor_sha256": hashlib.sha256(descriptor_bytes).hexdigest(),
    }
    for name, data in (("descriptor.json", descriptor_bytes),
                       ("pin.json", json.dumps(values, sort_keys=True, separators=(",", ":")).encode())):
        path = target / name
        with path.open("xb") as stream:
            os.chmod(path, 0o600)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())


def run(command: list[str], env: dict[str, str], timeout: int = 30) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(command, env=env, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          timeout=timeout, check=False)


def wait_status(cli: Path, env: dict[str, str], deadline: float) -> None:
    while time.monotonic() < deadline:
        result = run([str(cli), "core", "status", "--json"], env, 10)
        if result.returncode == 0:
            return
        time.sleep(0.25)
    fail("Core did not become ready")


def version_key(value: str) -> tuple[int, int, int, tuple[tuple[int, object], ...]]:
    base, separator, pre = value.partition("-")
    if not separator:
        return (*tuple(int(part) for part in base.split(".")), ((2, ""),))
    identifiers = tuple(
        (0, int(part)) if part.isdigit() else (1, part)
        for part in pre.split(".")
    )
    return (*tuple(int(part) for part in base.split(".")), identifiers)


def snapshot_app(root: Path) -> dict[str, tuple[int, str]]:
    result: dict[str, tuple[int, str]] = {}
    if not root.is_dir():
        return result
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            fail("synthetic app state contains an unexpected link")
        if not path.is_file() or path.name.endswith(".lock"):
            continue
        data = path.read_bytes()
        result[str(path.relative_to(root))] = (len(data), hashlib.sha256(data).hexdigest())
    return result


def cleanup_private_root(root: Path, identity: tuple[int, int], install: Path,
                         env: dict[str, str], allowed_versions: set[str]) -> bool:
    """Retain evidence on any uncertain daemon/lock/path state; never kill by name."""
    import fcntl
    try:
        current_stat = root.lstat()
        if not stat.S_ISDIR(current_stat.st_mode) or (current_stat.st_dev, current_stat.st_ino) != identity:
            return False
        socket = root / "runtime/mail-hantong/core-v2.sock"
        if socket.exists() or socket.is_symlink():
            pointer = install / "current.txt"
            if pointer.is_symlink():
                return False
            version = pointer.read_text(encoding="ascii").strip()
            if version not in allowed_versions:
                return False
            stopped = run([str(install / "versions" / version / "bin/mail-hantong-core"), "unix-stop"], env, 20)
            if stopped.returncode != 0 or socket.exists() or socket.is_symlink():
                return False
        with ExitStack() as handles:
            for relative in ("runtime/mail-hantong/core-v2.lock", "data/mail-hantong/data/database.lock",
                             "data/mail-hantong/data/database-key.lock"):
                path = root / relative
                if not path.exists() and not path.is_symlink():
                    continue
                fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
                handles.callback(os.close, fd)
                metadata = os.fstat(fd)
                if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.geteuid():
                    return False
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            current_stat = root.lstat()
            if (current_stat.st_dev, current_stat.st_ino) != identity:
                return False
            shutil.rmtree(root)
        return True
    except (OSError, RuntimeError, UnicodeError, subprocess.SubprocessError):
        return False


def main() -> int:
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        fail("R2 offline activation smoke requires Linux x86_64")
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-evidence-dir", type=Path, required=True)
    parser.add_argument("--candidate-evidence-dir", type=Path, required=True)
    parser.add_argument("--candidate-commit", required=True)
    args = parser.parse_args()
    old_descriptor, old_package, _, old_report = evidence(args.old_evidence_dir)
    candidate_descriptor, candidate_package, _, candidate_report = evidence(args.candidate_evidence_dir)
    if not re.fullmatch(r"[0-9a-f]{40}", args.candidate_commit) or candidate_report.commit != args.candidate_commit:
        fail("candidate commit does not match the frozen test subject")
    if version_key(candidate_report.version) <= version_key(old_report.version):
        fail("candidate is not newer than the verified old package")

    root = Path(tempfile.mkdtemp(prefix="hantong-r2-activation-")).resolve()
    metadata = root.stat()
    identity = (metadata.st_dev, metadata.st_ino)
    install = root / "data/mail-hantong-cli"
    env = {}
    try:
        home = root / "home"
        install = root / "data" / "mail-hantong-cli"
        bin_dir = home / ".local" / "bin"
        for path in (home, root / "data", root / "config", root / "runtime", bin_dir):
            path.mkdir(mode=0o700, parents=True, exist_ok=True)
        env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(root / "data"),
                   XDG_CONFIG_HOME=str(root / "config"), XDG_RUNTIME_DIR=str(root / "runtime"),
                   LANG="C.UTF-8", TERM="xterm")
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"):
            env.pop(name, None)

        old_stage = root / "old-package"
        extract(old_package, old_stage, old_report.package_sha256)
        result = run(["sh", str(old_stage / "install.sh"), "--install-root", str(install),
                      "--bin-dir", str(bin_dir)], env)
        if result.returncode != 0:
            fail("real old package installer failed")
        pin(install, old_descriptor, old_report)
        old_root = install / "versions" / old_report.version

        candidate_stage = root / "candidate-package"
        extract(candidate_package, candidate_stage, candidate_report.package_sha256)
        candidate_root = install / "versions" / candidate_report.version
        if candidate_root.exists():
            fail("candidate version already exists in private install")
        shutil.copytree(candidate_stage, candidate_root, symlinks=False)
        pin(install, candidate_descriptor, candidate_report)

        cli = bin_dir / "hantong"
        old_core = old_root / "bin" / "mail-hantong-core"
        candidate_core = candidate_root / "bin" / "mail-hantong-core"
        app_root = root / "data" / "mail-hantong"
        code, menu, _ = console(cli, env, True)
        if code != 0 or not menu:
            fail("synthetic protected initialization failed")
        wait_status(cli, env, time.monotonic() + 40)
        if (install / "current.txt").read_text(encoding="ascii").strip() != old_report.version:
            fail("old package did not remain current before activation")
        stop = run([str(old_core), "unix-stop"], env, 20)
        if stop.returncode != 0:
            fail("verified old Core stop failed")
        before = snapshot_app(app_root)
        if not {"data/mail-hantong.db", "secrets/headless-v1.age"}.issubset(before):
            fail("synthetic encrypted database/vault was not created")

        code, _, _ = console(candidate_core, env, False, args=("linux-update-v2", "bootstrap"))
        if code != 0:
            fail("candidate sealed bootstrap/activation failed")
        if (install / "current.txt").read_text(encoding="ascii").strip() != candidate_report.version:
            fail("candidate activation did not advance the protected pointer")
        wait_status(cli, env, time.monotonic() + 40)
        stop = run([str(candidate_core), "unix-stop"], env, 20)
        if stop.returncode != 0:
            fail("candidate stop before preservation check failed")
        if snapshot_app(app_root) != before:
            fail("synthetic state changed during activation/restart")
        print(json.dumps({"verified_old": old_report.version,
                          "verified_candidate": candidate_report.version,
                          "activation": "sealed_bootstrap",
                          "restart": "ready",
                          "synthetic_state_preserved": True,
                          "public_origin_fetch": False}, sort_keys=True))
    finally:
        if not cleanup_private_root(root, identity, install, env, {old_report.version, candidate_report.version}):
            fail("cleanup retained isolated evidence: daemon, lock, or identity not proven idle")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, KeyError, json.JSONDecodeError, subprocess.SubprocessError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
