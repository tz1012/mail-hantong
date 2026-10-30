import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from contextlib import contextmanager
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
INSTALL = ROOT / "packaging" / "windows" / "install.ps1"
UNINSTALL = ROOT / "packaging" / "windows" / "uninstall.ps1"
def ps(script, *args):
    return subprocess.run(["pwsh", "-NoProfile", "-NonInteractive", "-File", str(script), *map(str, args)], text=True, encoding="utf-8", capture_output=True, timeout=20)

@contextmanager
def held_file(path, share="None"):
    env = dict(os.environ, HANTONG_TEST_LOCK_PATH=str(path))
    command = "$f=[IO.File]::Open($env:HANTONG_TEST_LOCK_PATH,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::" + share + "); [Console]::WriteLine('ready'); [Console]::Out.Flush(); try { [Console]::ReadLine() | Out-Null } finally { $f.Dispose() }"
    with subprocess.Popen(["pwsh", "-NoProfile", "-Command", command], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8") as holder:
        try:
            if holder.stdout.readline().strip() != "ready":
                raise AssertionError("file lock was not acquired: " + holder.stderr.read())
            yield
        finally:
            holder.communicate("release\n", timeout=10)
@unittest.skipUnless(os.name == "nt" and shutil.which("pwsh"), "Windows PowerShell is required")
class CliInstallerTests(unittest.TestCase):
    def bundle(self, root, version="0.1.0"):
        src = root / "bundle"; (src / "bin").mkdir(parents=True)
        for name, content in {"bin/hantong.exe": b"cli", "bin/mail-hantong-core.exe": b"core", "LICENSE": b"license"}.items(): (src / name).write_bytes(content)
        files = [{"path": str(p.relative_to(src)).replace("\\", "/"), "sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "size": p.stat().st_size} for p in src.rglob("*") if p.is_file()]
        (src / "release-manifest.json").write_text(json.dumps({"format_version": 1, "version": version, "package_version": version, "target": "windows-x86_64", "files": files}), encoding="utf-8")
        return src
    def test_inst01_default_source_works_with_windows_powershell_file_invocation(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            src = self.bundle(root)
            shutil.copy2(INSTALL, src / "install.ps1")
            shutil.copy2(INSTALL.parent / "package-common.ps1", src / "package-common.ps1")
            # PowerShell 7's inherited module path is not the Windows 5.1 path.
            env = {key: value for key, value in os.environ.items() if key.lower() != "psmodulepath"}
            powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
            result = subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-File",
                                     str(src / "install.ps1"), "-InstallRoot", str(root / "install")],
                                    env=env, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
            self.assertEqual((root / "install/current.txt").read_text(), "0.1.0")
            self.assertEqual((root / "install/versions/0.1.0/bin/hantong.exe").read_bytes(), b"cli")
    def test_install_is_idempotent_and_uninstall_preserves_data(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"; src = self.bundle(root)
            for _ in range(2):
                result = ps(INSTALL, "-Source", src, "-InstallRoot", install_root); self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((install_root / "bin" / "hantong.cmd").exists())
            (install_root / "data").mkdir(); (install_root / "data" / "mail.db").write_text("keep", encoding="utf-8")
            result = ps(UNINSTALL, "-InstallRoot", install_root); self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((install_root / "data" / "mail.db").read_text(encoding="utf-8"), "keep")
    def test_corrupt_source_is_rejected_without_replacing_existing_version(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root, "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            bad = self.bundle(root / "bad", "2.0.0"); (bad / "bin" / "hantong.exe").write_bytes(b"tampered")
            result = ps(INSTALL, "-Source", bad, "-InstallRoot", install_root); self.assertNotEqual(result.returncode, 0)
            self.assertEqual((install_root / "current.txt").read_text(encoding="ascii"), "1.0.0")

    def test_update_keeps_old_version_and_uninstall_does_not_delete_unknown_files(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "two", "2.0.0"), "-InstallRoot", install_root).returncode, 0)
            self.assertEqual((install_root / "current.txt").read_text(encoding="ascii"), "2.0.0")
            sentinel = install_root / "versions" / "1.0.0" / "user-data.keep"
            sentinel.write_text("preserve", encoding="utf-8")
            result = ps(UNINSTALL, "-InstallRoot", install_root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")

    def test_unmanaged_shim_blocks_update_before_current_pointer_changes(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            shim = install_root / "bin" / "hantong.cmd"
            shim.write_text("unmanaged", encoding="ascii")
            result = ps(INSTALL, "-Source", self.bundle(root / "two", "2.0.0"), "-InstallRoot", install_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((install_root / "current.txt").read_text(encoding="ascii"), "1.0.0")

    def test_tampered_state_traversal_is_rejected_and_sentinel_survives(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            sentinel = install_root / "data" / "mail.db"
            sentinel.parent.mkdir()
            sentinel.write_text("keep", encoding="utf-8")
            state_path = install_root / "install-manifest.json"
            state = json.loads(state_path.read_text(encoding="utf-8"))
            state["versions"][0]["files"][0]["path"] = "../../data/mail.db"
            state["versions"][0]["files"][0]["sha256"] = hashlib.sha256(sentinel.read_bytes()).hexdigest()
            state_path.write_text(json.dumps(state), encoding="utf-8")
            result = ps(UNINSTALL, "-InstallRoot", install_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")

    def test_duplicate_case_manifest_path_is_rejected(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            src = self.bundle(root / "one", "1.0.0")
            manifest = json.loads((src / "release-manifest.json").read_text(encoding="utf-8"))
            manifest["files"].append(dict(manifest["files"][0], path="BIN/HANTONG.EXE"))
            (src / "release-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            result = ps(INSTALL, "-Source", src, "-InstallRoot", install_root)
            self.assertNotEqual(result.returncode, 0)

    def test_locked_transaction_is_rejected_without_changing_current(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            with held_file(install_root / ".install.lock"):
                result = ps(INSTALL, "-Source", self.bundle(root / "two", "2.0.0"), "-InstallRoot", install_root)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((install_root / "current.txt").read_text(encoding="ascii"), "1.0.0")

    def test_failed_activation_keeps_previous_version_and_can_resume(self):
        with tempfile.TemporaryDirectory() as td:
            root, installed = Path(td), Path(td) / "install"
            first = ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", installed)
            self.assertEqual(first.returncode, 0, first.stderr)
            update = self.bundle(root / "two", "2.0.0")
            with held_file(installed / "current.txt", "Read"):
                result = ps(INSTALL, "-Source", update, "-InstallRoot", installed)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((installed / "current.txt").read_text(), "1.0.0")
            result = ps(INSTALL, "-Source", update, "-InstallRoot", installed)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((installed / "current.txt").read_text(), "2.0.0")

    def test_junction_ancestor_is_rejected_without_touching_external_files(self):
        with tempfile.TemporaryDirectory() as td:
            root, installed = Path(td), Path(td) / "install"
            source = self.bundle(root / "one", "1.0.0")
            first = ps(INSTALL, "-Source", source, "-InstallRoot", installed)
            self.assertEqual(first.returncode, 0, first.stderr)
            original = installed / "versions" / "1.0.0" / "bin"
            external = root / "external"
            original.rename(external)
            env = dict(os.environ, HANTONG_TEST_LINK=str(original), HANTONG_TEST_DEST=str(external))
            made = subprocess.run(["pwsh", "-NoProfile", "-Command", "New-Item -ItemType Junction -Path $env:HANTONG_TEST_LINK -Target $env:HANTONG_TEST_DEST | Out-Null"], env=env, capture_output=True, timeout=20)
            self.assertEqual(made.returncode, 0, made.stderr)
            try:
                self.assertNotEqual(ps(UNINSTALL, "-InstallRoot", installed).returncode, 0)
                self.assertNotEqual(ps(INSTALL, "-Source", source, "-InstallRoot", installed).returncode, 0)
                self.assertEqual((external / "hantong.exe").read_bytes(), b"cli")
            finally:
                # Remove the test junction itself, never its target contents.
                original.rmdir()
if __name__ == "__main__": unittest.main()
