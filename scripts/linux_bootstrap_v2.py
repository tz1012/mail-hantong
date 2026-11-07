"""Fail-closed, offline-first bootstrap primitives for the beta.29 Linux move.

This module is intentionally a preparation boundary.  It verifies a pinned
candidate, creates a private staging directory, and prepares sealed execution
objects.  It never changes the active pointer, opens the database or vault,
starts a service, grants a scope, or contacts a mail provider.

The one-time Python entry point described by ADR-033 E9 may call these helpers
after its command and descriptor have been independently reviewed.  The
functions are kept small and deterministic so the security properties can be
tested without an installed product or user data.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import selectors
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
try:
    import resource
except ImportError:  # pragma: no cover - Windows import path
    resource = None
from typing import Iterable

if os.name == "posix":
    import fcntl
else:  # pragma: no cover - exercised by the Windows import/negative tests
    fcntl = None

from linux_pinned_candidate import CandidateReport, _read as _read_pinned, verify_candidate


TARGET = "linux-x86_64"
MAX_DESCRIPTOR = 256 * 1024
MAX_CONTEXT = 4096
MAX_MANIFEST = 4 * 1024 * 1024
MAX_PACKAGE = 256 * 1024 * 1024
MAX_MEMBER = 64 * 1024 * 1024
ROLE_ROOT, ROLE_IMAGE, ROLE_CONTEXT, ROLE_MANIFEST, ROLE_DESCRIPTOR = 197, 198, 199, 200, 201
DESCRIPTOR_KEYS = {
    "schema_version", "version", "package_version", "target", "commit",
    "source_filename", "cargo_lock_sha256", "source_sha256",
    "source_archive_sha256", "package_filename", "package_sha256",
}
CONTEXT_KEYS = {
    "protocol_version", "target", "release_version", "source_commit",
    "canonical_source_sha256", "descriptor_sha256", "manifest_sha256",
    "asset_relative_path", "asset_size", "asset_sha256",
}
ALLOWED_ASSETS = {
    "bin/hantong", "bin/hantong-t", "bin/hantong-mcp", "bin/mail-hantong-core",
}


class BootstrapError(ValueError):
    """Finite, non-secret bootstrap failure."""


@dataclass(frozen=True)
class BootstrapTuple:
    version: str
    commit: str
    descriptor_sha256: str
    base_url: str


@dataclass(frozen=True)
class SealedImage:
    fd: int
    size: int
    sha256: str


def _fail(kind: str, error: BaseException | None = None) -> BootstrapError:
    return BootstrapError(kind)


def _safe_regular(path: Path, label: str, limit: int, *, owner: int | None = None,
                  allow_symlink: bool = False) -> os.stat_result:
    try:
        item = path.lstat()
    except OSError as error:
        raise _fail(f"{label}_missing", error) from error
    if path.is_symlink():
        if not allow_symlink:
            raise _fail(f"{label}_shape")
        try:
            item = path.resolve(strict=True).stat()
        except OSError as error:
            raise _fail(f"{label}_shape", error) from error
    if not stat.S_ISREG(item.st_mode) or item.st_nlink != 1:
        raise _fail(f"{label}_shape")
    if owner is not None and item.st_uid != owner:
        raise _fail(f"{label}_ownership")
    if item.st_mode & 0o022 or item.st_mode & (stat.S_ISUID | stat.S_ISGID):
        raise _fail(f"{label}_mode")
    if item.st_size < 0 or item.st_size > limit:
        raise _fail(f"{label}_size")
    return item


def _safe_ancestor(path: Path, label: str, *, owner: int | None = None) -> None:
    if not path.is_absolute() or any(part in (".", "..") for part in path.parts):
        raise _fail(f"{label}_ancestor")
    cursor = path
    while True:
        try:
            item = cursor.lstat()
        except OSError as error:
            raise _fail(f"{label}_ancestor", error) from error
        if cursor.is_symlink() or not stat.S_ISDIR(item.st_mode):
            raise _fail(f"{label}_ancestor")
        if owner is not None and item.st_uid != owner:
            raise _fail(f"{label}_ownership")
        if owner is None and item.st_uid not in (0, os.geteuid()):
            raise _fail(f"{label}_ownership")
        sticky_os_ancestor = owner is None and cursor != path and item.st_uid == 0 and item.st_mode & stat.S_ISVTX
        if (item.st_mode & 0o022 and not sticky_os_ancestor) or item.st_mode & (stat.S_ISUID | stat.S_ISGID):
            raise _fail(f"{label}_ownership")
        if cursor.parent == cursor:
            return
        cursor = cursor.parent


def validate_python_prerequisites(python: Path = Path("/usr/bin/python3"),
                                  stdlib: Iterable[Path] = ()) -> Path:
    """Validate the fixed OS Python and absolute stdlib roots, without running it."""
    if os.name != "posix":
        raise _fail("platform")
    python = Path(python)
    if python != Path("/usr/bin/python3"):
        raise _fail("python_path")
    _safe_regular(python, "python", 16 * 1024 * 1024, owner=0, allow_symlink=True)
    resolved_python = python.resolve(strict=True)
    _safe_regular(resolved_python, "python", 16 * 1024 * 1024, owner=0)
    if resolved_python.parent != Path("/usr/bin"):
        raise _fail("python_path")
    _safe_ancestor(python.parent, "python", owner=0)
    roots = tuple(Path(value) for value in stdlib)
    if not roots:
        raise _fail("stdlib_missing")
    for root in roots:
        if not root.is_absolute() or root.is_symlink() or not root.is_dir():
            raise _fail("stdlib_path")
        _safe_ancestor(root, "stdlib", owner=0)
    return python


def validate_python_invocation(python: Path, payload: bytes) -> tuple[str, ...]:
    """Return the only permitted first-transition invocation."""
    if python != Path("/usr/bin/python3") or not isinstance(payload, bytes) or not payload:
        raise _fail("python_invocation")
    if b"\x00" in payload or len(payload) > 256 * 1024:
        raise _fail("python_payload")
    return (str(python), "-I", "-S", "-c", payload.decode("utf-8"))


def parse_descriptor(data: bytes) -> dict:
    if not isinstance(data, bytes) or not 1 <= len(data) <= MAX_DESCRIPTOR:
        raise _fail("descriptor_size")
    try:
        def unique(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise _fail("descriptor_duplicate_key")
                result[key] = value
            return result
        value = json.loads(data.decode("utf-8"), object_pairs_hook=unique,
                           parse_constant=lambda _: (_ for _ in ()).throw(_fail("descriptor_constant")))
    except BootstrapError:
        raise
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise _fail("descriptor_json", error) from error
    if not isinstance(value, dict) or set(value) != DESCRIPTOR_KEYS:
        raise _fail("descriptor_fields")
    if value["schema_version"] != 1 or type(value["schema_version"]) is not int:
        raise _fail("descriptor_schema")
    if value["target"] != TARGET:
        raise _fail("descriptor_target")
    if not isinstance(value["commit"], str) or not re.fullmatch(r"[0-9a-f]{40}", value["commit"]):
        raise _fail("descriptor_commit")
    for key in ("cargo_lock_sha256", "source_sha256", "source_archive_sha256", "package_sha256"):
        if not isinstance(value[key], str) or not re.fullmatch(r"[0-9a-f]{64}", value[key]):
            raise _fail("descriptor_digest")
    for key in ("version", "package_version", "source_filename", "package_filename"):
        if not isinstance(value[key], str) or not value[key] or "\x00" in value[key]:
            raise _fail("descriptor_value")
        if any(ord(char) < 32 or ord(char) == 127 for char in value[key]):
            raise _fail("descriptor_control")
    return value


def parse_context(data: bytes) -> dict:
    if not isinstance(data, bytes) or not 1 <= len(data) <= MAX_CONTEXT:
        raise _fail("context_size")
    try:
        def unique(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise _fail("context_duplicate_key")
                result[key] = value
            return result
        value = json.loads(data.decode("utf-8"), object_pairs_hook=unique,
                           parse_constant=lambda _: (_ for _ in ()).throw(_fail("context_constant")))
    except BootstrapError:
        raise
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise _fail("context_json", error) from error
    if not isinstance(value, dict) or set(value) != CONTEXT_KEYS:
        raise _fail("context_fields")
    if value["protocol_version"] != 2 or type(value["protocol_version"]) is not int:
        raise _fail("context_protocol")
    if value["target"] != TARGET or not isinstance(value["asset_relative_path"], str):
        raise _fail("context_target")
    if value["asset_relative_path"] not in ALLOWED_ASSETS:
        raise _fail("context_asset")
    if type(value["asset_size"]) is not int or value["asset_size"] <= 0 or value["asset_size"] > 64 * 1024 * 1024:
        raise _fail("context_asset_size")
    if not isinstance(value["source_commit"], str) or not re.fullmatch(r"[0-9a-f]{40}", value["source_commit"]):
        raise _fail("context_commit")
    for key in ("canonical_source_sha256", "descriptor_sha256", "manifest_sha256", "asset_sha256"):
        if not isinstance(value[key], str) or not re.fullmatch(r"[0-9a-f]{64}", value[key]):
            raise _fail("context_digest")
    for key in ("release_version", "asset_relative_path"):
        if any(ord(char) < 32 or char == "\x7f" for char in value[key]):
            raise _fail("context_control")
    return value


def make_context(*, version: str, commit: str, source_sha256: str,
                 descriptor_sha256: str, manifest_sha256: str,
                 asset_relative_path: str, asset_size: int, asset_sha256: str) -> bytes:
    value = {
        "asset_relative_path": asset_relative_path,
        "asset_sha256": asset_sha256,
        "asset_size": asset_size,
        "canonical_source_sha256": source_sha256,
        "descriptor_sha256": descriptor_sha256,
        "manifest_sha256": manifest_sha256,
        "protocol_version": 2,
        "release_version": version,
        "source_commit": commit,
        "target": TARGET,
    }
    raw = json.dumps(value, ensure_ascii=True, separators=(",", ":"), sort_keys=True).encode("ascii")
    parse_context(raw)
    return raw


def validate_tuple(value: BootstrapTuple) -> BootstrapTuple:
    if not isinstance(value, BootstrapTuple):
        raise _fail("tuple_type")
    if not re.fullmatch(r"[0-9a-f]{40}", value.commit or ""):
        raise _fail("tuple_commit")
    if not re.fullmatch(r"[0-9a-f]{64}", value.descriptor_sha256 or ""):
        raise _fail("tuple_descriptor")
    if not _release_version(value.version):
        raise _fail("tuple_version")
    if not isinstance(value.base_url, str) or not re.fullmatch(
            r"https://tz1012\.github\.io/mail-hantong/artifacts/linux-x86_64/", value.base_url):
        raise _fail("tuple_origin")
    return value


def _release_version(value: str) -> bool:
    if not isinstance(value, str) or len(value) > 64 or not re.fullmatch(
            r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*", value):
        return False
    return not any(part.isdigit() and len(part) > 1 and part.startswith("0")
                   for part in value.split("-", 1)[1].split("."))


def fixed_url(item: BootstrapTuple, filename: str) -> str:
    validate_tuple(item)
    if not isinstance(filename, str) or "/" in filename or "\\" in filename or filename not in {
            f"linux-pinned-candidate.json", f"mail-hantong-cli-{item.version}-linux-x86_64.tar.gz",
            f"mail-hantong-source-{item.version}.zip"}:
        raise _fail("url_filename")
    return f"{item.base_url}{item.version}/{item.commit}/{filename}"


def fetch_fixed(url: str, *, curl: Path = Path("/usr/bin/curl"), timeout: int = 300,
                max_bytes: int = MAX_PACKAGE) -> bytes:
    """Fetch one fixed HTTPS object with a bounded, cleared environment."""
    parts = url.split("/") if isinstance(url, str) else []
    if (len(parts) != 9 or "/".join(parts[:6]) != "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64"
            or not _release_version(parts[6])
            or not re.fullmatch(r"[0-9a-f]{40}", parts[7])
            or parts[8] not in {"linux-pinned-candidate.json", "mail-hantong-cli-" + parts[6] + "-linux-x86_64.tar.gz",
                                "mail-hantong-source-" + parts[6] + ".zip"}):
        raise _fail("url")
    if curl != Path("/usr/bin/curl"):
        raise _fail("curl_path")
    _safe_regular(curl, "curl", 16 * 1024 * 1024, owner=0, allow_symlink=True)
    resolved_curl = curl.resolve(strict=True)
    _safe_regular(resolved_curl, "curl", 16 * 1024 * 1024, owner=0)
    _safe_ancestor(resolved_curl.parent, "curl", owner=0)
    _safe_ancestor(curl.parent, "curl", owner=0)
    args = [str(curl), "--disable", "--silent", "--show-error", "--proto", "=https",
            "--tlsv1.2", "--noproxy", "*", "--connect-timeout", "15", "--max-time", str(timeout),
            "--dump-header", "-", "--output", "-", "--write-out", "\\n%{http_code}", url]
    child = None
    selector = None
    try:
        child = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.DEVNULL, env={}, close_fds=True)
        if child.stdout is None:
            raise _fail("download_pipe")
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)
        chunks: list[bytes] = []
        total = 0
        deadline = time.monotonic() + timeout + 5
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(args, timeout)
            events = selector.select(min(1.0, remaining))
            if not events:
                continue
            block = child.stdout.read1(64 * 1024)
            if not block:
                break
            total += len(block)
            if total > max_bytes + 64 * 1024:
                raise _fail("download_size")
            chunks.append(block)
        child.wait(timeout=max(0.1, deadline - time.monotonic()))
        data = b"".join(chunks)
    except subprocess.TimeoutExpired as error:
        if child is not None:
            child.kill(); child.wait()
        raise _fail("download_timeout", error) from error
    except (OSError, subprocess.SubprocessError) as error:
        raise _fail("download_process", error) from error
    finally:
        if selector is not None:
            selector.close()
        if child is not None and child.poll() is None:
            child.kill()
            child.wait()
    if child.returncode != 0:
        raise _fail("download_http")
    marker = b"\n200"
    if not data.endswith(marker):
        raise _fail("download_status")
    payload = data[:-len(marker)]
    separator = payload.find(b"\r\n\r\n")
    if separator < 0 or separator > 64 * 1024:
        raise _fail("download_headers")
    status_line = payload[:payload.find(b"\r\n")]
    status_parts = status_line.split(b" ", 2)
    if len(status_parts) < 2 or status_parts[0] not in (b"HTTP/1.0", b"HTTP/1.1", b"HTTP/2", b"HTTP/3") or status_parts[1] != b"200":
        raise _fail("download_status")
    body = payload[separator + 4:]
    if len(body) > max_bytes:
        raise _fail("download_size")
    return body


def _sealed_memfd(name: str, data: bytes, limit: int, *, executable: bool = False) -> SealedImage:
    if os.name != "posix" or fcntl is None or not hasattr(fcntl, "F_ADD_SEALS"):
        raise _fail("sealed_memfd_unsupported")
    if not 1 <= len(data) <= limit:
        raise _fail("memfd_size")
    flags = 0x0001 | 0x0002  # MFD_CLOEXEC | MFD_ALLOW_SEALING
    flags |= 0x0010 if executable else 0x0008  # MFD_EXEC or MFD_NOEXEC_SEAL
    try:
        fd = os.memfd_create(name, flags)
    except (AttributeError, OSError) as error:
        raise _fail("sealed_memfd_unsupported", error) from error
    try:
        offset = 0
        while offset < len(data):
            written = os.write(fd, data[offset:])
            if written <= 0:
                raise _fail("memfd_short_write")
            offset += written
        seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        fcntl.fcntl(fd, fcntl.F_ADD_SEALS, seals)
        actual = os.fstat(fd)
        if actual.st_size != len(data) or (fcntl.fcntl(fd, fcntl.F_GET_SEALS) & seals) != seals:
            raise _fail("memfd_seal")
        measured = os.pread(fd, len(data) + 1, 0)
        if measured != data:
            raise _fail("memfd_measure")
        return SealedImage(fd, len(data), hashlib.sha256(measured).hexdigest())
    except BaseException:
        os.close(fd)
        raise


def prepare_execution_roles(root: Path, image: bytes, context: bytes,
                            manifest: bytes, descriptor: bytes) -> dict[int, int]:
    """Create the E2 FD map; the caller owns all returned descriptors."""
    if not root.is_dir() or root.is_symlink():
        raise _fail("root_shape")
    _safe_ancestor(root, "root")
    if resource is None or resource.getrlimit(resource.RLIMIT_NOFILE)[0] <= ROLE_DESCRIPTOR:
        raise _fail("fd_limit")
    if not hasattr(os, "O_PATH"):
        raise _fail("opath_unsupported")
    root_fd = os.open(root, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW)
    opened = [root_fd]
    try:
        image_obj = _sealed_memfd("hantong-image", image, 64 * 1024 * 1024, executable=True)
        opened.append(image_obj.fd)
        context_obj = _sealed_memfd("hantong-context", context, MAX_CONTEXT)
        opened.append(context_obj.fd)
        manifest_obj = _sealed_memfd("hantong-manifest", manifest, MAX_MANIFEST)
        opened.append(manifest_obj.fd)
        descriptor_obj = _sealed_memfd("hantong-descriptor", descriptor, MAX_DESCRIPTOR)
        opened.append(descriptor_obj.fd)
        roles = {
            ROLE_ROOT: root_fd,
            ROLE_IMAGE: image_obj.fd,
            ROLE_CONTEXT: context_obj.fd,
            ROLE_MANIFEST: manifest_obj.fd,
            ROLE_DESCRIPTOR: descriptor_obj.fd,
        }
        return roles
    except BaseException:
        for fd in opened:
            try: os.close(fd)
            except OSError: pass
        raise


def map_roles_for_exec(roles: dict[int, int]) -> None:
    """Map already-owned role FDs in a child immediately before ``execve(fd)``.

    The parent never overwrites its descriptor table.  An occupied fixed role
    in the child must be the same object or the mapping is rejected.
    """
    if set(roles) != {ROLE_ROOT, ROLE_IMAGE, ROLE_CONTEXT, ROLE_MANIFEST, ROLE_DESCRIPTOR}:
        raise _fail("role_map")
    if fcntl is None or not hasattr(fcntl, "F_DUPFD_CLOEXEC"):
        raise _fail("role_dup_unsupported")
    # Validate every destination and source before mutating the child table.
    source_fds: list[int] = []
    for role, source in roles.items():
        if not isinstance(source, int) or source < 0:
            raise _fail("role_source")
        try:
            origin = os.fstat(source)
        except OSError as error:
            raise _fail("role_source", error) from error
        try:
            target = os.fstat(role)
        except OSError:
            target = None
        if target is not None and (target.st_dev, target.st_ino) != (origin.st_dev, origin.st_ino):
            raise _fail("role_occupied")
        source_fds.append(source)
    sources: dict[int, int] = {}
    try:
        for source in source_fds:
            if source in sources:
                continue
            try:
                duplicate = fcntl.fcntl(source, fcntl.F_DUPFD_CLOEXEC, 202)
                os.set_inheritable(duplicate, False)
            except OSError as error:
                raise _fail("role_dup", error) from error
            sources[source] = duplicate
        for role, source in roles.items():
            os.dup2(sources[source], role, inheritable=True)
    finally:
        for fd in sources.values():
            try: os.close(fd)
            except OSError: pass


def _open_private_child_dir(parent_fd: int, name: str) -> int:
    flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(name, flags, dir_fd=parent_fd)
    except FileNotFoundError:
        try:
            os.mkdir(name, 0o700, dir_fd=parent_fd)
            fd = os.open(name, flags, dir_fd=parent_fd)
            os.fsync(parent_fd)
        except OSError as error:
            raise _fail("stage_directory", error) from error
    except OSError as error:
        raise _fail("stage_directory", error) from error
    item = os.fstat(fd)
    if os.name == "posix" and (item.st_uid != os.geteuid() or item.st_mode & 0o077):
        os.close(fd)
        raise _fail("stage_directory")
    os.fsync(fd)
    return fd


def _rename_noreplace_at(parent_fd: int, source: str, destination: str) -> None:
    if os.name != "posix":
        raise _fail("rename_noreplace_unsupported")
    import ctypes
    libc = ctypes.CDLL(None, use_errno=True)
    function = getattr(libc, "renameat2", None)
    if function is None:
        raise _fail("rename_noreplace_unsupported")
    function.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    function.restype = ctypes.c_int
    result = function(parent_fd, os.fsencode(source), parent_fd, os.fsencode(destination), 1)
    if result != 0:
        error = OSError(ctypes.get_errno(), "renameat2")
        if error.errno == 17:
            raise _fail("version_exists", error) from error
        raise _fail("stage_rename", error) from error


def _check_stage_readiness(data: bytes) -> None:
    try:
        metadata = json.loads(data)
    except (ValueError, UnicodeError, RecursionError):
        raise _fail("stage_metadata") from None
    if not isinstance(metadata, dict) or metadata.get("linux_release_readiness") == "blocked_identity_and_origin":
        raise _fail("stage_readiness_blocked")


def stage_verified_candidate(descriptor: Path, package: Path, source: Path,
                             install_root: Path, version: str, commit: str) -> tuple[Path, CandidateReport]:
    """Verify and extract a private candidate stage; never activates it."""
    if not install_root.is_dir() or install_root.is_symlink():
        raise _fail("install_root")
    _safe_ancestor(install_root, "install_root")
    report = verify_candidate(descriptor, package, source, "Linux", "x86_64")
    if report.version != version or report.commit != commit:
        raise _fail("candidate_tuple")
    # Bind extraction to a second descriptor-checked, inode-pinned read.  Do
    # not reopen a mutable pathname through tarfile after verification.
    package_bytes = _read_pinned(package, MAX_PACKAGE, "package")
    if hashlib.sha256(package_bytes).hexdigest() != report.package_sha256:
        raise _fail("package_changed")
    versions = install_root / "versions"
    if versions.is_symlink() or not versions.is_dir() or (os.name == "posix" and (versions.stat().st_uid != os.geteuid() or versions.stat().st_mode & 0o077)):
        raise _fail("versions_root")
    if os.name != "posix" or not hasattr(os, "O_PATH"):
        raise _fail("opath_unsupported")
    versions_fd = os.open(versions, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        existing_fd = os.open(version, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=versions_fd)
    except FileNotFoundError:
        pass
    except OSError as error:
        os.close(versions_fd)
        raise _fail("stage_version", error) from error
    else:
        os.close(existing_fd)
        os.close(versions_fd)
        raise _fail("version_exists")
    stage_name = f".{version}-bootstrap-{os.getpid()}-{os.urandom(8).hex()}"
    try:
        stage_fd = _open_private_child_dir(versions_fd, stage_name)
    except BaseException:
        os.close(versions_fd)
        raise
    stage_meta = os.fstat(stage_fd)
    final_root = versions / version
    try:
        with tarfile.open(fileobj=io.BytesIO(package_bytes), mode="r:gz") as archive:
            count = 0
            for member in archive:
                count += 1
                if count > 4096 or not member.isfile():
                    raise _fail("stage_inventory")
                name = PurePosixPath(member.name)
                if name.is_absolute() or any(part in ("", ".", "..") for part in name.parts):
                    raise _fail("stage_path")
                stream = archive.extractfile(member)
                if stream is None or member.size > MAX_MEMBER:
                    raise _fail("stage_member")
                if name.as_posix() == "BUILD-METADATA.json":
                    metadata = stream.read(MAX_MANIFEST + 1)
                    if len(metadata) > MAX_MANIFEST:
                        raise _fail("stage_metadata_size")
                    _check_stage_readiness(metadata)
                    stream = io.BytesIO(metadata)
                parent_fd = os.dup(stage_fd)
                try:
                    for part in name.parts[:-1]:
                        child = _open_private_child_dir(parent_fd, part)
                        os.close(parent_fd)
                        parent_fd = child
                    leaf = name.parts[-1]
                    fd = os.open(leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent_fd)
                finally:
                    if 'parent_fd' in locals():
                        try: os.close(parent_fd)
                        except OSError: pass
                try:
                    with os.fdopen(fd, "wb") as output:
                        shutil.copyfileobj(stream, output, length=1024 * 1024)
                        output.flush(); os.fchmod(output.fileno(), 0o700 if name.parts[:1] == ("bin",) else 0o600); os.fsync(output.fileno())
                except BaseException:
                    try: os.close(fd)
                    except OSError: pass
                    raise
        os.fsync(stage_fd)
        observed_stage = os.stat(stage_name, dir_fd=versions_fd, follow_symlinks=False)
        if (observed_stage.st_ino,observed_stage.st_dev) != (stage_meta.st_ino,stage_meta.st_dev):
            raise _fail("stage_changed")
        _rename_noreplace_at(versions_fd, stage_name, version)
        final_fd = os.open(version, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=versions_fd)
        try:
            final_meta = os.fstat(final_fd)
            if (final_meta.st_ino, final_meta.st_dev) != (stage_meta.st_ino, stage_meta.st_dev):
                raise _fail("stage_changed")
        finally:
            os.close(final_fd)
        os.fsync(versions_fd)
        return final_root, report
    except (BootstrapError, OSError, tarfile.TarError) as error:
        # Keep the newly-created private stage for explicit recovery/audit.  A
        # recursive pathname cleanup could delete an attacker-replaced tree.
        if isinstance(error, BootstrapError):
            raise
        raise _fail("stage") from error
    finally:
        os.close(stage_fd)
        os.close(versions_fd)


def _rename_noreplace(source: Path, destination: Path) -> None:
    """Atomically install a new directory without replacing an existing one."""
    if os.name != "posix":
        raise _fail("rename_noreplace_unsupported")
    import ctypes
    libc = ctypes.CDLL(None, use_errno=True)
    function = getattr(libc, "renameat2", None)
    if function is None:
        raise _fail("rename_noreplace_unsupported")
    function.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    function.restype = ctypes.c_int
    result = function(-100, os.fsencode(source), -100, os.fsencode(destination), 1)
    if result != 0:
        error = OSError(ctypes.get_errno(), "renameat2")
        if error.errno == 17:
            raise _fail("version_exists", error) from error
        raise _fail("stage_rename", error) from error


def persist_pin(install_root: Path, item: BootstrapTuple, descriptor: bytes) -> Path:
    """Persist only public evidence, without overwriting a prior pin."""
    validate_tuple(item)
    parsed = parse_descriptor(descriptor)
    if parsed["version"] != item.version or parsed["commit"] != item.commit:
        raise _fail("pin_tuple")
    if hashlib.sha256(descriptor).hexdigest() != item.descriptor_sha256:
        raise _fail("pin_digest")
    if not install_root.is_dir() or install_root.is_symlink():
        raise _fail("pin_root")
    _safe_ancestor(install_root, "pin_root")
    marker = install_root / ".mail-hantong-owner"
    _safe_regular(marker, "pin_owner", 1024, owner=os.geteuid())
    if marker.read_bytes() != b"mail-hantong-cli-v1\n":
        raise _fail("pin_owner")
    if os.name != "posix":
        raise _fail("pin_platform")
    flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    root_fd = os.open(install_root, flags)
    opened = [root_fd]
    try:
        for name in ("release-pins", item.version, item.commit):
            try:
                child = os.open(name, flags, dir_fd=opened[-1])
            except FileNotFoundError:
                try:
                    os.mkdir(name, 0o700, dir_fd=opened[-1])
                    child = os.open(name, flags, dir_fd=opened[-1])
                except OSError as error:
                    raise _fail("pin_directory", error) from error
            except OSError as error:
                raise _fail("pin_directory", error) from error
            stat_result = os.fstat(child)
            if stat_result.st_uid != os.geteuid() or stat_result.st_mode & 0o077:
                os.close(child)
                raise _fail("pin_directory")
            opened.append(child)
        root_fd = opened[-1]
        root = install_root / "release-pins" / item.version / item.commit
    except BaseException:
        for fd in reversed(opened):
            try: os.close(fd)
            except OSError: pass
        raise
    try:
        pin = json.dumps({"commit": item.commit, "descriptor_sha256": item.descriptor_sha256,
                          "format_version": 1, "target": TARGET, "version": item.version},
                         separators=(",", ":"), sort_keys=True).encode("ascii")
        for name, data in (("pin.json", pin), ("descriptor.json", descriptor)):
            fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600, dir_fd=root_fd)
            try:
                written = 0
                while written < len(data):
                    count = os.write(fd, data[written:])
                    if count <= 0: raise _fail("pin_write")
                    written += count
                os.fsync(fd)
            finally:
                os.close(fd)
        for directory_fd in reversed(opened):
            os.fsync(directory_fd)
        return root
    except BaseException:
        # Never recursively remove a path after a race; leave the private
        # evidence for operator inspection and avoid deleting unknown files.
        raise
    finally:
        for fd in reversed(opened):
            try: os.close(fd)
            except OSError: pass


def build_payload(source: bytes, expected_sha256: str) -> bytes:
    """Validate a reviewed stdlib-only payload before embedding it in E9."""
    if not isinstance(source, bytes) or not re.fullmatch(r"[0-9a-f]{64}", expected_sha256 or ""):
        raise _fail("payload_digest")
    if len(source) == 0 or len(source) > 256 * 1024 or b"\x00" in source:
        raise _fail("payload_size")
    try:
        source.decode("utf-8")
        compile(source, "<reviewed-e9-payload>", "exec", dont_inherit=True)
    except (UnicodeDecodeError, SyntaxError) as error:
        raise _fail("payload_source", error) from error
    if hashlib.sha256(source).hexdigest() != expected_sha256:
        raise _fail("payload_digest")
    return source


def run_bootstrap(item: BootstrapTuple, install_root: Path) -> None:
    """One-time E9 handoff. Activation/locks remain in the sealed Core mode."""
    if sys.platform != "linux" or not sys.flags.isolated or not sys.flags.no_site:
        raise _fail("python_isolation")
    if os.uname().machine != "x86_64":
        raise _fail("platform")
    owner_marker = install_root / ".mail-hantong-owner"
    _safe_regular(owner_marker, "install_owner", 1024, owner=os.geteuid())
    if owner_marker.read_bytes() != b"mail-hantong-cli-v1\n":
        raise _fail("install_owner")
    roots = []
    for entry in sys.path:
        path = Path(entry)
        if not entry or not path.is_absolute():
            raise _fail("stdlib_path")
        if path.exists():
            if path.is_dir():
                roots.append(path)
            else:
                _safe_regular(path, "stdlib", 64 * 1024 * 1024, owner=0)
                _safe_ancestor(path.parent, "stdlib", owner=0)
        else:
            # Isolated OS Python may include its not-installed standard zip.
            if path.suffix != ".zip" or not path.name.startswith("python"):
                raise _fail("stdlib_missing")
            _safe_ancestor(path.parent, "stdlib", owner=0)
    validate_python_prerequisites(stdlib=roots)
    validate_tuple(item)
    _safe_ancestor(install_root, "install_root")
    if install_root.stat().st_uid != os.geteuid():
        raise _fail("install_root_owner")
    os.environ.clear()
    descriptor = fetch_fixed(fixed_url(item, "linux-pinned-candidate.json"), max_bytes=MAX_DESCRIPTOR)
    if hashlib.sha256(descriptor).hexdigest() != item.descriptor_sha256:
        raise _fail("descriptor_digest")
    parsed = parse_descriptor(descriptor)
    if parsed["version"] != item.version or parsed["commit"] != item.commit:
        raise _fail("descriptor_tuple")
    with tempfile.TemporaryDirectory(prefix=".hantong-bootstrap-", dir=install_root) as temporary:
        temporary = Path(temporary)
        descriptor_path = temporary / "linux-pinned-candidate.json"
        descriptor_path.write_bytes(descriptor)
        package_path = temporary / parsed["package_filename"]
        source_path = temporary / parsed["source_filename"]
        package_path.write_bytes(fetch_fixed(fixed_url(item, package_path.name), max_bytes=MAX_PACKAGE))
        source_path.write_bytes(fetch_fixed(fixed_url(item, source_path.name), max_bytes=128 * 1024 * 1024))
        root, report = stage_verified_candidate(descriptor_path, package_path, source_path,
                                                install_root, item.version, item.commit)
    persist_pin(install_root, item, descriptor)
    # Re-open only public staged bytes; sealed readback is checked against the
    # already independently verified manifest, never a fresh mutable hash claim.
    manifest = _read_pinned(root / "release-manifest.json", MAX_MANIFEST, "manifest")
    if hashlib.sha256(manifest).hexdigest() != report.manifest_sha256:
        raise _fail("manifest_changed")
    records = json.loads(manifest)["files"]
    core = next(row for row in records if row["path"] == "bin/mail-hantong-core")
    image = _read_pinned(root / core["path"], 64 * 1024 * 1024, "image")
    if len(image) != core["size"] or hashlib.sha256(image).hexdigest() != core["sha256"]:
        raise _fail("image_changed")
    context = make_context(version=item.version, commit=item.commit,
                           source_sha256=report.source_sha256, descriptor_sha256=item.descriptor_sha256,
                           manifest_sha256=report.manifest_sha256, asset_relative_path="bin/mail-hantong-core",
                           asset_size=core["size"], asset_sha256=core["sha256"])
    roles = prepare_execution_roles(root, image, context, manifest, descriptor)
    try:
        map_roles_for_exec(roles)
        os.execve(ROLE_IMAGE, ["mail-hantong-core", "linux-update-v2", "bootstrap"], {})
    finally:
        for fd in set(roles.values()) | set(roles):
            try:
                os.close(fd)
            except OSError:
                pass


def assemble_payload(verifier_source: bytes, bootstrap_source: bytes,
                     item: BootstrapTuple, install_root: str) -> bytes:
    """Build-time bundling only: the reviewed payload imports no project files."""
    validate_tuple(item)
    # Only built-in modules run before checking the OS import search roots.
    prefix = '''from __future__ import annotations
import sys as _boot_sys
import posix as _boot_os
if _boot_sys.platform != "linux" or not _boot_sys.flags.isolated or not _boot_sys.flags.no_site:
    raise SystemExit("python_isolation")
for _boot_entry in _boot_sys.path:
    if not _boot_entry.startswith("/") or any(p in (".", "..") for p in _boot_entry.split("/")):
        raise SystemExit("stdlib_path")
    _boot_cursor = _boot_entry
    while True:
        try:
            _boot_stat = _boot_os.lstat(_boot_cursor)
        except FileNotFoundError:
            if _boot_cursor != _boot_entry or not _boot_cursor.endswith(".zip"):
                raise SystemExit("stdlib_missing")
        else:
            if _boot_stat.st_uid != 0 or _boot_stat.st_mode & 0o6022 or _boot_stat.st_mode & 0o170000 not in (0o040000, 0o100000):
                raise SystemExit("stdlib_unsafe")
        if _boot_cursor == "/":
            break
        _boot_cursor = _boot_cursor.rsplit("/", 1)[0] or "/"
'''
    sources = []
    for source in (verifier_source, bootstrap_source):
        text = source.decode("utf-8").replace("\r\n", "\n").replace("from __future__ import annotations\n", "")
        text = text.replace("from linux_pinned_candidate import CandidateReport, _read as _read_pinned, verify_candidate\n", "")
        sources.append(text)
    payload = (prefix + sources[0] + "\n_read_pinned = _read\n" + sources[1]
               + "\ntry:\n    run_bootstrap(BootstrapTuple(" + repr(item.version) + "," + repr(item.commit)
               + "," + repr(item.descriptor_sha256) + "," + repr(item.base_url) + "), Path(" + repr(install_root) + "))\n"
               + "except BootstrapError as _error:\n    raise SystemExit(str(_error))\n"
               + "except Exception:\n    raise SystemExit('bootstrap_failed')\n").encode("utf-8")
    return build_payload(payload, hashlib.sha256(payload).hexdigest())

