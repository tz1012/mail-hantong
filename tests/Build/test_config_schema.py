import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "docs/contracts/config.schema.json"
CONTRACT = ROOT / "docs/contracts/config.md"


class ConfigSchemaTests(unittest.TestCase):
    def setUp(self):
        self.data = json.loads(SCHEMA.read_text(encoding="utf-8"))
        self.properties = self.data["properties"]

    def test_M105_schema_is_draft_2020_12_closed_and_versioned(self):
        self.assertEqual(self.data["$schema"], "https://json-schema.org/draft/2020-12/schema")
        self.assertEqual(self.data["type"], "object")
        self.assertFalse(self.data["additionalProperties"])
        self.assertEqual(self.properties["schema_version"]["const"], 1)
        self.assertIn("settings_version", self.data["required"])

    def test_M105_cli_defaults_english_and_only_supported_languages_are_declared(self):
        language = self.properties["cli"]["properties"]["language"]
        self.assertEqual(language["enum"], ["en", "ko"])
        self.assertEqual(language["default"], "en")
        self.assertNotIn("system", language["enum"])

    def test_M105_gui_language_is_optional_for_os_initial_fallback_and_theme_is_closed(self):
        gui = self.properties["gui"]
        self.assertNotIn("language", gui.get("required", []))
        theme = gui["properties"]["theme"]
        self.assertEqual(theme["enum"], ["light", "dark", "system"])
        self.assertEqual(theme["default"], "system")

    def test_M105_timezone_is_explicitly_runtime_validated(self):
        timezone = self.properties["locale"]["properties"]["timezone_id"]
        self.assertEqual(timezone["format"], "iana-timezone")
        self.assertGreaterEqual(timezone["maxLength"], timezone["minLength"])

    def test_M105_protected_policy_is_closed_and_not_generic_config_surface(self):
        security = self.properties["security"]
        self.assertFalse(security["additionalProperties"])
        self.assertEqual(set(security["properties"]), {"external_ai_enabled"})
        text = CONTRACT.read_text(encoding="utf-8")
        self.assertIn("보호 UI/승인 도우미", text)
        self.assertIn("일반 `hantong config set`", text)

    def test_M105_contract_records_cas_failure_and_previous_value_retention(self):
        text = CONTRACT.read_text(encoding="utf-8")
        for phrase in ("settings_version", "CONFLICT", "이전 유효 값", "토큰·앱 비밀번호·API 키"):
            self.assertIn(phrase, text)


if __name__ == "__main__":
    unittest.main()
