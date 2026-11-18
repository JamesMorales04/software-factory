"""Author tests for the pre-run binding and its canonical profile contract.

These tests exercise the plan output and the pure contract helpers; they do not
contact Core and are not independent acceptance.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
BIND_PATH = HERE.parent / "real-recovery-bind.py"
HELPER_PATH = HERE.parent / "real_recovery_profile.py"


def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


binding = _load("real_recovery_bind", BIND_PATH)
profile_contract = _load("real_recovery_profile", HELPER_PATH)


def _canonical_projection(
    *,
    provider: str = "opencode-go",
    model: str = "deepseek-v4.1-flash",
    executable: bool = True,
    auto_start: dict | None = None,
) -> dict:
    body = profile_contract.canonical_profile(
        name="Real recovery e2e",
        description="test",
        provider=provider,
        model=model,
    )
    return {
        "id": "profile-1",
        "revision": 1,
        "revisionId": "revision-1",
        "schemaVersion": "2",
        "contentHash": "a" * 64,
        "globalDefault": False,
        "projectDefaults": [],
        "executable": executable,
        "warnings": [],
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "name": body["name"],
        "description": body["description"],
        "category": body["category"],
        "roles": body["roles"],
        "operations": body["operations"],
        "progress": body["progress"],
        "autoStart": auto_start
        or {"enabled": False, "riskClasses": []},
        "implementer": body["roles"]["implementer"],
        "legacyV1": None,
    }


class PlanTests(unittest.TestCase):
    def test_plan_lists_the_exact_binding_sequence(self):
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
            code = binding.main([
                "--plan",
                "--compose-project", "e2e-recovery",
                "--project-repository", "http://git-fixture:8080/repo.git",
            ])
        self.assertEqual(code, 0)
        plan = json.loads(stdout.getvalue())
        steps = [step["step"] for step in plan["steps"]]
        self.assertEqual(steps, ["project", "profile", "grant", "interface"])
        project = plan["steps"][0]
        self.assertEqual(project["config"]["allowedPaths"], ["src"])
        self.assertEqual(
            project["config"]["capabilities"],
            ["repository-edit", "generic-toolchain"],
        )
        profile = plan["steps"][1]["request"]["body"]
        self.assertEqual(profile["category"], "BALANCED")
        self.assertEqual(
            set(profile["roles"]), {"lead", "implementer", "semanticReviewer"}
        )
        self.assertEqual(profile["auto_start"], {"enabled": False, "risk_classes": []})
        grant = plan["steps"][2]["request"]["body"]
        self.assertIs(grant["acknowledge"], True)
        self.assertIs(grant["autonomous"], False)
        self.assertIs(grant["subscription_confirmed"], False)
        self.assertNotIn("authorized_micros", json.dumps(plan))
        self.assertNotIn("Bearer", json.dumps(plan))

    def test_plan_refuses_the_installed_project(self):
        stderr = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
            code = binding.main([
                "--plan",
                "--compose-project", "software-factory",
                "--project-repository", "http://git-fixture:8080/repo.git",
            ])
        self.assertEqual(code, 2)
        self.assertIn("installed product project", stderr.getvalue())

    def test_plan_requires_a_project_source(self):
        stderr = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
            code = binding.main(["--plan", "--compose-project", "e2e-recovery"])
        self.assertEqual(code, 2)
        self.assertIn("--project-id or --project-repository", stderr.getvalue())

    def test_plan_requires_a_disposable_project(self):
        stderr = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
            code = binding.main([
                "--plan",
                "--compose-project", "factory",
                "--project-repository", "http://git-fixture:8080/repo.git",
            ])
        self.assertEqual(code, 2)
        self.assertIn("disposable", stderr.getvalue())


class ApplyTests(unittest.TestCase):
    def test_apply_requires_an_existing_private_token_file(self):
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            stderr = io.StringIO()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
                code = binding.main([
                    "--apply",
                    "--compose-project", "e2e-recovery",
                    "--project-repository", "http://git-fixture:8080/repo.git",
                    "--core-api", "http://127.0.0.1:1",
                    "--operator-token-file", str(Path(directory) / "missing.token"),
                    "--output", str(Path(directory) / "interface.json"),
                ])
            self.assertEqual(code, 2)
            self.assertIn("does not exist", stderr.getvalue())

    def test_apply_rejects_a_world_readable_token_file(self):
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            token = Path(directory) / "operator.token"
            token.write_text("x" * 32, encoding="utf-8")
            token.chmod(0o644)
            stderr = io.StringIO()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
                code = binding.main([
                    "--apply",
                    "--compose-project", "e2e-recovery",
                    "--project-repository", "http://git-fixture:8080/repo.git",
                    "--core-api", "http://127.0.0.1:1",
                    "--operator-token-file", str(token),
                    "--output", str(Path(directory) / "interface.json"),
                ])
            self.assertEqual(code, 2)
            self.assertIn("group/world readable", stderr.getvalue())


class ProfileContractTests(unittest.TestCase):
    def test_the_real_projection_is_accepted(self):
        problems = profile_contract.profile_mismatches(
            _canonical_projection(), provider="opencode-go", model="deepseek-v4.1-flash"
        )
        self.assertEqual(problems, [])

    def test_a_different_route_is_rejected(self):
        problems = profile_contract.profile_mismatches(
            _canonical_projection(model="another-model"),
            provider="opencode-go",
            model="deepseek-v4.1-flash",
        )
        self.assertIn("roles", problems)

    def test_a_non_executable_profile_is_rejected(self):
        problems = profile_contract.profile_mismatches(
            _canonical_projection(executable=False),
            provider="opencode-go",
            model="deepseek-v4.1-flash",
        )
        self.assertIn("executable", problems)

    def test_enabled_auto_start_is_rejected(self):
        problems = profile_contract.profile_mismatches(
            _canonical_projection(
                auto_start={"enabled": True, "riskClasses": ["R0"]}
            ),
            provider="opencode-go",
            model="deepseek-v4.1-flash",
        )
        self.assertIn("autoStart", problems)


class GrantTests(unittest.TestCase):
    def _grant(self) -> dict:
        profile = _canonical_projection()
        return {
            "grant": {
                "delegation_id": "delegation-1",
                "project_id": "project-1",
                "principal_id": "operator-token",
                "profile_id": profile["id"],
                "profile_revision_id": profile["revisionId"],
                "profile_revision": profile["revision"],
                "profile_hash": profile["contentHash"],
                "project_config_hash": "c" * 64,
                "acknowledged": True,
                "autonomous": False,
                "subscription_confirmed": False,
                "idempotency_key": "bind:grant:project-1",
                "created_at": "2026-01-01T00:00:00Z",
            },
            "delegation": {
                "id": "delegation-1",
                "principalId": "operator-token",
                "auditId": "audit-1",
                "projectId": "project-1",
                "revision": 1,
                "contentHash": "b" * 64,
                "objectiveClasses": ["FEATURE"],
                "allowedPaths": ["src"],
                "capabilities": ["repository-edit", "generic-toolchain"],
                "riskClasses": ["R0"],
                "autonomousStart": False,
                "autonomousRiskClasses": [],
                "route": {},
                "maxTimeoutSecs": 600,
                "maxParallel": 2,
                "operations": profile["operations"],
                "progress": {"diagnoseAfterStagnantCheckpoints": 2, "escalateAfterUnsuccessfulReplans": 1, "observationTimeoutSecs": 300},
                "expiresAt": "2026-01-02T00:00:00Z",
            },
            "usage": {},
            "replayed": False,
        }

    def test_the_real_grant_projection_is_accepted(self):
        binding._verify_grant(
            self._grant(),
            project_id="project-1",
            config_hash="c" * 64,
            profile=_canonical_projection(),
        )

    def test_a_grant_for_another_profile_is_rejected(self):
        result = self._grant()
        result["grant"]["profile_id"] = "other-profile"
        with self.assertRaises(binding.BindError):
            binding._verify_grant(
                result,
                project_id="project-1",
                config_hash="c" * 64,
                profile=_canonical_projection(),
            )

    def test_a_grant_with_autonomy_enabled_is_rejected(self):
        result = self._grant()
        result["grant"]["autonomous"] = True
        with self.assertRaises(binding.BindError):
            binding._verify_grant(
                result,
                project_id="project-1",
                config_hash="c" * 64,
                profile=_canonical_projection(),
            )


class ConfigTests(unittest.TestCase):
    def test_the_required_configuration_is_recognized(self):
        self.assertTrue(
            binding.config_is_bound({
                "active": True,
                "allowedPaths": ["src"],
                "capabilities": ["repository-edit", "generic-toolchain"],
                "qualityProfile": "generic",
            })
        )

    def test_a_foreign_capability_set_is_rejected(self):
        self.assertFalse(
            binding.config_is_bound({
                "active": True,
                "allowedPaths": ["src"],
                "capabilities": ["repository-edit"],
                "qualityProfile": "generic",
            })
        )


if __name__ == "__main__":
    unittest.main()
