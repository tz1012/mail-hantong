from __future__ import annotations

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("macos_interactive_artifacts", ROOT / "scripts/macos_interactive_artifacts.py")
assert SPEC and SPEC.loader
artifacts = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(artifacts)


def _macho(machine: str = "x86_64") -> bytes:
    cpu = 0x0100000C if machine == "arm64" else 0x01000007
    return b"\xcf\xfa\xed\xfe" + cpu.to_bytes(4, "little") + b"\0" * 64


class InteractiveArtifactsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hantong-artifacts-")
        self.root = Path(self.temp.name).resolve()
        self.source = self.root / "source"
        (self.source / "crates/hantong-secrets/src").mkdir(parents=True)
        (self.source / "scripts/fixtures").mkdir(parents=True)
        (self.source / "crates/hantong-secrets/src/lib.rs").write_bytes(b"pub fn test_marker() {}\n")
        (self.source / "scripts/fixtures/macos_keychain_compat_module.rs").write_bytes(b"fixture\n")
        self.target = self.root / "target"
        self.target.mkdir()
        for path in (self.source, self.target, self.source / "crates",
                     self.source / "crates/hantong-secrets",
                     self.source / "crates/hantong-secrets/src", self.source / "scripts",
                     self.source / "scripts/fixtures"):
            path.chmod(0o700)

    def tearDown(self):
        self.temp.cleanup()

    def test_SEC03_build_restores_fixture_after_compile(self):
        executable = self.target / "debug/deps" / "hantong_secrets-abc"
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"synthetic")
        payload = json.dumps({"reason": "compiler-artifact", "profile": {"test": True},
                              "target": {"name": "hantong_secrets", "kind": ["lib"]},
                              "executable": str(executable)})
        runner = mock.Mock(return_value=subprocess.CompletedProcess([], 0, payload + "\n", ""))
        result = artifacts.build_fixture(self.source, self.target, "seed", runner, environment={"PATH": "/usr/bin"})
        self.assertEqual(result, executable)
        self.assertNotIn("CI", runner.call_args.kwargs["env"])
        self.assertEqual(runner.call_args.kwargs["env"]["CARGO_TARGET_DIR"], str(self.target))
        self.assertEqual((self.source / "crates/hantong-secrets/src/lib.rs").read_bytes(), b"pub fn test_marker() {}\n")
        self.assertFalse((self.source / "crates/hantong-secrets/src/macos_keychain_compat.rs").exists())

    def test_SEC03_compile_cancel_restores_source(self):
        runner = mock.Mock(side_effect=KeyboardInterrupt)
        with self.assertRaisesRegex(artifacts.ArtifactError, "cancelled"):
            artifacts.build_fixture(self.source, self.target, "cancel", runner, environment={"PATH": "/usr/bin"})
        self.assertEqual((self.source / "crates/hantong-secrets/src/lib.rs").read_bytes(), b"pub fn test_marker() {}\n")
        self.assertFalse((self.source / "crates/hantong-secrets/src/macos_keychain_compat.rs").exists())

    def test_SEC03_compile_timeout_restores_source(self):
        runner = mock.Mock(side_effect=subprocess.TimeoutExpired(["cargo"], 1))
        with self.assertRaisesRegex(artifacts.ArtifactError, "compile_timeout"):
            artifacts.build_fixture(self.source, self.target, "timeout", runner, environment={"PATH": "/usr/bin"})
        self.assertFalse((self.source / "crates/hantong-secrets/src/macos_keychain_compat.rs").exists())

    def test_SEC03_unreaped_runner_preserves_injection_for_recovery(self):
        runner = mock.Mock(side_effect=artifacts.UnreapedProcessError("children still alive"))
        with self.assertRaises(artifacts.UnreapedProcessError):
            artifacts.build_fixture(self.source, self.target, "unreaped", runner,
                                    environment={"PATH": "/usr/bin"})
        self.assertTrue((self.source / "crates/hantong-secrets/src/macos_keychain_compat.rs").is_file())
        self.assertIn(b"mod macos_keychain_compat;", (self.source / "crates/hantong-secrets/src/lib.rs").read_bytes())

    def test_SEC03_unexpected_source_edit_refuses_restore(self):
        original = b"pub fn test_marker() {}\n"
        def edit_then_fail(*_args, **_kwargs):
            lib = self.source / "crates/hantong-secrets/src/lib.rs"
            lib.write_bytes(b"unexpected developer edit\n")
            return subprocess.CompletedProcess([], 1, "", "")
        with self.assertRaisesRegex(artifacts.ArtifactError, "source_changed_during_fixture"):
            artifacts.build_fixture(self.source, self.target, "edit", edit_then_fail, environment={"PATH": "/usr/bin"})
        self.assertNotEqual((self.source / "crates/hantong-secrets/src/lib.rs").read_bytes(), original)

    def test_SEC03_partial_fixture_injection_cleans_known_module(self):
        with mock.patch.object(artifacts, "_atomic_bytes", side_effect=artifacts.ArtifactError("fixture_restore")):
            with self.assertRaisesRegex(artifacts.ArtifactError, "fixture_restore"):
                artifacts.build_fixture(self.source, self.target, "partial",
                                        mock.Mock(), environment={"PATH": "/usr/bin"})
        self.assertFalse((self.source / "crates/hantong-secrets/src/macos_keychain_compat.rs").exists())
        self.assertEqual((self.source / "crates/hantong-secrets/src/lib.rs").read_bytes(), b"pub fn test_marker() {}\n")

    @unittest.skipUnless(hasattr(Path, "symlink_to"), "symlink API unavailable")
    def test_SEC03_replaced_source_leaf_is_not_followed_or_overwritten(self):
        external = self.root / "outside-lib.rs"
        external.write_bytes(b"outside bytes")

        def replace_then_fail(*_args, **_kwargs):
            lib = self.source / "crates/hantong-secrets/src/lib.rs"
            try:
                lib.unlink()
                lib.symlink_to(external)
            except (OSError, NotImplementedError):
                self.skipTest("symlink creation unavailable")
            return subprocess.CompletedProcess([], 1, "", "")

        with self.assertRaises(artifacts.ArtifactError):
            artifacts.build_fixture(self.source, self.target, "replace", replace_then_fail,
                                    environment={"PATH": "/usr/bin"})
        self.assertEqual(external.read_bytes(), b"outside bytes")

    def test_SEC03_package_verification_rejects_wrong_architecture_and_hash(self):
        root = self.root / "package"
        root.mkdir()
        root.chmod(0o700)
        core = _macho("arm64")
        cli = _macho("arm64")
        entries = {"bin/hantong": cli, "bin/mail-hantong-core": core}
        manifest = {"format_version": 1, "version": "0.1.0-test", "package_version": "0.1.0-test",
                    "target": "macos-x86_64", "files": [
            {"path": name, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
            for name, data in entries.items()]}
        (root / "release-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        for name, data in entries.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        with self.assertRaises(artifacts.PackageError):
            artifacts.verify_extracted_package(root, "macos-x86_64")
        manifest["target"] = "macos-aarch64"
        (root / "release-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        (root / "bin/hantong").write_bytes(b"changed")
        with self.assertRaisesRegex(artifacts.ArtifactError, "artifact_changed"):
            artifacts.verify_extracted_package(root, "macos-aarch64")

    def test_SEC03_archive_rejects_duplicate_and_link_members(self):
        archive = self.root / "bad.tar.gz"
        with tarfile.open(archive, "w:gz") as bundle:
            item = tarfile.TarInfo("bin/hantong")
            item.size = 1
            bundle.addfile(item, io.BytesIO(b"x"))
            duplicate = tarfile.TarInfo("bin/hantong")
            duplicate.size = 1
            bundle.addfile(duplicate, io.BytesIO(b"x"))
        destination = self.root / "output"
        self.root.chmod(0o700)
        with self.assertRaisesRegex(artifacts.ArtifactError, "archive_member"):
            artifacts.extract_verified_archive(archive, destination, hashlib.sha256(archive.read_bytes()).hexdigest())
        self.assertFalse(destination.exists())

    def test_SEC03_archive_rejects_symlink_member(self):
        archive = self.root / "link.tar.gz"
        with tarfile.open(archive, "w:gz") as bundle:
            item = tarfile.TarInfo("bin/hantong")
            item.type = tarfile.SYMTYPE
            item.linkname = "outside"
            bundle.addfile(item)
        destination = self.root / "output-link"
        with self.assertRaisesRegex(artifacts.ArtifactError, "archive_member"):
            artifacts.extract_verified_archive(archive, destination, hashlib.sha256(archive.read_bytes()).hexdigest())
        self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
