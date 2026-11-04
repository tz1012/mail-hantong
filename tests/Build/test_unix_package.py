import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import package_unix
from package_cli import PackageError


class UnixPackageTests(unittest.TestCase):
    def test_INST01_archive_is_deterministic_and_contains_no_links(self):
        with tempfile.TemporaryDirectory() as directory:
            first, second = (Path(directory) / name for name in ("first.tgz", "second.tgz"))
            entries = {"bin/hantong": b"synthetic", "README.md": b"example"}
            package_unix.write_archive(first, entries)
            package_unix.write_archive(second, entries)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with tarfile.open(first) as archive:
                self.assertTrue(all(item.isfile() for item in archive.getmembers()))
                self.assertEqual(archive.getmember("bin/hantong").mode, 0o755)

    def test_SEC03_archive_rejects_path_escape_before_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            for entry in ("../escape", "/absolute", "bin/../escape", "bin\\escape"):
                with self.subTest(entry=entry), self.assertRaises(PackageError):
                    package_unix.write_archive(Path(directory) / "candidate.tgz", {entry: b"x"})

    def test_INST01_elf_architecture_is_checked_instead_of_filename(self):
        header = bytearray(64)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = (62).to_bytes(2, "little")
        package_unix.check_elf(header, "x86_64")
        with self.assertRaises(PackageError):
            package_unix.check_elf(header, "aarch64")
        with self.assertRaises(PackageError):
            package_unix.check_elf(b"MZ" + bytes(62), "x86_64")

    def test_INST01_macho_architecture_is_checked_instead_of_filename(self):
        header = bytearray(32)
        header[:4] = b"\xcf\xfa\xed\xfe"
        header[4:8] = (0x01000007).to_bytes(4, "little")
        package_unix.check_macho(header, "x86_64")
        with self.assertRaises(PackageError):
            package_unix.check_macho(header, "arm64")
        header[4:8] = (0x0100000C).to_bytes(4, "little")
        package_unix.check_macho(header, "arm64")
        with self.assertRaises(PackageError):
            package_unix.check_macho(b"\x7fELF" + bytes(28), "x86_64")

    def test_INST01_unix_target_matrix_includes_macos_architectures(self):
        self.assertEqual(
            package_unix.TARGETS[("Darwin", "x86_64")],
            ("x86_64-apple-darwin", "macos-x86_64"),
        )
        self.assertEqual(
            package_unix.TARGETS[("Darwin", "arm64")],
            ("aarch64-apple-darwin", "macos-aarch64"),
        )


if __name__ == "__main__":
    unittest.main()
