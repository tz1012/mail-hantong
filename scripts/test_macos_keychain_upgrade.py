#!/usr/bin/env python3
"""Run an isolated old-source -> candidate-source macOS Keychain check.

The old and candidate test binaries share one CI-only Keychain.  The fixture
uses synthetic mail slots, updates one item, reopens both, and never touches
the user's Keychain, accounts, or mail data.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import shutil
import signal
import stat
import subprocess

from run_macos_test import _baseline, _validate_environment

FIXTURE = Path("scripts/fixtures/macos_keychain_compat_module.rs")
TEST = Path("crates/hantong-secrets/src/macos_keychain_compat.rs")
MODULE_MARKER = b"mod macos_keychain_compat;"


def fail(message: str) -> None:
    raise RuntimeError(message)


def checked_root(value: Path, label: str) -> Path:
    if not value.is_absolute() or value.is_symlink() or not value.is_dir():
        fail(f"{label} must be an existing non-symlink directory")
    resolved = value.resolve(strict=True)
    if resolved.stat().st_mode & 0o022:
        fail(f"{label} must not be group/world writable")
    return resolved


def _kill_process_group(process: subprocess.Popen[str]) -> None:
    # Cargo can exit while a test still owns its output pipe. Kill the owned
    # session even when its leader has exited; never leave Keychain users alive.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=10)


def run_test(source: Path, target: Path, root: Path, phase: str, expected_keychain: Path) -> None:
    current_default, current_list = _baseline()
    if current_default != expected_keychain or current_list != (expected_keychain,):
        fail(f"Keychain baseline changed before {phase}")
    env = dict(os.environ)
    env.update(HANTONG_COMPAT_ROOT=str(root), HANTONG_COMPAT_PHASE=phase)
    env["CARGO_TARGET_DIR"] = str(target)
    # The normal Mac CI runner gives each test executable a fresh temporary
    # Keychain. This harness deliberately spans processes, so use the already
    # validated job Keychain for all three phases.
    env.pop("CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER", None)
    env.pop("CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER", None)
    process = subprocess.Popen(
        ["cargo", "test", "-p", "hantong-secrets", "--lib", "--locked", "--offline", "--",
         "--exact", "macos_keychain_compat::mac_keychain_compatibility_fixture"],
        cwd=source, env=env, start_new_session=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        output, _ = process.communicate(timeout=900)
    except subprocess.TimeoutExpired:
        _kill_process_group(process)
        raise RuntimeError(f"{phase} compatibility test timed out")
    if process.returncode != 0:
        raise RuntimeError(f"{phase} compatibility test failed: {output[-4096:]}")
    if "running 1 test" not in output or "mac_keychain_compatibility_fixture ... ok" not in output:
        raise RuntimeError(f"{phase} compatibility test did not execute exactly one passing fixture")
    current_default, current_list = _baseline()
    if current_default != expected_keychain or current_list != (expected_keychain,):
        fail(f"Keychain baseline changed after {phase}")


def main() -> int:
    if platform.system() != "Darwin" or os.environ.get("CI") != "true" or os.environ.get("GITHUB_ACTIONS") != "true":
        fail("macOS Keychain upgrade check is restricted to GitHub CI")
    keychain = Path(os.environ.get("HANTONG_TEST_KEYCHAIN", ""))
    runner_temp = Path(os.environ.get("RUNNER_TEMP", ""))
    if not keychain.is_absolute() or not runner_temp.is_absolute() or keychain.is_symlink() or not keychain.is_file():
        fail("isolated CI Keychain is missing or unsafe")
    if keychain.parent.resolve(strict=True) != runner_temp.resolve(strict=True):
        fail("isolated CI Keychain is outside RUNNER_TEMP")
    _validate_environment()
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-source", type=Path, required=True)
    parser.add_argument("--candidate-source", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    args = parser.parse_args()
    old_source = checked_root(args.old_source, "old source")
    candidate_source = checked_root(args.candidate_source, "candidate source")
    work_dir = checked_root(args.work_dir, "work directory")
    fixture = candidate_source / FIXTURE
    if not fixture.is_file() or fixture.is_symlink():
        fail("compatibility fixture is missing")
    injected = []
    originals = {}
    root = work_dir / "keychain-compat-secrets"
    if root.exists() or root.is_symlink():
        fail("refusing to overwrite compatibility root")
    try:
        for source in (old_source, candidate_source):
            destination = source / TEST
            lib = source / "crates/hantong-secrets/src/lib.rs"
            destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            if destination.exists() or destination.is_symlink():
                fail("refusing to overwrite an existing compatibility test")
            lib_bytes = lib.read_bytes()
            if MODULE_MARKER in lib_bytes:
                fail("compatibility test module is already present")
            originals[lib] = lib_bytes
            shutil.copyfile(fixture, destination)
            os.chmod(destination, stat.S_IRUSR | stat.S_IWUSR)
            injected.append((destination, lib))
            lib.write_bytes(lib_bytes + b"\n#[cfg(all(test, target_os = \"macos\"))]\nmod macos_keychain_compat;\n")
        root = work_dir / "keychain-compat-secrets"
        root.mkdir(mode=0o700)
        old_target = work_dir / "old-target"
        candidate_target = work_dir / "candidate-target"
        old_target.mkdir(mode=0o700)
        candidate_target.mkdir(mode=0o700)
        expected_keychain = keychain.resolve(strict=True)
        run_test(old_source, old_target, root, "seed", expected_keychain)
        run_test(candidate_source, candidate_target, root, "upgrade", expected_keychain)
        run_test(candidate_source, candidate_target, root, "reopen", expected_keychain)
    finally:
        for destination, lib in injected:
            if destination.exists() or destination.is_symlink():
                destination.unlink()
            lib.write_bytes(originals[lib])
        if root.exists():
            shutil.rmtree(root)
    print("macOS old-source Keychain seed, candidate update, and restart preservation passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"error: macOS Keychain upgrade check failed: {error}", file=__import__("sys").stderr)
        raise SystemExit(2)
