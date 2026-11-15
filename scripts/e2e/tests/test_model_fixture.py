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
            "bounded Software Factory Stack Selector": "lead_stack_selection",
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


def stack_options(*, web=False, include_python=True):
    options = [
        {
            "id": "rust-service",
            "language": "rust",
            "framework": "axum",
            "summary": "Rust, axum and SQLx for a typed local service.",
            "requiredCapability": "generic-toolchain",
        }
    ]
    if include_python:
        options.append(
            {
                "id": "python-service",
                "language": "python",
                "framework": "fastapi",
                "summary": "Python and FastAPI for a compact service.",
                "requiredCapability": "generic-toolchain",
            }
        )
    if web:
        options.append(
            {
                "id": "typescript-react",
                "language": "typescript",
                "framework": "react",
                "summary": "TypeScript, React and Vite for a browser application.",
                "requiredCapability": "node-toolchain",
            }
        )
    return options


def stack_prompt(options, capabilities=("repository-edit", "generic-toolchain")):
    return json.dumps(
        {
            "objective": "Add the deterministic Compose fixture candidate and cover it with frozen checks.",
            "projectId": "project-1",
            "allowedPaths": ["src"],
            "capabilities": list(capabilities),
            "options": options,
        }
    )


LANGUAGE_ALIASES = {
    "python": {"python", "python3", "pip", "pytest"},
    "rust": {"rust", "rustc", "cargo"},
    "typescript": {"nodejs", "node", "typescript", "javascript", "npm", "pnpm", "yarn", "bun"},
}


def proposal_tokens(proposal):
    tokens = set()
    for toolchain in proposal.get("toolchains", []):
        tokens.add(str(toolchain.get("id", "")))
    for package in proposal.get("packages", []):
        tokens.add(str(package.get("name", "")))
        tokens.update(str(package.get("attr", "")).split("."))
    for command in proposal.get("commands", {}).values():
        argv = command.get("argv") or []
        if argv:
            tokens.add(str(argv[0]))
    return {token.strip().lower() for token in tokens if token and token.strip()}


class StackSelectionResponseTests(unittest.TestCase):
    def test_selects_an_authorized_option_and_echoes_its_identity(self) -> None:
        fixture = load_fixture()
        selection = content(fixture.stack_selection(stack_prompt(stack_options())))
        self.assertIn(selection["optionId"], {"rust-service", "python-service"})
        self.assertTrue(selection["language"])
        self.assertTrue(selection["framework"])
        self.assertTrue(selection["rationale"])
        self.assertLessEqual(len(selection["rationale"]), 500)

    def test_proposal_covers_the_selected_language_and_framework(self) -> None:
        fixture = load_fixture()
        for option_id, language, framework in (
            ("python-service", "python", "fastapi"),
            ("rust-service", "rust", "axum"),
        ):
            with self.subTest(option_id=option_id):
                options = [option for option in stack_options() if option["id"] == option_id]
                selection = content(fixture.stack_selection(stack_prompt(options)))
                self.assertEqual(selection["optionId"], option_id)
                self.assertEqual(selection["language"], language)
                self.assertEqual(selection["framework"], framework)
                proposal = selection["environmentProposal"]
                tokens = proposal_tokens(proposal)
                self.assertTrue(tokens & LANGUAGE_ALIASES[language], tokens)
                self.assertIn(framework, tokens)
                self.assertTrue(proposal.get("toolchains"))
                self.assertTrue(proposal.get("packages"))
                self.assertTrue(proposal.get("commands"))

    def test_proposal_is_bounded_and_cannot_supply_generated_files(self) -> None:
        fixture = load_fixture()
        proposal = content(
            fixture.stack_selection(stack_prompt(stack_options()))
        )["environmentProposal"]
        self.assertFalse(proposal.get("files"))
        self.assertFalse(proposal.get("allowUnfree"))
        self.assertNotIn("nixpkgs", proposal)
        for command in proposal["commands"].values():
            self.assertTrue(command["argv"])
            self.assertEqual(command["argv"][0], command["argv"][0].strip())

    def test_selects_the_only_authorized_option_when_capability_is_narrow(self) -> None:
        fixture = load_fixture()
        options = stack_options(web=True)
        selection = content(
            fixture.stack_selection(
                stack_prompt(options, capabilities=("repository-edit", "node-toolchain"))
            )
        )
        self.assertEqual(selection["optionId"], "typescript-react")
        tokens = proposal_tokens(selection["environmentProposal"])
        self.assertIn("react", tokens)

    def test_fails_closed_without_an_authorized_option(self) -> None:
        fixture = load_fixture()
        with self.assertRaises(ValueError):
            fixture.stack_selection(
                stack_prompt(stack_options(), capabilities=("repository-edit",))
            )

    def test_fails_closed_without_options(self) -> None:
        fixture = load_fixture()
        with self.assertRaises(ValueError):
            fixture.stack_selection(stack_prompt([]))


class PhaseTraversalTests(unittest.TestCase):
    """Pin the observed Lead phase order so a missing phase fails a light test."""

    def test_every_lead_phase_is_classified_and_answers_a_schema_shaped_object(self) -> None:
        fixture = load_fixture()
        fixture.COUNTERS.update({phase: 0 for phase in fixture.PHASES})
        ctx = {
            "projectId": "project-1",
            "actionId": "action-1",
            "requestId": "request-1",
            "objective": "Add the deterministic Compose fixture candidate.",
            "baseRevision": "base-1",
            "projectConfigHash": "b" * 64,
            "qualityProfile": "generic",
            "provider": "fixture",
            "model": "fixture-model",
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "checks": [
                {
                    "kind": "functional",
                    "description": "candidate content present",
                    "command": ["grep", "-q", "provider candidate", "src/provider.txt"],
                }
            ],
        }
        plan = {
            "requestId": "request-1",
            "actionId": "action-1",
            "planSha256": "c" * 64,
            "revision": 1,
            "riskClass": "R0",
        }
        phases = [
            (
                "assessment",
                "bounded Software Factory Semantic Assessor",
                f"Objective: {ctx['objective']}. Project: project-1. Action: action-1. Request: request-1.",
                lambda user: fixture.assessment(user, ctx),
                {"proposal", "schemaRevision", "projectId"},
            ),
            (
                "lead_stack_selection",
                "You are the bounded Software Factory Stack Selector.",
                stack_prompt(stack_options()),
                fixture.stack_selection,
                {"optionId", "language", "framework", "rationale", "environmentProposal"},
            ),
            (
                "planner",
                "bounded Software Factory Planner",
                json.dumps({"objective": "x", "projectConfig": {"allowedPaths": ["src"]}}),
                lambda user: fixture.planner(user, None),
                {"units"},
            ),
            (
                "environment",
                "Review exactly one frozen Candidate EnvironmentPlan",
                "EnvironmentPlan:\n"
                + json.dumps(plan)
                + "\n\nJSON Schema:\n{}",
                fixture.environment_review,
                {"decision", "requestId", "planSha256"},
            ),
            (
                "spec",
                "bounded Software Factory Spec Manager",
                json.dumps({"workGraph": {"units": [{"id": "unit-1"}]}}),
                fixture.spec_draft,
                {"requirements", "summary"},
            ),
            (
                "semantic_review",
                "independent semantic code reviewer",
                "candidateCommitSha: " + "d" * 40 + " candidateTreeSha: " + "e" * 40
                + " actionSpecSha256: " + "f" * 64,
                fixture.semantic_review,
                {"verdict", "candidateCommitSha"},
            ),
        ]
        for phase, system, user, call, required in phases:
            with self.subTest(phase=phase):
                self.assertEqual(fixture.classify_prompt(system, user), phase)
                fixture.record_phase(phase)
                body = content(call(user))
                self.assertTrue(required.issubset(body), body)
        counters = fixture.counters()
        self.assertEqual(counters["other"], 0)
        for phase, *_ in phases:
            self.assertEqual(counters[phase], 1, phase)


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
        self.assertEqual(counters["lead_stack_selection"], 0)


if __name__ == "__main__":
    unittest.main()
