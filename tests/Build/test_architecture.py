import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import check_architecture


def metadata(edges):
    names = {
        "hantong-contracts", "hantong-platform", "hantong-domain", "hantong-policy", "hantong-approval",
        "hantong-application", "hantong-outbox", "hantong-ipc-client", "hantong-ipc-server",
        "hantong-store", "hantong-secrets", "hantong-adapters", "hantong-cli", "mail-hantong-core",
        "hantong-approval-windows",
    }
    packages = []
    for name in names:
        deps = []
        for source, target, kind, alias in edges:
            if source == name:
                dep = {"name": alias or target, "package": target, "kind": kind}
                deps.append(dep)
        packages.append({"name": name, "id": f"path+file:///x/{name}#0.1.0", "dependencies": deps})
    return {"packages": packages, "workspace_members": [p["id"] for p in packages]}


class ArchitectureTests(unittest.TestCase):
    def test_INST04_only_core_may_reach_native_approval_issuer(self):
        self.assertEqual(check_architecture.validate_metadata(metadata([
            ("mail-hantong-core", "hantong-approval-windows", "normal", None),
            ("hantong-approval-windows", "hantong-approval", "normal", None),
            ("hantong-approval-windows", "hantong-platform", "normal", None),
        ])), [])
        for caller in ("hantong-cli", "hantong-ipc-client", "hantong-platform", "hantong-store", "hantong-approval"):
            errors = check_architecture.validate_metadata(metadata([
                (caller, "hantong-approval-windows", "normal", None),
                ("hantong-cli", "hantong-ipc-client", "normal", None),
                ("hantong-ipc-client", "hantong-platform", "normal", None),
            ]))
            self.assertTrue(any("forbidden dependency" in error and caller in error for error in errors))
            if caller in ("hantong-cli", "hantong-ipc-client", "hantong-platform"):
                self.assertTrue(any("transitive client dependency" in error and "hantong-approval-windows" in error for error in errors))

    def test_M003_accepts_declared_scaffold_graph(self):
        self.assertEqual(check_architecture.validate_metadata(metadata([])), [])

    def test_M003_rejects_direct_forbidden_cli_dependency(self):
        errors = check_architecture.validate_metadata(metadata([("hantong-cli", "hantong-store", "normal", None)]))
        self.assertTrue(any("hantong-cli" in error and "hantong-store" in error for error in errors))

    def test_M003_rejects_transitive_client_dependency(self):
        errors = check_architecture.validate_metadata(metadata([
            ("hantong-cli", "hantong-ipc-client", "normal", None),
            ("hantong-ipc-client", "hantong-secrets", "normal", None),
        ]))
        self.assertTrue(any("transitive" in error and "hantong-secrets" in error for error in errors))

    def test_M003_allows_store_to_use_secrets_key_boundary(self):
        errors = check_architecture.validate_metadata(metadata([
            ("hantong-store", "hantong-secrets", "normal", None),
        ]))
        self.assertEqual(errors, [])

    def test_M003_allows_store_hashing_without_platform_reaching_storage(self):
        errors = check_architecture.validate_metadata(metadata([
            ("hantong-store", "hantong-platform", "normal", None),
            ("hantong-platform", "hantong-contracts", "normal", None),
            ("hantong-cli", "hantong-ipc-client", "normal", None),
            ("hantong-ipc-client", "hantong-platform", "normal", None),
        ]))
        self.assertEqual(errors, [])
        errors = check_architecture.validate_metadata(metadata([
            ("hantong-cli", "hantong-ipc-client", "normal", None),
            ("hantong-ipc-client", "hantong-platform", "normal", None),
            ("hantong-platform", "hantong-store", "normal", None),
        ]))
        self.assertTrue(any("transitive" in error and "hantong-store" in error for error in errors))

    def test_M003_secret_binding_uses_pure_domain_without_reverse_access(self):
        self.assertEqual(check_architecture.validate_metadata(metadata([
            ("hantong-secrets", "hantong-domain", "normal", None),
        ])), [])
        errors = check_architecture.validate_metadata(metadata([
            ("hantong-domain", "hantong-secrets", "normal", None),
        ]))
        self.assertTrue(any("forbidden dependency" in error for error in errors))

    def test_M003_still_blocks_cli_direct_store_dependency(self):
        errors = check_architecture.validate_metadata(metadata([
            ("hantong-cli", "hantong-store", "normal", "store_api"),
        ]))
        self.assertTrue(any("hantong-cli" in error and "hantong-store" in error for error in errors))

    def test_M003_rejects_alias_dependency(self):
        data = metadata([])
        cli = next(p for p in data["packages"] if p["name"] == "hantong-cli")
        cli["dependencies"].append({"name": "hantong-store", "rename": "innocent", "kind": None})
        errors = check_architecture.validate_metadata(data)
        self.assertTrue(any("hantong-cli" in error and "hantong-store" in error for error in errors))

    def test_M003_rejects_dev_dependency_leak(self):
        errors = check_architecture.validate_metadata(metadata([
            ("hantong-cli", "hantong-store", "dev", None),
        ]))
        self.assertTrue(any("dev-dependency" in error for error in errors))

    def test_M003_rejects_unknown_package_and_bad_edge(self):
        data = metadata([])
        data["packages"].append({"name": "unmapped", "id": "path+file:///x/unmapped#0.1.0", "dependencies": []})
        data["workspace_members"].append(data["packages"][-1]["id"])
        errors = check_architecture.validate_metadata(data)
        self.assertTrue(any("unknown workspace package" in error for error in errors))

    def test_M003_allows_alias_for_allowed_target(self):
        data = metadata([])
        policy = next(p for p in data["packages"] if p["name"] == "hantong-policy")
        policy["dependencies"].append({"name": "hantong-approval", "rename": "approval_api", "kind": None})
        errors = check_architecture.validate_metadata(data)
        self.assertEqual(errors, [])

    def test_M003_allows_test_support_only_as_dev_dependency(self):
        data = metadata([])
        data["packages"].append({"name": "hantong-test-support", "id": "path+file:///x/hantong-test-support#0.1.0", "dependencies": []})
        data["workspace_members"].append(data["packages"][-1]["id"])
        cli = next(p for p in data["packages"] if p["name"] == "hantong-cli")
        cli["dependencies"].append({"name": "hantong-test-support", "kind": "dev"})
        self.assertEqual(check_architecture.validate_metadata(data), [])

    def test_M003_rejects_test_support_build_dependency(self):
        data = metadata([])
        data["packages"].append({"name": "hantong-test-support", "id": "path+file:///x/hantong-test-support#0.1.0", "dependencies": []})
        data["workspace_members"].append(data["packages"][-1]["id"])
        cli = next(p for p in data["packages"] if p["name"] == "hantong-cli")
        cli["dependencies"].append({"name": "hantong-test-support", "kind": "build"})
        self.assertTrue(any("dev-only" in e for e in check_architecture.validate_metadata(data)))

    def test_M003_cli_emits_deterministic_json_and_nonzero(self):
        with mock.patch.object(check_architecture, "cargo_metadata", return_value=metadata([
            ("hantong-cli", "hantong-secrets", "normal", None),
        ])), mock.patch("sys.stdout") as stdout:
            self.assertEqual(check_architecture.main([]), 1)
        stdout.write.assert_called()


if __name__ == "__main__":
    unittest.main()
