"""Offline verifier for a pinned Linux candidate.

This module deliberately has no transport, process execution, or installation
capability.  It verifies a local descriptor, source archive, and package only.
"""
from __future__ import annotations

from dataclasses import dataclass
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import tarfile
import zipfile


MAX_DESCRIPTOR = 256 * 1024
MAX_ARCHIVE = 256 * 1024 * 1024
MAX_SOURCE = 128 * 1024 * 1024
MAX_MEMBER = 64 * 1024 * 1024
MAX_MEMBERS = 4096
TARGET = "linux-x86_64"
TARGET_TRIPLE = "x86_64-unknown-linux-gnu"
REQUIRED = {"VERSION", "BUILD-METADATA.json", "release-manifest.json",
            "FILES.sha256", "installed-files.txt", "bin/hantong",
            "bin/hantong-t", "bin/hantong-mcp", "bin/mail-hantong-core"}


class CandidateError(ValueError):
    """Finite local verification failure; never includes artifact contents."""


@dataclass(frozen=True)
class CandidateReport:
    version: str
    commit: str
    target: str
    source_sha256: str
    package_sha256: str
    manifest_sha256: str
    verified_only: bool = True
    install_authorized: bool = False


def _regular(path: Path, label: str) -> None:
    try:
        item = path.lstat()
    except OSError as error:
        raise CandidateError(f"{label}_missing") from error
    if not stat.S_ISREG(item.st_mode) or item.st_nlink != 1:
        raise CandidateError(f"{label}_shape")


def _read(path: Path, limit: int, label: str) -> bytes:
    _regular(path, label)
    before = path.lstat()
    if before.st_size > limit:
        raise CandidateError(f"{label}_size")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        fd = os.open(path, flags)
    except OSError as error:
        raise CandidateError(f"{label}_open") from error
    try:
        opened = os.fstat(fd)
        identity = (before.st_dev, before.st_ino, before.st_uid, before.st_mode, before.st_nlink)
        opened_identity = (opened.st_dev, opened.st_ino, opened.st_uid, opened.st_mode, opened.st_nlink)
        if not stat.S_ISREG(opened.st_mode) or opened.st_nlink != 1 or opened.st_size > limit or identity != opened_identity:
            raise CandidateError(f"{label}_changed")
        with os.fdopen(os.dup(fd), "rb") as stream:
            data = stream.read(limit + 1)
        after = os.fstat(fd)
        path_after = path.lstat()
        if (len(data) > limit or after.st_size != opened.st_size
                or after.st_mtime_ns != opened.st_mtime_ns
                or (path_after.st_dev, path_after.st_ino, path_after.st_nlink) !=
                (opened.st_dev, opened.st_ino, opened.st_nlink)):
            raise CandidateError(f"{label}_changed")
        return data
    finally:
        os.close(fd)


def _json(data: bytes, label: str):
    if len(data) > 4 * 1024 * 1024:
        raise CandidateError(label)
    depth = 0
    quoted = escaped = False
    for byte in data:
        if quoted:
            if escaped:
                escaped = False
            elif byte == 92:
                escaped = True
            elif byte == 34:
                quoted = False
        elif byte == 34:
            quoted = True
        elif byte in (91, 123):
            depth += 1
            if depth > 64:
                raise CandidateError(label)
        elif byte in (93, 125):
            depth -= 1
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise CandidateError(f"{label}_duplicate_key")
            result[key] = value
        return result
    def invalid_constant(value):
        raise CandidateError(f"{label}_constant")
    try:
        return json.loads(data.decode("utf-8"), object_pairs_hook=unique,
                          parse_constant=invalid_constant)
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise CandidateError(label) from error


def _name(value: str) -> str:
    if (not isinstance(value, str) or not value or "\\" in value or ":" in value
            or "\x00" in value or "//" in value or value.endswith("/")
            or any(ord(c) < 32 or c in '<>"|?*' for c in value)):
        raise CandidateError("archive_path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", ".", "..") for part in value.split("/")):
        raise CandidateError("archive_path")
    return str(path)


def _source_members(data: bytes) -> dict[str, bytes]:
    result = {}
    try:
        marker = data.rfind(b"PK\x05\x06", max(0, len(data) - 65_557))
        if marker < 0 or marker + 22 > len(data):
            raise CandidateError("source_archive")
        if marker + 22 + struct.unpack_from("<H", data, marker + 20)[0] != len(data):
            raise CandidateError("source_archive")
        central_count = struct.unpack_from("<H", data, marker + 10)[0]
        if central_count == 0xFFFF or central_count > MAX_MEMBERS:
            raise CandidateError("source_members")
        disk, central_disk, disk_count = struct.unpack_from("<HHH", data, marker + 4)
        central_size, cursor = struct.unpack_from("<II", data, marker + 12)
        if disk or central_disk or disk_count != central_count or cursor + central_size != marker:
            raise CandidateError("source_archive")
        actual_count = 0
        while cursor < marker:
            if cursor + 46 > marker or data[cursor:cursor + 4] != b"PK\x01\x02":
                raise CandidateError("source_archive")
            lengths = struct.unpack_from("<HHH", data, cursor + 28)
            cursor += 46 + sum(lengths)
            actual_count += 1
            if actual_count > MAX_MEMBERS:
                raise CandidateError("source_members")
        if cursor != marker or actual_count != central_count:
            raise CandidateError("source_archive")
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            names = set()
            total = 0
            for count, info in enumerate(archive.infolist(), 1):
                if count > MAX_MEMBERS:
                    raise CandidateError("source_members")
                name = _name(info.filename)
                mode = (info.external_attr >> 16) & 0xFFFF
                file_type = stat.S_IFMT(mode)
                if name in names or file_type not in (0, stat.S_IFREG):
                    raise CandidateError("source_member")
                if info.file_size > MAX_MEMBER or info.compress_size > MAX_MEMBER:
                    raise CandidateError("source_member")
                total += info.file_size
                if total > MAX_SOURCE:
                    raise CandidateError("source_size")
                result[name] = archive.read(info)
                names.add(name)
            if not names:
                raise CandidateError("source_members")
    except (zipfile.BadZipFile, OSError) as error:
        if isinstance(error, CandidateError):
            raise
        raise CandidateError("source_archive") from error
    return result


def _canonical_source_hash(members: dict[str, bytes]) -> str:
    """Match traceability.source_hash over the source archive's file entries."""
    digest = hashlib.sha256(b"mail-hantong.source.lf.v2\0")
    suffixes = {".rs", ".py", ".toml", ".txt", ".eml", ".mbox", ".ps1", ".sh",
                ".cert", ".fullchain", ".key", ".manifest", ".html"}
    fixed = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "install.ps1", "install.sh",
             "tests/Corpus/index.json", "scripts/native-tools.json", "scripts/check-all",
             "scripts/traceability", "scripts/test-mail-server", "packaging/licenses/index.json"}
    names = []
    for name in members:
        path = PurePosixPath(name)
        include = name in fixed
        include = include or (path.parts and path.parts[0] in {"crates", "apps", "tests", "scripts", "packaging"}
                             and path.suffix.lower() in suffixes and path.name not in {"traceability.json", "execution.json"}
                             and "__pycache__" not in path.parts and (len(path.parts) < 2 or path.parts[-2] not in {"target", "artifacts", ".local"}))
        include = include or (path.parts[:2] == ("docs", "contracts") and path.suffix.lower() in {".md", ".json", ".sql"})
        if include:
            names.append(name)
    for name in sorted(names):
        encoded = name.encode()
        data = members[name]
        if not name.startswith("tests/Corpus/"):
            data = data.replace(b"\r\n", b"\n")
        digest.update(len(encoded).to_bytes(4, "big")); digest.update(encoded)
        digest.update(len(data).to_bytes(8, "big")); digest.update(data)
    return digest.hexdigest()


def _package_members(data: bytes) -> dict[str, bytes]:
    result = {}
    total = 0
    try:
        # Bound extension/PAX data before tarfile parses metadata preceding members.
        with gzip.GzipFile(fileobj=io.BytesIO(data)) as compressed:
            expanded = compressed.read(MAX_ARCHIVE + 1)
        if len(expanded) > MAX_ARCHIVE:
            raise CandidateError("package_size")
        # The packager emits USTAR regular files, so extensions are unnecessary.
        offset = count = 0
        while offset < len(expanded):
            header = expanded[offset:offset + 512]
            if header == bytes(512):
                if any(expanded[offset:]):
                    raise CandidateError("package_archive")
                break
            if len(header) != 512 or header[156:157] not in (b"\0", b"0"):
                raise CandidateError("package_member")
            size_field = header[124:136].strip(b" \0") or b"0"
            if not re.fullmatch(b"[0-7]{1,11}", size_field):
                raise CandidateError("package_member")
            size = int(size_field, 8)
            count += 1
            if size > MAX_MEMBER or count > MAX_MEMBERS:
                raise CandidateError("package_member")
            offset += 512 + ((size + 511) // 512) * 512
            if offset > len(expanded):
                raise CandidateError("package_archive")
        with tarfile.open(fileobj=io.BytesIO(expanded), mode="r:") as archive:
            for count, member in enumerate(archive, 1):
                if count > MAX_MEMBERS:
                    raise CandidateError("package_members")
                name = _name(member.name)
                if name in result or not member.isfile() or member.size > MAX_MEMBER:
                    raise CandidateError("package_member")
                total += member.size
                if total > MAX_ARCHIVE:
                    raise CandidateError("package_size")
                stream = archive.extractfile(member)
                if stream is None:
                    raise CandidateError("package_member")
                value = stream.read(member.size + 1)
                if len(value) != member.size:
                    raise CandidateError("package_member")
                result[name] = value
            if not result:
                raise CandidateError("package_members")
    except (tarfile.TarError, OSError, EOFError) as error:
        if isinstance(error, CandidateError):
            raise
        raise CandidateError("package_archive") from error
    return result


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def verify_candidate(descriptor_path: Path, package_path: Path, source_path: Path,
                     system: str, machine: str) -> CandidateReport:
    """Verify a pinned local candidate without installing or executing anything."""
    if system != "Linux" or machine != "x86_64":
        raise CandidateError("platform")
    descriptor = _json(_read(descriptor_path, MAX_DESCRIPTOR, "descriptor"), "descriptor")
    expected = {"schema_version", "version", "package_version", "target", "commit", "source_filename",
                "cargo_lock_sha256",
                "source_sha256", "source_archive_sha256", "package_filename", "package_sha256"}
    if not isinstance(descriptor, dict) or set(descriptor) != expected:
        raise CandidateError("descriptor")
    if (type(descriptor["schema_version"]) is not int or descriptor["schema_version"] != 1
            or descriptor["target"] != TARGET):
        raise CandidateError("descriptor")
    commit = descriptor["commit"]
    if not isinstance(commit, str) or len(commit) != 40 or any(c not in "0123456789abcdef" for c in commit):
        raise CandidateError("commit")
    version = descriptor["version"]
    package_version = descriptor["package_version"]
    if (not isinstance(version, str) or len(version) > 64 or not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*", version)
            or not isinstance(package_version, str) or len(package_version) > 64 or not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*)?", package_version)):
        raise CandidateError("version")
    for value in (version, package_version):
        if "-" in value and any(p.isdigit() and len(p) > 1 and p.startswith("0")
                                for p in value.split("-", 1)[1].split(".")):
            raise CandidateError("version")
    for key in ("source_filename", "package_filename", "source_sha256", "source_archive_sha256", "package_sha256"):
        if not isinstance(descriptor[key], str) or not descriptor[key]:
            raise CandidateError("descriptor")
    if (descriptor["package_filename"] != f"mail-hantong-cli-{version}-{TARGET}.tar.gz"
            or descriptor["source_filename"] != f"mail-hantong-source-{version}.zip"):
        raise CandidateError("filename")
    for key in ("source_sha256", "source_archive_sha256", "package_sha256", "cargo_lock_sha256"):
        if not isinstance(descriptor[key], str) or not re.fullmatch("[0-9a-f]{64}", descriptor[key]):
            raise CandidateError("descriptor")
    source = _read(source_path, MAX_SOURCE, "source")
    package = _read(package_path, MAX_ARCHIVE, "package")
    if source_path.name != descriptor["source_filename"] or package_path.name != descriptor["package_filename"]:
        raise CandidateError("filename")
    source_archive_digest = _sha(source)
    package_digest = _sha(package)
    if source_archive_digest != descriptor["source_archive_sha256"]:
        raise CandidateError("source_digest")
    if package_digest != descriptor["package_sha256"]:
        raise CandidateError("package_digest")
    source_members = _source_members(source)
    if source_members.get("SOURCE-COMMIT") != (commit + "\n").encode():
        raise CandidateError("commit")
    if "Cargo.lock" not in source_members:
        raise CandidateError("cargo_lock")
    cargo_lock_digest = _sha(source_members["Cargo.lock"])
    if descriptor.get("cargo_lock_sha256") != cargo_lock_digest:
        raise CandidateError("cargo_lock")
    source_digest = _canonical_source_hash(source_members)
    if source_digest != descriptor["source_sha256"]:
        raise CandidateError("source_digest")
    files = _package_members(package)
    if not REQUIRED.issubset(files):
        raise CandidateError("package_inventory")
    if files["VERSION"] != (version + "\n").encode():
        raise CandidateError("version")
    try:
        installed = files["installed-files.txt"].decode("utf-8")
    except UnicodeDecodeError as error:
        raise CandidateError("package_inventory") from error
    inventory = installed.splitlines()
    if len(inventory) != len(set(inventory)) or set(inventory) != set(files):
        raise CandidateError("package_inventory")
    manifest = _json(files["release-manifest.json"], "manifest")
    if (not isinstance(manifest, dict) or manifest.get("format_version") != 1
            or manifest.get("target") != TARGET or manifest.get("version") != version
            or manifest.get("package_version") != package_version
            or not isinstance(manifest.get("files"), list)):
        raise CandidateError("manifest")
    manifest_rows = {}
    for row in manifest["files"]:
        if not isinstance(row, dict) or set(row) != {"path", "sha256", "size"}:
            raise CandidateError("manifest")
        name = _name(row["path"])
        if name in manifest_rows or not isinstance(row["sha256"], str) or len(row["sha256"]) != 64:
            raise CandidateError("manifest")
        if not isinstance(row["size"], int) or isinstance(row["size"], bool) or row["size"] < 0:
            raise CandidateError("manifest")
        manifest_rows[name] = (row["sha256"], row["size"])
    expected_manifest = set(files) - {"release-manifest.json", "FILES.sha256"}
    if set(manifest_rows) != expected_manifest:
        raise CandidateError("manifest")
    for name, (digest, size) in manifest_rows.items():
        if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise CandidateError("manifest")
        if len(files[name]) != size or _sha(files[name]) != digest:
            raise CandidateError("manifest_hash")
    metadata = _json(files["BUILD-METADATA.json"], "metadata")
    if (not isinstance(metadata, dict) or metadata.get("source_commit") != commit
            or metadata.get("cargo_lock_sha256") != cargo_lock_digest):
        raise CandidateError("commit")
    if (metadata.get("format_version") != 1
            or metadata.get("release_label") != version or metadata.get("target") != TARGET_TRIPLE
            or metadata.get("source_hash") != source_digest
            or metadata.get("build_performed") is not True
            or metadata.get("source_relation") != "built_from_current_workspace"
            or not isinstance(metadata.get("binaries"), dict)):
        raise CandidateError("metadata")
    for name in REQUIRED - {"VERSION", "BUILD-METADATA.json", "release-manifest.json",
                             "FILES.sha256", "installed-files.txt"}:
        if metadata["binaries"].get(name) != _sha(files[name]):
            raise CandidateError("metadata")
    sums = {}
    try:
        checksum_lines = files["FILES.sha256"].decode("utf-8").splitlines()
    except UnicodeDecodeError as error:
        raise CandidateError("checksums") from error
    for line in checksum_lines:
        fields = line.split("  ", 1)
        if len(fields) != 2 or fields[1] in sums or len(fields[0]) != 64:
            raise CandidateError("checksums")
        sums[fields[1]] = fields[0]
    if set(sums) != set(files) - {"FILES.sha256"}:
        raise CandidateError("checksums")
    if any(sums[name] != _sha(value) for name, value in files.items() if name != "FILES.sha256"):
        raise CandidateError("checksums")
    for name in ("bin/hantong", "bin/hantong-t", "bin/hantong-mcp", "bin/mail-hantong-core"):
        data = files[name]
        if len(data) < 64 or data[:6] != b"\x7fELF\x02\x01" or int.from_bytes(data[18:20], "little") != 62:
            raise CandidateError("native")
    return CandidateReport(version, commit, TARGET, source_digest, package_digest,
                           _sha(files["release-manifest.json"]))
