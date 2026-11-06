from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
import zipfile
from unittest.mock import patch

ROOT = Path(__file__).parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from traceability import source_hash
SPEC = importlib.util.spec_from_file_location("linux_pinned_candidate", ROOT / "scripts/linux_pinned_candidate.py")
assert SPEC and SPEC.loader
candidate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = candidate
SPEC.loader.exec_module(candidate)


def _elf() -> bytes:
    data = bytearray(b"\x7fELF\x02\x01" + b"\0" * 10)
    data.extend((2).to_bytes(2, "little"))
    data.extend((62).to_bytes(2, "little"))
    data.extend(b"\0" * 42)
    return bytes(data).ljust(64, b"\0")


class LinuxPinnedCandidateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hantong-pinned-")
        self.root = Path(self.temp.name)
        self.source = self.root / "mail-hantong-source-0.1.0-linux-test.zip"
        cargo_lock = b"# synthetic lock\n"
        source_members = {"README.md": b"synthetic source", "Cargo.lock": cargo_lock,
                          "apps/example.rs": b"fn synthetic() {}\n",
                          "docs/contracts/example.md": b"synthetic contract\n",
                          "SOURCE-COMMIT": ("a" * 40 + "\n").encode()}
        with zipfile.ZipFile(self.source, "w") as archive:
            for name, data in source_members.items():
                archive.writestr(name, data)
        self.files = {
            "VERSION": b"0.1.0-linux-test\n",
            "BUILD-METADATA.json": b"",
            "release-manifest.json": b"",
            "FILES.sha256": b"",
            "installed-files.txt": b"",
            "bin/hantong": _elf(),
            "bin/hantong-t": _elf(),
            "bin/hantong-mcp": _elf(),
            "bin/mail-hantong-core": _elf(),
            "README.md": b"synthetic package",
        }
        canonical_digest = candidate._canonical_source_hash(source_members)
        canonical_root = self.root / "canonical-source"
        for name, data in source_members.items():
            if name in {"README.md", "SOURCE-COMMIT"}:
                continue
            path = canonical_root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.assertEqual(canonical_digest, source_hash(canonical_root))
        cargo_hash = hashlib.sha256(cargo_lock).hexdigest()
        self.files["BUILD-METADATA.json"] = json.dumps({
            "format_version": 1, "release_label": "0.1.0-linux-test",
            "target": "x86_64-unknown-linux-gnu", "source_hash": canonical_digest,
            "source_commit": "a" * 40, "cargo_lock_sha256": cargo_hash,
            "build_performed": True, "source_relation": "built_from_current_workspace",
            "binaries": {name: hashlib.sha256(self.files[name]).hexdigest()
                         for name in ("bin/hantong", "bin/hantong-t", "bin/hantong-mcp", "bin/mail-hantong-core")},
        }, separators=(",", ":")).encode()
        self.files["installed-files.txt"] = ("\n".join(sorted(self.files)) + "\n").encode()
        rows = [{"path": name, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
                for name, data in sorted(self.files.items())
                if name not in {"release-manifest.json", "FILES.sha256"}]
        self.files["release-manifest.json"] = json.dumps({
            "format_version": 1, "version": "0.1.0-linux-test",
            "package_version": "0.1.0", "target": "linux-x86_64", "files": rows,
        }, separators=(",", ":")).encode()
        self.files["FILES.sha256"] = "".join(
            f"{hashlib.sha256(data).hexdigest()}  {name}\n"
            for name, data in sorted(self.files.items()) if name != "FILES.sha256").encode()
        self.package = self.root / "mail-hantong-cli-0.1.0-linux-test-linux-x86_64.tar.gz"
        self._write_package(self.files)
        descriptor = {
            "schema_version": 1, "version": "0.1.0-linux-test", "package_version": "0.1.0",
            "target": "linux-x86_64",
            "commit": "a" * 40, "source_filename": self.source.name,
            "source_sha256": canonical_digest,
            "source_archive_sha256": hashlib.sha256(self.source.read_bytes()).hexdigest(),
            "cargo_lock_sha256": cargo_hash,
            "package_filename": self.package.name,
            "package_sha256": hashlib.sha256(self.package.read_bytes()).hexdigest(),
        }
        self.descriptor = self.root / "candidate.json"
        self.descriptor.write_text(json.dumps(descriptor), encoding="utf-8")

    def tearDown(self):
        self.temp.cleanup()

    def _write_package(self, files):
        with self.package.open("wb") as raw:
            import gzip
            with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as archive:
                    for name, data in sorted(files.items()):
                        item = tarfile.TarInfo(name)
                        item.size = len(data)
                        archive.addfile(item, io.BytesIO(data))

    def _write_special_package(self, entries):
        if any(item.size > candidate.MAX_MEMBER for item, _ in entries):
            item, _ = entries[0]
            header = bytearray(item.tobuf(format=tarfile.USTAR_FORMAT))
            header[124:136] = f"{item.size:011o}\0".encode()
            header[148:156] = b"        "
            checksum = sum(header)
            header[148:156] = f"{checksum:06o}\0 ".encode()
            import gzip
            with self.package.open("wb") as raw:
                with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
                    compressed.write(header + b"\0" * 1024)
            return
        self.package.write_bytes(b"")
        import gzip
        with self.package.open("wb") as raw:
            with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as archive:
                    for item, data in entries:
                        archive.addfile(item, io.BytesIO(data))

    def _rebind_package(self):
        value = self._descriptor()
        value["package_sha256"] = hashlib.sha256(self.package.read_bytes()).hexdigest()
        self.descriptor.write_text(json.dumps(value), encoding="utf-8")

    def _descriptor(self):
        return json.loads(self.descriptor.read_text(encoding="utf-8"))

    def test_INST01_valid_report_is_immutable_and_does_not_touch_sentinel(self):
        sentinel = self.root / "prior-user-data"
        sentinel.write_bytes(b"preserve")
        report = candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")
        self.assertTrue(report.verified_only)
        self.assertFalse(report.install_authorized)
        self.assertEqual(report.commit, "a" * 40)
        with self.assertRaises(AttributeError):
            report.version = "changed"
        self.assertEqual(sentinel.read_bytes(), b"preserve")

    def test_INST01_platform_and_missing_artifact_are_rejected(self):
        with self.assertRaisesRegex(candidate.CandidateError, "platform"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Darwin", "x86_64")
        with self.assertRaisesRegex(candidate.CandidateError, "platform"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "aarch64")
        with self.assertRaisesRegex(candidate.CandidateError, "package_missing"):
            candidate.verify_candidate(self.descriptor, self.root / "missing.tar.gz", self.source, "Linux", "x86_64")

    def test_SEC03_descriptor_commit_source_and_package_hashes_are_bound(self):
        original = self._descriptor()
        for key, value, error in (("commit", "b" * 40, "commit"),
                                  ("source_sha256", "0" * 64, "source_digest"),
                                  ("package_sha256", "0" * 64, "package_digest")):
            changed = dict(original, **{key: value})
            self.descriptor.write_text(json.dumps(changed), encoding="utf-8")
            with self.assertRaisesRegex(candidate.CandidateError, error):
                candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")
        self.descriptor.write_text(json.dumps(original), encoding="utf-8")

    def test_SEC03_manifest_and_archive_shape_are_rejected(self):
        changed = dict(self.files)
        manifest = json.loads(changed["release-manifest.json"])
        manifest["target"] = "linux-aarch64"
        changed["release-manifest.json"] = json.dumps(manifest).encode()
        self._write_package(changed)
        descriptor = dict(self._descriptor(), package_sha256=hashlib.sha256(self.package.read_bytes()).hexdigest())
        self.descriptor.write_text(json.dumps(descriptor), encoding="utf-8")
        with self.assertRaisesRegex(candidate.CandidateError, "manifest"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")

    def test_SEC03_duplicate_json_and_unsafe_source_member_are_rejected(self):
        original = self._descriptor()
        self.descriptor.write_text('{"schema_version":1,"schema_version":1}', encoding="utf-8")
        with self.assertRaisesRegex(candidate.CandidateError, "descriptor_duplicate_key"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")
        self.descriptor.write_text(json.dumps(original), encoding="utf-8")
        with zipfile.ZipFile(self.source, "w") as archive:
            archive.writestr("../escape", b"x")
        descriptor = self._descriptor()
        descriptor["source_archive_sha256"] = hashlib.sha256(self.source.read_bytes()).hexdigest()
        descriptor["source_sha256"] = candidate._canonical_source_hash({"../escape": b"x"})
        self.descriptor.write_text(json.dumps(descriptor), encoding="utf-8")
        with self.assertRaisesRegex(candidate.CandidateError, "archive_path"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")

    def test_SEC03_module_has_no_transport_or_execution_capability(self):
        text = (ROOT / "scripts/linux_pinned_candidate.py").read_text(encoding="utf-8")
        for forbidden in ("subprocess", "urllib", "socket", "os.system", "exec("):
            self.assertNotIn(forbidden, text)

    def test_SEC03_truncated_duplicate_link_and_oversized_members_are_rejected(self):
        original = self.package.read_bytes()
        self.package.write_bytes(original[: len(original) // 2])
        self._rebind_package()
        with self.assertRaisesRegex(candidate.CandidateError, "package_archive"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")
        duplicate = tarfile.TarInfo("VERSION")
        duplicate.size = 1
        self._write_special_package([(duplicate, b"x"), (duplicate, b"x")])
        self._rebind_package()
        with self.assertRaisesRegex(candidate.CandidateError, "package_member"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")

        link = tarfile.TarInfo("bin/hantong")
        link.type = tarfile.SYMTYPE
        link.linkname = "VERSION"
        self._write_special_package([(link, b"")])
        self._rebind_package()
        with self.assertRaisesRegex(candidate.CandidateError, "package_member"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")
        huge = tarfile.TarInfo("huge")
        huge.size = candidate.MAX_MEMBER + 1
        self._write_special_package([(huge, b"")])
        self._rebind_package()
        with self.assertRaisesRegex(candidate.CandidateError, "package_member"):
            candidate.verify_candidate(self.descriptor, self.package, self.source, "Linux", "x86_64")

    def test_SEC03_zip_count_spoof_is_rejected_before_zipfile_allocation(self):
        import struct
        data = bytearray(self.source.read_bytes())
        offset = data.rfind(b"PK\x05\x06")
        struct.pack_into("<HH", data, offset + 8, 0, 0)
        with patch.object(candidate.zipfile, "ZipFile", side_effect=AssertionError("parser reached")):
            with self.assertRaisesRegex(candidate.CandidateError, "source_archive"):
                candidate._source_members(bytes(data))

    def test_SEC03_pax_and_gnu_extensions_are_rejected_before_tar_parser(self):
        import gzip
        for kind in (tarfile.XHDTYPE, tarfile.XGLTYPE, tarfile.GNUTYPE_LONGNAME):
            header = tarfile.TarInfo("metadata")
            header.type = kind
            data = gzip.compress(header.tobuf() + bytes(1024))
            with self.subTest(kind=kind), patch.object(candidate.tarfile, "open", side_effect=AssertionError("parser reached")):
                with self.assertRaisesRegex(candidate.CandidateError, "package_member"):
                    candidate._package_members(data)

    def test_SEC03_expansion_and_deep_json_have_bounded_failures(self):
        import gzip
        with patch.object(candidate, "MAX_ARCHIVE", 1024):
            with self.assertRaisesRegex(candidate.CandidateError, "package_size"):
                candidate._package_members(gzip.compress(bytes(2048)))
        with self.assertRaises(candidate.CandidateError):
            candidate._json(b"[" * 2000 + b"]" * 2000, "descriptor")
        value = {"body": "[" * 2000 + '\\" ]'}
        self.assertEqual(candidate._json(json.dumps(value).encode(), "descriptor"), value)

    @unittest.skipUnless(hasattr(os, "O_NOFOLLOW") and hasattr(os, "mkfifo"), "Unix file descriptor guard required")
    def test_SEC03_file_replaced_by_fifo_is_rejected_without_blocking(self):
        path = self.root / "replaceable"
        path.write_bytes(b"synthetic")
        open_file = os.open
        def replace_then_open(name, flags):
            path.unlink()
            os.mkfifo(path)
            self.assertTrue(flags & os.O_NONBLOCK)
            return open_file(name, flags)
        with patch.object(candidate.os, "open", side_effect=replace_then_open):
            with self.assertRaisesRegex(candidate.CandidateError, "fixture_changed"):
                candidate._read(path, 100, "fixture")


if __name__ == "__main__":
    unittest.main()
