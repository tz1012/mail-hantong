"""Package assembly integration with fake compiler outputs and disposable Git roots."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import package_unix
from linux_pinned_candidate import verify_candidate
from package_cli import PackageError


class LinuxPackageBindingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hantong-package-binding-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "source"
        self.root.mkdir()
        for name in ("LICENSE", "packaging/unix/README.md", "docs/latest/CLI-LAUNCH.md",
                     "docs/latest/EXTERNAL-MCP-GUIDE.md", "packaging/unix/install.sh",
                     "packaging/unix/uninstall.sh", "install.sh"):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic fixture\n")
        (self.root / "Cargo.lock").write_bytes(b"version = 4\n")
        self.git("init", "--quiet")
        self.git("config", "core.autocrlf", "false")
        self.git("add", ".")
        self.git("-c", "user.name=Synthetic", "-c", "user.email=synthetic@example.invalid",
                 "-c", "commit.gpgsign=false", "-c", f"core.hooksPath={self.base / 'no-hooks'}",
                 "commit", "--quiet", "-m", "synthetic only")
        self.commit = self.git("rev-parse", "HEAD").strip()
        self.build = self.base / "build"
        binaries = self.build / "x86_64-unknown-linux-gnu/release"
        binaries.mkdir(parents=True)
        elf = bytearray(64)
        elf[:6] = b"\x7fELF\x02\x01"
        elf[18:20] = (62).to_bytes(2, "little")
        for binary in package_unix.BINARIES.values():
            (binaries / binary).write_bytes(elf)

    def git(self, *args):
        env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
        return subprocess.check_output(["git", *args], cwd=self.root, env=env, text=True, stderr=subprocess.PIPE)

    def assemble(self, during_build=None, *, system="Linux", machine="x86_64"):
        version = "0.1.0-beta.30"
        roots = {name: {"version": "0.1.0"} for name in package_unix.BINARIES}
        def fake_command(runner, command, root, env, **kwargs):
            if command[:2] == ["cargo", "metadata"]:
                return SimpleNamespace(stdout=json.dumps({"target_directory": str(self.build)}).encode(), stderr=b"")
            if command[:2] == ["cargo", "build"]:
                if during_build:
                    during_build()
                return SimpleNamespace(stdout=b"", stderr=b"")
            binary = Path(command[0]).name
            label = version if binary in ("hantong", "hantong-t") else "0.1.0"
            return SimpleNamespace(stdout=f"{binary} {label}".encode(), stderr=b"")
        with patch.object(package_unix.platform, "system", return_value=system), \
                patch.object(package_unix.platform, "machine", return_value=machine), \
                patch.object(package_unix.platform, "libc_ver", return_value=("glibc", "2.39")), \
                patch.object(package_unix, "run_command", side_effect=fake_command), \
                patch.object(package_unix, "selected_packages", return_value=([], roots)), \
                patch.object(package_unix, "dependency_licenses", return_value=({}, b"{}\n")), \
                patch.object(package_unix, "compiler_info", return_value={"fixture": True}):
            return package_unix.package(self.root, version, self.base / "output")

    def test_SEC03_linux_arm_is_refused_before_build_or_source_mutation(self):
        with patch.object(package_unix.platform,"system",return_value="Linux"), patch.object(package_unix.platform,"machine",return_value="aarch64"), patch.object(package_unix,"run_command",side_effect=AssertionError("build must not run")):
            with self.assertRaisesRegex(PackageError,"x86_64 only"):
                package_unix.package(self.root,"0.1.0-beta.30",self.base/"output")
        self.assertFalse((self.base/"output").exists())

    def test_INST01_assembled_linux_package_binds_source_commit_and_all_checksums(self):
        output = self.assemble()
        source = output / "mail-hantong-source-0.1.0-beta.30.zip"
        archive = output / "mail-hantong-cli-0.1.0-beta.30-linux-x86_64.tar.gz"
        descriptor = output / "linux-pinned-candidate.json"
        result = verify_candidate(descriptor, archive, source, "Linux", "x86_64")
        self.assertEqual(result.commit, self.commit)
        self.assertFalse(result.install_authorized)
        metadata = json.loads((output / "BUILD-METADATA.json").read_bytes())
        self.assertEqual(metadata["source_commit"], self.commit)
        self.assertEqual(metadata["linux_release_readiness"], "candidate_review_and_native_gates_required")
        for line in (output / "SHA256SUMS").read_text().splitlines():
            digest, name = line.split("  ")
            self.assertEqual(hashlib.sha256((output / name).read_bytes()).hexdigest(), digest)
        self.assertEqual(len(list(output.iterdir())), 5)

    def test_SEC03_dirty_source_is_not_packaged_or_marked_as_a_clean_commit(self):
        (self.root / "Cargo.lock").write_bytes(b"version = 3\n")
        with self.assertRaises((PackageError, ValueError)):
            self.assemble()
        self.assertFalse((self.base / "output/0.1.0-beta.30").exists())

    def test_SEC03_source_change_during_build_never_publishes_a_candidate(self):
        with self.assertRaises((PackageError, ValueError)):
            self.assemble(lambda: (self.root / "Cargo.lock").write_bytes(b"changed\n"))
        self.assertFalse((self.base / "output/0.1.0-beta.30").exists())

    def test_INST01_both_mac_package_paths_do_not_adopt_linux_provenance_protocol(self):
        for machine, cpu, triple in (("x86_64", 0x01000007, "x86_64-apple-darwin"),
                                     ("arm64", 0x0100000C, "aarch64-apple-darwin")):
            with self.subTest(machine=machine):
                directory = self.build / triple / "release"
                directory.mkdir(parents=True)
                macho = b"\xcf\xfa\xed\xfe" + cpu.to_bytes(4, "little") + bytes(24)
                for name in package_unix.BINARIES.values():
                    (directory / name).write_bytes(macho)
                # Each assembly keeps its output separate; no published origin is used.
                self.base = self.base / machine
                self.base.mkdir()
                with patch.object(package_unix, "clean_source_commit", side_effect=AssertionError("Linux pin invoked")):
                    output = self.assemble(system="Darwin", machine=machine)
                self.assertEqual(len(list(output.iterdir())), 3)
                self.assertNotIn("source_commit", json.loads((output / "BUILD-METADATA.json").read_bytes()))
                self.assertFalse((output / "linux-pinned-candidate.json").exists())


if __name__ == "__main__":
    unittest.main()
