"""Regression tests for installer progress reporting."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from tests.Build import test_bootstrap_installer as bootstrap

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "nt", "Windows installer")
class InstallerProgressTests(unittest.TestCase):
    def run_progress(self, root, *, corrupt=False, release_version="latest"):
        fixture = bootstrap.BootstrapInstallerTests()
        fixture.setUp()
        try:
            fixture.fixture(root, corrupt=corrupt)
        finally:
            fixture.tearDown()
        script = root / "progress.ps1"
        script.write_text(
            """
$ErrorActionPreference='Stop'
$script:Events=@()
function Write-Progress { param([int]$Id,[int]$ParentId,[string]$Activity,[string]$Status,[int]$PercentComplete,[switch]$Completed)
  $script:Events += [pscustomobject]@{ percent=$PercentComplete; completed=[bool]$Completed; id=$Id; parent=$ParentId }
}
. ([ScriptBlock]::Create([IO.File]::ReadAllText($env:HANTONG_BOOTSTRAP_SCRIPT)))
$script:FixtureRelease=Get-Content -LiteralPath (Join-Path $env:HANTONG_FIXTURE_ROOT 'release.json') -Raw | ConvertFrom-Json
function Invoke-RestMethod { param($Uri,$Headers) return $script:FixtureRelease }
function Invoke-WebRequest { param($Uri,[switch]$UseBasicParsing,$Headers,$OutFile)
  Copy-Item -LiteralPath (Join-Path $env:HANTONG_FIXTURE_ROOT ([IO.Path]::GetFileName(([Uri]$Uri).AbsolutePath))) -Destination $OutFile
}
$failed=$false
try { Install-MailHantong -ReleaseVersion $env:HANTONG_RELEASE -Destination (Join-Path $env:HANTONG_FIXTURE_ROOT 'installed') -SessionOnly } catch { $failed=$true }
Write-Output (ConvertTo-Json @($script:Events) -Compress)
Write-Output (ConvertTo-Json @{ failed=$failed } -Compress)
""",
            encoding="utf-8",
        )
        env = {k: v for k, v in os.environ.items() if k.lower() != "psmodulepath"}
        env.update(
            HANTONG_BOOTSTRAP_SCRIPT=str(ROOT / "install.ps1"),
            HANTONG_FIXTURE_ROOT=str(root),
            HANTONG_RELEASE=release_version,
        )
        powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
        return subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-File", str(script)],
                              env=env, capture_output=True, timeout=45)

    @staticmethod
    def events(result):
        lines = result.stdout.decode("utf-8", "replace").splitlines()
        return json.loads(lines[-2]), json.loads(lines[-1])

    def test_progress_success_is_monotonic_bounded_and_completes_once(self):
        with tempfile.TemporaryDirectory(prefix="hantong-progress-") as directory:
            result = self.run_progress(Path(directory))
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
            events, state = self.events(result)
            self.assertFalse(state["failed"])
            for progress_id in {event["id"] for event in events}:
                active = [event["percent"] for event in events if not event["completed"] and event["id"] == progress_id]
                self.assertTrue(active)
                self.assertEqual(active, sorted(active))
                self.assertTrue(all(0 <= value <= 99 for value in active))
                completed = [event for event in events if event["completed"] and event["id"] == progress_id]
                self.assertEqual(len(completed), 1)
                self.assertEqual(completed[0]["percent"], 100)
            self.assertEqual(result.stderr, b"")

    def test_progress_failures_clear_without_completion(self):
        for kwargs in ({"release_version": "bad"}, {"corrupt": True}):
            with self.subTest(kwargs=kwargs), tempfile.TemporaryDirectory(prefix="hantong-progress-") as directory:
                result = self.run_progress(Path(directory), **kwargs)
                events, state = self.events(result)
                self.assertTrue(state["failed"])
                self.assertFalse(any(event["percent"] == 100 for event in events))
                self.assertTrue(any(event["completed"] for event in events))

    def test_progress_resources_and_embedded_blocks_match(self):
        expected = {
            "en": json.loads((ROOT / "packaging/windows/install-progress-en.json").read_text()),
            "ko": json.loads((ROOT / "packaging/windows/install-progress-ko.json").read_text()),
        }
        blocks = []
        for path in (ROOT / "install.ps1", ROOT / "packaging/windows/install.ps1"):
            text = path.read_text()
            match = re.search(r"\$progressMessagesJson = @'\r?\n(.*?)\r?\n'@", text, re.S)
            self.assertIsNotNone(match)
            blocks.append(json.loads(match.group(1)))
        self.assertEqual(blocks[0], expected)
        self.assertEqual(blocks[1], expected)
        self.assertEqual(blocks[0], blocks[1])
