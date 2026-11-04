#!/usr/bin/env python3
"""Safely extract the reviewed public Google Desktop asset from a release ZIP."""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import tempfile
import zipfile


EXPECTED_SHA256 = "b6d15b610ef74e63e17f881a4a01802ac22cecc49bf69b0059664142b3c370fc"
MEMBER = "publisher/google-desktop.json"
MAX_ASSET_BYTES = 16_384


class PublisherError(ValueError):
    """The archive is not the reviewed, bounded publisher input."""


def _archive_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _safe_member(name: str) -> bool:
    path = PurePosixPath(name)
    return bool(name and not path.is_absolute() and "\\" not in name and ":" not in name
                and all(part not in ("", ".", "..") for part in path.parts))


def _is_link(info: zipfile.ZipInfo) -> bool:
    # Unix mode is stored in the upper 16 bits of external_attr.  Reject
    # links before considering the one allowed member.
    return (info.external_attr >> 16) & 0o170000 == 0o120000


def prepare(archive: Path, output: Path, expected_sha256: str = EXPECTED_SHA256) -> Path:
    archive = archive.resolve(strict=True)
    if not archive.is_file():
        raise PublisherError("publisher archive is not a regular file")
    if _archive_sha256(archive) != expected_sha256.lower():
        raise PublisherError("publisher archive checksum mismatch")

    selected: zipfile.ZipInfo | None = None
    try:
        with zipfile.ZipFile(archive) as bundle:
            for info in bundle.infolist():
                if not _safe_member(info.filename) or _is_link(info):
                    raise PublisherError("publisher archive contains an unsafe member")
                if info.filename == MEMBER:
                    if selected is not None:
                        raise PublisherError("publisher asset occurs more than once")
                    selected = info
            if selected is None or selected.is_dir() or not 1 <= selected.file_size <= MAX_ASSET_BYTES:
                raise PublisherError("publisher asset is missing or exceeds its bound")
            with bundle.open(selected) as source:
                data = source.read(MAX_ASSET_BYTES + 1)
            if not 1 <= len(data) <= MAX_ASSET_BYTES:
                raise PublisherError("publisher asset exceeds its bound")
    except (OSError, zipfile.BadZipFile, RuntimeError) as error:
        raise PublisherError("publisher archive could not be validated") from error

    output = output.absolute()
    output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if output.exists() or output.is_symlink():
        raise PublisherError("publisher output already exists")
    temporary: Path | None = None
    try:
        fd, name = tempfile.mkstemp(prefix=".publisher-", dir=output.parent)
        temporary = Path(name)
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "wb") as destination:
            destination.write(data)
            destination.flush()
            os.fsync(destination.fileno())
        os.replace(temporary, output)
        temporary = None
        return output
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-sha256", default=EXPECTED_SHA256)
    args = parser.parse_args()
    try:
        prepare(args.archive, args.output, args.expected_sha256)
        return 0
    except (OSError, PublisherError, ValueError):
        print("error: reviewed publisher asset was not prepared", file=__import__("sys").stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
