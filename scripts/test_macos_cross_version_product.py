#!/usr/bin/env python3
"""Exercise the released macOS binary followed by the candidate binary.

Both binaries use the same isolated CI Keychain and real application data
root.  The old binary creates the encrypted database/keychain item; the
candidate must reopen it after a process stop and restart.  No provider or
user account is involved.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import pwd
import shutil
import subprocess
import tempfile
import time

from test_macos_installed_product import extract_archive, run_cli, stop_core
from run_macos_test import _baseline, _validate_environment


def fail(message: str) -> None:
    raise RuntimeError(message)


def runtime_socket() -> Path:
    result = subprocess.run(["/usr/bin/getconf", "DARWIN_USER_TEMP_DIR"], check=True,
                            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=10)
    base = Path(result.stdout.strip())
    if not base.is_absolute() or base.is_symlink() or not base.is_dir():
        fail("macOS per-user runtime directory is invalid")
    return base / "mail-hantong" / "core-v2.sock"


def assert_socket_absent(path: Path, phase: str) -> None:
    if path.exists() or path.is_symlink():
        fail(f"Core runtime socket still exists after {phase}")


def assert_keychain_baseline(expected: Path, phase: str) -> None:
    current_default, current_list = _baseline()
    if current_default != expected or current_list != (expected,):
        fail(f"Keychain baseline changed {phase}")


def install(package_root: Path, install_root: Path, bin_dir: Path, environment: dict[str, str]) -> Path:
    subprocess.run(
        ["sh", str(package_root / "install.sh"), "--install-root", str(install_root), "--bin-dir", str(bin_dir)],
        env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=30, check=True,
    )
    versions = install_root / "versions"
    if versions.is_symlink() or not versions.is_dir():
        fail("installed version root is invalid")
    entries = [item for item in versions.iterdir() if item.is_dir() and not item.is_symlink()]
    if len(entries) != 1:
        fail("installed package must contain exactly one version directory")
    installed = entries[0] / "bin"
    cli = installed / "hantong"
    core = installed / "mail-hantong-core"
    if not cli.is_file() or not core.is_file():
        fail("installed binary set is incomplete")
    return cli


def run_version(package: Path, root: Path, install_root: Path, bin_dir: Path, environment: dict[str, str], restart: bool, socket: Path) -> None:
    stage = root / f"stage-{install_root.parent.name}"
    stage.mkdir(mode=0o700)
    extract_archive(package, stage)
    package_root = stage
    nested = [item for item in stage.iterdir() if item.is_dir()]
    if not (stage / "install.sh").is_file():
        if len(nested) != 1 or not (nested[0] / "install.sh").is_file():
            fail("package installer is missing")
        package_root = nested[0]
    cli = install(package_root, install_root, bin_dir, environment)
    versions = install_root / "versions"
    entries = [item for item in versions.iterdir() if item.is_dir() and not item.is_symlink()]
    if len(entries) != 1:
        fail("installed package version directory is ambiguous")
    core = entries[0] / "bin" / "mail-hantong-core"
    database = Path(pwd.getpwuid(os.geteuid()).pw_dir) / "Library" / "Application Support" / "mail-hantong" / "data" / "mail-hantong.db"
    stopped = False
    try:
        for _ in range(2 if restart else 1):
            stopped = False
            run_cli(cli, environment)
            subprocess.run([str(cli), "core", "status", "--json"], env=environment, stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=True)
            if not database.is_file() or database.stat().st_size == 0:
                fail("old/candidate Core did not preserve an encrypted database")
            with database.open("rb") as handle:
                if handle.read(16) == b"SQLite format 3\x00":
                    fail("old/candidate Core created a plaintext database")
            stop_core(core, environment)
            stopped = True
            deadline = time.monotonic() + 10
            while socket.exists() and time.monotonic() < deadline:
                time.sleep(0.1)
            assert_socket_absent(socket, f"{install_root.name} run")
    finally:
        if not stopped:
            subprocess.run([str(core), "unix-stop"], env=environment, stdin=subprocess.DEVNULL,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20, check=False)


def main() -> int:
    if platform.system() != "Darwin" or os.environ.get("CI") != "true" or os.environ.get("GITHUB_ACTIONS") != "true":
        fail("cross-version product check is restricted to GitHub CI")
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-package", type=Path, required=True)
    parser.add_argument("--candidate-package", type=Path, required=True)
    args = parser.parse_args()
    if not args.old_package.is_file() or not args.candidate_package.is_file():
        fail("both package archives are required")
    home = Path(pwd.getpwuid(os.geteuid()).pw_dir)
    app_root = home / "Library" / "Application Support" / "mail-hantong"
    if app_root.exists() or app_root.is_symlink():
        fail("refusing to touch an existing macOS application directory")
    socket = runtime_socket()
    assert_socket_absent(socket, "preflight")
    environment = dict(os.environ)
    _validate_environment()
    expected_keychain = Path(environment["HANTONG_TEST_KEYCHAIN"]).resolve(strict=True)
    assert_keychain_baseline(expected_keychain, "before product upgrade")
    temporary = tempfile.TemporaryDirectory(prefix="hantong-macos-cross-version-")
    root = Path(temporary.name).resolve()
    old_install = root / "old-install" / "mail-hantong-cli"
    candidate_install = root / "candidate-install" / "mail-hantong-cli"
    old_bin = root / "old-bin"
    candidate_bin = root / "candidate-bin"
    for directory in (old_install, candidate_install, old_bin, candidate_bin):
        directory.mkdir(mode=0o700, parents=True)
    try:
        run_version(args.old_package, root, old_install, old_bin, environment, restart=False, socket=socket)
        assert_keychain_baseline(expected_keychain, "after old product")
        run_version(args.candidate_package, root, candidate_install, candidate_bin, environment, restart=True, socket=socket)
        assert_keychain_baseline(expected_keychain, "after candidate product")
    finally:
        if app_root.exists() and socket.exists():
            fail("refusing cleanup while the Core runtime socket exists")
        if app_root.exists():
            shutil.rmtree(app_root)
        temporary.cleanup()
    print("macOS released-binary database/keychain reopen passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"error: macOS cross-version product check failed: {error}", file=__import__("sys").stderr)
        raise SystemExit(2)
