import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("live_certification", ROOT / "scripts/validate-live-certification.py")
validator = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(validator)


def set_path(obj, path, value):
    parts = path.split(".")
    current = obj
    for part in parts[:-1]:
        current = current[part]
    current[parts[-1]] = value


class CertificationManifestTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((ROOT / "docs/evidence/f1-s4c-07-certification-manifest.json").read_text())
        self.invalid_cases = json.loads((ROOT / "scripts/fixtures/live-certification-invalid-cases.json").read_text())

    def validate_manifest(self, manifest):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(manifest))
            return validator.validate(path, ROOT)

    def test_existing_c07_evidence_is_structurally_valid(self):
        self.assertEqual([], self.validate_manifest(self.manifest))

    def test_deliberately_invalid_fixtures_are_rejected(self):
        self.assertGreaterEqual(len(self.invalid_cases), 12)
        for case in self.invalid_cases:
            with self.subTest(case=case["name"]):
                changed = copy.deepcopy(self.manifest)
                set_path(changed, case["path"], case["value"])
                self.assertTrue(self.validate_manifest(changed), case["name"])

    def test_missing_required_candidate_run_correlation_is_rejected(self):
        changed = copy.deepcopy(self.manifest)
        changed["run_id"] = "unknown-run"
        self.assertTrue(self.validate_manifest(changed))

    def test_invalidation_cannot_claim_verified(self):
        changed = copy.deepcopy(self.manifest)
        changed["invalidated_by"] = "runtime change after tested head"
        self.assertIn("verified status contradicts invalidated_by", self.validate_manifest(changed))


if __name__ == "__main__":
    unittest.main()
