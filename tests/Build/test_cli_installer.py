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
POWERSHELL = shutil.which("pwsh") or (shutil.which("powershell.exe") if os.name == "nt" else None)
def ps(script, *args, env=None):
    return subprocess.run([POWERSHELL, "-NoProfile", "-NonInteractive", "-File", str(script), *map(str, args)], env=env, text=True, encoding="utf-8", errors="backslashreplace", capture_output=True, timeout=20)

@contextmanager
def held_file(path, share="None"):
    env = dict(os.environ, HANTONG_TEST_LOCK_PATH=str(path))
    command = "$f=[IO.File]::Open($env:HANTONG_TEST_LOCK_PATH,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::" + share + "); [Console]::WriteLine('ready'); [Console]::Out.Flush(); try { [Console]::ReadLine() | Out-Null } finally { $f.Dispose() }"
    with subprocess.Popen([POWERSHELL, "-NoProfile", "-Command", command], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="backslashreplace") as holder:
        try:
            if holder.stdout.readline().strip() != "ready":
                raise AssertionError("file lock was not acquired: " + holder.stderr.read())
            yield
        finally:
            holder.communicate("release\n", timeout=10)
@unittest.skipUnless(os.name == "nt" and POWERSHELL, "Windows PowerShell is required")
class CliInstallerTests(unittest.TestCase):
    def run_core_gate(self, root, install_root, record, fresh=None):
        functions = INSTALL.read_text(encoding="utf-8").split("# TESTABLE_CORE_STOP_FUNCTIONS_BEGIN", 1)[1].split("# TESTABLE_CORE_STOP_FUNCTIONS_END", 1)[0]
        records = root / "records.json"; records.write_text(json.dumps([record]), encoding="utf-8")
        fresh_path = root / "fresh.json"
        if fresh is not None: fresh_path.write_text(json.dumps([fresh]), encoding="utf-8")
        stop_log = root / "stopped.log"
        harness = f""". '{(INSTALL.parent / 'package-common.ps1').as_posix()}'
$script:InstallBase = '{install_root.as_posix()}'
{functions}
function Get-CurrentUserSid {{ return 'S-1-5-21-test' }}
$script:q = 0
function Get-CoreProcessRecords {{ $script:q++; if ($script:q -gt 1 -and (Test-Path '{fresh_path.as_posix()}')) {{ return @(Get-Content '{fresh_path.as_posix()}' -Raw | ConvertFrom-Json) }}; return @(Get-Content '{records.as_posix()}' -Raw | ConvertFrom-Json) }}
function Stop-CoreProcess([int] $ProcessId) {{ Add-Content -LiteralPath '{stop_log.as_posix()}' -Value ([string]$ProcessId) }}
$state = Read-InstallState '{install_root.as_posix()}'
Stop-OwnedCoreForUpdate '{install_root.as_posix()}' $state.versions
"""
        script = root / "gate-harness.ps1"; script.write_text(harness, encoding="utf-8")
        return ps(script)

    def test_inst01_update_stops_one_owned_old_version_core(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            record = {"Pid": 4101, "Path": str(install_root / "versions/1.0.0/bin/mail-hantong-core.exe"), "CommandLine": "run --lang en", "OwnerSid": "S-1-5-21-test"}
            result = self.run_core_gate(root, install_root, record)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / "stopped.log").read_text().strip(), "4101")

    def test_inst01_update_denies_foreign_user_core(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            record = {"Pid": 4102, "Path": str(install_root / "versions/1.0.0/bin/mail-hantong-core.exe"), "CommandLine": "run", "OwnerSid": "S-1-5-21-foreign"}
            result = self.run_core_gate(root, install_root, record)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((root / "stopped.log").exists())
            self.assertEqual((install_root / "current.txt").read_text(), "1.0.0")

    def test_inst01_update_denies_pid_path_change_before_stop(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            record = {"Pid": 4103, "Path": str(install_root / "versions/1.0.0/bin/mail-hantong-core.exe"), "CommandLine": "run", "OwnerSid": "S-1-5-21-test"}
            fresh = dict(record, Pid=4104)
            result = self.run_core_gate(root, install_root, record, fresh)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((root / "stopped.log").exists())
            self.assertEqual((install_root / "current.txt").read_text(), "1.0.0")

    def test_inst01_update_denies_foreign_core_path(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            record = {"Pid": 4104, "Path": str(root / "foreign/bin/mail-hantong-core.exe"), "CommandLine": "run --lang ko", "OwnerSid": "S-1-5-21-test"}
            result = self.run_core_gate(root, install_root, record)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((install_root / "current.txt").read_text(), "1.0.0")
            self.assertFalse((root / "stopped.log").exists())

    def test_inst01_legacy_version_without_release_manifest_remains_verifiable(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"
            self.assertEqual(ps(INSTALL, "-Source", self.bundle(root / "one", "1.0.0"), "-InstallRoot", install_root).returncode, 0)
            state_path = install_root / "install-manifest.json"
            state = json.loads(state_path.read_text(encoding="utf-8"))
            state["versions"][0]["files"] = [entry for entry in state["versions"][0]["files"] if entry["path"] != "release-manifest.json"]
            state["versions"][0]["files"] = [entry for entry in state["versions"][0]["files"] if entry["path"] != "update.ps1"]
            state_path.write_text(json.dumps(state), encoding="utf-8")
            (install_root / "versions/1.0.0/release-manifest.json").unlink()
            (install_root / "versions/1.0.0/update.ps1").unlink()
            record = {"Pid": 4105, "Path": str(root / "foreign/bin/mail-hantong-core.exe"), "CommandLine": "run", "OwnerSid": "S-1-5-21-test"}
            result = self.run_core_gate(root, install_root, record)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_inst01_production_script_has_no_test_process_overrides(self):
        text = INSTALL.read_text(encoding="utf-8")
        block = text.split("# TESTABLE_CORE_STOP_FUNCTIONS_BEGIN", 1)[1].split("# TESTABLE_CORE_STOP_FUNCTIONS_END", 1)[0]
        self.assertNotIn("HANTONG_TEST", block)
        self.assertNotIn("ConvertFrom-Json", block)

    def test_inst01_core_command_line_normalizes_windows_path_forms_but_keeps_strict_args(self):
        functions = INSTALL.read_text(encoding="utf-8").split("# TESTABLE_CORE_STOP_FUNCTIONS_BEGIN", 1)[1].split("# TESTABLE_CORE_STOP_FUNCTIONS_END", 1)[0]
        harness = f"""$script:InstallBase = 'C:\\'
{functions}
$path = 'C:\\Program Files\\Mail\\mail-hantong-core.exe'
$cases = @(
    @{{ line = '"\\\\?\\C:\\PROGRAM FILES\\Mail\\mail-hantong-core.exe" run '; expected = $true }}
    @{{ line = '"C:\\Program Files\\Mail\\mail-hantong-core.exe" run --lang ko'; expected = $true }}
    @{{ line = '"C:\\\\Program Files\\\\Mail\\\\mail-hantong-core.exe" run'; expected = $true }}
    @{{ line = '"C:\\other.exe" run'; expected = $false }}
    @{{ line = '"C:\\Program Files\\Mail\\mail-hantong-core.exe" terminal-review'; expected = $false }}
)
foreach ($case in $cases) {{ if ((Test-CoreCommandLine $case.line $path) -ne $case.expected) {{ throw ('unexpected command-line result: ' + $case.line) }} }}
"""
        with tempfile.TemporaryDirectory() as td:
            script = Path(td) / "command-line-harness.ps1"
            script.write_text(harness, encoding="utf-8")
            result = ps(script)
            self.assertEqual(result.returncode, 0, result.stderr)
    def test_inst01_non_utf8_child_error_is_preserved_and_returncode_is_not_hidden(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / "non-utf8-error.ps1"
            script.write_text("$s=[Console]::OpenStandardError(); $b=[byte[]](255,254,10); $s.Write($b,0,$b.Length); $s.Flush(); exit 7", encoding="utf-8")
            result = ps(script)
            self.assertEqual(result.returncode, 7)
            self.assertIn(r"\xff\xfe", result.stderr)
            self.assertIsNotNone(result.stdout)

    def test_inst01_update_adds_only_owned_optional_launchers_and_uninstall_removes_them(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            installed = root / "installed"
            first = self.bundle(root / "one", "1.0.0")
            self.assertEqual(ps(INSTALL, "-Source", first, "-InstallRoot", installed).returncode, 0)
            second = self.bundle(root / "two", "2.0.0")
            manifest_path = second / "release-manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            for binary in ("hantong-mcp", "hantong-t"):
                path = second / f"bin/{binary}.exe"
                path.write_bytes(b"synthetic optional executable")
                manifest["files"].append({"path": f"bin/{binary}.exe", "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "size": path.stat().st_size})
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            result = ps(INSTALL, "-Source", second, "-InstallRoot", installed)
            self.assertEqual(result.returncode, 0, result.stderr)
            for binary in ("hantong", "mail-hantong-core", "hantong-mcp", "hantong-t", "update"):
                self.assertTrue((installed / f"bin/{binary}.cmd").is_file())
            update_shim = (installed / "bin/update.cmd").read_text(encoding="utf-8")
            self.assertIn('if /i not "%~1"=="hantong" exit /b 2', update_shim)
            self.assertIn('if not "%~2"=="" exit /b 2', update_shim)
            self.assertIn('bin\\hantong.exe" update', update_shim)
            self.assertEqual(
                subprocess.run(["cmd.exe", "/d", "/c", str(installed / "bin/update.cmd"), "wrong", "hantong"],
                       capture_output=True).returncode,
                2,
            )
            accepted = subprocess.run(
                ["cmd.exe", "/d", "/c", str(installed / "bin/update.cmd"), "hantong"],
                capture_output=True,
            )
            self.assertNotEqual(accepted.returncode, 2)
            result = ps(UNINSTALL, "-InstallRoot", installed)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse((installed / "bin").exists())

    def bundle(self, root, version="0.1.0"):
        src = root / "bundle"; (src / "bin").mkdir(parents=True)
        for name, content in {"bin/hantong.exe": b"cli", "bin/mail-hantong-core.exe": b"core", "LICENSE": b"license", "update.ps1": b"synthetic updater"}.items(): (src / name).write_bytes(content)
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
    def test_inst01_nested_scriptblock_install_update_passes_explicit_state_root(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            source = self.bundle(root / "source", "1.0.0")
            shutil.copy2(INSTALL, source / "install.ps1")
            shutil.copy2(INSTALL.parent / "package-common.ps1", source / "package-common.ps1")
            harness = root / "nested.ps1"
            harness.write_text(f"""
$ErrorActionPreference = 'Stop'
$source = '{source.as_posix()}'
$install = '{(root / 'install').as_posix()}'
$installerText = [IO.File]::ReadAllText((Join-Path $source 'install.ps1'))
function Invoke-NestedInstaller {{
    & ([ScriptBlock]::Create($installerText)) -PackageRoot $source -Source $source -InstallRoot $install
}}
Invoke-NestedInstaller
Invoke-NestedInstaller
if (-not (Test-Path -LiteralPath (Join-Path $install 'install-manifest.json'))) {{ throw 'nested update did not preserve state' }}
$commonText = [IO.File]::ReadAllText((Join-Path $source 'package-common.ps1'))
function Invoke-NestedStateValidation {{
    $script:InstallBase = $null
    & ([ScriptBlock]::Create($commonText + "`nRead-InstallState '$install' | Out-Null"))
}}
Invoke-NestedStateValidation
""", encoding="utf-8")
            result = ps(harness)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / "install/current.txt").read_text(), "1.0.0")
    def test_install_is_idempotent_and_uninstall_preserves_data(self):
        with tempfile.TemporaryDirectory() as td:
            root, install_root = Path(td), Path(td) / "install"; src = self.bundle(root)
            for _ in range(2):
                result = ps(INSTALL, "-Source", src, "-InstallRoot", install_root); self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((install_root / "bin" / "hantong.cmd").exists())
            self.assertTrue((install_root / "bin" / "update.cmd").exists())
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
            made = subprocess.run([POWERSHELL, "-NoProfile", "-Command", "New-Item -ItemType Junction -Path $env:HANTONG_TEST_LINK -Target $env:HANTONG_TEST_DEST | Out-Null"], env=env, capture_output=True, timeout=20)
            self.assertEqual(made.returncode, 0, made.stderr)
            try:
                self.assertNotEqual(ps(UNINSTALL, "-InstallRoot", installed).returncode, 0)
                self.assertNotEqual(ps(INSTALL, "-Source", source, "-InstallRoot", installed).returncode, 0)
                self.assertEqual((external / "hantong.exe").read_bytes(), b"cli")
            finally:
                # Remove the test junction itself, never its target contents.
                original.rmdir()
if __name__ == "__main__": unittest.main()
