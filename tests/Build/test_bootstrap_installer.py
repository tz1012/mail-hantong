"""Exercise the one-command installer with local release assets only."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "nt", "Windows installer")
class BootstrapInstallerTests(unittest.TestCase):
    def test_inst01_user_path_plan_preserves_unrelated_entries_without_registry_mutation(self):
        with tempfile.TemporaryDirectory(prefix="hantong-path-plan-") as directory:
            script = Path(directory) / "plan.ps1"
            script.write_text("""
$ErrorActionPreference='Stop'
. $env:HANTONG_PATH_HELPER
$before=[Environment]::GetEnvironmentVariable('Path',[EnvironmentVariableTarget]::User)
if ((Get-UserPathAddition '' 'C:\\Mail\\bin') -cne 'C:\\Mail\\bin') { throw 'Empty PATH addition failed' }
if ($null -ne (Get-UserPathAddition 'C:\\Other;C:\\MAIL\\BIN\\' 'C:\\Mail\\bin')) { throw 'Duplicate entry was added' }
if ((Get-UserPathAddition 'C:\\Other' 'C:\\Mail\\bin') -cne 'C:\\Other;C:\\Mail\\bin') { throw 'Other entry changed' }
if ((Get-UserPathRemoval 'C:\\Other;C:\\MAIL\\BIN;C:\\More' 'C:\\Mail\\bin') -cne 'C:\\Other;C:\\More') { throw 'Removal changed unrelated entries' }
if ([Environment]::GetEnvironmentVariable('Path',[EnvironmentVariableTarget]::User) -cne $before) { throw 'Test mutated user PATH' }
""", encoding="utf-8")
            env = {k: v for k, v in os.environ.items() if k.lower() != "psmodulepath"}
            env["HANTONG_PATH_HELPER"] = str(ROOT / "packaging/windows/package-common.ps1")
            powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
            result = subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-File", str(script)],
                                    env=env, capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))

    def fixture(self, root, *, corrupt=False, escape=False):
        source = root / "source"
        (source / "bin").mkdir(parents=True)
        for binary in ("hantong", "mail-hantong-core"):
            (source / f"bin/{binary}.exe").write_bytes(b"synthetic executable")
        (source / "LICENSE").write_bytes(b"synthetic license")
        for path in (ROOT / "packaging/windows").glob("*.ps1"):
            shutil.copyfile(path, source / path.name)
        files = [{"path": path.relative_to(source).as_posix(),
                  "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "size": path.stat().st_size}
                 for path in sorted(source.rglob("*")) if path.is_file()]
        (source / "release-manifest.json").write_text(json.dumps({"format_version": 1,
            "version": "0.1.0-preview.99", "package_version": "0.1.0", "target": "windows-x86_64", "files": files}), encoding="utf-8")
        name = "mail-hantong-cli-0.1.0-preview.99-windows-x86_64.zip"
        archive = root / name
        with zipfile.ZipFile(archive, "w") as output:
            for path in sorted(source.rglob("*")):
                if path.is_file():
                    output.write(path, path.relative_to(source).as_posix())
            if escape:
                output.writestr("../escaped.keep", b"must not appear")
        digest = "0" * 64 if corrupt else hashlib.sha256(archive.read_bytes()).hexdigest()
        sums = root / "SHA256SUMS"
        sums.write_text(f"{digest}  {name}\n", encoding="ascii")
        prefix = "https://github.com/tz1012/mail-hantong/releases/download/v0.1.0-preview.99/"
        assets = [{"name": path.name, "size": path.stat().st_size, "browser_download_url": prefix + path.name}
                  for path in (archive, sums)]
        (root / "release.json").write_text(json.dumps({"tag_name": "v0.1.0-preview.99", "draft": False,
                                                      "prerelease": True, "assets": assets}), encoding="utf-8")

    def install(self, root, *, restricted=False):
        script = root / "run.ps1"
        script.write_text("""
$ErrorActionPreference='Stop'
. ([ScriptBlock]::Create([IO.File]::ReadAllText($env:HANTONG_BOOTSTRAP_SCRIPT)))
$script:FixtureRelease=Get-Content -LiteralPath (Join-Path $env:HANTONG_FIXTURE_ROOT 'release.json') -Raw | ConvertFrom-Json
function Invoke-RestMethod { param($Uri,$Headers) return $script:FixtureRelease }
function Invoke-WebRequest {
    param($Uri,[switch]$UseBasicParsing,$Headers,$OutFile)
    Copy-Item -LiteralPath (Join-Path $env:HANTONG_FIXTURE_ROOT ([IO.Path]::GetFileName(([Uri]$Uri).AbsolutePath))) -Destination $OutFile
}
Install-MailHantong -Destination $env:HANTONG_FIXTURE_INSTALL -SessionOnly
if (-not (Test-Path -LiteralPath (Join-Path $env:HANTONG_FIXTURE_INSTALL 'bin/hantong.cmd'))) { throw 'Missing launcher' }
""", encoding="utf-8")
        env = {k: v for k, v in os.environ.items() if k.lower() != "psmodulepath"}
        env.update(HANTONG_BOOTSTRAP_SCRIPT=str(ROOT / "install.ps1"), HANTONG_FIXTURE_ROOT=str(root),
                   HANTONG_FIXTURE_INSTALL=str(root / "installed CLI"))
        powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
        invocation = (["-ExecutionPolicy", "Restricted", "-Command",
                       "& ([ScriptBlock]::Create([IO.File]::ReadAllText($env:HANTONG_RUN_SCRIPT)))"]
                      if restricted else ["-File", str(script)])
        env["HANTONG_RUN_SCRIPT"] = str(script)
        return subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", *invocation],
                              env=env, capture_output=True, timeout=45)

    def test_inst01_installs_verified_preview_without_real_network_or_user_profile(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory)
            self.fixture(root)
            result = self.install(root)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
            self.assertEqual((root / "installed CLI/current.txt").read_text(), "0.1.0-preview.99")

    def test_sec07_wrong_checksum_stops_before_installation(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory)
            self.fixture(root, corrupt=True)
            self.assertNotEqual(self.install(root).returncode, 0)
            self.assertFalse((root / "installed CLI").exists())

    def test_inst01_one_line_runs_with_restricted_file_policy_without_changing_it(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory)
            self.fixture(root)
            result = self.install(root, restricted=True)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
            self.assertEqual((root / "installed CLI/current.txt").read_text(), "0.1.0-preview.99")

    def test_sec07_archive_escape_stops_before_installation(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory)
            self.fixture(root, escape=True)
            self.assertNotEqual(self.install(root).returncode, 0)
            self.assertFalse((root / "installed CLI").exists())
            self.assertFalse((root / "escaped.keep").exists())
