"""Exercise bootstrap release ordering and downgrade protection with synthetic API data."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
POWERSHELL = Path(os.environ.get("SystemRoot", "C:/Windows")) / "System32/WindowsPowerShell/v1.0/powershell.exe"


@unittest.skipUnless(os.name == "nt" and POWERSHELL.is_file(), "Windows PowerShell required")
class BootstrapReleaseSelectionTests(unittest.TestCase):
    def release(self, label, *, draft=False, assets=True):
        name = f"mail-hantong-cli-{label}-windows-x86_64.zip"
        items = []
        if assets:
            prefix = f"https://github.com/tz1012/mail-hantong/releases/download/v{label}/"
            items = [
                {"name": name, "size": 100, "browser_download_url": prefix + name},
                {"name": "SHA256SUMS", "size": 100, "browser_download_url": prefix + "SHA256SUMS"},
            ]
        return {"draft": draft, "tag_name": f"v{label}", "assets": items}

    def run_fixture(self, releases, *, release_version="latest", atom=False, current=None, marker="mail-hantong-cli-v1"):
        with tempfile.TemporaryDirectory(prefix="hantong-release-selection-") as directory:
            root = Path(directory)
            destination = root / "installed"
            if current is not None:
                destination.mkdir()
                (destination / ".mail-hantong-owner").write_text(marker, encoding="ascii")
                (destination / "current.txt").write_text(current + "\n", encoding="ascii")
            feed = "<feed xmlns=\"http://www.w3.org/2005/Atom\">" + "".join(
                f"<entry><link rel=\"alternate\" href=\"https://github.com/tz1012/mail-hantong/releases/tag/{release['tag_name']}\" /></entry>"
                for release in releases
            ) + "</feed>"
            script = root / "run.ps1"
            script.write_text(
                r"""
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
public sealed class HantongSelectionRateLimitException : Exception {
    private object response;
    public object Response { get { return response; } }
    public HantongSelectionRateLimitException(int status) { response = new HantongSelectionRateResponse(status); }
}
public sealed class HantongSelectionRateResponse { public int StatusCode { get; private set; } public HantongSelectionRateResponse(int status) { StatusCode = status; } }
'@
. ([ScriptBlock]::Create([IO.File]::ReadAllText($env:HANTONG_BOOTSTRAP_SCRIPT)))
$script:FixtureReleases = $env:HANTONG_RELEASES | ConvertFrom-Json
$script:FixtureFeed = $env:HANTONG_RELEASE_FEED
function Invoke-RestMethod {
    param($Uri, $Headers)
    if ($env:HANTONG_ATOM -eq '1') { throw [HantongSelectionRateLimitException]::new(403) }
    if ($Uri -match '/tags/') {
        $tag = $Uri.Substring($Uri.LastIndexOf('/') + 1)
        return @($script:FixtureReleases | Where-Object { $_.tag_name -ceq $tag })[0]
    }
    return ,@($script:FixtureReleases)
}
function Invoke-WebRequest {
    param($Uri, $Method, [switch]$UseBasicParsing, $Headers, $OutFile)
    if ($Uri -ceq 'https://github.com/tz1012/mail-hantong/releases.atom') {
        return [pscustomobject]@{ Content = $script:FixtureFeed }
    }
    if ($Method -eq 'Head') {
        return [pscustomobject]@{ Headers = @{ 'Content-Length' = '100' } }
    }
    throw "SELECTED:$Uri"
}
try {
    Install-MailHantong -ReleaseVersion $env:HANTONG_RELEASE_VERSION -Destination $env:HANTONG_INSTALL_ROOT -SessionOnly
} catch {
    Write-Output ("RESULT:" + $_.Exception.Message)
}
""",
                encoding="utf-8",
            )
            env = {key: value for key, value in os.environ.items() if key.lower() != "psmodulepath"}
            env.update(
                HANTONG_BOOTSTRAP_SCRIPT=str(ROOT / "install.ps1"),
                HANTONG_RELEASES=json.dumps(releases, separators=(",", ":")),
                HANTONG_RELEASE_VERSION=release_version,
                HANTONG_INSTALL_ROOT=str(destination),
                HANTONG_ATOM="1" if atom else "0",
                HANTONG_RELEASE_FEED=feed,
            )
            result = subprocess.run(
                [str(POWERSHELL), "-NoProfile", "-NonInteractive", "-File", str(script)],
                env=env,
                capture_output=True,
                timeout=30,
            )
            output = (result.stdout + result.stderr).decode("utf-8", "replace")
            return result, output

    def assert_selected(self, releases, expected, **kwargs):
        result, output = self.run_fixture(releases, **kwargs)
        self.assertEqual(result.returncode, 0, output)
        self.assertIn(f"RESULT:SELECTED:https://github.com/tz1012/mail-hantong/releases/download/v{expected}/mail-hantong-cli-{expected}-windows-x86_64.zip", output)

    def test_latest_selects_highest_published_release_from_unsorted_api_data(self):
        releases = [
            self.release("0.1.0-beta.9", draft=True),
            self.release("0.1.0-beta.8"),
            self.release("0.1.0-beta..7"),
            self.release("0.1.0-beta.6", assets=False),
            self.release("0.1.0-beta.10"),
        ]
        self.assert_selected(releases, "0.1.0-beta.10")

    def test_latest_stable_release_beats_same_core_prerelease(self):
        self.assert_selected(
            [self.release("0.1.0-beta.10"), self.release("0.1.0")],
            "0.1.0",
        )

    def test_latest_promotes_same_core_beta_above_legacy_preview(self):
        self.assert_selected(
            [self.release("0.1.0-beta.8"), self.release("0.1.0-preview.4"), self.release("0.1.0-beta.10")],
            "0.1.0-beta.10",
        )

    def test_latest_compares_large_numeric_identifiers_without_overflow(self):
        self.assert_selected(
            [self.release("9.0.0"), self.release("999999999999999999999999.0.0")],
            "999999999999999999999999.0.0",
        )

    def test_latest_atom_fallback_uses_the_same_semver_ordering(self):
        releases = [self.release("0.1.0-beta.9"), self.release("0.1.0-beta.8"), self.release("0.1.0-beta.10")]
        self.assert_selected(releases, "0.1.0-beta.10", atom=True)

    def test_explicit_version_keeps_exact_tag_selection(self):
        releases = [self.release("0.1.0-beta.10"), self.release("0.1.0-beta.8")]
        self.assert_selected(releases, "0.1.0-beta.8", release_version="0.1.0-beta.8")

    def test_latest_refuses_to_downgrade_owned_current_version(self):
        result, output = self.run_fixture([self.release("0.1.0-beta.8")], current="0.1.0-beta.10")
        self.assertEqual(result.returncode, 0, output)
        self.assertIn("RESULT:No compatible published Windows package is available.", output)
        self.assertNotIn("SELECTED:", output)

    def test_latest_refuses_legacy_preview_below_owned_beta(self):
        result, output = self.run_fixture([self.release("0.1.0-preview.4")], current="0.1.0-beta.11")
        self.assertEqual(result.returncode, 0, output)
        self.assertIn("RESULT:No compatible published Windows package is available.", output)
        self.assertNotIn("SELECTED:", output)

    def test_latest_fails_closed_on_wrong_ownership_marker_before_current_read(self):
        result, output = self.run_fixture([self.release("0.1.0-beta.8")], current="not-a-version", marker="wrong")
        self.assertEqual(result.returncode, 0, output)
        self.assertIn("RESULT:Installation ownership is missing.", output)
        self.assertNotIn("SELECTED:", output)


if __name__ == "__main__":
    unittest.main()
