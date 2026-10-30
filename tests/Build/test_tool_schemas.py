import json
from pathlib import Path
import unittest


ROOT = Path(__file__).parents[2]
SCHEMA_DIR = ROOT / "docs" / "contracts" / "tools"


class ToolSchemaContractTests(unittest.TestCase):
    def load(self, name):
        with (SCHEMA_DIR / name).open(encoding="utf-8") as stream:
            return json.load(stream)

    def test_common_response_has_explicit_success_and_error_shapes(self):
        common = self.load("common.schema.json")
        response = common["$defs"]["response"]
        self.assertIn("oneOf", response)
        self.assertEqual(len(response["oneOf"]), 2)
        required = [set(item["required"]) for item in response["oneOf"]]
        self.assertTrue(any("error" in fields for fields in required))
        self.assertTrue(any("error" not in fields for fields in required))
        content = common["$defs"]["untrusted_content"]
        self.assertEqual(content["type"], "array")
        self.assertIn("maxItems", content)

    def test_common_response_bounds_metadata_and_account_results(self):
        common = self.load("common.schema.json")
        response = common["$defs"]["response"]["oneOf"][0]
        account_results = response["properties"]["account_results"]
        self.assertIn("maxItems", account_results)
        self.assertEqual(common["$defs"]["account_result"]["additionalProperties"], False)
        metadata = common["$defs"]["metadata"]
        self.assertEqual(metadata["additionalProperties"], False)
        self.assertTrue(metadata["properties"])

    def test_tool_refs_are_local_and_send_binds_draft_and_idempotency(self):
        for path in SCHEMA_DIR.glob("*.schema.json"):
            schema = json.loads(path.read_text(encoding="utf-8"))
            self.assertFalse(schema["$id"].startswith("http"))
            refs = [schema.get("$id", "")]
            refs.extend(value for value in json.dumps(schema).split('"') if value.startswith("$ref"))
            self.assertTrue(all(not value.startswith("http") for value in refs))
        send = self.load("mail.send.execute.schema.json")
        self.assertEqual(
            set(send["properties"]["params"]["required"]),
            {"account_id", "draft_id", "operation_id", "expected_version", "payload_hash", "approval_id", "idempotency_key"},
        )

    def test_mail03_body_request_binds_shared_envelope_and_exact_locator_variants(self):
        body = self.load("mail.messages.get.schema.json")
        self.assertEqual(set(body["required"]), {"schema_version", "request_id", "tool", "params"})
        self.assertFalse(body["additionalProperties"])
        self.assertEqual(body["properties"]["tool"]["const"], "mail.messages.get")
        variants = body["properties"]["params"]["oneOf"]
        self.assertEqual({v["properties"]["mode"]["const"] for v in variants}, {"imap_body", "locator", "gmail_body"})
        for variant in variants:
            self.assertFalse(variant["additionalProperties"])
            self.assertIn("account_id", variant["required"])
            if variant["properties"]["mode"]["const"] == "gmail_body":
                self.assertIn("message_id", variant["required"])
                self.assertEqual(variant["properties"]["max_bytes"]["maximum"], 65536)
                self.assertNotIn("uid", variant["properties"])
            else:
                self.assertIn("max_bytes", variant["required"])

    def test_acc03_gmail_discovery_requires_account_and_sync_cas_without_token_inputs(self):
        found = set()
        for tool in ("mail.folders.list", "mail.messages.search", "mail.messages.get", "mail.sync.page"):
            for variant in self.load(tool + ".schema.json")["properties"]["params"]["oneOf"]:
                mode = variant["properties"].get("mode", {}).get("const", "")
                if not mode.startswith("gmail_"):
                    continue
                found.add(mode)
                self.assertFalse(variant["additionalProperties"])
                self.assertIn("account_id", variant["required"])
                self.assertEqual(variant["properties"]["account_id"]["pattern"], "^google_[0-9a-f]{64}$")
                self.assertFalse({"token", "access_token", "refresh_token", "approval_id", "callback", "code"} & variant["properties"].keys())
                if mode == "gmail_page":
                    self.assertTrue({"expected_version", "operation_id", "idempotency_key"} <= set(variant["required"]))
                    self.assertEqual(variant["properties"]["limit"]["maximum"], 32)
        self.assertEqual(found, {"gmail_labels", "gmail_messages", "gmail_body", "gmail_page", "gmail_status"})


if __name__ == "__main__":
    unittest.main()
