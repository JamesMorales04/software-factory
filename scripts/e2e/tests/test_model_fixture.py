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
    options.append(
        {
            "id": "rust-cli",
            "language": "rust",
            "framework": "clap",
            "summary": "Rust with clap for a small, portable command-line tool.",
            "requiredCapability": "generic-toolchain",
        }
    )
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


ALLOWED_SPDX_SUBSET = {
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "ISC",
    "MIT",
    "MPL-2.0",
    "Unlicense",
    "Zlib",
}
FLOATING_VERSIONS = {"latest", "stable", "nightly", "current", "*", "master", "main"}


def dotted_numeric(value):
    return bool(value) and value.split(".") and all(
        segment.isdigit() and segment for segment in value.split(".")
    )


def nix_attr_ok(attr):
    if not attr or len(attr) > 128:
        return False
    return all(
        part
        and len(part) <= 64
        and all(character.isalnum() or character in "-_" for character in part)
        for part in attr.split(".")
    )


def documented_rust_contract_violations(proposal):
    """Regression mirror of the documented Rust EnvironmentProposal rules.

    This is NOT the validator oracle: the real ``EnvironmentProposal::validate``
    result must come from Core (see the harness R4 ficha). It exists so a
    fixture regression fails a light test before a heavy Compose window.
    """
    violations = []
    allowed = {
        "toolchains",
        "packages",
        "services",
        "commands",
        "nixpkgs",
        "inputs",
        "browsers",
        "allowUnfree",
        "files",
    }
    unknown = set(proposal) - allowed
    if unknown:
        violations.append(f"unknown proposal fields: {sorted(unknown)}")
    if proposal.get("files"):
        violations.append("a model proposal cannot supply generated files")
    if proposal.get("allowUnfree"):
        violations.append("allowUnfree must remain false")
    if proposal.get("nixpkgs") is not None:
        violations.append("fixture omits nixpkgs to use the builder default")

    toolchains = proposal.get("toolchains") or []
    if len(toolchains) > 16:
        violations.append("too many toolchains")
    seen_toolchains = set()
    for toolchain in toolchains:
        toolchain_id = str(toolchain.get("id", ""))
        version = str(toolchain.get("version", ""))
        if not toolchain_id or len(toolchain_id) > 64:
            violations.append(f"toolchain id out of bounds: {toolchain_id!r}")
        if (
            not version
            or any(character.isspace() for character in version)
            or version.lower() in FLOATING_VERSIONS
            or version.startswith(("^", "~"))
            or ">=" in version
            or "<=" in version
        ):
            violations.append(f"toolchain version is not exact: {version!r}")
        lowered = toolchain_id.lower()
        if lowered in seen_toolchains:
            violations.append(f"duplicate toolchain id: {toolchain_id!r}")
        seen_toolchains.add(lowered)

    packages = proposal.get("packages") or []
    if len(packages) > 64:
        violations.append("too many packages")
    seen_packages = set()
    for package in packages:
        name = str(package.get("name", ""))
        version = str(package.get("version", ""))
        license_name = str(package.get("license", ""))
        attr = str(package.get("attr", "")) or name
        source = str(package.get("source", ""))
        if not name or len(name) > 200:
            violations.append(f"package name out of bounds: {name!r}")
        if not dotted_numeric(version):
            violations.append(f"package version is not numeric dotted: {version!r}")
        if license_name in {"", "NONE", "NOASSERTION", "UNLICENSED", "UNKNOWN"}:
            violations.append(f"package license is not usable: {license_name!r}")
        elif license_name not in ALLOWED_SPDX_SUBSET:
            violations.append(f"package license is outside the allowed subset: {license_name!r}")
        if not nix_attr_ok(attr):
            violations.append(f"package attr is not a dotted Nix identifier: {attr!r}")
        if source not in {"nixpkgs", f"nixpkgs:{attr}"}:
            violations.append(
                f"package {name!r} source must be exactly nixpkgs or nixpkgs:{attr} (got {source!r})"
            )
        lowered = name.lower()
        if lowered in seen_packages:
            violations.append(f"duplicate package name: {name!r}")
        seen_packages.add(lowered)

    for field in ("services", "inputs", "browsers"):
        if proposal.get(field):
            violations.append(f"{field} must stay empty in the fixture proposal")

    commands = proposal.get("commands") or {}
    if len(commands) > 32:
        violations.append("too many commands")
    for name, command in commands.items():
        if not name or len(name) > 64:
            violations.append(f"command name out of bounds: {name!r}")
        argv = command.get("argv") or []
        if not argv:
            violations.append(f"command {name!r} has an empty argv")
        for value in argv:
            if not value or any(ord(character) < 32 for character in value):
                violations.append(f"command {name!r} has an invalid argv value")
        cwd = command.get("cwd", ".")
        if cwd != "." and (cwd.startswith(("/", "~")) or ".." in cwd.split("/") or "\\" in cwd):
            violations.append(f"command {name!r} cwd escapes the project root")
        timeout = command.get("timeoutSecs", 300)
        if not isinstance(timeout, int) or not 1 <= timeout <= 7200:
            violations.append(f"command {name!r} timeout is out of bounds")
        expect_exit = command.get("expectExit", 0)
        if not isinstance(expect_exit, int) or not 0 <= expect_exit <= 255:
            violations.append(f"command {name!r} expectExit is out of bounds")

    return violations


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

    def test_fails_closed_for_a_builder_unsupported_option(self) -> None:
        fixture = load_fixture()
        options = stack_options(web=True)
        with self.assertRaises(ValueError) as raised:
            fixture.stack_selection(
                stack_prompt(options, capabilities=("repository-edit", "node-toolchain"))
            )
        self.assertIn("not builder-valid", str(raised.exception))

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


class R4ProposalContractTests(unittest.TestCase):
    """HF01: every fixture package must pin its exact nixpkgs source."""

    def proposals(self):
        fixture = load_fixture()
        proposals = {}
        for option_id, options in (("python-service", stack_options()),):
            selection = content(
                fixture.stack_selection(
                    stack_prompt(options, capabilities=("repository-edit", "generic-toolchain"))
                )
            )
            proposals[option_id] = selection
        return fixture, proposals

    def test_every_package_declares_its_exact_nixpkgs_source(self) -> None:
        _, proposals = self.proposals()
        for option_id, selection in proposals.items():
            for package in selection["environmentProposal"]["packages"]:
                with self.subTest(option=option_id, package=package["name"]):
                    attr = package.get("attr") or package["name"]
                    self.assertIn(package.get("source"), {"nixpkgs", f"nixpkgs:{attr}"})

    def test_all_three_environments_satisfy_the_documented_rust_contract(self) -> None:
        _, proposals = self.proposals()
        self.assertEqual(len(proposals), 1)
        for option_id, selection in proposals.items():
            with self.subTest(option=option_id):
                self.assertEqual(
                    documented_rust_contract_violations(selection["environmentProposal"]), []
                )

    def test_rust_options_are_fail_closed_in_this_pin(self) -> None:
        fixture = load_fixture()
        for option_id in ("rust-cli", "rust-service"):
            with self.subTest(option=option_id):
                option = next(o for o in stack_options() if o["id"] == option_id)
                with self.assertRaises(ValueError) as raised:
                    fixture.stack_selection(stack_prompt([option]))
                self.assertIn("not builder-valid", str(raised.exception))

    def test_missing_source_is_the_hf01_regression(self) -> None:
        import copy

        _, proposals = self.proposals()
        proposal = copy.deepcopy(proposals["python-service"]["environmentProposal"])
        del proposal["packages"][0]["source"]
        violations = documented_rust_contract_violations(proposal)
        self.assertTrue(any("source must be exactly" in item for item in violations), violations)

    def test_pins_match_the_builder_observation(self) -> None:
        _, proposals = self.proposals()
        python = proposals["python-service"]["environmentProposal"]
        self.assertEqual(
            [(p["name"], p["version"], p["license"]) for p in python["packages"]],
            [("fastapi", "0.115.12", "MIT")],
        )
        self.assertEqual(
            [(t["id"], t["version"]) for t in python["toolchains"]],
            [("python3", "3.12")],
        )

    def test_top_level_model_fields_are_bounded(self) -> None:
        _, proposals = self.proposals()
        for option_id, selection in proposals.items():
            with self.subTest(option=option_id):
                self.assertLessEqual(len(selection["optionId"]), 64)
                self.assertTrue(1 <= len(selection["language"]) <= 64)
                self.assertTrue(1 <= len(selection["framework"]) <= 64)
                self.assertTrue(1 <= len(selection["rationale"]) <= 500)



class R5ContextRaceTests(unittest.TestCase):
    """The assessment may be dispatched before the exact intake ids exist."""

    def template(self, directory, *, request_id="", action_id=""):
        import json as json_module
        from pathlib import Path

        context = {
            "schema": "factory-compose-model-fixture-v1",
            "projectId": "project-1",
            "intakeRequestId": "",
            "requestId": request_id,
            "actionId": action_id,
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
        path = Path(directory) / "context.json"
        path.write_text(json_module.dumps(context), encoding="utf-8")
        return context

    def prompt(self):
        return (
            "Objective: Add the deterministic Compose fixture candidate."
            ". Project: project-1. Action: action-1. Request: request-1."
        )

    def test_assessment_answers_from_the_static_template_before_exact_ids(self) -> None:
        import tempfile

        fixture = load_fixture()
        with tempfile.TemporaryDirectory() as directory:
            context = self.template(directory)
            body = content(fixture.assessment(self.prompt(), context))
            self.assertEqual(body["proposal"]["actionId"], "action-1")
            self.assertEqual(body["requestId"], "request-1")

    def test_exact_ids_become_authoritative_once_written(self) -> None:
        import tempfile

        fixture = load_fixture()
        with tempfile.TemporaryDirectory() as directory:
            context = self.template(directory, request_id="other-request", action_id="action-1")
            with self.assertRaises(ValueError):
                fixture.assessment(self.prompt(), context)

    def test_missing_context_still_fails_closed(self) -> None:
        fixture = load_fixture()
        with self.assertRaises(ValueError):
            fixture.assessment(self.prompt(), None)

    def test_dispatch_barrier_never_sees_a_missing_context(self) -> None:
        import tempfile
        import threading

        fixture = load_fixture()
        barrier = threading.Barrier(2)
        result = {}

        with tempfile.TemporaryDirectory() as directory:
            context = self.template(directory)

            def dispatch():
                barrier.wait()
                result["body"] = content(fixture.assessment(self.prompt(), context))

            def finalize():
                barrier.wait()
                # The exact-id rewrite happens after the dispatch has begun; the
                # template written before the POST already makes it answerable.
                self.template(directory, request_id="request-1", action_id="action-1")

            threads = [threading.Thread(target=dispatch), threading.Thread(target=finalize)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
        self.assertEqual(result["body"]["requestId"], "request-1")
        self.assertEqual(result["body"]["proposal"]["actionId"], "action-1")

    def test_phase_errors_are_exposed_for_fast_diagnosis(self) -> None:
        fixture = load_fixture()
        fixture.ERRORS.clear()
        fixture.record_error("assessment", "assessment context is not available")
        diagnostics = fixture.diagnostics()
        self.assertEqual(
            diagnostics["errors"]["|assessment"], "assessment context is not available"
        )
        fixture.ERRORS.clear()


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
