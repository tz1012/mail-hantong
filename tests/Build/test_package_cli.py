import copy
import hashlib
import json
from pathlib import Path
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import package_cli as package


def pe(machine=0x8664):
    data = bytearray(256)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 68, machine)
    struct.pack_into("<H", data, 86, 2)
    struct.pack_into("<H", data, 88, 0x20B)
    struct.pack_into("<H", data, 156, 3)
    return bytes(data)


class PackageCliTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.env = {"CARGO_HOME": str(self.root / "cargo-home")}
        self.registry = self.root / "cargo-home/registry/src/local-test"
        self.output = self.root / "output"
        self.target = self.root / "target"
        self.binary_dir = self.target / package.TARGET / "release"
        self.binary_dir.mkdir(parents=True)
        for path in ("LICENSE", "README.md", "docs/latest/CLI-LAUNCH.md",
                     "packaging/windows/install.ps1", "packaging/windows/uninstall.ps1", "packaging/windows/package-common.ps1"):
            source = self.root / path
            source.parent.mkdir(parents=True, exist_ok=True)
            source.write_text(f"Synthetic release input: {path}\n", encoding="utf-8")
        self.packages = []
        for name, binary in package.BINARIES.items():
            manifest = self.root / "apps" / name / "Cargo.toml"
            manifest.parent.mkdir(parents=True)
            manifest.write_text("[package]\n", encoding="utf-8")
            self.packages.append({"id": name, "name": name, "version": "0.1.0", "source": None,
                                  "manifest_path": str(manifest), "targets": [{"name": binary, "kind": ["bin"]}]})
            (self.binary_dir / f"{binary}.exe").write_bytes(pe())
        self.source = "registry+https://github.com/rust-lang/crates.io-index"
        for name, native in (("openssl-src", "openssl/LICENSE.txt"), ("sqlcipher-src", "sqlcipher/LICENSE-sqlcipher")):
            directory = self.registry / f"{name}-1.0.0"
            directory.mkdir(parents=True)
            (directory / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
            (directory / "LICENSE").write_text("Synthetic crate license\n", encoding="utf-8")
            nested = directory / native
            nested.parent.mkdir(parents=True)
            nested.write_text(f"Synthetic upstream notice: {name}\n", encoding="utf-8")
            if name == "sqlcipher-src":
                (directory / "sqlcipher/LICENSE-libtomcrypt").write_text("Synthetic bundled crypto notice\n", encoding="utf-8")
            self.packages.append({"id": name, "name": name, "version": "1.0.0", "source": self.source,
                                  "manifest_path": str(directory / "Cargo.toml"), "license": "MIT", "license_file": None})
        lock = "version = 4\n" + "".join(
            f'[[package]]\nname = "{p["name"]}"\nversion = "1.0.0"\nsource = "{self.source}"\nchecksum = "{"a" * 64}"\n'
            for p in self.packages[2:])
        (self.root / "Cargo.lock").write_text(lock, encoding="utf-8")
        self.metadata = {"packages": self.packages, "workspace_members": list(package.BINARIES),
                         "target_directory": str(self.target), "resolve": {"nodes": [
                             {"id": "hantong-cli", "deps": []},
                             {"id": "mail-hantong-core", "deps": [
                                 {"pkg": "openssl-src", "dep_kinds": [{"kind": "build"}]},
                                 {"pkg": "sqlcipher-src", "dep_kinds": [{"kind": None}]}]},
                             {"id": "openssl-src", "deps": []}, {"id": "sqlcipher-src", "deps": []}]}}
        self.calls = []
        self.bad_version = False
        self.bad_help = False
        self.change_source = False
        patcher = mock.patch.object(package, "build_environment", return_value=self.env)
        patcher.start()
        self.addCleanup(patcher.stop)

    def runner(self, command, **kwargs):
        self.calls.append((command, kwargs))
        output = b""
        if command[:2] == ["cargo", "metadata"]:
            output = json.dumps(self.metadata).encode()
        elif command[0] == "rustc":
            output = b"rustc 1.95.0 (synthetic)\nrelease: 1.95.0\nhost: x86_64-pc-windows-msvc\ncommit-hash: synthetic\n"
        elif command[:2] == ["cargo", "build"] and self.change_source:
            (self.root / "apps/hantong-cli/Cargo.toml").write_text("changed", encoding="utf-8")
        elif command[0] != "cargo":
            binary = Path(command[0]).stem
            self.assertEqual(kwargs["stdin"], package.subprocess.DEVNULL)
            if command[1] == "--version":
                output = f"{binary} {'9.0.0' if self.bad_version else '0.1.0'}\n".encode()
            else:
                output = b"" if self.bad_help else f"{binary}\nUsage: --help\n".encode()
        return SimpleNamespace(returncode=0, stdout=output, stderr=b"")

    def build(self, **kwargs):
        return package.package(self.root, output_dir=self.output, runner=self.runner,
                               host_system="nt", **kwargs)

    def test_INST01_cli_archive_is_reproducible_contains_real_notices_and_excludes_gui_profiles(self):
        (self.binary_dir / "mail-hantong-gui.exe").write_bytes(pe())
        (self.root / ".env").write_text("DO_NOT_PACKAGE", encoding="utf-8")
        first = self.build(skip_build=True)
        snapshot = {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in first.iterdir()}
        self.assertEqual(self.build(skip_build=True), first)
        self.assertEqual(snapshot, {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in first.iterdir()})
        archive = next(first.glob("*.zip"))
        with zipfile.ZipFile(archive) as zipped:
            names = zipped.namelist()
            self.assertEqual(names, sorted(names))
            self.assertEqual([name for name in names if name.endswith(".exe")], ["bin/hantong.exe", "bin/mail-hantong-core.exe"])
            self.assertFalse(any("profile" in name or ".env" in name or "gui.exe" in name for name in names))
            self.assertEqual(zipped.read("LICENSE"), (self.root / "LICENSE").read_bytes())
            self.assertTrue(any(name.endswith("openssl/LICENSE.txt") for name in names))
            self.assertTrue(any(name.endswith("sqlcipher/LICENSE-sqlcipher") for name in names))
            self.assertTrue(any(name.endswith("sqlcipher/LICENSE-libtomcrypt") for name in names))
            self.assertTrue(all(info.date_time == (1980, 1, 1, 0, 0, 0) for info in zipped.infolist()))
            inventory = json.loads(zipped.read("THIRD-PARTY.json"))
            self.assertIn("not performed", inventory["license_review"])
            self.assertEqual({p["name"] for p in inventory["packages"]}, {"openssl-src", "sqlcipher-src"})
            self.assertNotIn(str(self.root).encode(), zipped.read("BUILD-METADATA.json"))
            build = json.loads(zipped.read("BUILD-METADATA.json"))
            self.assertEqual(build["source_hash"], package.source_hash(self.root))
            self.assertEqual(build["source_relation"], "not_verified")
            self.assertEqual(build["rustflags"], package.RUSTFLAGS)
            self.assertEqual(build["rustc"]["release"], "1.95.0")
            manifest = json.loads(zipped.read("release-manifest.json"))
            self.assertEqual(manifest["target"], "windows-x86_64")
            self.assertEqual(manifest["version"], package.DEFAULT_VERSION)
            self.assertEqual({entry["path"] for entry in manifest["files"]}, set(names) - {"release-manifest.json"})
            for entry in manifest["files"]:
                data = zipped.read(entry["path"])
                self.assertEqual(entry["sha256"], package.sha(data))
                self.assertEqual(entry["size"], len(data))
        self.assertFalse(any(command[:2] == ["cargo", "build"] for command, _ in self.calls))
        self.assertEqual(sum(command[0] not in ("cargo", "rustc") for command, _ in self.calls), 8)

    def test_INST01_build_uses_verified_environment_locked_offline_explicit_target_and_smoke(self):
        result = self.build()
        commands = [command for command, _ in self.calls]
        build = next(command for command in commands if command[:2] == ["cargo", "build"])
        self.assertEqual(build, ["cargo", "build", "--release", "--target", package.TARGET,
                                "-p", "hantong-cli", "-p", "mail-hantong-core", "--bins", "--locked", "--offline"])
        self.assertTrue(all(kwargs["env"] == self.env | {"RUSTFLAGS": package.RUSTFLAGS} for _, kwargs in self.calls))
        self.assertNotIn("RUSTFLAGS", self.env)
        self.assertEqual([command[1] for command in commands if command[0] not in ("cargo", "rustc")], ["--help", "--version"] * 2)
        self.assertFalse(any("run" in command for command in commands))
        self.assertEqual(json.loads((result / "BUILD-METADATA.json").read_bytes())["source_relation"], "built_from_current_workspace")

    def test_INST01_source_changes_conflicting_flags_and_missing_native_notice_block_release(self):
        for variable in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS"):
            self.env[variable] = "conflicting"
            with self.subTest(variable=variable), self.assertRaisesRegex(package.PackageError, "static CRT"):
                self.build()
            del self.env[variable]
        self.change_source = True
        with self.assertRaisesRegex(package.PackageError, "sources changed"):
            self.build()
        self.change_source = False
        (self.registry / "sqlcipher-src-1.0.0/sqlcipher/LICENSE-libtomcrypt").unlink()
        with self.assertRaisesRegex(package.PackageError, "required bundled native notice is missing"):
            self.build(skip_build=True)
        self.assertFalse((self.output / package.DEFAULT_VERSION).exists())

    def test_INST01_invalid_versions_targets_architectures_and_smoke_fail_without_release(self):
        for value in ("0.1.0", "../0.1.0-preview.1", "0.1.0-preview.01", "01.1.0-preview.1", "0.1.0-preview.1+meta"):
            with self.subTest(value=value), self.assertRaises(package.PackageError):
                self.build(version=value, skip_build=True)
        original = copy.deepcopy(self.metadata)
        self.metadata["packages"][0]["targets"][0]["name"] = "unexpected"
        with self.assertRaisesRegex(package.PackageError, "target or package version"):
            self.build(skip_build=True)
        self.metadata = original
        (self.binary_dir / "hantong.exe").write_bytes(pe(0xAA64))
        with self.assertRaisesRegex(package.PackageError, "x86_64 console"):
            self.build(skip_build=True)
        (self.binary_dir / "hantong.exe").write_bytes(pe())
        self.bad_version = True
        with self.assertRaisesRegex(package.PackageError, "binary version"):
            self.build(skip_build=True)
        self.bad_version, self.bad_help = False, True
        with self.assertRaisesRegex(package.PackageError, "smoke validation"):
            self.build(skip_build=True)
        (self.binary_dir / "hantong.exe").unlink()
        with self.assertRaisesRegex(package.PackageError, "build without --skip-build"):
            self.build(skip_build=True)
        self.assertFalse((self.output / package.DEFAULT_VERSION).exists())

    def test_INST01_missing_and_traversing_licenses_are_actionable_and_never_publish(self):
        self.metadata["packages"][2]["license_file"] = "../LICENSE"
        (self.registry / "LICENSE").write_text("outside crate", encoding="utf-8")
        with self.assertRaisesRegex(package.PackageError, "outside|escapes"):
            self.build(skip_build=True)
        self.metadata["packages"][2]["license_file"] = None
        directory = self.registry / "openssl-src-1.0.0"
        (directory / "LICENSE").unlink()
        (directory / "openssl/LICENSE.txt").unlink()
        with self.assertRaisesRegex(package.PackageError, "openssl-src 1.0.0: license text is missing"):
            self.build(skip_build=True)
        self.assertFalse((self.output / package.DEFAULT_VERSION).exists())

    def test_INST01_zip_names_and_existing_release_are_never_overwritten_unsafely(self):
        for name in ("../outside", "/absolute", "C:/drive", "folder\\file", "a/../b", "aux", "x/NUL.txt", "x/file.", "file "):
            with self.subTest(name=name), self.assertRaises(package.PackageError):
                package.write_zip(self.root / "bad.zip", {name: b"untrusted"})
        with self.assertRaisesRegex(package.PackageError, "case-insensitive"):
            package.write_zip(self.root / "bad.zip", {"LICENSE": b"one", "license": b"two"})
        destination = self.build(skip_build=True)
        sums = (destination / "SHA256SUMS").read_bytes()
        (self.root / "README.md").write_text("updated", encoding="utf-8")
        with self.assertRaisesRegex(package.PackageError, "different content"):
            self.build(skip_build=True)
        self.assertEqual((destination / "SHA256SUMS").read_bytes(), sums)

    def test_INST01_missing_license_fallback_is_bound_to_registry_revision_and_exact_bytes(self):
        name, version, revision = "missing-license-crate", "1.0.0", "b" * 40
        directory = self.registry / f"{name}-{version}"
        directory.mkdir()
        (directory / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
        vcs_path = directory / ".cargo_vcs_info.json"
        vcs = {"git": {"sha1": revision}, "path_in_vcs": "member"}
        vcs_path.write_text(json.dumps(vcs), encoding="utf-8")
        repository = "https://github.com/example/missing-license-crate"
        package_data = {"id": name, "name": name, "version": version, "source": self.source,
                        "manifest_path": str(directory / "Cargo.toml"), "license": "MIT",
                        "repository": repository, "license_file": None}
        checksum = "a" * 64
        lock = {"package": [{"name": name, "version": version, "source": self.source, "checksum": checksum}]}
        fallback = self.root / "packaging/licenses"
        content = b"Synthetic pinned upstream license\n"
        local = fallback / f"{name}-{version}" / "LICENSE"
        local.parent.mkdir(parents=True)
        local.write_bytes(content)
        index = {"format_version": 1, "packages": [{
            "name": name, "version": version, "revision": revision, "path_in_vcs": "member",
            "registry_source": self.source, "locked_checksum": checksum, "license_expression": "MIT",
            "repository": repository, "files": [{"path": "LICENSE", "upstream_path": "LICENSE",
                "url": f"{repository}/blob/{revision}/LICENSE", "sha256": package.sha(content),
                "git_blob_sha1": hashlib.sha1(f"blob {len(content)}\0".encode() + content).hexdigest()}]}]}
        index_path = fallback / "index.json"
        collect = lambda: package.dependency_licenses([package_data], lock, self.registry.parent, fallback)
        with self.assertRaisesRegex(package.PackageError, "license text is missing"):
            collect()
        index_path.write_bytes(package.json_bytes(index))
        files, inventory = collect()
        self.assertEqual(list(files.values()), [content])
        entry = json.loads(inventory)["packages"][0]
        self.assertEqual(entry["license_source"], "pinned-upstream-fallback")
        self.assertEqual(entry["license_files"][0]["revision"], revision)
        mutations = [
            lambda x: x["packages"][0].update(version="1.0.1"),
            lambda x: x["packages"].append(copy.deepcopy(x["packages"][0])),
            lambda x: x["packages"][0].update(locked_checksum="c" * 64),
            lambda x: x["packages"][0].update(revision="c" * 40),
            lambda x: x["packages"][0].update(license_expression="Apache-2.0"),
            lambda x: x["packages"][0]["files"][0].update(url=f"{repository}/blob/main/LICENSE"),
            lambda x: x["packages"][0]["files"][0].update(sha256="c" * 64),
            lambda x: x["packages"][0]["files"][0].update(git_blob_sha1="c" * 40),
            lambda x: x["packages"][0]["files"][0].update(path="../LICENSE"),
        ]
        for number, mutate in enumerate(mutations):
            changed = copy.deepcopy(index)
            mutate(changed)
            index_path.write_bytes(package.json_bytes(changed))
            with self.subTest(mutation=number), self.assertRaises(package.PackageError):
                collect()
        index_path.write_bytes(package.json_bytes(index))
        local.write_bytes(content + b"tampered")
        with self.assertRaisesRegex(package.PackageError, "content hash"):
            collect()
        local.write_bytes(content)
        vcs["git"]["sha1"] = "c" * 40
        vcs_path.write_text(json.dumps(vcs), encoding="utf-8")
        with self.assertRaisesRegex(package.PackageError, "does not match the locked crate"):
            collect()


if __name__ == "__main__":
    unittest.main()
