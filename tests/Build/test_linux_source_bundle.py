from __future__ import annotations

import hashlib
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile
import os
from unittest.mock import patch

ROOT = Path(__file__).parents[2]
import sys
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("linux_source_bundle", ROOT / "scripts/linux_source_bundle.py")
assert SPEC and SPEC.loader
bundle = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bundle
SPEC.loader.exec_module(bundle)
from linux_pinned_candidate import _canonical_source_hash  # noqa: E402


def git(path: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(path), *args], check=True,
                          capture_output=True, text=True).stdout.strip()


class LinuxSourceBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hantong-source-bundle-")
        self.base = Path(self.temp.name).resolve()
        self.root = self.base / "repo"
        self.root.mkdir()
        git(self.root, "init", "--quiet")
        git(self.root, "config", "user.email", "tests@example.invalid")
        git(self.root, "config", "user.name", "Synthetic Tests")
        git(self.root, "config", "commit.gpgsign", "false")
        git(self.root, "config", "core.hooksPath", str(self.root / "empty-hooks"))
        (self.root / "apps").mkdir()
        (self.root / "apps/example.rs").write_text("fn main() {}\n", encoding="utf-8")
        (self.root / "Cargo.lock").write_text("# lock\n", encoding="utf-8")
        git(self.root, "add", "apps/example.rs", "Cargo.lock")
        git(self.root, "commit", "--quiet", "-m", "fixture")
        self.commit = git(self.root, "rev-parse", "HEAD")
        members = {"apps/example.rs": b"fn main() {}\n", "Cargo.lock": b"# lock\n",
                   "SOURCE-COMMIT": (self.commit + "\n").encode()}
        self.source_hash = _canonical_source_hash(members)
        self.destination = self.base / "source.zip"

    def tearDown(self):
        self.temp.cleanup()

    def test_INST01_clean_commit_bundle_is_canonical_and_bound(self):
        self.assertEqual(bundle.clean_source_commit(self.root), self.commit)
        digest = bundle.write_source_bundle(self.root, self.destination, self.commit, self.source_hash)
        self.assertEqual(digest, hashlib.sha256(self.destination.read_bytes()).hexdigest())
        with zipfile.ZipFile(self.destination) as archive:
            self.assertEqual(set(archive.namelist()), {"apps/example.rs", "Cargo.lock", "SOURCE-COMMIT"})
            self.assertEqual(archive.read("SOURCE-COMMIT"), (self.commit + "\n").encode())
        self.assertEqual(bundle.clean_source_commit(self.root), self.commit)

    def test_SEC03_dirty_untracked_wrong_commit_and_existing_destination_rejected(self):
        (self.root / "dirty.txt").write_text("dirty", encoding="utf-8")
        with self.assertRaisesRegex(bundle.SourceBundleError, "source_dirty"):
            bundle.clean_source_commit(self.root)
        (self.root / "dirty.txt").unlink()
        self.assertEqual(bundle.clean_source_commit(self.root), self.commit)
        self.destination.write_bytes(b"keep")
        with self.assertRaisesRegex(bundle.SourceBundleError, "destination"):
            bundle.write_source_bundle(self.root, self.destination, self.commit, self.source_hash)
        self.assertEqual(self.destination.read_bytes(), b"keep")

    def test_SEC03_source_hash_and_commit_mismatch_rejected(self):
        for commit, digest, expected in (("0" * 40, self.source_hash, "commit"),
                                          (self.commit, "0" * 64, "source_digest")):
            with self.subTest(expected=expected), self.assertRaisesRegex(bundle.SourceBundleError, expected):
                bundle.write_source_bundle(self.root, self.destination, commit, digest)
        self.assertFalse(self.destination.exists())

    def test_SEC03_link_is_rejected_and_no_external_file_is_archived(self):
        if not hasattr(Path, "symlink_to"):
            self.skipTest("symlinks unavailable")
        try:
            (self.root / "outside-link").symlink_to(Path(self.temp.name) / "outside")
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable")
        (Path(self.temp.name) / "outside").write_text("outside", encoding="utf-8")
        git(self.root, "add", "outside-link")
        git(self.root, "commit", "--quiet", "-m", "link")
        commit = git(self.root, "rev-parse", "HEAD")
        with self.assertRaisesRegex(bundle.SourceBundleError, "source_member"):
            bundle.write_source_bundle(self.root, self.destination, commit, self.source_hash)
        self.assertFalse(self.destination.exists())

    def test_SEC03_archive_recheck_rejects_source_change(self):
        original = bundle._read_git_archive
        def changed(root, commit):
            result = original(root, commit)
            (self.root / "late.txt").write_text("late", encoding="utf-8")
            return result
        bundle._read_git_archive = changed
        try:
            with self.assertRaisesRegex(bundle.SourceBundleError, "source_dirty"):
                bundle.write_source_bundle(self.root, self.destination, self.commit, self.source_hash)
        finally:
            bundle._read_git_archive = original
        self.assertFalse(self.destination.exists())

    def test_SEC03_linked_output_parent_is_rejected_without_writes(self):
        target = Path(self.temp.name) / "output"
        target.mkdir()
        linked = Path(self.temp.name) / "linked-output"
        try:
            linked.symlink_to(target, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable")
        with self.assertRaisesRegex(bundle.SourceBundleError, "destination"):
            bundle.write_source_bundle(self.root, linked / "source.zip", self.commit, self.source_hash)
        self.assertEqual(list(target.iterdir()), [])

    @unittest.skipUnless(os.name == "posix", "Linux pinned-handle unlink semantics required")
    def test_SEC03_failure_cleanup_preserves_a_replaced_destination(self):
        original = bundle._read_git_archive
        def replaced(root, commit):
            members = original(root, commit)
            if self.destination.exists():
                self.destination.unlink()
                self.destination.write_bytes(b"unrelated synthetic replacement")
            return members
        with patch.object(bundle, "_read_git_archive", side_effect=replaced):
            with self.assertRaisesRegex(bundle.SourceBundleError, "destination_changed"):
                bundle.write_source_bundle(self.root, self.destination, self.commit, self.source_hash)
        self.assertEqual(self.destination.read_bytes(), b"unrelated synthetic replacement")

    def test_SEC03_gitlink_is_rejected_before_archive_generation(self):
        git(self.root, "update-index", "--add", "--cacheinfo", f"160000,{self.commit},submodule")
        git(self.root, "commit", "--quiet", "-m", "synthetic gitlink")
        commit = git(self.root, "rev-parse", "HEAD")
        with self.assertRaisesRegex(bundle.SourceBundleError, "source_member"):
            bundle._read_git_archive(self.root, commit)

    def test_SEC03_archive_attributes_cannot_silently_omit_committed_files(self):
        (self.root / ".gitattributes").write_bytes(b"omitted.txt export-ignore\n")
        (self.root / "omitted.txt").write_bytes(b"committed fixture\n")
        git(self.root, "add", ".gitattributes", "omitted.txt")
        git(self.root, "commit", "--quiet", "-m", "synthetic export-ignore")
        commit = git(self.root, "rev-parse", "HEAD")
        with self.assertRaisesRegex(bundle.SourceBundleError, "source_inventory"):
            bundle._read_git_archive(self.root, commit)

    def test_SEC03_archive_attributes_cannot_substitute_committed_bytes(self):
        (self.root / ".gitattributes").write_bytes(b"substituted.txt export-subst\n")
        (self.root / "substituted.txt").write_bytes(b"$Format:%H$\n")
        git(self.root, "add", ".gitattributes", "substituted.txt")
        git(self.root, "commit", "--quiet", "-m", "synthetic export-subst")
        commit = git(self.root, "rev-parse", "HEAD")
        with self.assertRaisesRegex(bundle.SourceBundleError, "source_inventory"):
            bundle._read_git_archive(self.root, commit)


if __name__ == "__main__":
    unittest.main()
