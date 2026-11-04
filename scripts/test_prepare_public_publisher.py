#!/usr/bin/env python3
"""Focused synthetic tests for prepare_public_publisher."""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import tempfile
import unittest
import zipfile

from prepare_public_publisher import MEMBER, PublisherError, prepare


class PublisherTests(unittest.TestCase):
    def archive(self, root: Path, members: dict[str, bytes]) -> Path:
        path = root / "asset.zip"
        with zipfile.ZipFile(path, "w") as bundle:
            for name, data in members.items():
                bundle.writestr(name, data)
        return path

    def test_hash_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = self.archive(root, {MEMBER: b"synthetic"})
            with self.assertRaises(PublisherError):
                prepare(archive, root / "out.json", "0" * 64)

    def test_extracts_only_exact_bounded_member(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = self.archive(root, {MEMBER: b"synthetic", "publisher/other.json": b"ignored"})
            result = prepare(archive, root / "out.json", hashlib.sha256(archive.read_bytes()).hexdigest())
            self.assertEqual(result.read_bytes(), b"synthetic")
            if os.name != "nt":
                self.assertEqual(result.stat().st_mode & 0o777, 0o600)

    def test_traversal_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = self.archive(root, {MEMBER: b"synthetic", "../escape": b"bad"})
            with self.assertRaises(PublisherError):
                prepare(archive, root / "out.json", hashlib.sha256(archive.read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
