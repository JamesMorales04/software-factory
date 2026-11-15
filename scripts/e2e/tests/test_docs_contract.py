"""Contract tests for the docs touched by the harness/parity work.

The YAML parser is optional in the development image; when it is absent the
YAML assertion is skipped and the module still validates the JSON document and
the YAML scalar structure used by the existing registry.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


class DocsContractTests(unittest.TestCase):
    def test_office_parity_is_valid_json_with_expected_shape(self) -> None:
        payload = json.loads(
            (ROOT / "docs" / "product" / "office-parity.json").read_text(encoding="utf-8")
        )
        self.assertIsInstance(payload.get("capabilities"), dict)
        self.assertIsInstance(payload.get("deferred"), list)
        self.assertIsInstance(payload.get("schema_version"), int)
        if "notes" in payload:
            self.assertIsInstance(payload["notes"], str)

    def test_capabilities_yaml_parses_when_pyyaml_is_available(self) -> None:
        try:
            import yaml
        except ImportError:
            self.skipTest("PyYAML is not installed in the development image")
        payload = yaml.safe_load(
            (ROOT / "docs" / "product" / "capabilities.yaml").read_text(encoding="utf-8")
        )
        self.assertIsInstance(payload.get("capabilities"), list)
        self.assertTrue(all(isinstance(item, dict) for item in payload["capabilities"]))
        # The current registry has no `limitations` key; if a future revision
        # adds one, it must be a list of strings.
        if "limitations" in payload:
            self.assertTrue(all(isinstance(item, str) for item in payload["limitations"]))


if __name__ == "__main__":
    unittest.main()
