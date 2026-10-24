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
        ok, category, _ = impact_check.evaluate(["office/src/components/Button.tsx"], "")
        self.assertTrue(ok)
        self.assertEqual(category, "ordinary-change")

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

    def test_workflow_runs_for_every_pr_and_body_edits(self):
        workflow = Path(".github/workflows/governance.yml").read_text(encoding="utf-8")
        trigger = workflow.split("  pull_request:", 1)[1].split("  push:", 1)[0]
        self.assertNotIn("paths:", trigger)
        self.assertIn("synchronize", trigger)
        self.assertIn("edited", trigger)
        self.assertIn("Assess PR architecture impact", workflow)


if __name__ == "__main__":
    unittest.main()
