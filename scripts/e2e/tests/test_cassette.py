from __future__ import annotations

import copy
import hashlib
import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from cassette import (  # noqa: E402
    CassetteError,
    canonical_request_sha256,
    load,
    validate,
)

ROOT = HERE.parents[2]
EXAMPLE = ROOT / "scripts/fixtures/provider-cassette.example.json"


class CassetteTests(unittest.TestCase):
    def test_example_is_valid_and_turn_digest_matches(self) -> None:
        cassette = load(EXAMPLE)
        self.assertEqual(cassette["schema"], "factory-provider-cassette-v1")
        self.assertEqual(cassette["turns"][0]["requestSha256"], canonical_request_sha256(cassette["turns"][0]["request"]))

    def test_rejects_unknown_schema(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["schema"] = "other"
        with self.assertRaises(CassetteError):
            validate(payload)

    def test_rejects_sequential_index_gap(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["index"] = 2
        with self.assertRaises(CassetteError):
            validate(payload)

    def test_rejects_digest_mismatch(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["request"]["objective"] = "tampered"
        with self.assertRaises(CassetteError):
            validate(payload)

    def test_rejects_secret_shaped_material(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["request"]["authorization"] = "Bearer abcdefghijklmnop"
        with self.assertRaises(CassetteError):
            validate(payload)

    def test_rejects_prefixed_api_key_header(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["request"]["x-api-key"] = "opaque-credential-value"
        with self.assertRaises(CassetteError):
            validate(payload)

    def test_rejects_x_auth_token_header(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["request"]["x-auth-token"] = "opaque-credential-value"
        with self.assertRaises(CassetteError):
            validate(payload)

    def test_rejects_common_secret_field_names(self) -> None:
        for key in ("client_secret", "aws_secret_access_key", "private_key"):
            payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
            payload["turns"][0]["request"][key] = "opaque-credential-value"
            with self.assertRaises(CassetteError, msg=key):
                validate(payload)

    def test_rejects_github_pat_value(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["request"]["note"] = "github_pat_11AAABBBCCCDDDEEEFFF"
        with self.assertRaises(CassetteError):
            validate(payload)

    def test_allows_token_usage_counters(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["usage"] = {"inputTokens": 10, "outputTokens": 5}
        validate(payload)

    def test_tool_call_requires_name(self) -> None:
        payload = json.loads(EXAMPLE.read_text(encoding="utf-8"))
        payload["turns"][0]["toolCalls"] = [{"arguments": {}}]
        with self.assertRaises(CassetteError):
            validate(payload)


if __name__ == "__main__":
    unittest.main()
