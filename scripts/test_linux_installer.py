#!/usr/bin/env python3
"""Isolated Linux installation preservation checks; never uses the real HOME."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    assert os.name == "posix"
    with tempfile.TemporaryDirectory(prefix="hantong install ") as directory:
        fixture = Path(directory).resolve()
        package = fixture / "package"
        package.mkdir()
        for name in ("install.sh", "uninstall.sh"):
            shutil.copyfile(ROOT / "packaging/unix" / name, package / name)
        (package / "VERSION").write_text("0.1.0-beta.22\n")
        (package / "release-manifest.json").write_text('{"synthetic_fixture":true}\n')
        (package / "bin").mkdir()
        for name in ("hantong", "hantong-t", "hantong-mcp", "mail-hantong-core"):
            (package / "bin" / name).write_text("#!/bin/sh\nprintf 'fixture executable\\n'\n")
        files = sorted(str(path.relative_to(package)) for path in package.rglob("*") if path.is_file())
        files += ["FILES.sha256", "installed-files.txt"]
        (package / "installed-files.txt").write_text("\n".join(sorted(files)) + "\n")
        checksums = "".join(f"{hashlib.sha256((package / name).read_bytes()).hexdigest()}  {name}\n"
            for name in sorted(files) if name != "FILES.sha256")
        (package / "FILES.sha256").write_text(checksums)
        home = fixture / "home with ' quote"
        home.mkdir()
        env = {**os.environ, "HOME": str(home), "XDG_DATA_HOME": str(home / ".local/share")}
        sentinel = home / "mail-user-data"
        sentinel.write_bytes(b"must stay unchanged")
        subprocess.run(["sh", str(package / "install.sh")], env=env, check=True)
        root = home / ".local/share/mail-hantong-cli"
        installed = root / "versions/0.1.0-beta.22"
        unknown = installed / "unknown.keep"
        unknown.write_bytes(b"keep me")
        # Existing metadata links must fail before changing any launcher or target.
        launcher_before = (home / ".local/bin/hantong").read_bytes()
        for metadata in (".mail-hantong-owner", "launcher-dir.txt", "launchers.sha256", "current.txt"):
            path = root / metadata
            saved = path.read_bytes()
            path.unlink()
            path.symlink_to(sentinel)
            failed = subprocess.run(["sh", str(package / "install.sh")], env=env, capture_output=True)
            assert failed.returncode != 0
            assert sentinel.read_bytes() == b"must stay unchanged"
            assert (home / ".local/bin/hantong").read_bytes() == launcher_before
            path.unlink()
            path.write_bytes(saved)
        subprocess.run([str(home / ".local/bin/hantong")], env=env, check=True)
        subprocess.run(["sh", str(package / "install.sh")], env=env, check=True)
        assert unknown.read_bytes() == b"keep me"
        modified = installed / "bin/hantong-t"
        modified.write_bytes(b"user modified program; preserve on uninstall")
        subprocess.run(["sh", str(installed / "uninstall.sh")], env=env, check=True)
        assert not (installed / "bin/hantong").exists()
        assert not (home / ".local/bin/hantong").exists()
        assert not (root / "current.txt").exists()
        assert unknown.read_bytes() == b"keep me"
        assert modified.read_bytes() == b"user modified program; preserve on uninstall"
        assert sentinel.read_bytes() == b"must stay unchanged"
        # An unrelated command with the same name must never be replaced.
        bindir = home / ".local/bin"
        protected = bindir / "hantong"
        protected.write_bytes(b"unrelated user program")
        failed = subprocess.run(["sh", str(package / "install.sh")], env=env, capture_output=True)
        assert failed.returncode != 0
        assert protected.read_bytes() == b"unrelated user program"
        assert sentinel.read_bytes() == b"must stay unchanged"
        print("Linux installer: install/reinstall/quoted path/uninstall preservation passed")


if __name__ == "__main__":
    main()
