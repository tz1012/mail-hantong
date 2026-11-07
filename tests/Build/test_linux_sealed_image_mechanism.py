"""Synthetic kernel mechanism experiments, not a product execution-identity implementation."""
import errno
import hashlib
import mmap
import os
import sys
import unittest

if sys.platform == "linux":
    import fcntl


@unittest.skipUnless(sys.platform == "linux", "Linux anonymous-file kernel API required")
class LinuxSealedImageMechanismTests(unittest.TestCase):
    def snapshot(self, payload):
        # Explicit non-executable memfd: this experiment never executes an image.
        fd = os.memfd_create("synthetic-only", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING | 0x0008)
        self.addCleanup(os.close, fd)
        self.assertEqual(os.write(fd, payload), len(payload))
        return fd

    def seal_and_measure(self, fd, expected):
        required = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        fcntl.fcntl(fd, fcntl.F_ADD_SEALS, required)
        self.assertEqual(fcntl.fcntl(fd, fcntl.F_GET_SEALS) & required, required)
        value = os.pread(fd, len(expected) + 1, 0)
        return len(value) == len(expected) and hashlib.sha256(value).digest() == hashlib.sha256(expected).digest()

    def test_SEC03_sealed_copy_rejects_same_size_write_truncate_and_writable_mapping(self):
        payload = b"synthetic image bytes"
        fd = self.snapshot(payload)
        self.assertTrue(self.seal_and_measure(fd, payload))
        for action in (lambda: os.pwrite(fd, b"X", 0), lambda: os.ftruncate(fd, len(payload) - 1),
                       lambda: mmap.mmap(fd, len(payload), access=mmap.ACCESS_WRITE)):
            with self.subTest(action=action), self.assertRaises(OSError) as failure:
                action()
            self.assertIn(failure.exception.errno, (errno.EPERM, errno.EACCES))
        self.assertEqual(os.pread(fd, len(payload), 0), payload)

    def test_SEC03_copy_changed_after_original_measurement_is_not_admitted(self):
        original = b"trusted synthetic bytes"
        changed_copy = b"X" + original[1:]
        fd = self.snapshot(changed_copy)
        self.assertFalse(self.seal_and_measure(fd, original))

    def test_SEC03_preexisting_writable_mapping_blocks_sealing_without_fallback(self):
        fd = self.snapshot(b"synthetic")
        mapping = mmap.mmap(fd, 9, access=mmap.ACCESS_WRITE)
        try:
            with self.assertRaises(OSError) as failure:
                self.seal_and_measure(fd, b"synthetic")
            self.assertEqual(failure.exception.errno, errno.EBUSY)
        finally:
            mapping.close()
        self.assertTrue(self.seal_and_measure(fd, b"synthetic"))

    def test_SEC03_equal_snapshots_do_not_prove_identical_kernel_image_object(self):
        first = self.snapshot(b"same synthetic bytes")
        second = self.snapshot(b"same synthetic bytes")
        self.assertTrue(self.seal_and_measure(first, b"same synthetic bytes"))
        self.assertTrue(self.seal_and_measure(second, b"same synthetic bytes"))
        self.assertNotEqual((os.fstat(first).st_dev, os.fstat(first).st_ino),
                            (os.fstat(second).st_dev, os.fstat(second).st_ino))


if __name__ == "__main__":
    unittest.main()
