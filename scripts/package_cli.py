#!/usr/bin/env python3
"""Package the Windows x64 CLI developer preview, never user data or profiles."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import struct
import subprocess
import sys
import tempfile
import tomllib
import zipfile

from cargo_native import build_environment
from traceability import source_hash, ValidationError

ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-msvc"
DEFAULT_VERSION = "0.1.0-preview.2"
BINARIES = {"hantong-cli": "hantong", "mail-hantong-core": "mail-hantong-core"}
LICENSE_NAME = re.compile(r"^(?:licen[sc]e|copying|copyright|notice)(?:[._-].*)?$", re.I)
MAX_INPUT = 256 * 1024 * 1024
RUSTFLAGS = "-C target-feature=+crt-static"
NATIVE_NOTICES = {"openssl-src": ("openssl/LICENSE.txt",),
                  "sqlcipher-src": ("sqlcipher/LICENSE-sqlcipher", "sqlcipher/LICENSE-libtomcrypt")}


class PackageError(ValueError):
    """A packaging prerequisite failed; do not publish a partial artifact."""


def release_version(value: str) -> str:
    number = r"(?:0|[1-9][0-9]*)"
    if len(value) > 64 or not re.fullmatch(rf"{number}\.{number}\.{number}-[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*", value):
        raise PackageError("--version must be a prerelease SemVer, for example 0.1.0-preview.1")
    if any(part.isdigit() and len(part) > 1 and part.startswith("0") for part in value.split("-", 1)[1].split(".")):
        raise PackageError("numeric prerelease identifiers cannot have leading zeroes")
    return value


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def checked_source_hash(root: Path) -> str:
    try:
        return source_hash(root)
    except ValidationError as error:
        raise PackageError(str(error)) from None


def archive_name(value: str) -> str:
    path = PurePosixPath(value)
    if (not value or path.is_absolute() or "\\" in value or ":" in value
            or any(part in ("", ".", "..") for part in value.split("/"))
            or any(ord(c) < 32 or c in '<>"|?*' for c in value)
            or any(part.endswith((".", " ")) or re.fullmatch(r"(?:con|prn|aux|nul|clock\$|com[1-9]|lpt[1-9])(?:\..*)?", part, re.I)
                   for part in value.split("/"))):
        raise PackageError("unsafe package entry name")
    return value


def read_file(path: Path, boundary: Path) -> bytes:
    boundary = boundary.resolve()
    try:
        resolved = path.resolve(strict=True)
        relative = path.absolute().relative_to(boundary)
    except (OSError, ValueError):
        raise PackageError("required package input is missing or outside its source directory") from None
    if not resolved.is_relative_to(boundary):
        raise PackageError("package input escapes its source directory")
    cursor = boundary
    for component in relative.parts:
        cursor /= component
        if cursor.is_symlink() or getattr(cursor, "is_junction", lambda: False)():
            raise PackageError("linked package inputs are not supported")
    if not resolved.is_file() or resolved.stat().st_size > MAX_INPUT:
        raise PackageError("package input must be a bounded regular file")
    return resolved.read_bytes()


def run_command(runner, command: list[str], root: Path, env: dict, *, capture: bool, timeout: int):
    try:
        result = runner(command, cwd=root, env=env, stdin=subprocess.DEVNULL,
                        capture_output=capture, check=False, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        raise PackageError(f"unable to complete {Path(command[0]).name} within its time limit") from None
    if result.returncode:
        raise PackageError(f"{Path(command[0]).name} failed; no release was published")
    return result


def compiler_info(runner, root: Path, env: dict) -> dict | None:
    """Optional bounded provenance; never include paths or arbitrary command output."""
    try:
        result = run_command(runner, ["rustc", "-Vv"], root, env, capture=True, timeout=15)
        if len(result.stdout) > 8192 or result.stderr:
            return None
        fields = {}
        allowed = {"release", "host", "commit-hash", "commit-date", "LLVM version"}
        for line in result.stdout.decode("ascii").splitlines():
            key, separator, value = line.partition(": ")
            if separator and key in allowed and re.fullmatch(r"[A-Za-z0-9.+() _-]{1,160}", value):
                fields[key] = value
        return fields or None
    except (PackageError, UnicodeError):
        return None


def check_pe(data: bytes) -> None:
    if len(data) < 128 or data[:2] != b"MZ":
        raise PackageError("release binary is not a Windows PE executable")
    offset = struct.unpack_from("<I", data, 0x3C)[0]
    if offset > len(data) - 96 or data[offset:offset + 4] != b"PE\0\0":
        raise PackageError("release binary has an invalid PE header")
    machine = struct.unpack_from("<H", data, offset + 4)[0]
    flags = struct.unpack_from("<H", data, offset + 22)[0]
    magic = struct.unpack_from("<H", data, offset + 24)[0]
    subsystem = struct.unpack_from("<H", data, offset + 24 + 68)[0]
    if machine != 0x8664 or magic != 0x20B or subsystem != 3 or not flags & 2 or flags & 0x2000:
        raise PackageError("release binaries must be Windows x86_64 console executables")


def selected_packages(metadata: dict, root: Path, version: str) -> tuple[list[dict], dict[str, dict]]:
    packages = {p["id"]: p for p in metadata["packages"]}
    members = set(metadata["workspace_members"])
    roots = {}
    for name, binary in BINARIES.items():
        matches = [p for ident, p in packages.items() if ident in members and p["name"] == name]
        if len(matches) != 1:
            raise PackageError(f"expected exactly one workspace package: {name}")
        package = matches[0]
        bins = [t["name"] for t in package["targets"] if "bin" in t["kind"]]
        if bins != [binary] or package["version"] != version.split("-", 1)[0]:
            raise PackageError(f"{name} binary target or package version does not match the release")
        read_file(Path(package["manifest_path"]), root)
        roots[name] = package
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [p["id"] for p in roots.values()]
    reached = set()
    while pending:
        ident = pending.pop()
        if ident in reached:
            continue
        reached.add(ident)
        if ident not in packages or ident not in nodes:
            raise PackageError("Cargo dependency graph is incomplete")
        for dep in nodes[ident]["deps"]:
            if any(kind["kind"] in (None, "normal", "build") for kind in dep["dep_kinds"]):
                pending.append(dep["pkg"])
    for ident in reached & members:
        read_file(Path(packages[ident]["manifest_path"]), root)
    if any(packages[ident]["source"] is None and ident not in members for ident in reached):
        raise PackageError("external path dependencies require a reviewed licensing collection rule")
    return [packages[ident] for ident in reached if ident not in members], roots


def upstream_licenses(package: dict, checksum: str, directory: Path, fallback_root: Path | None) -> list[tuple[str, bytes, dict]]:
    """Use only an explicitly indexed copy bound to the exact locked source revision."""
    name, version = package["name"], package["version"]
    if fallback_root is None or not (fallback_root / "index.json").exists():
        raise PackageError(f"{name} {version}: license text is missing; restore the locked crate source or add reviewed upstream license material")
    try:
        raw = read_file(fallback_root / "index.json", fallback_root)
        if len(raw) > 65536:
            raise PackageError("upstream license index exceeds its size bound")
        index = json.loads(raw)
        if type(index["format_version"]) is not int or index["format_version"] != 1 or not isinstance(index["packages"], list):
            raise PackageError("unsupported upstream license index")
        matches = [entry for entry in index["packages"] if (entry["name"], entry["version"]) == (name, version)]
        if len(matches) != 1:
            raise PackageError(f"{name} {version}: missing license requires one exact provenance index entry")
        entry = matches[0]
        revision = entry["revision"]
        repository = entry["repository"]
        vcs = json.loads(read_file(directory / ".cargo_vcs_info.json", directory))
        if (entry["registry_source"] != package["source"] or entry["locked_checksum"] != checksum
                or entry["license_expression"] != package.get("license") or repository != package.get("repository")
                or not re.fullmatch(r"https://github.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository)
                or not re.fullmatch(r"[0-9a-f]{40}", revision) or vcs["git"]["sha1"] != revision
                or vcs["git"].get("dirty", False) is not False or vcs["path_in_vcs"] != entry["path_in_vcs"]):
            raise PackageError(f"{name} {version}: upstream license provenance does not match the locked crate")
        if not isinstance(entry["files"], list) or not 1 <= len(entry["files"]) <= 8:
            raise PackageError("upstream license file list is invalid")
        result, seen = [], set()
        for item in entry["files"]:
            local = archive_name(item["path"])
            upstream = archive_name(item["upstream_path"])
            if "/" in local or not LICENSE_NAME.fullmatch(local) or local.casefold() in seen:
                raise PackageError("upstream license filename is invalid or duplicated")
            seen.add(local.casefold())
            expected_url = f"{repository}/blob/{revision}/{upstream}"
            if item["url"] != expected_url:
                raise PackageError("upstream license URL is not bound to its source revision")
            data = read_file(fallback_root / f"{name}-{version}" / local, fallback_root)
            blob_sha = hashlib.sha1(f"blob {len(data)}\0".encode() + data, usedforsecurity=False).hexdigest()
            if sha(data) != item["sha256"] or blob_sha != item["git_blob_sha1"]:
                raise PackageError(f"{name} {version}: upstream license content hash does not match its index")
            result.append((local, data, {"source_url": expected_url, "revision": revision, "git_blob_sha1": blob_sha}))
        return result
    except (KeyError, TypeError, json.JSONDecodeError, UnicodeError):
        raise PackageError(f"{name} {version}: malformed upstream license provenance") from None


def dependency_licenses(packages: list[dict], lock: dict, registry: Path,
                        fallback_root: Path | None = None) -> tuple[dict[str, bytes], bytes]:
    checksums = {(p["name"], p["version"], p.get("source")): p.get("checksum") for p in lock["package"]}
    files = {}
    entries = []
    for package in sorted(packages, key=lambda p: (p["name"], p["version"], p["id"])):
        name, version, source = package["name"], package["version"], package["source"]
        if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or not re.fullmatch(r"[A-Za-z0-9.+-]+", version):
            raise PackageError("unsafe dependency identity")
        if not source or not source.startswith("registry+"):
            raise PackageError(f"{name} {version}: only locally available registry licenses are supported")
        checksum = checksums.get((name, version, source))
        if not checksum or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise PackageError(f"{name} {version}: locked registry checksum is missing")
        manifest = Path(package["manifest_path"])
        read_file(manifest, registry)
        directory = manifest.parent.resolve()
        if directory.name != f"{name}-{version}":
            raise PackageError(f"{name} {version}: registry source directory does not match metadata")
        declared = package.get("license_file")
        if declared:
            if not LICENSE_NAME.fullmatch(Path(declared).name):
                raise PackageError(f"{name} {version}: unusual license-file needs manual review")
            read_file(directory / declared, directory)
        candidates = []
        visited = 0
        for current, directories, filenames in os.walk(directory, followlinks=False):
            directories.sort()
            for subdir in directories:
                path = Path(current) / subdir
                if path.is_symlink() or getattr(path, "is_junction", lambda: False)():
                    raise PackageError(f"{name} {version}: linked dependency source needs review")
            for filename in sorted(filenames):
                visited += 1
                if visited > 100_000:
                    raise PackageError(f"{name} {version}: source inventory exceeds the packaging bound")
                if LICENSE_NAME.fullmatch(filename):
                    candidates.append(Path(current) / filename)
        material = ([(path.relative_to(directory).as_posix(), read_file(path, directory), {}) for path in candidates]
                    if candidates else upstream_licenses(package, checksum, directory, fallback_root))
        relative_notices = {path for path, _, _ in material}
        for required in NATIVE_NOTICES.get(name, ()):
            if required not in relative_notices:
                raise PackageError(f"{name} {version}: required bundled native notice is missing: {required}")
        copied = []
        prefix = f"licenses/{name}-{version}-{sha(source.encode())[:8]}"
        for path, content, provenance in material:
            if not content.strip():
                raise PackageError(f"{name} {version}: empty license/notice text needs review")
            entry = archive_name(f"{prefix}/{path}")
            files[entry] = content
            copied.append({"path": entry, "sha256": sha(content), **provenance})
        entries.append({"name": name, "version": version, "source": "cargo-registry",
                        "locked_checksum": checksum, "license_expression": package.get("license"),
                        "license_source": "registry-source" if candidates else "pinned-upstream-fallback",
                        "license_files": copied})
    inventory = {"scope": "Cargo metadata normal/build dependency closure for CLI and Core; may include build-time and conservatively resolved dependencies",
                 "license_review": "not performed; SPDX completeness and bundled-native-component coverage are not asserted",
                 "packages": entries}
    return files, json_bytes(inventory)


def write_zip(path: Path, contents: dict[str, bytes]) -> None:
    folded = set()
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, data in sorted(contents.items()):
            archive_name(name)
            if name.casefold() in folded:
                raise PackageError("package entries collide on a case-insensitive filesystem")
            folded.add(name.casefold())
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, data)


def publish(staged: Path, destination: Path) -> None:
    if destination.exists():
        if destination.is_symlink() or getattr(destination, "is_junction", lambda: False)() or not destination.is_dir():
            raise PackageError("output already exists and is not an identical release")
        names = {p.name for p in staged.iterdir()}
        if {p.name for p in destination.iterdir()} != names or any(
                read_file(destination / name, destination) != (staged / name).read_bytes() for name in names):
            raise PackageError("output already exists with different content; choose a new prerelease version")
        return
    try:
        staged.rename(destination)
    except OSError:
        raise PackageError("unable to publish release atomically; existing output was not replaced") from None


def package(root: Path = ROOT, *, version: str = DEFAULT_VERSION, output_dir: Path | None = None,
            skip_build: bool = False, runner=None, host_system: str | None = None) -> Path:
    version = release_version(version)
    if (os.name if host_system is None else host_system) != "nt":
        raise PackageError("this release packager requires Windows and the MSVC x86_64 toolchain")
    root = root.resolve()
    runner = subprocess.run if runner is None else runner
    contents = {}
    required = {"LICENSE": "LICENSE", "README.md": "README.md", "CLI-LAUNCH.md": "docs/latest/CLI-LAUNCH.md",
                "install.ps1": "packaging/windows/install.ps1", "uninstall.ps1": "packaging/windows/uninstall.ps1",
                "package-common.ps1": "packaging/windows/package-common.ps1"}
    for destination, source in required.items():
        try:
            contents[destination] = read_file(root / source, root)
        except PackageError:
            raise PackageError(f"required release document is unavailable: {source}") from None
    lock_bytes = read_file(root / "Cargo.lock", root)
    try:
        env = dict(build_environment(root))
    except (OSError, ValueError, KeyError):
        raise PackageError("verified native tools are missing; run python scripts/prepare_native_tools.py") from None
    for variable in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS"):
        if env.get(variable):
            raise PackageError(f"{variable} conflicts with the package's static CRT build; clear it for this invocation")
    env["RUSTFLAGS"] = RUSTFLAGS
    source_before = checked_source_hash(root)
    result = run_command(runner, ["cargo", "metadata", "--format-version", "1", "--locked", "--offline", "--filter-platform", TARGET], root, env, capture=True, timeout=120)
    try:
        metadata = json.loads(result.stdout)
        dependencies, roots = selected_packages(metadata, root, version)
        registry = Path(env.get("CARGO_HOME", Path.home() / ".cargo")) / "registry/src"
        licenses, inventory = dependency_licenses(dependencies, tomllib.loads(lock_bytes.decode("utf-8")), registry,
                                                 root / "packaging/licenses")
    except (KeyError, TypeError, json.JSONDecodeError, tomllib.TOMLDecodeError, UnicodeError):
        raise PackageError("Cargo metadata or lockfile is malformed") from None
    contents.update(licenses)
    contents["THIRD-PARTY.json"] = inventory
    contents["THIRD-PARTY-NOTICES.md"] = (
        "# Third-party source notices\n\nTHIRD-PARTY.json records the locally resolved CLI/Core normal and build dependency closure. "
        "License and notice files are copied verbatim from those locked registry sources, including matching nested files. "
        "Explicitly indexed missing texts may come from their exact upstream source revision; their source URLs, revisions, and content hashes appear in the inventory. "
        "This conservative inventory is not legal clearance, an SPDX completeness claim, or proof that every bundled native component's obligations are satisfied. "
        "Review upstream terms before redistribution. Workspace code is covered by LICENSE.\n").encode()
    command = ["cargo", "build", "--release", "--target", TARGET, "-p", "hantong-cli", "-p", "mail-hantong-core", "--bins", "--locked", "--offline"]
    if not skip_build:
        run_command(runner, command, root, env, capture=False, timeout=1800)
    compiler = compiler_info(runner, root, env)
    if checked_source_hash(root) != source_before:
        raise PackageError("workspace sources changed during packaging; retry from a stable workspace")
    target_dir = Path(metadata["target_directory"])
    binary_dir = target_dir / TARGET / "release"
    output_dir = (root / "artifacts/cli" if output_dir is None else output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".package-cli-", dir=output_dir) as temporary:
        temporary = Path(temporary)
        smoke_dir = temporary / "smoke"
        smoke_dir.mkdir()
        hashes = {}
        for name, binary in BINARIES.items():
            try:
                data = read_file(binary_dir / f"{binary}.exe", target_dir)
            except PackageError:
                raise PackageError(f"release binary is missing or unsafe: {TARGET}/release/{binary}.exe; build without --skip-build") from None
            check_pe(data)
            executable = smoke_dir / f"{binary}.exe"
            executable.write_bytes(data)
            for flag in ("--help", "--version"):
                smoke = run_command(runner, [str(executable), flag], smoke_dir, env, capture=True, timeout=15)
                output = smoke.stdout.decode("utf-8", "strict")
                if smoke.stderr or len(smoke.stdout) > 131072 or binary not in output:
                    raise PackageError(f"{binary} {flag} smoke validation failed")
                if flag == "--version" and output.strip() != f"{binary} {roots[name]['version']}":
                    raise PackageError(f"{binary} binary version does not match Cargo metadata")
            if executable.read_bytes() != data:
                raise PackageError("smoke validation modified an executable")
            contents[f"bin/{binary}.exe"] = data
            hashes[f"bin/{binary}.exe"] = sha(data)
        build = json_bytes({"format_version": 1, "release_label": version, "package_version": roots["hantong-cli"]["version"],
                            "target": TARGET, "channel": "developer-preview", "build_performed": not skip_build,
                            "build_command": command, "rustflags": RUSTFLAGS, "rustc": compiler,
                            "source_hash": source_before,
                            "source_relation": "not_verified" if skip_build else "built_from_current_workspace",
                            "cargo_lock_sha256": sha(lock_bytes), "binaries": hashes,
                            "third_party_inventory_sha256": sha(inventory), "smoke": ["--help", "--version"],
                            "archive_format": "deterministic uncompressed ZIP; fixed timestamps; no user profiles"})
        contents["BUILD-METADATA.json"] = build
        contents["release-manifest.json"] = json_bytes({
            "format_version": 1, "version": version, "package_version": roots["hantong-cli"]["version"],
            "target": "windows-x86_64",
            "files": [{"path": name, "sha256": sha(data), "size": len(data)} for name, data in sorted(contents.items())]})
        release = temporary / "release"
        release.mkdir()
        filename = f"mail-hantong-cli-{version}-windows-x86_64.zip"
        write_zip(release / filename, contents)
        (release / "BUILD-METADATA.json").write_bytes(build)
        sums = "".join(f"{sha((release / name).read_bytes())}  {name}\n" for name in (filename, "BUILD-METADATA.json"))
        (release / "SHA256SUMS").write_text(sums, encoding="utf-8", newline="\n")
        if checked_source_hash(root) != source_before or any(
                read_file(root / source, root) != contents[name] for name, source in required.items()):
            raise PackageError("workspace sources or release documents changed during packaging; retry from a stable workspace")
        destination = output_dir / version
        publish(release, destination)
    return destination


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", default=DEFAULT_VERSION)
    parser.add_argument("--skip-build", action="store_true", help="reuse existing target binaries only after PE/help/version validation")
    parser.add_argument("--output-dir", type=Path, help="output root; the release-version subdirectory is created atomically")
    args = parser.parse_args(argv)
    try:
        result = package(version=args.version, output_dir=args.output_dir, skip_build=args.skip_build)
    except (PackageError, OSError, UnicodeError) as error:
        print(f"error: {error if isinstance(error, PackageError) else 'unable to read or write a required packaging input'}", file=sys.stderr)
        return 2
    print(f"Created CLI developer preview: {result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
