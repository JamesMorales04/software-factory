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
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

CONTEXT_PATH = Path(os.environ.get("FACTORY_MODEL_CONTEXT", "/context/context.json"))
CASSETTE_ENV = os.environ.get("FACTORY_MODEL_CASSETTE", "").strip()
CASSETTE_PATH = Path(CASSETTE_ENV) if CASSETTE_ENV else None
CASSETTE = None
CASSETTE_ERROR = None
HOST = "0.0.0.0"
PORT = 8080


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
            if CASSETTE_ERROR is not None:
                self.send_json(503, {"healthy": False, "cassette_error": CASSETTE_ERROR})
            else:
                mode = "cassette" if CASSETTE_PATH is not None else "deterministic"
                self.send_json(200, {"healthy": True, "fixture": mode})
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
                response = replay_cassette(body)
            elif "independent semantic code reviewer" in system:
                response = semantic_review(user)
            else:
                response = assessment(user, context())
            self.send_json(200, response)
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
