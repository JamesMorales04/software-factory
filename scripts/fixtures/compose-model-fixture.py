#!/usr/bin/env python3
"""Deterministic OpenAI-compatible model fixture for Compose E2E only.

This server is test infrastructure, not a product provider. It receives no
operator, GitHub or provider credential. Assessment responses echo the exact
Project/Action/Request/config/base identities written by the shell to the
read-only context file. Semantic-review responses echo the exact candidate
identities in the reviewer prompt. Usage is reported so Core can settle a
measured model-call reservation.
"""

import hashlib
import json
import os
import re
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

CONTEXT_PATH = Path(os.environ.get("FACTORY_MODEL_CONTEXT", "/context/context.json"))
CASSETTE_ENV = os.environ.get("FACTORY_MODEL_CASSETTE", "").strip()
CASSETTE_PATH = Path(CASSETTE_ENV) if CASSETTE_ENV else None
CASSETTE = None
CASSETTE_ERROR = None
HOST = "0.0.0.0"
PORT = 8080

# Per-phase counters are test-support evidence only. They are read from the
# fixture container by the scenario harness and never presented as product
# inference or provider usage.
PHASES = (
    "assessment",
    "lead_stack_selection",
    "planner",
    "environment",
    "spec",
    "semantic_review",
    "other",
)
COUNTERS = {phase: 0 for phase in PHASES}
COUNTERS_LOCK = threading.Lock()


def record_phase(phase):
    with COUNTERS_LOCK:
        COUNTERS[phase] = COUNTERS.get(phase, 0) + 1
        return dict(COUNTERS)


def counters():
    with COUNTERS_LOCK:
        return dict(COUNTERS)


def classify_prompt(system, user=""):
    # Greenfield stack selection is its own bounded Lead phase; it must be
    # matched before the generic assessment fallback, because its user prompt is
    # JSON without the assessment identity block.
    if "bounded Software Factory Stack Selector" in system:
        return "lead_stack_selection"
    if "bounded Software Factory Planner" in system:
        return "planner"
    if "bounded Software Factory Spec Manager" in system:
        return "spec"
    if "Review exactly one frozen Candidate EnvironmentPlan" in system:
        return "environment"
    if "independent semantic code reviewer" in system:
        return "semantic_review"
    if "Semantic Assessor" in system or "assessment" in system.lower():
        return "assessment"
    # The baseline assessment prompt is identified by its identity block rather
    # than by a role name; count it as an assessment only when it looks like one.
    if re.search(r"Project: \S+", user) and re.search(r"Action: \S+", user):
        return "assessment"
    return "other"



def canonical_request_sha256(value):
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def load_cassette():
    global CASSETTE, CASSETTE_ERROR
    if CASSETTE_PATH is None:
        return
    try:
        with CASSETTE_PATH.open("r", encoding="utf-8") as handle:
            payload = json.load(handle)
        if payload.get("schema") != "factory-provider-cassette-v1":
            raise ValueError("cassette schema is not factory-provider-cassette-v1")
        turns = payload.get("turns")
        if not isinstance(turns, list) or not turns:
            raise ValueError("cassette has no turns")
        CASSETTE = payload
    except Exception as error:  # noqa: BLE001 - fail closed and report on /health
        CASSETTE_ERROR = str(error)


def replay_cassette(request):
    if CASSETTE is None:
        raise ValueError(f"cassette is unavailable: {CASSETTE_ERROR or 'not loaded'}")
    digest = canonical_request_sha256(request)
    turn = next((item for item in CASSETTE["turns"] if item.get("requestSha256") == digest), None)
    if turn is None:
        raise ValueError("cassette has no request matching the exact canonical digest")
    return turn["response"]


def context():
    try:
        with CONTEXT_PATH.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except FileNotFoundError:
        return None
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError("model context is unreadable") from error


def chat_response(content, input_tokens, output_tokens):
    return {
        "choices": [
            {"message": {"role": "assistant", "content": json.dumps(content)}}
        ],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens,
        },
    }


def assessment(user, ctx):
    if ctx is None:
        raise ValueError("assessment context is not available")
    project = re.search(r"Project: ([^\s.]+)", user)
    action = re.search(r"Action: ([^\s.]+)", user)
    request = re.search(r"Request: ([^\s.]+)", user)
    objective = re.search(r"Objective: ([\s\S]*?)\. Project: ", user)
    if not all((project, action, request, objective)):
        raise ValueError("assessment prompt identities are missing")
    if (
        project.group(1) != ctx["projectId"]
        or action.group(1) != ctx["actionId"]
        or request.group(1) != ctx["requestId"]
    ):
        raise ValueError("assessment prompt identities do not match the context")
    if objective.group(1) != ctx["objective"]:
        raise ValueError("assessment objective does not match the context")

    checks = ctx["checks"]
    acceptance = [check for check in checks if check["kind"] != "negative"]
    negative = [check for check in checks if check["kind"] == "negative"]
    proposal = {
        "projectId": ctx["projectId"],
        "actionId": ctx["actionId"],
        "baseRevision": ctx["baseRevision"],
        "projectConfigHash": ctx["projectConfigHash"],
        "objective": ctx["objective"],
        "class": "feature",
        "assumptions": ["Deterministic Compose model fixture; not live inference."],
        "risks": ["Model output is test data."],
        "acceptanceCriteria": acceptance,
        "negativeCases": negative,
        "qualityProfile": ctx["qualityProfile"],
        "riskClass": "R0",
        "budget": {"timeoutSecs": 300},
        "route": {
            "provider": ctx["provider"],
            "model": ctx["model"],
            "mode": "build",
        },
        "maxParallel": 1,
        "workUnits": [
            {
                "id": "unit-1",
                "kind": "implementation",
                "description": ctx["objective"],
                "dependsOn": [],
                "allowedPaths": ctx["allowedPaths"],
                "capabilities": ctx["capabilities"],
                "acceptanceCriteria": [],
                "negativeCases": [],
            }
        ],
    }
    return chat_response(
        {
            "schemaRevision": 1,
            "projectId": ctx["projectId"],
            "actionId": ctx["actionId"],
            "requestId": ctx["requestId"],
            "evidenceRefs": ["repository-files"],
            "intent": "change",
            "complexity": "small",
            "effortEstimate": {
                "status": "estimated",
                "unit": "person-hours",
                "minPersonHours": 1,
                "maxPersonHours": 2,
                "assumptions": ["Single deterministic Compose unit."],
                "unknownReason": "",
            },
            "knowledgeGaps": [],
            "changeImpact": ["src"],
            "validationNeeds": ["Run the frozen checks."],
            "assumptions": ["Deterministic Compose model fixture."],
            "uncertainty": ["Not live provider inference."],
            "suggestedNextAction": "Freeze and approve the plan.",
            "proposal": proposal,
        },
        500,
        500,
    )


# Deterministic fixture environments for the stack options Core can offer.
# These are test data only: Core owns the real catalog, validates the selected
# option and capability, and compiles the environment. The proposal must cover
# the selected language and framework with an exact toolchain id, package
# name/nix attribute segment or command executable (see
# native/src/planning/stack.rs), so each entry carries that token explicitly.
STACK_SELECTION_PREFERENCE = (
    "python-service",
    "rust-service",
    "rust-cli",
    "typescript-react",
)

STACK_ENVIRONMENTS = {
    "python": {
        "toolchains": [{"id": "python3", "version": "3.12"}],
        "packages": [
            {
                "name": "python3",
                "version": "3.12",
                "license": "MIT",
                "attr": "python312",
            },
            {
                "name": "fastapi",
                "version": "0.110.0",
                "license": "MIT",
                "attr": "python312Packages.fastapi",
            },
        ],
        "commands": {
            "build": ["python3", "-m", "compileall", "-q", "src"],
            "test": ["python3", "-m", "pytest", "-q"],
            "lint": ["python3", "-m", "ruff", "check", "src"],
            "format": ["python3", "-m", "ruff", "format", "--check", "src"],
            "typecheck": ["python3", "-m", "mypy", "src"],
            "run": ["python3", "-m", "src"],
        },
    },
    "rust": {
        "toolchains": [{"id": "rustc", "version": "1.75.0"}],
        "packages": [
            {
                "name": "rustc",
                "version": "1.75.0",
                "license": "MIT",
                "attr": "rustc",
            },
            {
                "name": "cargo",
                "version": "1.75.0",
                "license": "MIT",
                "attr": "cargo",
            },
            {
                "name": "axum",
                "version": "0.7.5",
                "license": "MIT",
                "attr": "axum",
            },
            {
                "name": "clap",
                "version": "4.5.4",
                "license": "MIT",
                "attr": "clap",
            },
        ],
        "commands": {
            "build": ["cargo", "build"],
            "test": ["cargo", "test"],
            "lint": ["cargo", "clippy", "--", "-D", "warnings"],
            "format": ["cargo", "fmt", "--check"],
            "typecheck": ["cargo", "check"],
            "run": ["cargo", "run"],
        },
    },
    "typescript": {
        "toolchains": [{"id": "nodejs", "version": "20.11.0"}],
        "packages": [
            {
                "name": "nodejs",
                "version": "20.11.0",
                "license": "MIT",
                "attr": "nodejs",
            },
            {
                "name": "react",
                "version": "18.2.0",
                "license": "MIT",
                "attr": "react",
            },
        ],
        "commands": {
            "build": ["npm", "run", "build"],
            "test": ["npm", "test"],
            "lint": ["npm", "run", "lint"],
            "format": ["npm", "run", "format"],
            "typecheck": ["npm", "run", "typecheck"],
            "run": ["npm", "start"],
        },
    },
}


def stack_selection(user):
    """One deterministic StackSelectionModel + concrete environment proposal.

    The catalog, capability check and environment coverage are validated by
    Core; this fixture only returns stable test data for an authorized option.
    It never adds options, capabilities or paths, and the output is SIMULATED.
    """
    payload = json.loads(user)
    options = payload.get("options") or []
    capabilities = set(payload.get("capabilities") or [])
    if not options:
        raise ValueError("stack selection prompt has no deterministic options")
    authorized = [
        option
        for option in options
        if isinstance(option, dict)
        and option.get("requiredCapability") in capabilities
    ]
    if not authorized:
        raise ValueError("stack selection has no authorized deterministic option")
    by_id = {option.get("id"): option for option in authorized}
    chosen = next(
        (by_id[option_id] for option_id in STACK_SELECTION_PREFERENCE if option_id in by_id),
        authorized[0],
    )
    language = str(chosen.get("language") or "").strip().lower()
    environment = STACK_ENVIRONMENTS.get(language)
    if environment is None:
        raise ValueError(f"stack selection has no fixture environment for {language!r}")
    proposal = {
        "toolchains": environment["toolchains"],
        "packages": environment["packages"],
        "commands": {
            name: {"argv": list(argv)} for name, argv in environment["commands"].items()
        },
    }
    rationale = (
        f"Selects {chosen.get('id')} because the objective is a small deterministic "
        "Compose service and the project authorizes its required capability."
    )
    return chat_response(
        {
            "optionId": chosen.get("id"),
            "language": chosen.get("language"),
            "framework": chosen.get("framework"),
            "rationale": rationale,
            "environmentProposal": proposal,
        },
        400,
        300,
    )


def planner(user, ctx):
    payload = json.loads(user)
    allowed = (payload.get("projectConfig") or {}).get("allowedPaths") or ["src"]
    scope = str(allowed[0]).rstrip("/") or "src"
    objective = payload.get("objective") or (ctx or {}).get("objective") or "fixture objective"
    return chat_response(
        {
            "units": [
                {
                    "id": "unit-1",
                    "title": "Implement the requested deterministic change",
                    "objective": objective,
                    "dependsOn": [],
                    "paths": [f"{scope}/provider.txt"],
                    "acceptance": ["The frozen functional check passes."],
                }
            ],
            "assumptions": ["Deterministic Compose model fixture; not live inference."],
            "risks": ["Model output is test data."],
        },
        400,
        300,
    )


def environment_review(user):
    marker = "EnvironmentPlan:\n"
    start = user.find(marker)
    if start < 0:
        raise ValueError("environment review prompt has no EnvironmentPlan")
    start += len(marker)
    end = user.find("\n\nJSON Schema:", start)
    if end < 0:
        raise ValueError("environment review prompt has no schema boundary")
    plan = json.loads(user[start:end])
    return chat_response(
        {
            "schemaVersion": 1,
            "requestId": plan["requestId"],
            "actionId": plan["actionId"],
            "planSha256": plan["planSha256"],
            "planRevision": plan["revision"],
            "riskClass": plan["riskClass"],
            "decision": "APPROVE",
            "rationale": "The deterministic fixture plan is pinned, licensed and bounded.",
            "constraints": [],
        },
        300,
        80,
    )


def spec_draft(user):
    payload = json.loads(user)
    units = ((payload.get("workGraph") or {}).get("units")) or []
    ids = [unit.get("id") for unit in units if unit.get("id")]
    if not ids:
        raise ValueError("spec prompt has no work graph units")
    requirements = [
        {
            "statement": f"Unit {unit_id} exposes the frozen observable behavior.",
            "workUnitId": unit_id,
            "acceptance": ["The frozen functional check passes."],
            "negative": ["The frozen negative check fails for absent content."],
        }
        for unit_id in ids
    ]
    return chat_response(
        {
            "summary": "Observable formal requirements for the deterministic fixture.",
            "requirements": requirements,
            "assumptions": ["Deterministic Compose model fixture; not live inference."],
            "risks": ["Model output is test data."],
        },
        400,
        400,
    )


def semantic_review(user):
    commit = re.search(r"candidateCommitSha: ([0-9a-f]+)", user)
    tree = re.search(r"candidateTreeSha: ([0-9a-f]+)", user)
    spec = re.search(r"actionSpecSha256: ([0-9a-f]+)", user)
    if not all((commit, tree, spec)):
        raise ValueError("semantic review prompt identities are missing")
    return chat_response(
        {
            "schemaRevision": 1,
            "candidateCommitSha": commit.group(1),
            "candidateTreeSha": tree.group(1),
            "actionSpecSha256": spec.group(1),
            "verdict": "approve",
            "summary": "The deterministic Compose fixture satisfies the frozen objective.",
            "confidence": 90,
            "findings": [],
        },
        300,
        100,
    )


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        # Never log prompt bodies, credentials or downstream URLs.
        sys.stderr.write("compose-model-fixture: " + fmt % args + "\n")

    def send_json(self, status, value, *, phase=None):
        body = json.dumps(value).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.send_header("x-factory-test-fixture", "deterministic")
        if phase:
            self.send_header("x-factory-fixture-phase", phase)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path in ("/health", "/v1/health"):
            if CASSETTE_ERROR is not None:
                self.send_json(503, {"healthy": False, "cassette_error": CASSETTE_ERROR})
            else:
                mode = "cassette" if CASSETTE_PATH is not None else "deterministic"
                self.send_json(200, {"healthy": True, "fixture": mode})
        elif self.path in ("/counters", "/v1/counters"):
            self.send_json(200, {"schema": "factory-model-fixture-counters-v1", "counters": counters()})
        else:
            self.send_json(404, {"error": "not found"})

    def do_POST(self):
        if self.path not in ("/v1/chat/completions", "/chat/completions"):
            self.send_json(404, {"error": "not found"})
            return
        length = int(self.headers.get("content-length", "0"))
        if length <= 0 or length > 2 * 1024 * 1024:
            self.send_json(400, {"error": "invalid request body"})
            return
        try:
            body = json.loads(self.rfile.read(length))
            messages = body.get("messages") or []
            system = next(
                (item.get("content", "") for item in messages if item.get("role") == "system"),
                "",
            )
            user = next(
                (item.get("content", "") for item in messages if item.get("role") == "user"),
                "",
            )
            if CASSETTE_PATH is not None:
                phase = "cassette"
                response = replay_cassette(body)
            else:
                phase = classify_prompt(system, user)
                record_phase(phase)
                if phase == "planner":
                    response = planner(user, context())
                elif phase == "lead_stack_selection":
                    response = stack_selection(user)
                elif phase == "environment":
                    response = environment_review(user)
                elif phase == "spec":
                    response = spec_draft(user)
                elif phase == "semantic_review":
                    response = semantic_review(user)
                else:
                    response = assessment(user, context())
            self.send_json(200, response, phase=phase)
        except Exception as error:  # noqa: BLE001 - fixture must fail closed
            self.send_json(400, {"error": str(error)})


if __name__ == "__main__":
    load_cassette()
    if CASSETTE_ERROR is not None:
        sys.stderr.write(f"compose-model-fixture: cassette error: {CASSETTE_ERROR}\n")
    mode = "cassette" if CASSETTE_PATH is not None else "deterministic"
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    sys.stderr.write(f"compose-model-fixture: {mode} mode listening on 8080\n")
    server.serve_forever()
