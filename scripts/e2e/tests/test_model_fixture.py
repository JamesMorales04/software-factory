"""Deterministic unit tests for the Compose model fixture responses.

These tests never start a server, Podman or Core. They pin the prompt
classification and the response shapes that Core's schema decoders require for
the AUTONOMOUS stage, so a fixture regression is caught before a heavy Compose
window. Evidence from the fixture is simulated test support, never inference.
"""

from __future__ import annotations

import importlib.util
import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FIXTURE = ROOT / "scripts" / "fixtures" / "compose-model-fixture.py"


def load_fixture():
    spec = importlib.util.spec_from_file_location("compose_model_fixture", FIXTURE)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def content(response):
    return json.loads(response["choices"][0]["message"]["content"])


class PromptClassificationTests(unittest.TestCase):
    def test_classifies_each_lead_role(self) -> None:
        fixture = load_fixture()
        cases = {
            "bounded Software Factory Planner": "planner",
            "bounded Software Factory Spec Manager": "spec",
            "Review exactly one frozen Candidate EnvironmentPlan": "environment",
            "independent semantic code reviewer": "semantic_review",
            "bounded Software Factory Semantic Assessor": "assessment",
        }
        for prompt, phase in cases.items():
            self.assertEqual(fixture.classify_prompt(prompt), phase)

    def test_unknown_prompt_is_not_silently_assessed(self) -> None:
        fixture = load_fixture()
        self.assertEqual(fixture.classify_prompt("unrelated system prompt"), "other")

    def test_baseline_identity_block_is_an_assessment(self) -> None:
        fixture = load_fixture()
        prompt = "Objective: add x. Project: p-1. Action: a-1. Request: r-1."
        self.assertEqual(fixture.classify_prompt("bounded Lead", prompt), "assessment")


class PlannerResponseTests(unittest.TestCase):
    def test_planner_stays_inside_the_granted_scope(self) -> None:
        fixture = load_fixture()
        prompt = json.dumps(
            {
                "objective": "Add the fixture endpoint.",
                "projectConfig": {"allowedPaths": ["src"]},
            }
        )
        graph = content(fixture.planner(prompt, None))
        self.assertEqual(len(graph["units"]), 1)
        unit = graph["units"][0]
        self.assertRegex(unit["id"], r"^[a-z0-9][a-z0-9-]*$")
        self.assertEqual(unit["paths"], ["src/provider.txt"])
        self.assertTrue(unit["acceptance"])
        self.assertEqual(unit["dependsOn"], [])

    def test_planner_uses_the_first_allowed_path(self) -> None:
        fixture = load_fixture()
        prompt = json.dumps(
            {"objective": "x", "projectConfig": {"allowedPaths": ["app"]}}
        )
        self.assertEqual(
            content(fixture.planner(prompt, None))["units"][0]["paths"],
            ["app/provider.txt"],
        )


class EnvironmentReviewResponseTests(unittest.TestCase):
    def test_review_echoes_the_exact_plan_identity(self) -> None:
        fixture = load_fixture()
        plan = {
            "requestId": "req-1",
            "actionId": "act-1",
            "planSha256": "a" * 64,
            "revision": 3,
            "riskClass": "R0",
        }
        prompt = (
            "You are the bounded Intelligent Lead. Review exactly one frozen "
            "Candidate EnvironmentPlan and return ONLY one JSON object.\n\n"
            f"EnvironmentPlan:\n{json.dumps(plan)}\n\nJSON Schema:\n{{}}"
        )
        review = content(fixture.environment_review(prompt))
        self.assertEqual(review["schemaVersion"], 1)
        self.assertEqual(review["requestId"], "req-1")
        self.assertEqual(review["actionId"], "act-1")
        self.assertEqual(review["planSha256"], "a" * 64)
        self.assertEqual(review["planRevision"], 3)
        self.assertEqual(review["riskClass"], "R0")
        self.assertEqual(review["decision"], "APPROVE")
        self.assertGreaterEqual(len(review["rationale"]), 8)

    def test_review_fails_closed_without_a_plan(self) -> None:
        fixture = load_fixture()
        with self.assertRaises(ValueError):
            fixture.environment_review("no plan here")


class SpecDraftResponseTests(unittest.TestCase):
    def test_spec_covers_every_work_graph_unit(self) -> None:
        fixture = load_fixture()
        prompt = json.dumps(
            {
                "objective": "x",
                "workGraph": {"units": [{"id": "unit-1"}, {"id": "unit-2"}]},
            }
        )
        draft = content(fixture.spec_draft(prompt))
        self.assertEqual(
            [requirement["workUnitId"] for requirement in draft["requirements"]],
            ["unit-1", "unit-2"],
        )
        for requirement in draft["requirements"]:
            self.assertTrue(requirement["acceptance"])
            self.assertTrue(requirement["negative"])

    def test_spec_fails_closed_without_units(self) -> None:
        fixture = load_fixture()
        with self.assertRaises(ValueError):
            fixture.spec_draft(json.dumps({"workGraph": {"units": []}}))


class CounterTests(unittest.TestCase):
    def test_counters_are_per_phase_and_monotonic(self) -> None:
        fixture = load_fixture()
        fixture.COUNTERS.update({phase: 0 for phase in fixture.PHASES})
        fixture.record_phase("planner")
        fixture.record_phase("planner")
        fixture.record_phase("environment")
        counters = fixture.counters()
        self.assertEqual(counters["planner"], 2)
        self.assertEqual(counters["environment"], 1)
        self.assertEqual(counters["spec"], 0)


if __name__ == "__main__":
    unittest.main()
