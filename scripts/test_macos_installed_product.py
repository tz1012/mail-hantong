#!/usr/bin/env python3
"""CI-only native macOS startup smoke for the just-built package.

This deliberately creates no account and never contacts a provider.  It is
refused outside the dedicated GitHub macOS runner because macOS path discovery
uses the runner's real passwd home rather than an arbitrary HOME override.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import pwd
import re
import shutil
import subprocess
import tarfile
import tempfile
import time


def fail(message: str) -> None:
    raise RuntimeError(message)


def extract_archive(archive: Path, destination: Path) -> None:
    with tarfile.open(archive, "r:gz") as bundle:
        members = bundle.getmembers()
        for member in members:
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or "\\" in member.name:
                fail("package contains an unsafe path")
            if not (member.isdir() or member.isreg()) or path == Path("."):
                fail("package contains a non-regular entry")
        bundle.extractall(destination, filter="data")


def run_cli(executable: Path, environment: dict[str, str]) -> None:
    import pty
    import select

    master, slave = pty.openpty()
    process = subprocess.Popen(
        [str(executable), "--lang", "en"], stdin=slave, stdout=slave, stderr=slave,
        env=environment, close_fds=True,
    )
    os.close(slave)
    deadline = time.monotonic() + 45
    output = bytearray()
    menu_ready = False
    try:
        while process.poll() is None and time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.25)
            if ready:
                chunk = os.read(master, 16 * 1024)
                output.extend(chunk)
                if len(output) > 256 * 1024:
                    fail("startup output exceeded its bound")
                if b"2) Exit" in output or b"2) exit" in output:
                    menu_ready = True
                    os.write(master, b"2\n")
                    break
        while process.poll() is None and time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.25)
            if ready:
                output.extend(os.read(master, 16 * 1024))
                if len(output) > 256 * 1024:
                    fail("startup output exceeded its bound")
        if not menu_ready:
            diagnostics = re.findall(
                rb"hantong_startup stage=[a-z_]+ outcome=failed code=[A-Z_]+|\b[A-Z]+(?:_[A-Z]+)+\b",
                bytes(output),
            )
            fail(f"installed CLI did not present the English no-account menu; exit={process.returncode}; codes={diagnostics[-8:]}")
        if process.poll() is None:
            fail("installed CLI did not finish the no-account exit flow")
        if process.returncode != 0:
            fail("installed CLI no-account exit failed")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        os.close(master)


def stop_core(core: Path, environment: dict[str, str]) -> None:
    result = subprocess.run(
        [str(core), "unix-stop"], env=environment,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        timeout=20, check=False,
    )
    if result.returncode != 0:
        fail("verified unix-stop did not stop the Core")


def main() -> int:
    if platform.system() != "Darwin" or os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("CI") != "true":
        raise RuntimeError("macOS installed-product smoke is restricted to the GitHub CI runner")
    parser = argparse.ArgumentParser()
    parser.add_argument("--package-dir", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()

    home = Path(pwd.getpwuid(os.geteuid()).pw_dir)
    app_root = home / "Library" / "Application Support" / "mail-hantong"
    if app_root.exists() or app_root.is_symlink():
        raise RuntimeError("refusing to touch an existing macOS application directory")
    archives = sorted(args.package_dir.glob(f"mail-hantong-cli-{args.version}-macos-*.tar.gz"))
    if len(archives) != 1:
        raise RuntimeError("expected exactly one macOS package archive")

    cleanup_core: tuple[Path, dict[str, str]] | None = None
    stop_verified = False
    temporary = tempfile.TemporaryDirectory(prefix="hantong-macos-smoke-")
    try:
        root = Path(temporary.name).resolve()
        package = root / "package"
        package.mkdir(mode=0o700)
        extract_archive(archives[0], package)
        package_root = package
        if not (package / "install.sh").is_file():
            nested = [item for item in package.iterdir() if item.is_dir()]
            if len(nested) != 1 or not (nested[0] / "install.sh").is_file():
                raise RuntimeError("package installer is missing")
            package_root = nested[0]
        install_root = root / "home" / "Library" / "Application Support" / "mail-hantong-cli"
        bin_dir = root / "home" / ".local" / "bin"
        install_root.parent.mkdir(mode=0o700, parents=True)
        bin_dir.mkdir(mode=0o700, parents=True)
        environment = {**os.environ, "HOME": str(root / "home"), "XDG_DATA_HOME": str(root / "data")}
        subprocess.run(
            ["sh", str(package_root / "install.sh"), "--install-root", str(install_root), "--bin-dir", str(bin_dir)],
            env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=30, check=True,
        )
        installed = install_root / "versions" / args.version / "bin"
        cli = installed / "hantong"
        core = installed / "mail-hantong-core"
        if not cli.is_file() or not core.is_file():
            raise RuntimeError("installed native binaries are missing")
        cleanup_core = (core, environment)
        run_cli(cli, environment)
        subprocess.run(
            [str(cli), "core", "status", "--json"], env=environment,
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=15, check=True,
        )
        database = app_root / "data" / "mail-hantong.db"
        if not app_root.is_dir() or not database.is_file() or database.stat().st_size == 0:
            raise RuntimeError("Core did not create the macOS application database")
        with database.open("rb") as source:
            if source.read(16) == b"SQLite format 3\x00":
                raise RuntimeError("Core database is not encrypted")
        stop_core(core, environment)
        run_cli(cli, environment)
        stop_core(core, environment)
        stop_verified = True
    finally:
        if cleanup_core is not None and not stop_verified:
            subprocess.run(
                [str(cleanup_core[0]), "unix-stop"], env=cleanup_core[1],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                timeout=20, check=False,
            )
        if stop_verified and app_root.exists():
            shutil.rmtree(app_root)
        temporary.cleanup()
    print("macOS installed-product startup, database creation, stop, and reopen passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"error: macOS installed-product smoke failed: {error}", file=__import__("sys").stderr)
        raise SystemExit(2)
