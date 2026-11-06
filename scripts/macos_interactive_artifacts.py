#!/usr/bin/env python3
"""Bounded, dormant artifact helpers for the interactive macOS compatibility run.

This module only prepares disposable source copies and verified package files.  It
does not select or mutate a Keychain, change ACLs, install a package, or launch a
product.  The controller owns the process and human-review boundaries.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tarfile
import tempfile
from typing import Callable

from package_cli import PackageError, archive_name
from package_unix import check_macho


MAX_ARCHIVE_MEMBERS = 4096
MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
MAX_MEMBER_BYTES = 256 * 1024 * 1024
MAX_CARGO_OUTPUT = 2 * 1024 * 1024
MAX_SOURCE_BYTES = 64 * 1024 * 1024
MODULE_MARKER = b"mod macos_keychain_compat;"
MODULE_PATH = Path("crates/hantong-secrets/src/macos_keychain_compat.rs")


class ArtifactError(ValueError):
    """A finite, non-secret artifact or source lifecycle failure."""


class UnreapedProcessError(RuntimeError):
    """The owned runner cannot prove all compiler descendants have stopped."""


def _regular(path: Path, label: str) -> None:
    if not path.is_absolute() or path.is_symlink() or not path.is_file():
        raise ArtifactError(f"{label}_unsafe")


def _directory(path: Path, label: str) -> Path:
    if not path.is_absolute() or path.is_symlink() or not path.is_dir():
        raise ArtifactError(f"{label}_unsafe")
    resolved = path.resolve(strict=True)
    if os.name == "posix" and resolved.stat().st_mode & 0o022:
        raise ArtifactError(f"{label}_permissions")
    return resolved


def _pinned_bytes(path: Path, limit: int) -> bytes:
    _regular(path, "artifact")
    path_before = os.lstat(path)
    if not stat.S_ISREG(path_before.st_mode) or path_before.st_nlink != 1:
        raise ArtifactError("artifact_shape")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError as error:
        raise ArtifactError("artifact_open") from error
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_size > limit or before.st_nlink != 1:
            raise ArtifactError("artifact_shape")
        if (path_before.st_dev, path_before.st_ino, path_before.st_uid, path_before.st_mode,
                path_before.st_nlink) != (before.st_dev, before.st_ino, before.st_uid,
                                          before.st_mode, before.st_nlink):
            raise ArtifactError("artifact_changed")
        # Buffered fd reads avoid short-read behavior observed for some native
        # files on Windows while the same descriptor remains pinned.
        with os.fdopen(os.dup(fd), "rb") as stream:
            data = stream.read(limit + 1)
        after = os.fstat(fd)
        if (before.st_dev, before.st_ino, before.st_uid, before.st_mode, before.st_size,
                before.st_mtime_ns, before.st_ctime_ns) != (
                after.st_dev, after.st_ino, after.st_uid, after.st_mode, after.st_size,
                after.st_mtime_ns, after.st_ctime_ns):
            raise ArtifactError("artifact_changed")
        path_after = os.lstat(path)
        if (path_before.st_dev, path_before.st_ino, path_before.st_uid, path_before.st_mode,
                path_before.st_nlink) != (path_after.st_dev, path_after.st_ino, path_after.st_uid,
                                          path_after.st_mode, path_after.st_nlink):
            raise ArtifactError("artifact_changed")
        if len(data) != before.st_size:
            raise ArtifactError("artifact_short_read")
        return data
    finally:
        os.close(fd)


def _safe_member(name: str) -> str:
    try:
        return archive_name(name)
    except (PackageError, ValueError) as error:
        raise ArtifactError("archive_path") from error


def _json_no_duplicates(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ArtifactError("manifest")
        value[key] = item
    return value


def extract_verified_archive(archive: Path, destination: Path, expected_sha256: str,
                             *, max_members: int = MAX_ARCHIVE_MEMBERS,
                             max_bytes: int = MAX_ARCHIVE_BYTES) -> Path:
    """Verify and extract a flat regular-file package into a new private directory."""
    _regular(archive, "archive")
    if len(expected_sha256) != 64 or any(c not in "0123456789abcdef" for c in expected_sha256):
        raise ArtifactError("archive_digest")
    if destination.exists() or destination.is_symlink() or not destination.is_absolute():
        raise ArtifactError("destination_exists")
    data = _pinned_bytes(archive, max_bytes)
    if hashlib.sha256(data).hexdigest() != expected_sha256:
        raise ArtifactError("archive_digest")
    if destination.parent.is_symlink():
        raise ArtifactError("destination_unsafe")
    parent = destination.parent.resolve(strict=True)
    if os.name == "posix" and parent.stat().st_mode & 0o022:
        raise ArtifactError("destination_permissions")
    temporary = Path(tempfile.mkdtemp(prefix=".hantong-extract-", dir=parent))
    try:
        seen: set[str] = set()
        total = 0
        with tarfile.open(fileobj=__import__("io").BytesIO(data), mode="r:gz") as bundle:
            count = 0
            for member in bundle:
                count += 1
                if count > max_members:
                    raise ArtifactError("archive_members")
                name = _safe_member(member.name)
                if name in seen or not member.isfile() or member.size > MAX_MEMBER_BYTES:
                    raise ArtifactError("archive_member")
                seen.add(name)
                total += member.size
                if total > max_bytes:
                    raise ArtifactError("archive_size")
                target = temporary / Path(*name.split("/"))
                target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                source = bundle.extractfile(member)
                if source is None:
                    raise ArtifactError("archive_member")
                with target.open("xb") as output:
                    shutil.copyfileobj(source, output, length=1024 * 1024)
                os.chmod(target, 0o755 if name.startswith("bin/") else 0o644)
            if count == 0:
                raise ArtifactError("archive_members")
        os.replace(temporary, destination)
        return destination
    except Exception:
        if temporary.exists():
            shutil.rmtree(temporary)
        raise


def verify_extracted_package(root: Path, target: str,
                             required: tuple[str, ...] = ("bin/hantong", "bin/mail-hantong-core")) -> dict[str, str]:
    """Verify manifest hashes and native architecture before a caller can launch files."""
    root = _directory(root, "package")
    manifest_path = root / "release-manifest.json"
    try:
        manifest = json.loads(_pinned_bytes(manifest_path, 4 * 1024 * 1024), object_pairs_hook=_json_no_duplicates)
    except (UnicodeDecodeError, json.JSONDecodeError, TypeError, ArtifactError) as error:
        if isinstance(error, ArtifactError):
            raise
        raise ArtifactError("manifest") from error
    if (not isinstance(manifest, dict)
            or set(manifest) != {"format_version", "version", "package_version", "target", "files"}
            or manifest.get("format_version") != 1 or manifest.get("target") != target
            or target not in {"macos-x86_64", "macos-aarch64"}
            or not isinstance(manifest.get("files"), list)
            or not 1 <= len(manifest["files"]) <= MAX_ARCHIVE_MEMBERS):
        raise ArtifactError("manifest")
    entries: dict[str, tuple[str, int]] = {}
    for item in manifest["files"]:
        if not isinstance(item, dict) or set(item) != {"path", "sha256", "size"}:
            raise ArtifactError("manifest")
        if not isinstance(item.get("path"), str):
            raise ArtifactError("manifest")
        name = _safe_member(item["path"])
        digest = item["sha256"]
        size = item["size"]
        if (name in entries or not isinstance(digest, str) or len(digest) != 64
                or any(c not in "0123456789abcdef" for c in digest)
                or not isinstance(size, int) or isinstance(size, bool) or size < 0):
            raise ArtifactError("manifest")
        entries[name] = (digest, size)
    machine = "arm64" if target == "macos-aarch64" else "x86_64"
    result: dict[str, str] = {}
    for name, (digest, size) in entries.items():
        path = root / Path(*name.split("/"))
        data = _pinned_bytes(path, MAX_MEMBER_BYTES)
        if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
            raise ArtifactError("artifact_changed")
        if name.startswith("bin/") and name in required:
            check_macho(data, machine)
            result[name] = digest
    if any(name not in result for name in required):
        raise ArtifactError("manifest_missing_binary")
    return result


def _snapshot(path: Path) -> tuple[int, int, int, int, int, int, int]:
    item = os.lstat(path)
    return (item.st_dev, item.st_ino, item.st_uid, item.st_mode, item.st_nlink,
            item.st_size, item.st_mtime_ns)


def _atomic_bytes(path: Path, data: bytes, mode: int) -> None:
    temporary = path.with_name(f".{path.name}.hantong-write")
    if temporary.exists() or temporary.is_symlink():
        raise ArtifactError("fixture_restore")
    try:
        with temporary.open("xb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    except OSError as error:
        if temporary.exists() and not temporary.is_symlink():
            temporary.unlink()
        raise ArtifactError("fixture_restore") from error


def _exclusive_copy(source: Path, destination: Path, data: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(destination, flags, 0o600)
        try:
            with os.fdopen(fd, "wb") as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
        except Exception:
            fd = -1
            raise
    except OSError as error:
        raise ArtifactError("fixture_create") from error


def _inject(source: Path, fixture: Path, module_path: Path) -> tuple[Path, bytes, bytes, tuple[int, int, int, int, int, int, int]]:
    lib = source / "crates/hantong-secrets/src/lib.rs"
    destination = source / module_path
    _regular(lib, "source_lib")
    _regular(fixture, "fixture")
    if destination.exists() or destination.is_symlink():
        raise ArtifactError("source_fixture_exists")
    original = _pinned_bytes(lib, MAX_SOURCE_BYTES)
    if MODULE_MARKER in original:
        raise ArtifactError("source_fixture_exists")
    lib_mode = stat.S_IMODE(os.lstat(lib).st_mode)
    fixture_bytes = _pinned_bytes(fixture, MAX_MEMBER_BYTES)
    injected = original + b'\n#[cfg(all(test, target_os = "macos"))]\nmod macos_keychain_compat;\n'
    destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        _exclusive_copy(fixture, destination, fixture_bytes)
        _atomic_bytes(lib, injected, stat.S_IMODE(lib.stat().st_mode))
        return lib, original, fixture_bytes, _snapshot(destination)
    except Exception:
        try:
            destination_bytes = _pinned_bytes(destination, MAX_MEMBER_BYTES)
        except ArtifactError:
            destination_bytes = None
        if destination_bytes == fixture_bytes:
            destination.unlink()
        try:
            lib_bytes = _pinned_bytes(lib, MAX_SOURCE_BYTES)
        except ArtifactError:
            lib_bytes = None
        if lib_bytes == injected:
            _atomic_bytes(lib, original, lib_mode)
        raise


def _restore_injected(lib: Path, original: bytes, fixture_bytes: bytes,
                      destination: Path, destination_identity: tuple[int, int, int, int, int, int, int]) -> None:
    expected = original + b'\n#[cfg(all(test, target_os = "macos"))]\nmod macos_keychain_compat;\n'
    try:
        try:
            current_lib = _pinned_bytes(lib, MAX_SOURCE_BYTES)
        except ArtifactError as error:
            raise ArtifactError("source_changed_during_fixture") from error
        if current_lib != expected:
            raise ArtifactError("source_changed_during_fixture")
        if (destination.is_symlink() or not destination.is_file()
                or _pinned_bytes(destination, MAX_MEMBER_BYTES) != fixture_bytes
                or _snapshot(destination) != destination_identity):
            raise ArtifactError("fixture_restore")
        if _snapshot(destination) != destination_identity:
            raise ArtifactError("fixture_restore")
        _atomic_bytes(lib, original, stat.S_IMODE(os.lstat(lib).st_mode))
        destination.unlink()
    except ArtifactError:
        raise
    except OSError as error:
        raise ArtifactError("fixture_restore") from error


def build_fixture(source: Path, target: Path, phase_name: str,
                  run_command: Callable[..., object] | None = None,
                  *, fixture: Path | None = None,
                  module_path: Path = MODULE_PATH,
                  timeout: int = 900, cancel: Callable[[], bool] | None = None,
                  environment: dict[str, str] | None = None) -> Path:
    """Inject a disposable fixture, compile one test executable, and restore exactly."""
    source = _directory(source, "source")
    target = _directory(target, "target")
    if not phase_name or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-_" for c in phase_name):
        raise ArtifactError("phase")
    if module_path != MODULE_PATH or module_path.is_absolute() or ".." in module_path.parts:
        raise ArtifactError("module_path")
    if run_command is None or environment is None:
        raise ArtifactError("runner_required")
    reserved = ("CI", "GITHUB_ACTIONS", "RUNNER_TEMP", "GITHUB_WORKSPACE",
                "HANTONG_TEST_KEYCHAIN", "HANTONG_COMPAT_ROOT", "HANTONG_COMPAT_PHASE",
                "CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER", "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER")
    if any(key in environment for key in reserved):
        raise ArtifactError("environment")
    fixture = fixture or source / "scripts/fixtures/macos_keychain_compat_module.rs"
    fixture = fixture if fixture.is_absolute() else source / fixture
    destination = source / module_path
    lib = original = fixture_bytes = destination_identity = None
    restore_allowed = True
    try:
        if cancel and cancel():
            raise ArtifactError("cancelled")
        lib, original, fixture_bytes, destination_identity = _inject(source, fixture, module_path)
        command = ["cargo", "test", "-p", "hantong-secrets", "--lib", "--no-run",
                   "--locked", "--offline", "--message-format=json"]
        env = dict(environment)
        env["CARGO_TARGET_DIR"] = str(target)
        try:
            result = run_command(command, cwd=source, env=env,
                                 stdin=subprocess.DEVNULL, capture_output=True, text=True,
                                 timeout=timeout, check=False)
        except KeyboardInterrupt as error:
            raise ArtifactError("cancelled") from error
        except UnreapedProcessError:
            restore_allowed = False
            raise
        except subprocess.TimeoutExpired as error:
            raise ArtifactError("compile_timeout") from error
        if cancel and cancel():
            raise ArtifactError("cancelled")
        if getattr(result, "returncode", 1) != 0:
            raise ArtifactError("compile_failed")
        output = getattr(result, "stdout", "")
        if isinstance(output, bytes):
            output = output.decode("utf-8", "replace")
        if len(output) > MAX_CARGO_OUTPUT:
            raise ArtifactError("compile_output")
        candidates = []
        for line in output.splitlines():
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            target_info = item.get("target", {})
            if (item.get("reason") == "compiler-artifact" and item.get("profile", {}).get("test") is True
                    and target_info.get("name") == "hantong_secrets"
                    and target_info.get("kind") == ["lib"]
                    and isinstance(item.get("executable"), str)):
                    candidates.append(Path(item["executable"]))
        if len(candidates) != 1:
            raise ArtifactError("compile_artifact")
        executable = candidates[0]
        if (not executable.is_absolute()
                or executable.parent.resolve() != target.resolve() / "debug/deps"
                or not executable.name.startswith("hantong_secrets-")):
            raise ArtifactError("fixture_executable")
        _regular(executable, "fixture_executable")
        return executable
    finally:
        if (restore_allowed and lib is not None and original is not None
                and fixture_bytes is not None and destination_identity is not None):
            _restore_injected(lib, original, fixture_bytes, destination, destination_identity)
