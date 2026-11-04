#!/usr/bin/env python3
"""Build a native Unix CLI package without user profiles or credentials."""
from __future__ import annotations

import argparse
import gzip
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import tomllib

from package_cli import (BINARIES, PackageError, archive_name, checked_source_hash,
                         compiler_info, dependency_licenses, json_bytes, publish,
                         read_file, release_version, run_command, selected_packages, sha)

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    ("Linux", "x86_64"): ("x86_64-unknown-linux-gnu", "linux-x86_64"),
    ("Linux", "aarch64"): ("aarch64-unknown-linux-gnu", "linux-aarch64"),
    ("Darwin", "x86_64"): ("x86_64-apple-darwin", "macos-x86_64"),
    ("Darwin", "arm64"): ("aarch64-apple-darwin", "macos-aarch64"),
    ("Darwin", "aarch64"): ("aarch64-apple-darwin", "macos-aarch64"),
}


def check_elf(data: bytes, machine: str) -> None:
    expected = {"x86_64": 62, "aarch64": 183}[machine]
    if (len(data) < 64 or data[:6] != b"\x7fELF\x02\x01"
            or int.from_bytes(data[18:20], "little") != expected):
        raise PackageError("native executable has an unexpected ELF architecture")


def check_macho(data: bytes, machine: str) -> None:
    if len(data) < 8 or data[:4] not in (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf"):
        raise PackageError("native executable has an unexpected Mach-O format")
    endian = "little" if data[:4] == b"\xcf\xfa\xed\xfe" else "big"
    expected = {"x86_64": 0x01000007, "arm64": 0x0100000C, "aarch64": 0x0100000C}[machine]
    if int.from_bytes(data[4:8], endian) != expected:
        raise PackageError("native executable has an unexpected Mach-O architecture")


def check_native(data: bytes, system: str, machine: str) -> None:
    if system == "Linux":
        check_elf(data, machine)
    else:
        check_macho(data, machine)


def write_archive(path: Path, contents: dict[str, bytes]) -> None:
    with path.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                for name, data in sorted(contents.items()):
                    archive_name(name)
                    item = tarfile.TarInfo(name)
                    item.size = len(data)
                    item.mode = 0o755 if name.startswith("bin/") or name.endswith(".sh") else 0o644
                    item.mtime = 0
                    archive.addfile(item, io.BytesIO(data))


def package(root: Path, version: str, output: Path, publisher: Path | None = None) -> Path:
    version = release_version(version)
    system = platform.system()
    machine = platform.machine()
    try:
        target, release_target = TARGETS[(system, machine)]
    except KeyError as error:
        raise PackageError("build on a native supported Unix architecture") from error
    root = root.resolve()
    env = dict(os.environ)
    env["HANTONG_RELEASE_VERSION"] = version
    runner = subprocess.run
    before = checked_source_hash(root)
    lock = read_file(root / "Cargo.lock", root)
    metadata = json.loads(run_command(runner, ["cargo", "metadata", "--format-version", "1",
        "--locked", "--offline", "--filter-platform", target], root, env, capture=True, timeout=120).stdout)
    dependencies, roots = selected_packages(metadata, root, version)
    registry = Path(env.get("CARGO_HOME", Path.home() / ".cargo")) / "registry/src"
    contents, inventory = dependency_licenses(dependencies, tomllib.loads(lock.decode()), registry,
                                              root / "packaging/licenses")
    for destination, source in {
        "LICENSE": "LICENSE", "README.md": "packaging/unix/README.md",
        "CLI-LAUNCH.md": "docs/latest/CLI-LAUNCH.md",
        "EXTERNAL-MCP-GUIDE.md": "docs/latest/EXTERNAL-MCP-GUIDE.md",
        "install.sh": "packaging/unix/install.sh", "uninstall.sh": "packaging/unix/uninstall.sh",
        "update.sh": "install.sh",
    }.items():
        contents[destination] = read_file(root / source, root)
    if publisher is not None:
        publisher = publisher.resolve()
        if publisher.is_relative_to(root):
            raise PackageError("publisher asset must remain outside the source tree")
        data = read_file(publisher, publisher.parent)
        if not 1 <= len(data) <= 16_384:
            raise PackageError("publisher asset exceeds its bound")
        contents["publisher/google-desktop.json"] = data
    command = ["cargo", "build", "--release", "--target", target]
    for name in BINARIES:
        command += ["-p", name]
    command += ["--bins", "--locked", "--offline"]
    run_command(runner, command, root, env, capture=False, timeout=1800)
    compiler = compiler_info(runner, root, env)
    binary_root = Path(metadata["target_directory"]) / target / "release"
    hashes = {}
    for name, binary in BINARIES.items():
        executable = binary_root / binary
        data = read_file(executable, Path(metadata["target_directory"]))
        check_native(data, system, machine)
        for flag in ("--help", "--version"):
            result = run_command(runner, [str(executable), flag], root, env, capture=True, timeout=15)
            expected = version if binary in ("hantong", "hantong-t") else roots[name]["version"]
            if (result.stderr or len(result.stdout) > 131_072 or binary.encode() not in result.stdout
                    or (flag == "--version" and result.stdout.decode().strip() != f"{binary} {expected}")):
                raise PackageError("native executable smoke check failed")
        contents[f"bin/{binary}"] = data
        hashes[f"bin/{binary}"] = sha(data)
    contents["VERSION"] = (version + "\n").encode()
    contents["THIRD-PARTY.json"] = inventory
    contents["BUILD-METADATA.json"] = json_bytes({
        "format_version": 1, "release_label": version, "target": target, "source_hash": before,
        "build_performed": True, "source_relation": "built_from_current_workspace",
        "build_command": command, "rustc": compiler, "cargo_lock_sha256": sha(lock),
        "binaries": hashes, "google_publisher_config_included": publisher is not None,
        "third_party_inventory_sha256": sha(inventory),
        "build_libc": platform.libc_ver()[1] if system == "Linux" else None,
    })
    contents["installed-files.txt"] = ("\n".join(sorted([*contents, "installed-files.txt", "release-manifest.json", "FILES.sha256"])) + "\n").encode()
    contents["release-manifest.json"] = json_bytes({
        "format_version": 1, "version": version, "package_version": roots["hantong-cli"]["version"],
        "target": release_target,
        "files": [{"path": name, "sha256": sha(data), "size": len(data)} for name, data in sorted(contents.items())],
    })
    contents["FILES.sha256"] = "".join(f"{sha(data)}  {name}\n" for name, data in sorted(contents.items())).encode()
    if checked_source_hash(root) != before:
        raise PackageError("source changed during packaging")
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".unix-package-", dir=output) as temporary:
        stage = Path(temporary) / "release"
        stage.mkdir()
        archive = stage / f"mail-hantong-cli-{version}-{release_target}.tar.gz"
        write_archive(archive, contents)
        (stage / "BUILD-METADATA.json").write_bytes(contents["BUILD-METADATA.json"])
        (stage / "SHA256SUMS").write_text("".join(f"{sha(item.read_bytes())}  {item.name}\n"
            for item in (archive, stage / "BUILD-METADATA.json")), encoding="utf-8", newline="\n")
        destination = output / version
        publish(stage, destination)
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "artifacts/unix")
    parser.add_argument("--publisher-config", type=Path)
    args = parser.parse_args()
    try:
        print(package(ROOT, args.version, args.output_dir.resolve(), args.publisher_config))
        return 0
    except (PackageError, OSError, ValueError) as error:
        detail = str(error) if isinstance(error, PackageError) else "native Unix package prerequisites or validation failed"
        print(f"error: {detail}", file=__import__("sys").stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
