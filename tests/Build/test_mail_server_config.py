import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
COMPOSE = ROOT / "tests/Integration/imap/compose.yaml"


class MailServerConfigTests(unittest.TestCase):
    def setUp(self):
        self.text = COMPOSE.read_text(encoding="utf-8")

    def test_M007_uses_verified_greenmail_release_and_loopback_ports(self):
        self.assertIn("docker.io/greenmail/standalone:2.1.15", self.text)
        self.assertIn('"127.0.0.1:13025:3025"', self.text)
        self.assertIn('"127.0.0.1:13143:3143"', self.text)
        self.assertIn('restart: "no"', self.text)

    def test_M007_fixture_credentials_and_no_host_mount_or_external_delivery(self):
        self.assertIn("tester:fixture-password@example.invalid", self.text)
        self.assertNotIn("volumes:", self.text)
        self.assertNotIn("network_mode:", self.text)
        self.assertNotRegex(self.text, r"(?i)(relay|smtp\.gmail|outlook\.com|\.env)")

    def test_M007_readiness_is_healthchecked_and_wrappers_pin_compose_file(self):
        self.assertIn("healthcheck:", self.text)
        self.assertIn("nc -z 127.0.0.1 3025", self.text)
        ps = (ROOT / "scripts/test-mail-server.ps1").read_text(encoding="utf-8")
        sh = (ROOT / "scripts/test-mail-server").read_text(encoding="utf-8")
        for content in (ps, sh):
            self.assertIn("tests/Integration/imap/compose.yaml", content)
            self.assertIn("up", content)
            self.assertIn("down", content)
            self.assertIn("status", content)
            self.assertNotIn("docker compose up", content.replace("docker compose -f", ""))


if __name__ == "__main__":
    unittest.main()
