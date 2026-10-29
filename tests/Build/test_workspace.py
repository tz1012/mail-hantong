import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
EXPECTED_PACKAGES = {
    "hantong-contracts",
    "hantong-platform",
    "hantong-domain",
    "hantong-approval",
    "hantong-approval-windows",
    "hantong-policy",
    "hantong-application",
    "hantong-outbox",
    "hantong-ipc-client",
    "hantong-ipc-server",
    "hantong-store",
    "hantong-secrets",
    "hantong-adapters",
    "mail-hantong-core",
    "hantong-cli",
    "hantong-test-support",
}


class WorkspaceScaffoldTests(unittest.TestCase):
    def cargo(self, *args):
        return subprocess.run(
            ["cargo", *args], cwd=ROOT, check=True, capture_output=True, text=True
        )

    def test_M001_workspace_metadata_has_expected_packages_and_binary(self):
        result = self.cargo("metadata", "--no-deps", "--format-version", "1")
        metadata = json.loads(result.stdout)
        self.assertEqual(set(metadata["workspace_members"]), {
            package["id"] for package in metadata["packages"]
        })
        self.assertEqual({package["name"] for package in metadata["packages"]}, EXPECTED_PACKAGES)
        cli = next(package for package in metadata["packages"] if package["name"] == "hantong-cli")
        self.assertEqual([target["name"] for target in cli["targets"] if target["kind"] == ["bin"]], ["hantong"])

    def test_M001_workspace_uses_pinned_toolchain_and_license_file(self):
        toolchain = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
        self.assertIn('channel = "1.95.0"', toolchain)
        root_manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('resolver = "3"', root_manifest)
        self.assertIn('license-file = "LICENSE"', root_manifest)


if __name__ == "__main__":
    unittest.main()
