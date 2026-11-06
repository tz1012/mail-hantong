"""Document the legacy shared-feed hazard with mock transports, never the network."""
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "posix" and Path("/usr/bin/python3").is_file(), "POSIX fixture required")
class UnixFeedIsolationTests(unittest.TestCase):
    def fixture(self, system, machine, linux_only_first):
        with tempfile.TemporaryDirectory(prefix="hantong-feed-fixture-") as directory:
            root = Path(directory)
            shim = root / "shim"
            shim.mkdir()
            marker = root / "installed"
            old = "0.1.0-beta.29"
            new = "0.1.0-beta.30"
            versions = [new, old] if linux_only_first else [old]
            (root / "feed").write_text("\n".join(
                f"<id>tag:github.com,2008:Repository/1/v{v}</id>" for v in versions))
            for version in versions:
                release = root / version
                release.mkdir()
                targets = ["linux-x86_64"] if version == new else ["macos-x86_64", "macos-aarch64", "linux-x86_64"]
                rows = []
                for target in targets:
                    name = f"mail-hantong-cli-{version}-{target}.tar.gz"
                    data = b'#!/bin/sh\nprintf fixture > "$FIXTURE_MARKER"\n'
                    with tarfile.open(release / name, "w:gz") as archive:
                        member = tarfile.TarInfo("install.sh")
                        member.size = len(data)
                        archive.addfile(member, io.BytesIO(data))
                    rows.append(hashlib.sha256((release / name).read_bytes()).hexdigest() + "  " + name)
                (release / "SHA256SUMS").write_text("\n".join(rows) + "\n")
            (shim / "uname").write_text('#!/bin/sh\ncase "$1" in -s) echo "$FIXTURE_OS";; -m) echo "$FIXTURE_CPU";; *) exit 1;; esac\n')
            (shim / "curl").write_text('''#!/usr/bin/python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(a for a in args if a.startswith("https://"))
prefix = "https://github.com/tz1012/mail-hantong/"
if not url.startswith(prefix): sys.exit(91)
suffix = url[len(prefix):]
root = pathlib.Path(os.environ["FIXTURE_ROOT"])
if suffix == "releases.atom": source = root / "feed"
elif suffix.startswith("releases/download/v"):
    version, name = suffix[len("releases/download/v"):].split("/")
    source = root / version / name
else: sys.exit(92)
if not source.is_file(): sys.exit(22)
shutil.copyfile(source, args[args.index("-o") + 1])
''')
            for path in shim.iterdir():
                path.chmod(0o700)
            env = {**os.environ, "PATH": str(shim) + os.pathsep + os.environ["PATH"],
                   "FIXTURE_ROOT": str(root), "FIXTURE_MARKER": str(marker),
                   "FIXTURE_OS": system, "FIXTURE_CPU": machine, "TMPDIR": str(root)}
            result = subprocess.run(["sh", str(ROOT / "install.sh")], env=env,
                                    capture_output=True, timeout=10)
            return result, marker.exists()

    def test_INST01_unchanged_feed_keeps_linux_and_both_mac_targets(self):
        for system, machine in (("Linux", "x86_64"), ("Darwin", "x86_64"), ("Darwin", "arm64")):
            with self.subTest(system=system, machine=machine):
                result, installed = self.fixture(system, machine, False)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(installed)

    def test_SEC03_partial_shared_release_is_not_a_safe_linux_channel(self):
        for machine in ("x86_64", "arm64"):
            with self.subTest(machine=machine):
                result, installed = self.fixture("Darwin", machine, True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"no unique checksum", result.stderr)
                self.assertFalse(installed)


@unittest.skipUnless(os.name == "nt", "Windows PowerShell fixture required")
class WindowsPartialFeedTests(unittest.TestCase):
    def test_INST01_normal_api_skips_new_release_without_windows_assets(self):
        spec = importlib.util.spec_from_file_location("selection_fixture", ROOT / "tests/Build/test_bootstrap_release_selection.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        fixture = module.BootstrapReleaseSelectionTests()
        releases = [fixture.release("0.1.0-beta.30", assets=False),
                    fixture.release("0.1.0-beta.29")]
        fixture.assert_selected(releases, "0.1.0-beta.29")

    def test_SEC03_partial_shared_release_rejects_403_and_429_without_install(self):
        spec = importlib.util.spec_from_file_location("rate_fixture", ROOT / "tests/Build/test_bootstrap_rate_limit.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        fixture = module.BootstrapRateLimitTests()
        feed = '<feed xmlns="http://www.w3.org/2005/Atom"><entry><link rel="alternate" href="https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.30" /></entry></feed>'
        for status in (403, 429):
            with self.subTest(status=status):
                result, marker = fixture.run_fixture(status, feed_override=feed)
                self.assertNotEqual(result.returncode, 0)
                self.assertIsNone(marker)


if __name__ == "__main__":
    unittest.main()
