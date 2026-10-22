import copy
import importlib.util
import json
import subprocess
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
        self.source = json.loads((ROOT / "docs/evidence/f1-s4c-07-live-2026-10-21.json").read_text())
        self.invalid_cases = json.loads((ROOT / "scripts/fixtures/live-certification-invalid-cases.json").read_text())

    def validate_manifest(self, manifest, source=None):
        if source is None:
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "manifest.json"
                path.write_text(json.dumps(manifest))
                return validator.validate(path, ROOT)
        with tempfile.TemporaryDirectory(prefix=".certification-test-", dir=ROOT) as directory:
            temp = Path(directory)
            (temp / "source.json").write_text(json.dumps(source))
            candidate = copy.deepcopy(manifest)
            candidate["source_evidence"] = str((temp / "source.json").relative_to(ROOT))
            manifest_path = temp / "manifest.json"
            manifest_path.write_text(json.dumps(candidate))
            return validator.validate(manifest_path, ROOT)

    def _non_browser_campaign(self):
        manifest = copy.deepcopy(self.manifest)
        source = copy.deepcopy(self.source)
        manifest["schema_version"] = 2
        manifest.pop("browser")
        profile = source["preflight"]["frozen-spec-contracts"]["detail"]
        profile["capabilities"].remove("browser")
        run = source["runs"][0]["run"]
        run["capabilities"].remove("browser")
        run["result"].pop("browserInvoked")
        run["result"].pop("browserSuccessfulCommands")
        run["result"].pop("testServerManaged")
        run["result"].pop("testServerEndpoint")
        source.pop("browser_certification")
        candidate_evidence = source["candidate_evidence"][manifest["candidate_id"]]
        candidate_evidence["spec"]["capabilities"].remove("browser")
        candidate_evidence["spec"].pop("browserEndpoints")
        candidate_evidence["spec"].pop("browserTestServer")
        criteria = candidate_evidence["spec"]["acceptanceCriteria"]
        criteria[:] = [criterion for criterion in criteria if criterion["kind"] != "browser"]
        source["decision"]["checks"][:] = [check for check in source["decision"]["checks"] if check["kind"] != "capability"]
        source["candidate_evidence"][manifest["candidate_id"]]["decision"]["checks"][:] = [
            check for check in source["candidate_evidence"][manifest["candidate_id"]]["decision"]["checks"]
            if check["kind"] != "capability"
        ]
        return manifest, source

    def test_existing_c07_evidence_remains_valid(self):
        self.assertEqual([], self.validate_manifest(self.manifest))

    def test_non_browser_cli_campaign_uses_frozen_spec_acceptance(self):
        manifest, source = self._non_browser_campaign()
        self.assertEqual([], self.validate_manifest(manifest, source))

    def test_browser_campaign_missing_required_interaction_is_rejected(self):
        source = copy.deepcopy(self.source)
        source["browser_certification"]["successful_commands"] = ["snapshot"]
        self.assertTrue(self.validate_manifest(self.manifest, source))

    def test_non_browser_manifest_cannot_claim_browser_certification(self):
        manifest, source = self._non_browser_campaign()
        manifest["browser"] = {"successful_commands": ["open", "snapshot"], "managed_test_server": True}
        self.assertTrue(self.validate_manifest(manifest, source))

    def test_missing_or_mismatched_frozen_capability_profile_is_rejected(self):
        source = copy.deepcopy(self.source)
        source["preflight"]["frozen-spec-contracts"]["detail"]["capabilities"].remove("browser")
        self.assertTrue(self.validate_manifest(self.manifest, source))
        source = copy.deepcopy(self.source)
        source["preflight"].pop("frozen-spec-contracts")
        self.assertTrue(self.validate_manifest(self.manifest, source))

    def test_claimed_recovery_requires_failed_run_and_corrective_run(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["schema_version"] = 2
        manifest.pop("browser")
        manifest["recovery"] = {"claimed": True, "corrective_run_id": "some-corrective-run"}
        self.assertTrue(self.validate_manifest(manifest, self.source))

    def test_rejected_gate_cannot_be_reported_as_accepted(self):
        source = copy.deepcopy(self.source)
        source["decision"]["status"] = "REJECTED"
        self.assertTrue(self.validate_manifest(self.manifest, source))

    def test_identity_and_cleanup_mismatches_are_rejected(self):
        source = copy.deepcopy(self.source)
        source["candidate_evidence"].pop(self.manifest["candidate_id"])
        self.assertTrue(self.validate_manifest(self.manifest, source))
        source = copy.deepcopy(self.source)
        source["cleanup"]["workspace_active"] = ["still-active"]
        self.assertTrue(self.validate_manifest(self.manifest, source))

    def test_simulation_cannot_be_represented_as_real(self):
        source = copy.deepcopy(self.source)
        source["paseo_mode"] = "simulated"
        self.assertTrue(self.validate_manifest(self.manifest, source))

    def test_invalid_fixtures_are_rejected(self):
        self.assertGreaterEqual(len(self.invalid_cases), 12)
        for case in self.invalid_cases:
            with self.subTest(case=case["name"]):
                changed = copy.deepcopy(self.manifest)
                set_path(changed, case["path"], case["value"])
                self.assertTrue(self.validate_manifest(changed), case["name"])

    def test_json_schema_enforces_nested_and_length_constraints(self):
        schema = json.loads((ROOT / "docs/quality/certification-manifests.schema.json").read_text())
        for mutate in (
            lambda m: m.update(certification_id=""),
            lambda m: m["verifier"].pop("status"),
            lambda m: m["reviewer"].update(unexpected=True),
            lambda m: m["cleanup"].update(contradictions="none"),
            lambda m: m["browser"].update(successful_commands=[]),
        ):
            changed = copy.deepcopy(self.manifest)
            mutate(changed)
            self.assertTrue(validator._schema_errors(changed, schema))

    def test_optional_candidate_repository_checks_commit_and_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            candidate_repo = Path(directory)
            subprocess.run(["git", "init", "-q", str(candidate_repo)], check=True)
            subprocess.run(["git", "-C", str(candidate_repo), "config", "user.name", "Certification Test"], check=True)
            subprocess.run(["git", "-C", str(candidate_repo), "config", "user.email", "certification@example.invalid"], check=True)
            (candidate_repo / "fixture.txt").write_text("candidate\n")
            subprocess.run(["git", "-C", str(candidate_repo), "add", "fixture.txt"], check=True)
            subprocess.run(["git", "-C", str(candidate_repo), "commit", "-qm", "candidate"], check=True)
            commit = subprocess.run(["git", "-C", str(candidate_repo), "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
            tree = subprocess.run(["git", "-C", str(candidate_repo), "rev-parse", "HEAD^{tree}"], check=True, capture_output=True, text=True).stdout.strip()
            self.assertEqual([], validator.validate_candidate_git(candidate_repo, commit, tree))
            self.assertTrue(validator.validate_candidate_git(candidate_repo, commit, "0" * 40))
            self.assertTrue(validator.validate_candidate_git(candidate_repo, "1" * 40, tree))


if __name__ == "__main__":
    unittest.main()
