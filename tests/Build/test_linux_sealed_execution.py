"""Bounded Linux-only tests for sealed memfd execution identity.

These tests exercise a synthetic ELF fixture only.  They do not assert release
provenance, product IPC identity, PID reuse, or updater behavior.
"""
import errno
import hashlib
import mmap
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
from pathlib import Path

if sys.platform == "linux":
    import fcntl
else:
    fcntl = None


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "tests" / "Fakes" / "linux_sealed_exec_fixture.c"
MFD_EXEC = 0x0010
IMAGE_FD = 198
EXEC_FD = 197
EXEC_DENIED = 44
REQUIRED_SEALS = ((fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW |
                   fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
                  if fcntl is not None else 0)


@unittest.skipUnless(sys.platform == "linux", "Linux memfd execution required")
class LinuxSealedExecutionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cc = shutil.which("cc") or shutil.which("gcc")
        if not cls.cc:
            raise AssertionError("Linux compiler required for sealed execution fixture")
        cls.tmp = tempfile.TemporaryDirectory(prefix="hantong-sealed-exec-")
        cls.binary = Path(cls.tmp.name) / "fixture"
        result = subprocess.run(
            [cls.cc, "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
             str(FIXTURE), "-o", str(cls.binary)],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode:
            raise AssertionError("fixture compiler failed: " + result.stderr[-500:])

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def make_image(self, payload=None):
        payload = payload if payload is not None else self.binary.read_bytes()
        flags = os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING | MFD_EXEC
        fd = os.memfd_create("hantong-synthetic-exec", flags)
        self.write_all(fd, payload)
        return fd

    @staticmethod
    def write_all(fd, payload):
        offset = 0
        while offset < len(payload):
            count = os.write(fd, payload[offset:])
            if count <= 0:
                raise AssertionError("memfd write made no progress")
            offset += count

    def seal(self, fd, expected):
        fcntl.fcntl(fd, fcntl.F_ADD_SEALS, REQUIRED_SEALS)
        self.assertEqual(fcntl.fcntl(fd, fcntl.F_GET_SEALS) & REQUIRED_SEALS,
                         REQUIRED_SEALS)
        value = os.pread(fd, len(expected) + 1, 0)
        self.assertEqual(len(value), len(expected))
        self.assertEqual(hashlib.sha256(value).digest(), hashlib.sha256(expected).digest())

    def execute(self, fd, extra_fd=None, timeout=5, mode="run"):
        child = os.fork()
        reaped = False
        if child == 0:
            try:
                os.dup2(fd, EXEC_FD)
                flags = fcntl.fcntl(EXEC_FD, fcntl.F_GETFD)
                fcntl.fcntl(EXEC_FD, fcntl.F_SETFD, flags & ~fcntl.FD_CLOEXEC)
                os.dup2(fd if extra_fd is None else extra_fd, IMAGE_FD)
                flags = fcntl.fcntl(IMAGE_FD, fcntl.F_GETFD)
                fcntl.fcntl(IMAGE_FD, fcntl.F_SETFD, flags & ~fcntl.FD_CLOEXEC)
                os.execve(EXEC_FD, ["hantong-sealed-fixture", mode], {})
            except OSError as failure:
                os._exit(EXEC_DENIED if failure.errno in (errno.EACCES, errno.EPERM) else 127)
            except BaseException:
                os._exit(127)
        deadline = time.monotonic() + timeout
        try:
            while True:
                if time.monotonic() >= deadline:
                    try:
                        os.kill(child, signal.SIGKILL)
                    except ProcessLookupError:
                        reaped = True
                        return None
                    os.waitpid(child, 0)
                    reaped = True
                    return None
                try:
                    waited, status = os.waitpid(child, os.WNOHANG)
                except InterruptedError:
                    continue
                except ChildProcessError:
                    reaped = True
                    raise
                if waited == child:
                    reaped = True
                    return os.waitstatus_to_exitcode(status)
                time.sleep(0.01)
        finally:
            if not reaped:
                try:
                    os.kill(child, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    os.waitpid(child, 0)
                except ChildProcessError:
                    pass

    def seal_then_execute(self, fd, expected, **kwargs):
        self.seal(fd, expected)
        return self.execute(fd, **kwargs)

    def test_SEC03_sealed_copy_executes_and_binds_proc_image_to_same_fd(self):
        expected = self.binary.read_bytes()
        fd = self.make_image(expected)
        self.addCleanup(os.close, fd)
        self.assertEqual(self.seal_then_execute(fd, expected), 0)

    def test_SEC03_substituted_inherited_image_fd_is_rejected(self):
        expected = self.binary.read_bytes()
        executed = self.make_image(expected)
        forged = self.make_image(expected)
        self.addCleanup(os.close, executed)
        self.addCleanup(os.close, forged)
        self.seal(executed, expected)
        self.seal(forged, expected)
        self.assertEqual(self.execute(executed, forged), 41)

    def test_SEC03_changed_bytes_during_copy_fail_before_exec(self):
        expected = self.binary.read_bytes()
        source = Path(self.tmp.name) / "copy-source"
        source.write_bytes(expected)
        candidate = os.memfd_create("hantong-copy-candidate",
                                    os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING | MFD_EXEC)
        self.addCleanup(os.close, candidate)
        chunk = 4096
        with source.open("rb") as source_file:
            first = source_file.read(chunk)
        self.write_all(candidate, first)
        with source.open("r+b") as source_file:
            source_file.seek(chunk)
            source_file.write(bytes([expected[chunk] ^ 1]))
            source_file.flush()
        with source.open("rb") as source_file:
            source_file.seek(chunk)
            remainder = source_file.read()
        self.write_all(candidate, remainder)
        with mock.patch.object(self, "execute") as execute:
            with self.assertRaises(AssertionError):
                self.seal_then_execute(candidate, expected)
            execute.assert_not_called()

    def test_SEC03_source_mutation_after_sealed_copy_cannot_change_snapshot(self):
        source = Path(self.tmp.name) / "source"
        expected = self.binary.read_bytes()
        source.write_bytes(expected)
        fd = self.make_image(source.read_bytes())
        self.addCleanup(os.close, fd)
        source.write_bytes(b"changed source bytes")
        self.assertEqual(self.seal_then_execute(fd, expected), 0)

    def test_SEC03_post_final_check_write_truncate_and_mapping_are_denied(self):
        expected = self.binary.read_bytes()
        fd = self.make_image(expected)
        self.addCleanup(os.close, fd)
        self.seal(fd, expected)
        actions = (lambda: os.pwrite(fd, b"X", 0),
                   lambda: os.ftruncate(fd, len(expected) - 1),
                   lambda: mmap.mmap(fd, len(expected), access=mmap.ACCESS_WRITE))
        for action in actions:
            with self.subTest(action=action):
                try:
                    action()
                except OSError as failure:
                    self.assertIn(failure.errno, (errno.EPERM, errno.EACCES))
                else:
                    self.fail("sealed image mutation unexpectedly succeeded")
        self.assertEqual(self.execute(fd), 0)

    def test_SEC03_unsealed_image_is_rejected_by_fixture(self):
        fd = self.make_image(self.binary.read_bytes())
        self.addCleanup(os.close, fd)
        self.assertEqual(self.execute(fd), 42)

    def test_SEC03_explicit_nonexec_seal_denies_exec(self):
        fd = os.memfd_create("hantong-nonexec", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING | 0x0008)
        self.addCleanup(os.close, fd)
        os.write(fd, self.binary.read_bytes())
        self.seal(fd, self.binary.read_bytes())
        self.assertEqual(self.execute(fd), EXEC_DENIED)

    def test_SEC03_owned_child_timeout_is_reaped(self):
        fd = self.make_image(self.binary.read_bytes())
        self.addCleanup(os.close, fd)
        self.seal(fd, self.binary.read_bytes())
        self.assertIsNone(self.execute(fd, timeout=0.05, mode="sleep"))
        with self.assertRaises(ChildProcessError):
            os.waitpid(-1, os.WNOHANG)


if __name__ == "__main__":
    unittest.main()
