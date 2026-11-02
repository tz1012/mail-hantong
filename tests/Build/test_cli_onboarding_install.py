"""Exercise first-run package ownership with native Windows PowerShell."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "packaging/windows"
POWERSHELL = Path(os.environ.get("SystemRoot", "C:/Windows")) / "System32/WindowsPowerShell/v1.0/powershell.exe"


@unittest.skipUnless(os.name == "nt" and POWERSHELL.is_file(), "Windows PowerShell required")
class OnboardingInstallTests(unittest.TestCase):
    def test_inst03_latest_bootstrap_selects_release_from_powershell51_rest_array(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bootstrap = str(ROOT / "install.ps1").replace("'", "''")
            script = root / "probe.ps1"
            script.write_text("""$ErrorActionPreference = 'Stop'
. '%s'
function Invoke-RestMethod {
    $assets = @(
        [pscustomobject]@{name='mail-hantong-cli-0.1.0-beta.4-windows-x86_64.zip';size=100;browser_download_url='https://github.com/tz1012/mail-hantong/releases/download/v0.1.0-beta.4/mail-hantong-cli-0.1.0-beta.4-windows-x86_64.zip'},
        [pscustomobject]@{name='SHA256SUMS';size=100;browser_download_url='https://github.com/tz1012/mail-hantong/releases/download/v0.1.0-beta.4/SHA256SUMS'}
    )
    $draft = [pscustomobject]@{draft=$true;tag_name='v0.1.0-beta.5';assets=$assets}
    $release = [pscustomobject]@{draft=$false;tag_name='v0.1.0-beta.4';assets=$assets}
    # Match native Invoke-RestMethod's non-enumerated array output on PS 5.1.
    return ,@($draft,$release)
}
function Invoke-WebRequest {
    param($Uri,[switch]$UseBasicParsing,$Headers,$OutFile)
    if ($Uri -cne 'https://github.com/tz1012/mail-hantong/releases/download/v0.1.0-beta.4/mail-hantong-cli-0.1.0-beta.4-windows-x86_64.zip') { throw 'Wrong release asset' }
    $script:SelectedExpectedAsset = $true
    throw 'Download intentionally stopped before network access'
}
try { Install-MailHantong -SessionOnly } catch {
    if (-not $script:SelectedExpectedAsset) { throw }
}
if (-not $script:SelectedExpectedAsset) { throw 'No release selected' }
""" % bootstrap, encoding="utf-8")
            result = subprocess.run([str(POWERSHELL), "-NoProfile", "-NonInteractive",
                "-File", str(script)], capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))

    def test_inst03_cli04_installed_manifest_survives_repeat_and_is_owned_on_uninstall(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, installed = root / "source", root / "installed"
            (source / "bin").mkdir(parents=True)
            (source / "bin/hantong.exe").write_bytes(b"synthetic cli")
            (source / "bin/mail-hantong-core.exe").write_bytes(b"synthetic core")
            (source / "LICENSE").write_bytes(b"synthetic license")
            for name in ("install.ps1", "uninstall.ps1", "package-common.ps1"):
                shutil.copy2(SCRIPTS / name, source / name)
            records = [{"path": p.relative_to(source).as_posix(),
                        "size": p.stat().st_size,
                        "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
                       for p in source.rglob("*") if p.is_file()]
            manifest = source / "release-manifest.json"
            manifest.write_text(json.dumps({"format_version": 1, "version": "0.1.0",
                "package_version": "0.1.0-test", "target": "windows-x86_64", "files": records}), encoding="utf-8")
            env = {k: v for k, v in os.environ.items() if k.lower() != "psmodulepath"}

            def run(script, *args):
                result = subprocess.run([str(POWERSHELL), "-NoProfile", "-NonInteractive", "-File",
                    str(script), *map(str, args)], env=env, capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))

            for _ in range(2):
                run(source / "install.ps1", "-Source", source, "-InstallRoot", installed)
                copied = installed / "versions/0.1.0/release-manifest.json"
                self.assertEqual(copied.read_bytes(), manifest.read_bytes())
                state = json.loads((installed / "install-manifest.json").read_text(encoding="utf-8-sig"))
                owned = state["versions"][0]["files"]
                manifests = [entry for entry in owned if entry["path"] == "release-manifest.json"]
                self.assertEqual(len(manifests), 1)
                self.assertEqual(manifests[0]["sha256"], hashlib.sha256(manifest.read_bytes()).hexdigest())
            # Older installed previews did not own/copy the release manifest.
            # Updating to a new version must retain that version and its files.
            state["versions"][0]["files"] = [entry for entry in owned if entry["path"] != "release-manifest.json"]
            (installed / "install-manifest.json").write_text(json.dumps(state), encoding="utf-8")
            copied.unlink()
            release = json.loads(manifest.read_text(encoding="utf-8"))
            release["version"] = "0.1.1"
            manifest.write_text(json.dumps(release), encoding="utf-8")
            run(source / "install.ps1", "-Source", source, "-InstallRoot", installed)
            self.assertEqual((installed / "versions/0.1.0/bin/hantong.exe").read_bytes(), b"synthetic cli")
            self.assertEqual((installed / "versions/0.1.1/release-manifest.json").read_bytes(), manifest.read_bytes())
            self.assertEqual((installed / "current.txt").read_text(encoding="ascii"), "0.1.1")
            sentinel = installed / "data/keep.txt"
            sentinel.parent.mkdir()
            sentinel.write_text("preserve", encoding="utf-8")
            run(installed / "versions/0.1.1/uninstall.ps1", "-InstallRoot", installed)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")
            self.assertFalse((installed / "versions/0.1.0/release-manifest.json").exists())
            self.assertFalse((installed / "versions/0.1.1/release-manifest.json").exists())


if __name__ == "__main__":
    unittest.main()
