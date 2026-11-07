"""Attack-oriented tests for the E9 bootstrap preparation boundary.

All artifacts are synthetic.  No network, installed root, Core, database,
vault, account, or active pointer is used by this module.
"""
import errno
import ast
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import linux_bootstrap_v2 as bootstrap  # noqa: E402


def descriptor(**changes):
    value = {
        "schema_version": 1,
        "version": "0.1.0-test",
        "package_version": "0.1.0-test",
        "target": "linux-x86_64",
        "commit": "a" * 40,
        "source_filename": "mail-hantong-source-0.1.0-test.zip",
        "cargo_lock_sha256": "b" * 64,
        "source_sha256": "c" * 64,
        "source_archive_sha256": "d" * 64,
        "package_filename": "mail-hantong-cli-0.1.0-test-linux-x86_64.tar.gz",
        "package_sha256": "e" * 64,
    }
    value.update(changes)
    return value


def context(**changes):
    value = {
        "protocol_version": 2,
        "target": "linux-x86_64",
        "release_version": "0.1.0-test",
        "source_commit": "a" * 40,
        "canonical_source_sha256": "b" * 64,
        "descriptor_sha256": "c" * 64,
        "manifest_sha256": "d" * 64,
        "asset_relative_path": "bin/mail-hantong-core",
        "asset_size": 1,
        "asset_sha256": "e" * 64,
    }
    value.update(changes)
    return value


class LinuxBootstrapV2Tests(unittest.TestCase):
    def test_SEC03_descriptor_rejects_unknown_and_duplicate_fields(self):
        with self.assertRaisesRegex(bootstrap.BootstrapError, "descriptor_fields"):
            bootstrap.parse_descriptor(json.dumps(descriptor(extra=True)).encode())
        raw = b'{"schema_version":1,"schema_version":1}'
        with self.assertRaisesRegex(bootstrap.BootstrapError, "duplicate"):
            bootstrap.parse_descriptor(raw)

    def test_SEC03_descriptor_rejects_wrong_commit_or_digest(self):
        for changes in ({"commit": "A" * 40}, {"package_sha256": "0" * 63}):
            with self.subTest(changes=changes), self.assertRaises(bootstrap.BootstrapError):
                bootstrap.parse_descriptor(json.dumps(descriptor(**changes)).encode())

    def test_SEC03_context_has_exact_ten_fields_and_rejects_duplicate(self):
        encoded = bootstrap.make_context(
            version="0.1.0-test", commit="a" * 40, source_sha256="b" * 64,
            descriptor_sha256="c" * 64, manifest_sha256="d" * 64,
            asset_relative_path="bin/mail-hantong-core", asset_size=1,
            asset_sha256="e" * 64)
        self.assertEqual(bootstrap.parse_context(encoded), context())
        with self.assertRaisesRegex(bootstrap.BootstrapError, "fields"):
            bootstrap.parse_context(json.dumps(context(extra=True)).encode())
        with self.assertRaisesRegex(bootstrap.BootstrapError, "duplicate"):
            bootstrap.parse_context(b'{"protocol_version":2,"protocol_version":2}')

    def test_SEC03_context_rejects_control_and_wrong_asset(self):
        for changes in ({"asset_relative_path": "bin/evil\x00"},
                        {"asset_relative_path": "bin/other"},
                        {"source_commit": "z" * 40}):
            with self.subTest(changes=changes), self.assertRaises(bootstrap.BootstrapError):
                bootstrap.parse_context(json.dumps(context(**changes)).encode())

    def test_SEC03_fixed_origin_tuple_rejects_redirect_and_wrong_platform(self):
        item = bootstrap.BootstrapTuple(
            "0.1.0-test", "a" * 40, "b" * 64,
            "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/")
        self.assertIn("0.1.0-test", bootstrap.fixed_url(item, "linux-pinned-candidate.json"))
        for bad in (bootstrap.BootstrapTuple(item.version, item.commit, item.descriptor_sha256, "https://evil/"),
                    bootstrap.BootstrapTuple(item.version, item.commit.upper(), item.descriptor_sha256, item.base_url)):
            with self.subTest(bad=bad), self.assertRaises(bootstrap.BootstrapError):
                bootstrap.validate_tuple(bad)
        with self.assertRaises(bootstrap.BootstrapError):
            bootstrap.fixed_url(item, "linux-pinned-candidate.json?redirect=https://evil")

    def test_SEC03_download_clears_environment_and_forbids_redirect_options(self):
        calls = {}
        class Selector:
            def register(self, *_): pass
            def select(self, _): return [True]
            def close(self): pass
        class Child:
            returncode = 0
            stdout = io.BytesIO(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok\n200")
            def communicate(self, timeout):
                calls["timeout"] = timeout
                return b"ok", b""
            def wait(self, timeout=None): pass
            def poll(self): return self.returncode
        def popen(args, **kwargs):
            calls["args"] = args; calls["kwargs"] = kwargs
            return Child()
        url = "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/0.1.0-test/" + "a" * 40 + "/linux-pinned-candidate.json"
        with mock.patch.object(bootstrap, "_safe_regular"), mock.patch.object(bootstrap, "_safe_ancestor"), \
                mock.patch.object(bootstrap.Path, "resolve", return_value=Path("/usr/bin/curl")), \
                mock.patch.object(bootstrap.selectors, "DefaultSelector", return_value=Selector()), \
                mock.patch.object(bootstrap.subprocess, "Popen", side_effect=popen):
            self.assertEqual(bootstrap.fetch_fixed(url), b"ok")
        self.assertEqual(calls["kwargs"]["env"], {})
        self.assertEqual(calls["args"][1], "--disable")
        self.assertNotIn("--location", calls["args"])

    def test_SEC03_download_timeout_is_finite_and_owned_child_is_killed(self):
        class Child:
            returncode = -9
            killed = False
            stdout = io.BytesIO()
            def communicate(self, timeout):
                raise bootstrap.subprocess.TimeoutExpired("curl", timeout)
            def kill(self): self.killed = True
            def wait(self): pass
            def poll(self): return self.returncode
        child = Child()
        url = "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/0.1.0-test/" + "a" * 40 + "/linux-pinned-candidate.json"
        with mock.patch.object(bootstrap, "_safe_regular"), mock.patch.object(bootstrap, "_safe_ancestor"), \
                mock.patch.object(bootstrap.Path, "resolve", return_value=Path("/usr/bin/curl")), \
                mock.patch.object(bootstrap.selectors, "DefaultSelector", return_value=mock.Mock(register=lambda *_: None, select=lambda _: [])), \
                mock.patch.object(bootstrap.time, "monotonic", side_effect=[0, 10]), \
                mock.patch.object(bootstrap.subprocess, "Popen", return_value=child), \
                self.assertRaisesRegex(bootstrap.BootstrapError, "timeout"):
            bootstrap.fetch_fixed(url, timeout=1)
        self.assertTrue(child.killed)

    def test_SEC03_python_prerequisites_reject_path_and_empty_stdlib(self):
        with self.assertRaises(bootstrap.BootstrapError):
            bootstrap.validate_python_prerequisites(Path("/usr/bin/env"), [Path("/usr/lib")])
        with self.assertRaises(bootstrap.BootstrapError):
            bootstrap.validate_python_prerequisites(Path("/usr/bin/python3"), [])

    def test_SEC03_payload_bundles_verifier_without_mutable_project_imports(self):
        item = bootstrap.BootstrapTuple("0.1.0-test", "a" * 40, "b" * 64,
                    "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/")
        payload = bootstrap.assemble_payload((ROOT / "scripts/linux_pinned_candidate.py").read_bytes(),
                    (ROOT / "scripts/linux_bootstrap_v2.py").read_bytes(), item, "/synthetic/absent")
        tree = ast.parse(payload.decode("utf-8"))
        self.assertFalse(any(isinstance(node, ast.ImportFrom) and node.module == "linux_pinned_candidate"
                             for node in ast.walk(tree)))
        digest = hashlib.sha256(payload).hexdigest()
        self.assertEqual(bootstrap.build_payload(payload, digest), payload)
        with self.assertRaisesRegex(bootstrap.BootstrapError, "payload_digest"):
            bootstrap.build_payload(payload + b"\n", digest)

    def test_SEC03_blocked_old_candidate_cannot_be_staged(self):
        with self.assertRaisesRegex(bootstrap.BootstrapError,"stage_readiness_blocked"):
            bootstrap._check_stage_readiness(b'{"linux_release_readiness":"blocked_identity_and_origin"}')
        bootstrap._check_stage_readiness(b'{"linux_release_readiness":"candidate_review_and_native_gates_required"}')

    def test_SEC03_release_tuple_rejects_empty_and_leading_zero_components(self):
        for version in ("0.1.0-..", "0.1.0-a..b", "0.1.0-beta.01", "00.1.0-test", "0.1.0-test/evil"):
            item = bootstrap.BootstrapTuple(version, "a" * 40, "b" * 64,
                       "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/")
            with self.subTest(version=version), self.assertRaises(bootstrap.BootstrapError):
                bootstrap.validate_tuple(item)

    @unittest.skipUnless(sys.platform == "linux" and hasattr(os, "memfd_create") and
                         hasattr(bootstrap, "fcntl") and hasattr(bootstrap.fcntl, "F_ADD_SEALS"),
                         "Linux sealed memfd prerequisites required")
    def test_SEC03_sealed_image_rejects_mutation_and_keeps_digest(self):
        image = bootstrap._sealed_memfd("synthetic", b"synthetic-elf", 1024)
        self.addCleanup(os.close, image.fd)
        self.assertEqual(image.sha256, hashlib.sha256(b"synthetic-elf").hexdigest())
        with self.assertRaises(OSError) as error:
            os.pwrite(image.fd, b"X", 0)
        self.assertIn(error.exception.errno, (errno.EPERM, errno.EACCES))
        self.assertEqual(os.fstat(image.fd).st_size, image.size)

    @unittest.skipUnless(sys.platform == "linux", "Linux held-dir staging required")
    def test_SEC03_stage_refuses_existing_version_without_activation(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory); (root / "versions" / "0.1.0-test").mkdir(parents=True)
            if sys.platform == "linux": os.chmod(root / "versions", 0o700)
            descriptor_path = root / "descriptor"; package = root / "package"; source = root / "source"
            for path in (descriptor_path, package, source): path.write_bytes(b"synthetic")
            report = mock.Mock(version="0.1.0-test", commit="a" * 40,
                               package_sha256=hashlib.sha256(b"synthetic").hexdigest())
            with mock.patch.object(bootstrap, "_safe_ancestor"), \
                    mock.patch.object(bootstrap, "verify_candidate", return_value=report), \
                    self.assertRaisesRegex(bootstrap.BootstrapError, "version_exists"):
                bootstrap.stage_verified_candidate(descriptor_path, package, source, root, report.version, report.commit)
            self.assertTrue((root / "versions" / "0.1.0-test").is_dir())

    @unittest.skipUnless(sys.platform == "linux", "Linux held-dir staging required")
    def test_SEC03_stage_failure_cleans_only_new_private_stage(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory); (root / "versions").mkdir()
            if sys.platform == "linux": os.chmod(root / "versions", 0o700)
            descriptor_path = root / "descriptor"; package = root / "package"; source = root / "source"
            for path in (descriptor_path, package, source): path.write_bytes(b"synthetic")
            report = mock.Mock(version="0.1.0-test", commit="a" * 40,
                               package_sha256=hashlib.sha256(b"synthetic").hexdigest())
            with mock.patch.object(bootstrap, "_safe_ancestor"), \
                    mock.patch.object(bootstrap, "verify_candidate", return_value=report), \
                    mock.patch.object(bootstrap.tarfile, "open", side_effect=OSError("synthetic")), \
                    self.assertRaisesRegex(bootstrap.BootstrapError, "stage"):
                bootstrap.stage_verified_candidate(descriptor_path, package, source, root, report.version, report.commit)
            # Failed staging leaves only the newly-created private stage for
            # explicit recovery; it is never recursively deleted by path.
            self.assertEqual(len(list((root / "versions").iterdir())), 1)

    @unittest.skipUnless(sys.platform == "linux", "Linux no-follow directory open required")
    def test_SEC03_stage_parent_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory); outside = root / "outside"
            (root / "versions").mkdir(); outside.mkdir()
            os.chmod(root / "versions", 0o700)
            os.symlink(outside, root / "versions" / "nested")
            versions_fd = os.open(root / "versions", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            try:
                with self.assertRaises(bootstrap.BootstrapError):
                    bootstrap._open_private_child_dir(versions_fd, "nested")
            finally:
                os.close(versions_fd)

    @unittest.skipUnless(sys.platform == "linux", "Linux FD role preparation required")
    def test_SEC03_prepare_requires_root_not_link_and_noexec_capability(self):
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-test-") as directory:
            root = Path(directory); (root / "versions").mkdir()
            with mock.patch.object(bootstrap, "_safe_ancestor"), \
                    mock.patch.object(bootstrap.os, "memfd_create", side_effect=OSError("noexec"), create=True), \
                    self.assertRaises(bootstrap.BootstrapError):
                bootstrap.prepare_execution_roles(root, b"image", b"context", b"manifest", b"descriptor")

    def test_SEC03_role_map_rejects_invalid_source_before_mutation(self):
        roles = {bootstrap.ROLE_ROOT: -1, bootstrap.ROLE_IMAGE: -1,
                 bootstrap.ROLE_CONTEXT: -1, bootstrap.ROLE_MANIFEST: -1,
                 bootstrap.ROLE_DESCRIPTOR: -1}
        with self.assertRaisesRegex(bootstrap.BootstrapError, "role_(source|dup_unsupported)"):
            bootstrap.map_roles_for_exec(roles)

    @unittest.skipUnless(sys.platform == "linux", "Linux ownership guard required")
    def test_SEC03_pin_requires_managed_install_marker(self):
        raw = json.dumps(descriptor()).encode()
        item = bootstrap.BootstrapTuple("0.1.0-test", "a" * 40, hashlib.sha256(raw).hexdigest(),
                    "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/")
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-pin-") as directory:
            root = Path(directory)
            with self.assertRaisesRegex(bootstrap.BootstrapError, "pin_owner"):
                bootstrap.persist_pin(root, item, raw)

    @unittest.skipUnless(sys.platform == "linux", "Linux ownership guard required")
    def test_SEC03_pin_rejects_owner_marker_symlink(self):
        raw = json.dumps(descriptor()).encode()
        item = bootstrap.BootstrapTuple("0.1.0-test", "a" * 40, hashlib.sha256(raw).hexdigest(),
                    "https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/")
        with tempfile.TemporaryDirectory(prefix="hantong-bootstrap-pin-") as directory:
            root = Path(directory)
            marker_target = root / "marker-target"; marker_target.write_bytes(b"mail-hantong-cli-v1\n")
            root.joinpath(".mail-hantong-owner").symlink_to(marker_target)
            with self.assertRaisesRegex(bootstrap.BootstrapError, "pin_owner"):
                bootstrap.persist_pin(root, item, raw)


if __name__ == "__main__":
    unittest.main()
