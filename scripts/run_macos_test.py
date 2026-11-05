#!/usr/bin/env python3
"""Run one native macOS test executable with an isolated synthetic Keychain.

This runner is intentionally restricted to the GitHub macOS CI environment.
It changes only the current user's default/search Keychain while holding a
process-wide lock, and restores the exact baseline before deleting its own
temporary Keychain.
"""
from __future__ import annotations

import os
from pathlib import Path
import platform
import signal
import stat
import subprocess
import sys
import uuid

SECURITY = "/usr/bin/security"
SYNTHETIC_PASSWORD = "hantong-ci-synthetic-keychain"
CHILD_TIMEOUT = 900


class RunnerError(RuntimeError):
    pass


def _uid() -> int:
    getter = getattr(os, "getuid", None)
    if getter is None:
        return -1
    return getter()


def _canonical(path: Path) -> Path:
    return path.expanduser().resolve(strict=True)


def _security(args: list[str], *, capture: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [SECURITY, *args], check=True, text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        timeout=30,
    )


def _paths(output: str) -> tuple[Path, ...]:
    values = []
    for line in output.splitlines():
        value = line.strip()
        if not value:
            continue
        if value.startswith('"') and value.endswith('"'):
            value = value[1:-1]
        values.append(_canonical(Path(value)))
    if not values:
        raise RunnerError("security returned an empty keychain path list")
    return tuple(values)


def _baseline() -> tuple[Path, tuple[Path, ...]]:
    default = _paths(_security(["default-keychain", "-d", "user"]).stdout)[0]
    listed = _paths(_security(["list-keychains", "-d", "user"]).stdout)
    return default, listed


def _set_baseline(default: Path, listed: tuple[Path, ...]) -> None:
    _security(["default-keychain", "-d", "user", "-s", str(default)], capture=False)
    _security(["list-keychains", "-d", "user", "-s", *map(str, listed)], capture=False)


def _validate_environment() -> tuple[Path, Path, Path]:
    if platform.system() != "Darwin" or os.environ.get("CI") != "true" or os.environ.get("GITHUB_ACTIONS") != "true":
        raise RunnerError("macOS test runner is restricted to GitHub CI")
    runner_temp = os.environ.get("RUNNER_TEMP")
    workspace = os.environ.get("GITHUB_WORKSPACE")
    configured = os.environ.get("HANTONG_TEST_KEYCHAIN")
    if not runner_temp or not workspace or not configured:
        raise RunnerError("RUNNER_TEMP, GITHUB_WORKSPACE, and HANTONG_TEST_KEYCHAIN are required")
    if not Path(runner_temp).is_absolute() or not Path(workspace).is_absolute() or not Path(configured).is_absolute():
        raise RunnerError("macOS CI paths must be absolute")
    root = _canonical(Path(runner_temp))
    workspace_root = _canonical(Path(workspace))
    if _uid() != -1 and (root.stat().st_uid != _uid() or root.stat().st_mode & 0o022):
        raise RunnerError("RUNNER_TEMP must be owned by the current user and not group/world writable")
    expected = root / "hantong-ci.keychain-db"
    if _canonical(Path(configured)) != expected:
        raise RunnerError("HANTONG_TEST_KEYCHAIN must be RUNNER_TEMP/hantong-ci.keychain-db")
    if not expected.is_file() or expected.is_symlink():
        raise RunnerError("the configured CI Keychain must already exist and be regular")
    target_path = workspace_root / "target"
    if target_path.is_symlink():
        raise RunnerError("GITHUB_WORKSPACE/target must not be a symlink")
    target = _canonical(target_path)
    if not target.is_dir():
        raise RunnerError("GITHUB_WORKSPACE/target is missing")
    return root, workspace_root, target


def _validate_child(value: str, target: Path) -> Path:
    candidate = Path(value)
    if candidate.is_symlink():
        raise RunnerError("test executable must not be a symlink")
    try:
        resolved = candidate.resolve(strict=True)
    except OSError as error:
        raise RunnerError("test executable is missing") from error
    if not resolved.is_file() or candidate.is_symlink():
        raise RunnerError("test executable must be a regular file")
    try:
        resolved.relative_to(target)
    except ValueError as error:
        raise RunnerError("test executable must be under GITHUB_WORKSPACE/target") from error
    return resolved


def _terminate(process: subprocess.Popen[bytes]) -> None:
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    except OSError:
        if process.poll() is None:
            process.terminate()
    if process.poll() is None:
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, getattr(signal, "SIGKILL", 9))
            except OSError:
                process.kill()
            process.wait(timeout=10)
    try:
        os.killpg(process.pid, getattr(signal, "SIGKILL", 9))
    except ProcessLookupError:
        pass
    except OSError:
        if process.poll() is None:
            process.kill()


def _validate_lock(lock_fd: int) -> None:
    lock_stat = os.fstat(lock_fd)
    if not stat.S_ISREG(lock_stat.st_mode) or (_uid() != -1 and lock_stat.st_uid != _uid()) or lock_stat.st_mode & 0o777 != 0o600:
        raise RunnerError("Keychain lock must be private to the current user")


def run(argv: list[str]) -> int:
    root, workspace, target = _validate_environment()
    if not argv:
        raise RunnerError("a test executable is required")
    child = _validate_child(argv[0], target)
    lock_path = root / "hantong-ci.keychain.lock"
    if lock_path.exists() and lock_path.is_symlink():
        raise RunnerError("Keychain lock must not be a symlink")
    flags = os.O_RDWR | os.O_CREAT
    nofollow = getattr(os, "O_NOFOLLOW", 0)
    try:
        lock_fd = os.open(lock_path, flags | nofollow, 0o600)
    except OSError as error:
        raise RunnerError("could not create the private Keychain lock") from error
    import fcntl

    try:
        _validate_lock(lock_fd)
    except BaseException:
        os.close(lock_fd)
        raise
    with os.fdopen(lock_fd, "r+b") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        baseline_default, baseline_list = _baseline()
        expected = root / "hantong-ci.keychain-db"
        if baseline_default != expected or baseline_list != (expected,):
            raise RunnerError("Keychain baseline does not match the CI Keychain")
        own = root / f"hantong-test-{uuid.uuid4().hex}.keychain-db"
        if own.exists() or own.is_symlink():
            raise RunnerError("temporary Keychain path already exists")
        process: subprocess.Popen[bytes] | None = None
        child_code: int | None = None
        created = False
        hangup = getattr(signal, "SIGHUP", signal.SIGTERM)
        previous_handlers = {
            signal.SIGTERM: signal.getsignal(signal.SIGTERM),
            hangup: signal.getsignal(hangup),
        }

        def interrupted(signum: int, frame: object) -> None:
            if process is not None:
                _terminate(process)
            raise RunnerError("macOS test runner interrupted")

        try:
            signal.signal(signal.SIGTERM, interrupted)
            signal.signal(hangup, interrupted)
            _security(["create-keychain", "-p", SYNTHETIC_PASSWORD, str(own)], capture=False)
            created = True
            _security(["unlock-keychain", "-p", SYNTHETIC_PASSWORD, str(own)], capture=False)
            _security(["set-keychain-settings", "-lut", "21600", str(own)], capture=False)
            _security(["default-keychain", "-d", "user", "-s", str(own)], capture=False)
            _security(["list-keychains", "-d", "user", "-s", str(own)], capture=False)
            environment = dict(os.environ)
            environment["HANTONG_TEST_KEYCHAIN"] = str(own)
            signal.signal(signal.SIGTERM, interrupted)
            process = subprocess.Popen(
                [str(child), *argv[1:]], env=environment, start_new_session=True,
            )
            try:
                child_code = process.wait(timeout=CHILD_TIMEOUT)
            except subprocess.TimeoutExpired:
                _terminate(process)
                raise RunnerError("macOS test executable timed out")
            except KeyboardInterrupt:
                _terminate(process)
                raise
        finally:
            try:
                for signum in previous_handlers:
                    signal.signal(signum, signal.SIG_IGN)
                if process is not None:
                    _terminate(process)
                _set_baseline(baseline_default, baseline_list)
                restored_default, restored_list = _baseline()
                if restored_default != baseline_default or restored_list != baseline_list:
                    raise RunnerError("Keychain baseline was not restored exactly")
                if created:
                    _security(["delete-keychain", str(own)], capture=False)
            finally:
                for signum, handler in previous_handlers.items():
                    signal.signal(signum, handler)
        assert child_code is not None
        return child_code


def main(argv: list[str] | None = None) -> int:
    try:
        return run(list(sys.argv[1:] if argv is None else argv))
    except KeyboardInterrupt:
        return 130
    except subprocess.TimeoutExpired:
        print("error: macOS test runner timed out", file=sys.stderr)
        return 2
    except subprocess.SubprocessError:
        print("error: macOS test runner security operation failed", file=sys.stderr)
        return 2
    except (OSError, RunnerError):
        print("error: macOS test runner refused or failed safely", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
