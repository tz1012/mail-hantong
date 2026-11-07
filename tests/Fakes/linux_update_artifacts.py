#!/usr/bin/env python3
"""Create a completely synthetic pinned Linux update fixture.

This generator uses only the Python standard library and never contacts a
service.  It intentionally emits fake ELF headers and no executable payload.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import pathlib
import tarfile
import zipfile
import gzip

VERSION = "0.1.0-beta.29"
PACKAGE_VERSION = VERSION
TARGET = "linux-x86_64"
COMMIT = "a" * 40
SUFFIXES = {".rs", ".py", ".toml", ".txt", ".eml", ".mbox", ".ps1", ".sh", ".cert", ".fullchain", ".key", ".manifest", ".html"}
FIXED = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "install.ps1", "install.sh", "tests/Corpus/index.json", "scripts/native-tools.json", "scripts/check-all", "scripts/traceability", "scripts/test-mail-server", "packaging/licenses/index.json"}


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(members: dict[str, bytes]) -> str:
    selected = []
    for name in sorted(members):
        parts = pathlib.PurePosixPath(name).parts
        include = name in FIXED
        include |= bool(parts and parts[0] in {"crates", "apps", "tests", "scripts", "packaging"} and pathlib.PurePosixPath(name).suffix.lower() in SUFFIXES)
        include |= name.startswith("docs/contracts/") and pathlib.PurePosixPath(name).suffix.lower() in {".md", ".json", ".sql"}
        if include:
            selected.append(name)
    out = bytearray(b"mail-hantong.source.lf.v2\0")
    for name in sorted(selected):
        data = members[name] if name.startswith("tests/Corpus/") else members[name].replace(b"\r\n", b"\n")
        out += len(name.encode()).to_bytes(4, "big") + name.encode()
        out += len(data).to_bytes(8, "big") + data
    return sha(bytes(out))


def zip_bytes(members: dict[str, bytes]) -> bytes:
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for name in sorted(members):
            info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            zf.writestr(info, members[name])
    return stream.getvalue()


def tar_gzip(members: dict[str, bytes]) -> bytes:
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as tf:
        for name in sorted(members):
            info = tarfile.TarInfo(name)
            info.size = len(members[name]); info.mode = 0o755 if name.startswith("bin/") else 0o644; info.uid = info.gid = 0; info.mtime = 0
            tf.addfile(info, io.BytesIO(members[name]))
    return gzip.compress(raw.getvalue(), mtime=0)


def fake_elf(name: str) -> bytes:
    data = bytearray(128)
    data[:6] = b"\x7fELF\x02\x01"
    data[16:18] = (2).to_bytes(2, "little")
    data[18:20] = (62).to_bytes(2, "little")
    data[64:64 + len(name)] = name.encode()
    return bytes(data)


def build(args: argparse.Namespace) -> None:
    out = pathlib.Path(args.output); out.mkdir(parents=True, exist_ok=True)
    source_members = {"SOURCE-COMMIT": (COMMIT + "\n").encode(), "Cargo.lock": b"# synthetic lock\n", "Cargo.toml": b"[workspace]\nmembers=[]\n", "crates/example.rs": b"fn main() {}\n", "docs/contracts/example.md": b"fixture\n"}
    if args.crlf_source: source_members["crates/example.rs"] = b"fn main() {}\r\n"
    source = zip_bytes(source_members)
    source_hash = canonical(source_members)
    lock_hash = sha(source_members["Cargo.lock"])
    binaries = {f"bin/{name}": fake_elf(name) for name in ("hantong", "hantong-t", "hantong-mcp", "mail-hantong-core")}
    metadata = {"format_version": 1, "release_label": VERSION, "target": "x86_64-unknown-linux-gnu", "source_hash": source_hash, "build_performed": True, "source_relation": "built_from_current_workspace", "source_commit": COMMIT, "cargo_lock_sha256": lock_hash, "candidate_review_required": True, "binaries": {k: sha(v) for k, v in binaries.items()}}
    if args.bad_metadata: metadata["source_relation"] = "unknown"
    if args.bad_binary: binaries["bin/hantong"] = b"not-an-elf"
    files = {"VERSION": (VERSION + "\n").encode(), **binaries, "BUILD-METADATA.json": json.dumps(metadata, separators=(",", ":"), sort_keys=True).encode()}
    files["installed-files.txt"] = ("\n".join(sorted([*files, "FILES.sha256", "installed-files.txt", "release-manifest.json"])) + "\n").encode()
    manifest_rows = [{"path": n, "sha256": sha(v), "size": len(v)} for n, v in sorted(files.items())]
    files["release-manifest.json"] = json.dumps({"format_version": 1, "version": VERSION, "package_version": PACKAGE_VERSION, "target": TARGET, "files": manifest_rows}, separators=(",", ":"), sort_keys=True).encode()
    sums = {n: sha(v) for n, v in {**files}.items()}
    files["FILES.sha256"] = "".join(f"{h}  {n}\n" for n, h in sorted({**sums, "release-manifest.json": sha(files["release-manifest.json"])}.items())).encode()
    if args.extra_file: files["extra"] = b"unexpected"
    if args.duplicate_inventory: files["installed-files.txt"] += files["VERSION"]
    package = tar_gzip(files)
    descriptor = {"schema_version": 1, "version": VERSION, "package_version": PACKAGE_VERSION, "target": TARGET, "commit": COMMIT, "source_filename": f"mail-hantong-source-{VERSION}.zip", "cargo_lock_sha256": lock_hash, "source_sha256": source_hash, "source_archive_sha256": sha(source), "package_filename": f"mail-hantong-cli-{VERSION}-{TARGET}.tar.gz", "package_sha256": sha(package)}
    descriptor_bytes = json.dumps(descriptor, separators=(",", ":"), sort_keys=True).encode()
    if args.tamper_descriptor:
        altered = dict(descriptor); altered["target"] = "linux-arm64"
        descriptor_bytes = json.dumps(altered, separators=(",", ":"), sort_keys=True).encode()
    if args.tamper_package: package = package[:-1] + bytes([package[-1] ^ 1])
    if args.tamper_source: source = source[:-1] + bytes([source[-1] ^ 1])
    (out / "descriptor.json").write_bytes(descriptor_bytes); (out / descriptor["source_filename"]).write_bytes(source); (out / descriptor["package_filename"]).write_bytes(package)
    (out / "expected-descriptor-sha256").write_text(sha(descriptor_bytes) + "\n", encoding="ascii")


def main() -> None:
    p = argparse.ArgumentParser(); p.add_argument("output", type=pathlib.Path)
    for name in ("tamper_descriptor", "tamper_package", "tamper_source", "duplicate_inventory", "extra_file", "bad_metadata", "bad_binary", "crlf_source"):
        p.add_argument(f"--{name.replace('_', '-')}", action="store_true")
    build(p.parse_args())


if __name__ == "__main__": main()
