"""Exercise the existing shell installer using only disposable synthetic programs."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "posix" and shutil.which("sha256sum"), "POSIX shell required")
class LinuxLocalUpdateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hantong-pinned-local-")
        self.addCleanup(self.temp.cleanup)
        # macOS commonly exposes TemporaryDirectory below /var via the
        # /var -> /private/var alias.  The installer intentionally rejects
        # symlinked ancestors, so fixtures use the canonical path while
        # retaining all installer security checks.
        self.root = Path(self.temp.name).resolve()
        self.home = self.root / "home"
        self.home.mkdir()
        self.env = {**os.environ, "HOME": str(self.home),
                    "XDG_DATA_HOME": str(self.home / "data"),
                    "XDG_CONFIG_HOME": str(self.home / "config"),
                    "XDG_RUNTIME_DIR": str(self.home / "runtime")}
        self.sentinels = {}
        for name in ("accounts", "key", "profile", "vault", "mail"):
            path = self.home / "user-state" / name
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(("synthetic-" + name).encode())
            self.sentinels[path] = path.read_bytes()
        self.install = self.home / "data/mail-hantong-cli"
        self.old = self.package("0.1.0-beta.29")
        self.assertEqual(self.run_install(self.old).returncode, 0)
        self.old_pointer = (self.install / "current.txt").read_bytes()
        self.new = self.package("0.1.0-beta.30")

    def package(self, version):
        package = self.root / version
        (package / "bin").mkdir(parents=True)
        shutil.copyfile(ROOT / "packaging/unix/install.sh", package / "install.sh")
        (package / "VERSION").write_text(version + "\n")
        (package / "release-manifest.json").write_text('{"synthetic_only":true}\n')
        for name in ("hantong", "hantong-t", "hantong-mcp", "mail-hantong-core"):
            (package / "bin" / name).write_text("#!/bin/sh\nprintf '%s\\n' '" + version + "'\n")
        files = sorted(str(p.relative_to(package)) for p in package.rglob("*") if p.is_file())
        files += ["installed-files.txt", "FILES.sha256"]
        (package / "installed-files.txt").write_text("\n".join(sorted(files)) + "\n")
        (package / "FILES.sha256").write_text("".join(
            hashlib.sha256((package / name).read_bytes()).hexdigest() + "  " + name + "\n"
            for name in sorted(files) if name != "FILES.sha256"))
        return package

    def run_install(self, package, env=None):
        return subprocess.run(["sh", str(package / "install.sh")], env=env or self.env,
                              capture_output=True, timeout=20)

    def preserved(self):
        for path, data in self.sentinels.items():
            self.assertEqual(path.read_bytes(), data)
        self.assertTrue((self.install / "versions/0.1.0-beta.29/bin/hantong").is_file())

    def shim(self, command, body):
        directory = self.root / "shim"
        directory.mkdir()
        path = directory / command
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o700)
        return {**self.env, "PATH": str(directory) + os.pathsep + os.environ["PATH"]}

    def test_INST01_update_preserves_state_and_relaunches_synthetic_program(self):
        self.assertEqual(self.run_install(self.new).returncode, 0)
        launcher = self.home / ".local/bin/hantong"
        for _ in range(2):
            result = subprocess.run([str(launcher)], env=self.env, capture_output=True,
                                    timeout=5, check=True)
            self.assertEqual(result.stdout.strip(), b"0.1.0-beta.30")
        self.preserved()

    def test_SEC03_bad_checksum_keeps_previous_pointer(self):
        (self.new / "bin/hantong").write_bytes(b"tampered")
        self.assertNotEqual(self.run_install(self.new).returncode, 0)
        self.assertEqual((self.install / "current.txt").read_bytes(), self.old_pointer)
        self.preserved()

    def test_INST01_staging_failure_keeps_previous_pointer(self):
        real = shutil.which("sha256sum")
        env = self.shim("sha256sum", "case \"$PWD\" in */.install.*) exit 1;; esac\n"
                        + "exec '" + real + "' \"$@\"\n")
        result = self.run_install(self.new, env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"staged package checksum mismatch", result.stderr)
        self.assertEqual((self.install / "current.txt").read_bytes(), self.old_pointer)
        self.preserved()

    def test_INST01_interrupted_pointer_activation_preserves_old_then_rolls_forward(self):
        real = shutil.which("mv")
        env = self.shim("mv", "for arg do case \"$arg\" in */current.txt) exit 1;; esac; done\n"
                        + "exec '" + real + "' \"$@\"\n")
        self.assertNotEqual(self.run_install(self.new, env).returncode, 0)
        self.assertEqual((self.install / "current.txt").read_bytes(), self.old_pointer)
        self.preserved()
        self.assertEqual(self.run_install(self.new).returncode, 0)
        self.assertEqual((self.install / "current.txt").read_text().strip(), "0.1.0-beta.30")
        self.preserved()

    def test_INST01_unproved_backward_compatibility_refuses_downgrade(self):
        self.assertEqual(self.run_install(self.new).returncode, 0)
        result = self.run_install(self.old)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"refusing to downgrade", result.stderr)
        self.assertEqual((self.install / "current.txt").read_text().strip(), "0.1.0-beta.30")
        self.preserved()


if __name__ == "__main__":
    unittest.main()
