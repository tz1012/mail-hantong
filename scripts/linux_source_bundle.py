"""Create a deterministic, provenance-bound source archive from a clean Git tree.

This module is deliberately local-only.  It never contacts a release service and
does not install or execute the archived source.  The returned digest identifies
the exact bytes written to the destination.
"""
from __future__ import annotations

import hashlib
import io
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile
import zipfile

from linux_pinned_candidate import (
    MAX_MEMBER,
    MAX_MEMBERS,
    MAX_SOURCE,
    CandidateError,
    _canonical_source_hash,
    _name,
)


MAX_GIT_ARCHIVE = MAX_SOURCE
SOURCE_COMMIT_NAME = "SOURCE-COMMIT"


class SourceBundleError(ValueError):
    """Finite local source-bundle validation failure."""


def _fail(kind: str, error: BaseException | None = None) -> SourceBundleError:
    return SourceBundleError(kind)


def _run_git(root: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), *args],
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="strict",
            timeout=20,
        )
    except (OSError, subprocess.SubprocessError, UnicodeError) as error:
        raise _fail("git_unavailable", error) from error
    return result.stdout.strip()


def clean_source_commit(root: Path) -> str:
    """Return the full HEAD SHA only when *root* is a clean Git worktree."""
    root = Path(root)
    if not root.is_dir() or root.is_symlink():
        raise _fail("source_root")
    try:
        resolved = root.resolve(strict=True)
    except OSError as error:
        raise _fail("source_root", error) from error
    top = Path(_run_git(resolved, "rev-parse", "--show-toplevel"))
    if top.resolve() != resolved:
        raise _fail("source_root")
    status = _run_git(resolved, "status", "--porcelain=v1", "--untracked-files=all")
    if status:
        raise _fail("source_dirty")
    commit = _run_git(resolved, "rev-parse", "--verify", "HEAD")
    if len(commit) != 40 or any(char not in "0123456789abcdef" for char in commit):
        raise _fail("commit")
    return commit


def _zip_member_name(name: str) -> str:
    try:
        return _name(name)
    except CandidateError as error:
        raise _fail("archive_path", error) from error


def _read_git_archive(root: Path, commit: str) -> dict[str, bytes]:
    # Bound tracked blobs before Git writes, and reject silently omitted gitlinks.
    tree = _run_git(root, "ls-tree", "-r", "-l", "-z", "--full-tree", commit)
    count = size = 0
    expected_members = {}
    for row in tree.split("\0"):
        if not row:
            continue
        metadata, name = row.split("\t", 1)
        fields = metadata.split()
        if len(fields) != 4 or fields[0] not in ("100644", "100755") or fields[1] != "blob":
            raise _fail("source_member")
        _zip_member_name(name)
        count += 1
        blob_size = int(fields[3])
        expected_members[name] = (blob_size, fields[2])
        size += blob_size
        if count > MAX_MEMBERS or blob_size > MAX_MEMBER or size > MAX_SOURCE:
            raise _fail("source_size")
    with tempfile.TemporaryFile(mode="w+b") as source_file:
        try:
            subprocess.run(
                ["git", "-c", "core.autocrlf=false", "-c", "core.eol=lf", "-C", str(root),
                 "archive", "--format=zip", commit],
                check=True,
                stdout=source_file,
                stderr=subprocess.PIPE,
                timeout=30,
            )
        except (OSError, subprocess.SubprocessError) as error:
            raise _fail("archive") from error
        if os.fstat(source_file.fileno()).st_size > MAX_GIT_ARCHIVE:
            raise _fail("source_size")
        members: dict[str, bytes] = {}
        total = 0
        try:
            source_file.seek(0)
            with zipfile.ZipFile(source_file) as archive:
                infos = archive.infolist()
                if not infos or len(infos) > MAX_MEMBERS:
                    raise _fail("source_members")
                for info in infos:
                    mode = (info.external_attr >> 16) & 0xFFFF
                    file_type = stat.S_IFMT(mode)
                    if info.filename.endswith("/") and file_type in (0, stat.S_IFDIR):
                        continue
                    name = _zip_member_name(info.filename)
                    if (name == SOURCE_COMMIT_NAME or name in members
                            or file_type not in (0, stat.S_IFREG)):
                        raise _fail("source_member")
                    if info.file_size > MAX_MEMBER or info.compress_size > MAX_MEMBER:
                        raise _fail("source_member")
                    value = archive.read(info)
                    if len(value) != info.file_size:
                        raise _fail("source_member")
                    total += len(value)
                    if total > MAX_SOURCE:
                        raise _fail("source_size")
                    members[name] = value
        except (OSError, zipfile.BadZipFile) as error:
            raise _fail("archive") from error
        if set(members) != set(expected_members):
            raise _fail("source_inventory")
        for name, value in members.items():
            expected_size, git_oid = expected_members[name]
            # Match the exact committed blob, including export-subst/eol changes.
            blob = b"blob " + str(len(value)).encode("ascii") + b"\0" + value
            if len(value) != expected_size or hashlib.sha1(blob, usedforsecurity=False).hexdigest() != git_oid:
                raise _fail("source_inventory")
        return members


def write_source_bundle(root: Path, destination: Path, expected_commit: str,
                        expected_source_hash: str) -> str:
    """Write a validated source ZIP once, returning its SHA-256 digest.

    The destination is created exclusively.  An existing destination is never
    replaced, and a failed write removes only the file created by this call.
    """
    root = Path(root)
    destination = Path(destination)
    if (not isinstance(expected_commit, str) or
            not re.fullmatch(r"[0-9a-f]{40}", expected_commit) or
            not isinstance(expected_source_hash, str) or
            not re.fullmatch(r"[0-9a-f]{64}", expected_source_hash)):
        raise _fail("expected_provenance")
    commit = clean_source_commit(root)
    if commit != expected_commit:
        raise _fail("commit")
    members = _read_git_archive(root.resolve(), commit)
    canonical = _canonical_source_hash({**members, SOURCE_COMMIT_NAME: (commit + "\n").encode()})
    if canonical != expected_source_hash:
        raise _fail("source_digest")
    parent = destination.parent
    if not parent.is_dir() or destination.exists() or destination.is_symlink():
        raise _fail("destination")
    for cursor in (parent.absolute(), *parent.absolute().parents):
        if cursor.is_symlink() or getattr(cursor, "is_junction", lambda: False)():
            raise _fail("destination")
    parent_stat = parent.stat()
    if os.name == "posix" and (parent_stat.st_uid != os.geteuid() or parent_stat.st_mode & 0o022):
        raise _fail("destination")
    owned_fd = None
    parent_fd = None
    completed = False
    try:
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name in sorted(members):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (0o100644 << 16)
                archive.writestr(info, members[name])
            info = zipfile.ZipInfo(SOURCE_COMMIT_NAME, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (0o100644 << 16)
            archive.writestr(info, (commit + "\n").encode())
        data = buffer.getvalue()
        if len(data) > MAX_SOURCE:
            raise _fail("source_size")
        if os.name == "posix":
            parent_fd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            if (os.fstat(parent_fd).st_dev, os.fstat(parent_fd).st_ino) != (parent_stat.st_dev, parent_stat.st_ino):
                raise _fail("destination")
        owned_fd = os.open(destination.name if parent_fd is not None else destination,
                           os.O_RDWR | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0) |
                           getattr(os, "O_BINARY", 0), 0o644, dir_fd=parent_fd)
        with os.fdopen(os.dup(owned_fd), "w+b") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
            output.seek(0)
            if output.read(MAX_SOURCE + 1) != data:
                raise _fail("destination_changed")
        digest = hashlib.sha256(data).hexdigest()
        if clean_source_commit(root) != commit:
            raise _fail("source_changed")
        if _canonical_source_hash({**_read_git_archive(root.resolve(), commit),
                                   SOURCE_COMMIT_NAME: (commit + "\n").encode()}) != expected_source_hash:
            raise _fail("source_changed")
        actual = destination.lstat()
        owned = os.fstat(owned_fd)
        current_parent = parent.stat()
        if ((actual.st_dev, actual.st_ino) != (owned.st_dev, owned.st_ino)
                or (current_parent.st_dev, current_parent.st_ino) != (parent_stat.st_dev, parent_stat.st_ino)):
            raise _fail("destination_changed")
        completed = True
        return digest
    except SourceBundleError:
        raise
    except (OSError, zipfile.BadZipFile) as error:
        raise _fail("destination", error) from error
    finally:
        if owned_fd is not None and not completed:
            try:
                owned = os.fstat(owned_fd)
                if os.name == "nt":
                    # Windows CRT handles deny deletion while open; actual Linux
                    # packaging keeps the handle pinned through unlink below.
                    os.close(owned_fd)
                    owned_fd = None
                current = (os.stat(destination.name, dir_fd=parent_fd, follow_symlinks=False)
                           if parent_fd is not None else destination.lstat())
                if (current.st_dev, current.st_ino) == (owned.st_dev, owned.st_ino):
                    os.unlink(destination.name, dir_fd=parent_fd) if parent_fd is not None else destination.unlink()
            except (FileNotFoundError, OSError):
                pass
        if owned_fd is not None:
            os.close(owned_fd)
        if parent_fd is not None:
            os.close(parent_fd)
