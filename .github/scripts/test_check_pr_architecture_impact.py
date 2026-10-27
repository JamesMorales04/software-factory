import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location(
    "impact_check", ".github/scripts/check-pr-architecture-impact.py"
)
impact_check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(impact_check)


class ImpactClassificationTests(unittest.TestCase):
    base = "a" * 40
    head = "b" * 40

    def test_required_pr_change_families_are_protected(self):
        examples = [
            ".agents/skills/example/SKILL.md",
            "native/src/db/lead.rs",
            "native/src/bin/worker.rs",
            ".github/workflows/native-rust.yml",
            "docs/quality/live-certification.md",
            "docs/engineering/linting.md",
            "docs/architecture/MASTER.md",
            "docs/product/capabilities.yaml",
        ]
        for path in examples:
            with self.subTest(path=path):
                self.assertTrue(impact_check.is_protected(path))

    def test_ordinary_application_code_does_not_need_impact_entry(self):
        ok, category, _ = impact_check.evaluate(["factory-cli/src/text.rs"], "")
        self.assertTrue(ok)
        self.assertEqual(category, "ordinary-change")

    def office_body(self, impact: str = "changed") -> str:
        if impact == "none":
            return f"""{impact_check.OFFICE_START}
impact: none
rationale: Internal Office refactor with no user-visible or operational change.
base_sha: {self.base}
head_sha: {self.head}
{impact_check.OFFICE_END}"""
        return f"""{impact_check.OFFICE_START}
impact: changed
capabilities: CAP-011
routes: /actions
core_api: GET /actions
user_actions: inspect an Action
states: loading,empty,error,unauthorized
acceptance: office/e2e-browser/rust-journey.mjs
rationale: This changes a real operator journey and its acceptance evidence.
base_sha: {self.base}
head_sha: {self.head}
{impact_check.OFFICE_END}"""

    def container_body(self, impact: str = "changed") -> str:
        if impact == "none":
            return f"""{impact_check.CONTAINER_START}
impact: none
rationale: Internal documentation-only change with no runtime or Compose behavior.
base_sha: {self.base}
head_sha: {self.head}
{impact_check.CONTAINER_END}"""
        return f"""{impact_check.CONTAINER_START}
impact: changed
services: core,worker
commands: podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev cargo xtask check
real_podman: bash scripts/compose-e2e.sh
negative_tests: compose-policy,provider egress
not_tested: Windows,SELinux
rationale: This changes container behavior and requires real Podman evidence.
base_sha: {self.base}
head_sha: {self.head}
{impact_check.CONTAINER_END}"""

    def test_office_change_requires_office_declaration(self):
        ok, classification, _ = impact_check.evaluate(["office/src/App.tsx"], "", self.base, self.head)
        self.assertFalse(ok)
        self.assertEqual(classification, "office-impact-required")
        ok, classification, _ = impact_check.evaluate(
            ["office/src/App.tsx"], self.office_body(), self.base, self.head
        )
        self.assertTrue(ok)
        self.assertEqual(classification, "office")
        self.assertTrue(
            impact_check.evaluate(
                ["office/src/App.tsx"], self.office_body("none"), self.base, self.head
            )[0]
        )

    def test_office_declaration_rejects_unknown_capability_route_api_or_nonbrowser_acceptance(self):
        variants = {
            "capability": ("CAP-999", "GET /actions", "office/e2e-browser/rust-journey.mjs"),
            "route": ("CAP-011", "GET /actions", "office/e2e-browser/rust-journey.mjs"),
            "api": ("CAP-011", "GET /missing", "office/e2e-browser/rust-journey.mjs"),
            "acceptance": ("CAP-011", "GET /actions", "scripts/compose-e2e.sh"),
        }
        for name, (capability, api, acceptance) in variants.items():
            if name == "route":
                body = self.office_body().replace("routes: /actions", "routes: /missing")
            else:
                body = (
                    self.office_body()
                    .replace("capabilities: CAP-011", f"capabilities: {capability}")
                    .replace("core_api: GET /actions", f"core_api: {api}")
                    .replace("acceptance: office/e2e-browser/rust-journey.mjs", f"acceptance: {acceptance}")
                )
            with self.subTest(name=name):
                self.assertFalse(impact_check.evaluate(["office/src/App.tsx"], body, self.base, self.head)[0])

    def test_container_declaration_requires_bound_fields(self):
        body = self.container_body().replace("real_podman: bash scripts/compose-e2e.sh", "real_podman: none")
        self.assertFalse(impact_check.evaluate(["compose.yaml"], body, self.base, self.head)[0])
        body = self.container_body().replace(self.head, "c" * 40)
        self.assertFalse(impact_check.evaluate(["compose.yaml"], body, self.base, self.head)[0])

    def test_container_change_requires_container_declaration(self):
        ok, classification, _ = impact_check.evaluate(["compose.yaml"], "", self.base, self.head)
        self.assertFalse(ok)
        self.assertEqual(classification, "container-native-impact-required")
        ok, classification, _ = impact_check.evaluate(
            ["compose.yaml"], self.container_body(), self.base, self.head
        )
        self.assertTrue(ok)
        self.assertEqual(classification, "container-native")
        self.assertTrue(
            impact_check.evaluate(
                ["compose.yaml"], self.container_body("none"), self.base, self.head
            )[0]
        )

    def test_compose_variants_and_provider_boundaries_are_container_native(self):
        self.assertTrue(impact_check.is_container_native("compose.prod.yaml"))
        self.assertTrue(impact_check.is_container_native("compose.edge.yml"))
        self.assertTrue(impact_check.is_container_native("native/src/provider_sandbox.rs"))
        self.assertTrue(impact_check.is_container_native("native/src/execution.rs"))
        ok, classification, _ = impact_check.evaluate(
            ["compose.prod.yaml"], self.container_body(), self.base, self.head
        )
        self.assertTrue(ok)
        self.assertEqual(classification, "container-native")
        architecture = f"""{impact_check.START}
category: runtime
rationale: This changes provider sandbox behavior and the container execution boundary.
base_sha: {self.base}
head_sha: {self.head}
{impact_check.END}"""
        self.assertFalse(
            impact_check.evaluate(["native/src/provider_sandbox.rs"], architecture, self.base, self.head)[0]
        )
        self.assertTrue(
            impact_check.evaluate(
                ["native/src/provider_sandbox.rs"],
                architecture + "\n" + self.container_body(),
                self.base,
                self.head,
            )[0]
        )

    def test_protected_container_change_needs_both_declarations(self):
        architecture = f"""{impact_check.START}
category: runtime
rationale: This changes the Worker runtime and its container execution boundary.
base_sha: {self.base}
head_sha: {self.head}
{impact_check.END}"""
        path = ["native/src/bin/worker.rs"]
        self.assertFalse(impact_check.evaluate(path, architecture, self.base, self.head)[0])
        ok, classification, _ = impact_check.evaluate(
            path, architecture + "\n" + self.container_body(), self.base, self.head
        )
        self.assertTrue(ok)
        self.assertEqual(classification, "runtime+container-native")

    def test_protected_diff_requires_meaningful_declaration(self):
        self.assertFalse(impact_check.evaluate(["native/src/db/authorization.rs"], "")[0])
        body = f"""<!-- architecture-impact:start -->
category: authorization
rationale: This changes how project grants are checked before a run begins.
base_sha: {self.base}
head_sha: {self.head}
<!-- architecture-impact:end -->"""
        ok, category, _ = impact_check.evaluate(["native/src/db/authorization.rs"], body, self.base, self.head)
        self.assertTrue(ok)
        self.assertEqual(category, "authorization")

    def test_placeholder_and_invalid_category_fail_closed(self):
        for category, rationale in [("architecture", "TODO"), ("misc", "A sufficiently long rationale that is not accepted.")]:
            body = f"""{impact_check.START}
category: {category}
rationale: {rationale}
base_sha: {self.base}
head_sha: {self.head}
{impact_check.END}"""
            with self.subTest(category=category):
                self.assertFalse(impact_check.evaluate([".github/workflows/test.yml"], body, self.base, self.head)[0])

    def test_duplicate_or_missing_delimiters_fail_closed(self):
        body = f"{impact_check.START}\ncategory: ci\nrationale: This workflow changes mandatory pull request governance checks.\nbase_sha: {self.base}\nhead_sha: {self.head}\n"
        self.assertFalse(impact_check.evaluate([".github/workflows/test.yml"], body, self.base, self.head)[0])
        reversed_body = f"{impact_check.END}\n{impact_check.START}\n"
        self.assertFalse(impact_check.evaluate([".github/workflows/test.yml"], reversed_body, self.base, self.head)[0])

    def test_assessment_is_bound_to_current_base_and_head(self):
        body = f"""{impact_check.START}
category: governance
rationale: This changes the governance workflow and needs review of its behavior.
base_sha: {self.base}
head_sha: {self.head}
{impact_check.END}"""
        self.assertTrue(impact_check.evaluate(["native/src/db/authorization.rs"], body, self.base, self.head)[0])
        self.assertFalse(impact_check.evaluate(["native/src/db/authorization.rs"], body, self.base, "c" * 40)[0])
        self.assertFalse(impact_check.evaluate(["native/src/db/authorization.rs"], body, "d" * 40, self.head)[0])

    def test_stale_edited_event_uses_the_current_pull_request_head(self):
        self.assertEqual(
            impact_check.resolved_head(self.head, "c" * 40), "c" * 40
        )
        self.assertEqual(impact_check.resolved_head(self.head, "not-a-sha"), self.head)

    def test_workflow_runs_for_every_pr_and_body_edits(self):
        workflow = Path(".github/workflows/governance.yml").read_text(encoding="utf-8")
        trigger = workflow.split("  pull_request:", 1)[1].split("  push:", 1)[0]
        self.assertNotIn("paths:", trigger)
        self.assertIn("synchronize", trigger)
        self.assertIn("edited", trigger)
        self.assertIn("Assess PR architecture impact", workflow)


if __name__ == "__main__":
    unittest.main()
