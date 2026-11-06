#!/usr/bin/env python3
"""Deterministic OpenAI-compatible model fixture for Compose E2E only.

This server is test infrastructure, not a product provider. It receives no
operator, GitHub or provider credential. Assessment responses echo the exact
Project/Action/Request/config/base identities written by the shell to the
read-only context file. Semantic-review responses echo the exact candidate
identities in the reviewer prompt. Usage is reported so Core can settle a
measured model-call reservation.
"""

import json
import os
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

CONTEXT_PATH = Path(os.environ.get("FACTORY_MODEL_CONTEXT", "/context/context.json"))
HOST = "0.0.0.0"
PORT = 8080


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
        "budget": {"maxAttempts": 1, "timeoutSecs": 300},
        "route": {
            "provider": ctx["provider"],
            "model": ctx["model"],
            "mode": "build",
        },
        "maxRepairCycles": 0,
        "maxWallClockSecs": 600,
        "maxParallel": 1,
        "maxSessions": 2,
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

    def send_json(self, status, value):
        body = json.dumps(value).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.send_header("x-factory-test-fixture", "deterministic")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path in ("/health", "/v1/health"):
            self.send_json(200, {"healthy": True, "fixture": "compose-deterministic"})
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
            if "independent semantic code reviewer" in system:
                response = semantic_review(user)
            else:
                response = assessment(user, context())
            self.send_json(200, response)
        except Exception as error:  # noqa: BLE001 - fixture must fail closed
            self.send_json(400, {"error": str(error)})


if __name__ == "__main__":
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    sys.stderr.write("compose-model-fixture: listening on 8080\n")
    server.serve_forever()
