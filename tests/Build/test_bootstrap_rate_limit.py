"""Verify the bootstrap's bounded public-feed fallback for GitHub rate limits."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "nt", "Windows installer")
class BootstrapRateLimitTests(unittest.TestCase):
    def run_fixture(self, status: int, *, malformed: bool = False, release_version: str = "latest", head_mode: str = "normal", feed_override: str | None = None):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-rate-limit-") as directory:
            root = Path(directory)
            archive_name = "mail-hantong-cli-0.1.0-beta.6-windows-x86_64.zip"
            archive = root / archive_name
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("install.ps1", "param($PackageRoot,$Source,$InstallRoot); New-Item -ItemType Directory -Force (Join-Path $InstallRoot 'bin') | Out-Null; Set-Content (Join-Path $InstallRoot 'current.txt') '0.1.0-beta.6'")
            sums = root / "SHA256SUMS"
            sums.write_text(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive_name}\n", encoding="ascii")
            feed = "<feed xmlns=\"http://www.w3.org/2005/Atom\"><entry><link rel=\"alternate\" href=\"https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.6\" /></entry></feed>"
            if malformed:
                feed = "<not-a-release>"
            if feed_override is not None:
                feed = feed_override
            script = root / "run.ps1"
            script.write_text(
                """
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
public sealed class HantongRateLimitException : Exception {
    private object response;
    public object Response { get { return response; } }
    public HantongRateLimitException(int status) { response = new RateResponse(status); }
}
public sealed class RateResponse { private int statusCode; public int StatusCode { get { return statusCode; } } public RateResponse(int status) { statusCode = status; } }
'@
. ([ScriptBlock]::Create([IO.File]::ReadAllText($env:HANTONG_BOOTSTRAP_SCRIPT)))
$script:FixtureRoot = $env:HANTONG_FIXTURE_ROOT
$script:FixtureFeed = $env:HANTONG_FIXTURE_FEED
function Invoke-RestMethod { throw [HantongRateLimitException]::new($env:HANTONG_RATE_STATUS) }
function Invoke-WebRequest {
    param($Uri, $Method, [switch]$UseBasicParsing, $Headers, $OutFile)
    if ($Uri -eq 'https://github.com/tz1012/mail-hantong/releases.atom') { return [pscustomobject]@{ Content = $script:FixtureFeed } }
    $name = [IO.Path]::GetFileName(([Uri]$Uri).AbsolutePath)
    $path = Join-Path $script:FixtureRoot $name
    if ($Method -eq 'Head') {
        if ($env:HANTONG_HEAD_MODE -eq 'missing') { return [pscustomobject]@{ Headers = @{} } }
        $length = (Get-Item -LiteralPath $path).Length
        if ($env:HANTONG_HEAD_MODE -eq 'oversized' -and $name -like '*.zip') { $length = 134217729 }
        return [pscustomobject]@{ Headers = @{ 'Content-Length' = $length } }
    }
    Copy-Item -LiteralPath $path -Destination $OutFile
}
Install-MailHantong -ReleaseVersion $env:HANTONG_RELEASE_VERSION -Destination $env:HANTONG_INSTALL_ROOT -SessionOnly
""",
                encoding="utf-8",
            )
            env = {k: v for k, v in os.environ.items() if k.lower() != "psmodulepath"}
            env.update(
                HANTONG_BOOTSTRAP_SCRIPT=str(ROOT / "install.ps1"),
                HANTONG_FIXTURE_ROOT=str(root),
                HANTONG_FIXTURE_FEED=feed,
                HANTONG_RATE_STATUS=str(status),
                HANTONG_RELEASE_VERSION=release_version,
                HANTONG_HEAD_MODE=head_mode,
                HANTONG_INSTALL_ROOT=str(root / "installed"),
            )
            powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
            result = subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-File", str(script)], env=env, capture_output=True, timeout=45)
            marker = root / "installed/current.txt"
            return result, marker.read_text(encoding="utf-8").strip() if marker.exists() else None

    def test_inst01_public_atom_fallback_handles_403_and_429_without_auth(self):
        for status in (403, 429):
            with self.subTest(status=status):
                result, marker = self.run_fixture(status)
                self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
                self.assertEqual(marker, "0.1.0-beta.6")

    def test_sec07_malformed_public_atom_feed_fails_closed(self):
        result, marker = self.run_fixture(403, malformed=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(marker)

    def test_sec07_non_rate_limit_error_does_not_use_atom_fallback(self):
        result, marker = self.run_fixture(500)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(marker)

    def test_sec07_atom_dtd_and_external_entity_are_rejected(self):
        feed = '<!DOCTYPE feed [<!ENTITY x SYSTEM "https://example.invalid/secret">]><feed xmlns="http://www.w3.org/2005/Atom"><entry><link rel="alternate" href="https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.6" /></entry></feed>'
        result, marker = self.run_fixture(403, feed_override=feed)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(marker)

    def test_sec07_explicit_tag_and_head_size_failures_are_bounded(self):
        result, marker = self.run_fixture(403, release_version="0.1.0-beta.6")
        self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
        self.assertEqual(marker, "0.1.0-beta.6")
        for mode in ("oversized", "missing"):
            with self.subTest(mode=mode):
                result, marker = self.run_fixture(403, head_mode=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertIsNone(marker)

