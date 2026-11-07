"""Safety tests for the offline real-package activation harness."""
from __future__ import annotations

import io
import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import os
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "linux_real_activation", ROOT / "scripts" / "test_linux_real_activation.py"
)
assert SPEC and SPEC.loader
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class LinuxRealActivationHarnessTests(unittest.TestCase):
    def test_SEC03_extract_rejects_links_and_parent_paths(self):
        with tempfile.TemporaryDirectory(prefix="hantong-r2-harness-test-") as directory:
            archive = Path(directory) / "bad.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                link = tarfile.TarInfo("install.sh")
                link.type = tarfile.SYMTYPE
                link.linkname = "/etc/passwd"
                output.addfile(link)
            with self.assertRaisesRegex(RuntimeError, "non-regular"):
                module.extract(archive, Path(directory) / "out", hashlib.sha256(archive.read_bytes()).hexdigest())

    def test_SEC03_extract_rejects_parent_path(self):
        with tempfile.TemporaryDirectory(prefix="hantong-r2-harness-test-") as directory:
            archive = Path(directory) / "bad.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                item = tarfile.TarInfo("../install.sh")
                item.size = 1
                output.addfile(item, io.BytesIO(b"x"))
            with self.assertRaisesRegex(RuntimeError, "unsafe path"):
                module.extract(archive, Path(directory) / "out", hashlib.sha256(archive.read_bytes()).hexdigest())

    @unittest.skipUnless(os.name == "posix", "requires POSIX lock semantics")
    def test_SEC03_busy_storage_retains_root_until_lock_is_released(self):
        import fcntl
        with tempfile.TemporaryDirectory(prefix="hantong-r2-cleanup-") as directory:
            root = Path(directory) / "owned"
            root.mkdir(mode=0o700)
            lock = root / "data/mail-hantong/data/database.lock"
            lock.parent.mkdir(parents=True)
            with lock.open("xb") as held:
                fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
                identity = (root.stat().st_dev, root.stat().st_ino)
                self.assertFalse(module.cleanup_private_root(root, identity, root / "install", {}, set()))
                self.assertTrue(root.is_dir())
            self.assertTrue(module.cleanup_private_root(root, identity, root / "install", {}, set()))
            self.assertFalse(root.exists())

    @unittest.skipUnless(os.name == "posix", "requires POSIX inode identity")
    def test_SEC03_replaced_root_is_never_cleaned(self):
        with tempfile.TemporaryDirectory(prefix="hantong-r2-cleanup-") as directory:
            root = Path(directory) / "owned"
            root.mkdir(mode=0o700)
            identity = (root.stat().st_dev, root.stat().st_ino)
            saved = root.with_name("saved")
            root.rename(saved)
            root.mkdir(mode=0o700)
            self.assertFalse(module.cleanup_private_root(root, identity, root / "install", {}, set()))
            self.assertTrue(root.is_dir())
            self.assertTrue(saved.is_dir())

    def test_SEC03_semantic_version_orders_beta_numerically(self):
        self.assertLess(module.version_key("0.1.0-beta.9"), module.version_key("0.1.0-beta.30"))
        self.assertEqual(module.version_key("0.1.0-beta.30"), module.version_key("0.1.0-beta.30"))


if __name__ == "__main__":
    unittest.main()
