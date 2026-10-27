import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "office_parity", ".github/scripts/check-office-parity.py"
)
office_parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(office_parity)


def valid_parity() -> dict:
    return {
        "schema_version": 1,
        "capabilities": {
            "CAP-011": {
                "name": "Factory Office",
                "user_facing": True,
                "job": "Inspect and operate Factory work.",
                "routes": ["/actions"],
                "core_api": ["GET /actions"],
                "user_actions": ["inspect an Action"],
                "states": ["loading", "empty", "error"],
                "acceptance": ["office/e2e-browser/rust-journey.mjs"],
                "notes": "Test fixture.",
            }
        },
        "deferred": [
            {
                "module": "Example",
                "planned_slice": "Slice 9",
                "reason": "Not implemented in this fixture.",
            }
        ],
    }


class OfficeParityTests(unittest.TestCase):
    def write_repo(self, root: Path, parity: dict) -> None:
        (root / "docs/product").mkdir(parents=True)
        (root / "docs/product/capabilities.yaml").write_text(
            "capabilities:\n- id: CAP-011\n  name: Office\n", encoding="utf-8"
        )
        (root / "docs/product/office-parity.json").write_text(
            json.dumps(parity), encoding="utf-8"
        )
        (root / "office/e2e-browser").mkdir(parents=True)
        (root / "office/e2e-browser/rust-journey.mjs").write_text(
            "// fixture\n", encoding="utf-8"
        )

    def test_repository_matrix_passes(self):
        self.assertEqual(office_parity.validate(Path(".").resolve()), [])

    def test_unknown_capability_fails(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.write_repo(root, valid_parity())
            parity = valid_parity()
            parity["capabilities"]["CAP-999"] = parity["capabilities"].pop("CAP-011")
            (root / "docs/product/office-parity.json").write_text(
                json.dumps(parity), encoding="utf-8"
            )
            errors = office_parity.validate(root)
            self.assertTrue(any("unknown capability" in error for error in errors))

    def test_missing_acceptance_file_fails(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            parity = valid_parity()
            parity["capabilities"]["CAP-011"]["acceptance"] = ["missing/test.mjs"]
            self.write_repo(root, parity)
            errors = office_parity.validate(root)
            self.assertTrue(any("acceptance path is missing" in error for error in errors))

    def test_placeholders_and_empty_fields_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            parity = valid_parity()
            parity["capabilities"]["CAP-011"]["routes"] = ["TODO"]
            parity["capabilities"]["CAP-011"]["states"] = []
            self.write_repo(root, parity)
            errors = office_parity.validate(root)
            self.assertTrue(any("placeholder" in error for error in errors))
            self.assertTrue(any("states" in error for error in errors))

    def test_non_user_facing_entry_is_exempt(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            parity = valid_parity()
            parity["capabilities"]["CAP-011"]["user_facing"] = False
            parity["capabilities"]["CAP-011"]["routes"] = []
            self.write_repo(root, parity)
            self.assertEqual(office_parity.validate(root), [])


if __name__ == "__main__":
    unittest.main()
