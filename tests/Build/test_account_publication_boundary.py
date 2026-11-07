"""Keep the native publication capability's unsafe audit surface explicit."""
import unittest
from pathlib import Path


class AccountPublicationBoundaryTests(unittest.TestCase):
    def test_sec03_raw_publication_factory_has_only_its_audited_native_caller(self):
        root = Path(__file__).resolve().parents[2]
        token = "from_pinned_default_handles"
        uses = {}
        for tree in ("apps", "crates"):
            for path in (root / tree).rglob("*.rs"):
                count = path.read_text(encoding="utf-8").count(token)
                if count:
                    uses[path.relative_to(root).as_posix()] = count
        # Additional calls/import aliases require a new security review; safe
        # arbitrary-file code must not become another authority constructor.
        self.assertEqual(uses, {
            "crates/hantong-platform/src/account_publication.rs": 1,
            "crates/hantong-approval-windows/src/profile_publication.rs": 1,
        })


if __name__ == "__main__":
    unittest.main()
