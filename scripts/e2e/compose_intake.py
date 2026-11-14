"""Stage definitions for the deterministic Compose intake scenario.

Each stage is idempotent with respect to the state file it is given. A stage
never assumes that a previous shell process is still alive; it reads the exact
identity from `state.json` and re-verifies the live stack. This is what makes
`--from`/`--to` useful after a late failure.

The stage names are intentionally stable because they are part of the operator
interface and of the scenario manifest.
"""

from __future__ import annotations

import json
import os
import subprocess
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable

from common import (
    Api,
    Compose,
    Config,
    ScenarioError,
    State,
    assert_equal,
    assert_in,
    ensure_dir,
    log,
    now_utc,
    require,
    restore_volumes,
    tail,
    wait_core,
    wait_until,
    wait_url,
    write_json,
)


@dataclass
class Context:
    config: Config
    state: State
    compose: Compose
    api: Api

    def admit_scope(self, scope: str) -> None:
        # Test-support admission returns 204 with no body; use the raw request
        # path so an empty payload is not treated as a non-JSON failure.
        self.api.request(
            "POST", "/test-support/subscription-dispatch-admissions", {"scope": scope}
        )

    def require_api(self) -> Api:
        return self.api


STAGES: list[tuple[str, Callable[[Context], None], str]] = []


def stage(name: str, description: str):
    def register(function: Callable[[Context], None]) -> Callable[[Context], None]:
        STAGES.append((name, function, description))
        return function

    return register


def _git(cwd: Path, arguments: list[str]) -> str:
    try:
        result = subprocess.run(
            ["git", *arguments],
            cwd=cwd,
            text=True,
            capture_output=True,
            timeout=60,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ScenarioError(f"git {' '.join(arguments)} failed in {cwd}: {error}") from error
    if result.returncode != 0:
        raise ScenarioError(
            f"git {' '.join(arguments)} failed in {cwd}: {tail(result.stderr)}"
        )
    return result.stdout.strip()


@stage("prepare", "create disposable Git fixture, model context directory and run layout")
def prepare(context: Context) -> None:
    config = context.config
    ensure_dir(config.run_dir)
    ensure_dir(config.artifacts_dir)
    ensure_dir(config.git_dir)
    ensure_dir(config.model_dir)
    ensure_dir(config.run_dir / "tmp")
    os.chmod(config.model_dir, 0o755)
    if config.cassette is not None:
        target = config.model_dir / "cassette.json"
        target.write_bytes(config.cassette.read_bytes())
        os.chmod(target, 0o644)
        context.state.set("cassette", str(target))

    source = config.git_dir / "source"
    server = config.git_dir / "server"
    ensure_dir(source / "src")
    ensure_dir(server)
    if not (source / ".git").exists():
        _git(source, ["init", "-q"])
        _git(source, ["config", "user.name", "Compose E2E"])
        _git(source, ["config", "user.email", "compose-e2e@example.invalid"])
        (source / "src" / "seed.txt").write_text("seed\n", encoding="utf-8")
        _git(source, ["add", "src/seed.txt"])
        _git(source, ["commit", "-qm", "seed"])
    repository = server / "repo.git"
    if not repository.exists():
        _git(server, ["clone", "-q", "--bare", str(source), str(repository)])
        _git(repository, ["update-server-info"])
    context.state.set("git_server", str(repository))
    context.state.set("model_dir", str(config.model_dir))
    context.state.set("run_dir", str(config.run_dir))


@stage("start", "start the canonical Compose stack and wait for every readiness boundary")
def start(context: Context) -> None:
    config = context.config
    context.state.require("git_server", "model_dir")
    if config.restore:
        restore_volumes(config, context.compose)
    else:
        context.compose.down(volumes=True)
    context.compose.up(build=not config.no_build)
    core = context.compose.core_container()
    require(bool(core), "Core container was not created")
    context.state.set("core_container", core)

    wait_url(f"{config.base}/health", 300)
    wait_url(f"{config.base}/core-api/health", 60)
    wait_core(context.compose, "http://supervisor:8766/health", 180)
    wait_core(context.compose, "http://runtime:8770/health", config.timeout)
    wait_core(context.compose, "http://git-fixture:8080/repo.git/HEAD", 60)
    wait_core(context.compose, "http://model-fixture:8080/health", 120)


@stage("enroll", "perform first-run owner enrollment and wait for the Worker")
def enroll(context: Context) -> None:
    config = context.config
    code = context.compose.exec_core(["/usr/local/bin/factory-core", "issue-bootstrap-code"]).strip()
    require(code != "OWNER_ALREADY_ENROLLED", "fresh stack was already enrolled")
    context.api.request(
        "POST",
        "/auth/bootstrap",
        {"code": code, "deviceName": "Compose E2E"},
    )
    status = context.api.text("/setup/status")
    assert_in(status, '"required":false', "enrollment did not invalidate the setup requirement")

    def worker_ready() -> bool:
        workers = context.api.get("/workers")
        return any("compose-worker" in json.dumps(worker) for worker in workers)

    wait_until(worker_ready, 60, "the Compose Worker registration")
    context.state.set("enrolled_at", now_utc())


@stage("import", "import the disposable Git repository and freeze its project configuration")
def import_project(context: Context) -> None:
    project = context.api.post(
        "/projects",
        {
            "repository": "http://git-fixture:8080/repo.git",
            "idempotency_key": "e2e-project",
        },
    )
    project_id = project["id"]
    config = context.api.get(f"/projects/{project_id}/config")
    updated = context.api.put(
        f"/projects/{project_id}/config",
        {
            "expectedRevision": config["revision"],
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "qualityProfile": "generic",
            "active": True,
        },
    )
    context.state.set("project_id", project_id)
    context.state.set("project_config_hash", updated["configHash"])
    context.state.set("project_base_commit", updated["baseCommit"])


@stage("freeze", "freeze an Action with functional and negative acceptance checks")
def freeze_action(context: Context) -> None:
    context.state.require("project_id", "project_base_commit")
    action = context.api.post(
        "/actions",
        {
            "project_id": context.state.get("project_id"),
            "objective": "compose deterministic candidate",
            "provider": "opencode",
            "model": "deepseek-v4.1-flash",
            "idempotency_key": "e2e-action",
            "allowed_paths": ["src"],
            "checks": [
                {
                    "id": "functional",
                    "kind": "functional",
                    "description": "candidate content present",
                    "command": ["grep", "-q", "provider candidate", "src/provider.txt"],
                    "expectExit": 0,
                    "timeoutSecs": 30,
                },
                {
                    "id": "negative",
                    "kind": "negative",
                    "description": "absent content rejected",
                    "command": ["grep", "-q", "absent", "src/provider.txt"],
                    "expectExit": 1,
                    "timeoutSecs": 30,
                },
            ],
        },
    )
    context.state.set("action_id", action["id"])


@stage("delegate", "create the bounded owner delegation for the fixture Campaign")
def delegate(context: Context) -> None:
    context.state.require("project_id")
    expires = (
        datetime.now(timezone.utc) + timedelta(days=1)
    ).isoformat().replace("+00:00", "Z")
    context.api.post(
        "/delegations",
        {
            "delegation": {
                "id": "e2e-grant",
                "principalId": "owner",
                "auditId": "e2e",
                "projectId": context.state.get("project_id"),
                "revision": 1,
                "contentHash": "",
                "objectiveClasses": ["feature"],
                "allowedPaths": ["src"],
                "capabilities": ["repository-edit", "generic-toolchain"],
                "riskClasses": ["R0", "R1"],
                "autonomousStart": False,
                "autonomousRiskClasses": [],
                "route": {
                    "provider": "opencode",
                    "model": "deepseek-v4.1-flash",
                    "mode": "build",
                },
                "maxAttempts": 2,
                "maxTimeoutSecs": 300,
                "maxRepairCycles": 0,
                "maxWallClockSecs": 1800,
                "maxParallel": 1,
                "maxSessions": 16,
                "maxRequests": 4,
                "maxCumulativeSessions": 32,
                "maxCumulativeAttempts": 32,
                "expiresAt": expires,
            },
            "idempotency_key": "e2e-delegation",
        },
    )


@stage("settings", "configure the deterministic model gateway, execution profile and project policy")
def settings(context: Context) -> None:
    context.state.require("project_id", "project_config_hash")
    _configure_fixture_providers(context)
    role = {
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "reasoning": "medium",
    }
    profile = context.api.post(
        "/execution-profiles",
        {
            "name": "Compose Intake Balanced",
            "description": "Deterministic Compose intake profile",
            "category": "BALANCED",
            "roles": {
                "lead": role,
                "implementer": role,
                "semanticReviewer": role,
            },
            "operations": {
                "modelRequestTimeoutSecs": 300,
                "toolCallTimeoutSecs": 600,
                "maxRequestBytes": 2097152,
                "maxResponseBytes": 2097152,
                "maxGenerationTokens": 32768,
                "maxInFlightOperations": 2,
            },
            "progress": {
                "diagnoseAfterStagnantCheckpoints": 2,
                "escalateAfterUnsuccessfulReplans": 1,
                "observationTimeoutSecs": 300,
            },
            "auto_start": {"enabled": False, "risk_classes": []},
        },
    )
    assert_equal(profile.get("executable"), True, "execution profile is not executable")
    context.api.post(f"/execution-profiles/{profile['id']}/default", {"scope": "global"})
    context.api.post(
        f"/projects/{context.state.get('project_id')}/execution-policy",
        {
            "execution_profile_id": profile["id"],
            "expected_revision": profile["revision"],
            "expected_revision_hash": profile["contentHash"],
            "project_config_hash": context.state.get("project_config_hash"),
            "acknowledge": True,
            "idempotency_key": "e2e-intake-policy",
        },
    )
    context.state.set("profile_id", profile["id"])
    context.state.set("profile_revision", profile["revision"])
    context.state.set("profile_revision_id", profile["revisionId"])
    context.state.set("profile_hash", profile["contentHash"])


@stage("intake", "create the unified intake request and its deterministic model context")
def intake(context: Context) -> None:
    context.state.require(
        "project_id",
        "action_id",
        "profile_id",
        "project_base_commit",
        "project_config_hash",
        "model_dir",
    )
    objective = "Add the deterministic Compose fixture candidate and cover it with frozen checks."
    created = context.api.post(
        "/intake/requests",
        {
            "project_id": context.state.get("project_id"),
            "objective": objective,
            "execution_profile_id": context.state.get("profile_id"),
            "mode": "REVIEW_PLAN",
            "idempotency_key": "e2e-intake-request",
            "authorized_micros": 5000000,
        },
    )
    intake_id = created["id"]
    action_id = created["actionId"]
    lead_request_id = created["leadRequestId"]
    context.admit_scope(intake_id)
    context.admit_scope(lead_request_id)
    # Approval creates the Campaign id inside Core; the disposable E2E database
    # admits '*' once. Persisted funding policies still take precedence.
    context.admit_scope("*")

    model_context = {
        "schema": "factory-compose-model-fixture-v1",
        "projectId": context.state.get("project_id"),
        "intakeRequestId": intake_id,
        "requestId": lead_request_id,
        "actionId": action_id,
        "objective": objective,
        "baseRevision": context.state.get("project_base_commit"),
        "projectConfigHash": context.state.get("project_config_hash"),
        "qualityProfile": "generic",
        "allowedPaths": ["src"],
        "capabilities": ["repository-edit", "generic-toolchain"],
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "checks": [
            {
                "id": "build",
                "kind": "build",
                "description": "deterministic build",
                "command": ["true"],
                "expectExit": 0,
                "timeoutSecs": 30,
            },
            {
                "id": "test",
                "kind": "test",
                "description": "deterministic test",
                "command": ["true"],
                "expectExit": 0,
                "timeoutSecs": 30,
            },
            {
                "id": "functional",
                "kind": "functional",
                "description": "provider candidate content exists",
                "command": ["grep", "-q", "provider candidate", "src/provider.txt"],
                "expectExit": 0,
                "timeoutSecs": 30,
            },
            {
                "id": "negative",
                "kind": "negative",
                "description": "absent content is rejected",
                "command": ["grep", "-q", "absent", "src/provider.txt"],
                "expectExit": 1,
                "timeoutSecs": 30,
            },
        ],
    }
    context_path = Path(context.state.get("model_dir")) / "context.json"
    write_json(context_path, model_context)
    os.chmod(context_path, 0o644)

    context.state.set("intake_id", intake_id)
    context.state.set("intake_action_id", action_id)
    context.state.set("intake_lead_request_id", lead_request_id)
    context.state.set("intake_objective", objective)
    context.state.set("model_context", str(context_path))


@stage("assess", "run the bounded Lead assessment and freeze the plan hash")
def assess(context: Context) -> None:
    context.state.require("intake_id")
    assessed = context.api.post(f"/intake/requests/{context.state.get('intake_id')}/assess")
    assert_equal(assessed.get("state"), "AWAITING_APPROVAL", "intake assessment state")
    plan_hash = (assessed.get("plan") or {}).get("plan_hash")
    require(bool(plan_hash), "intake assessment produced no frozen plan hash")
    context.state.set("plan_hash", plan_hash)
    context.state.set("assessment_state", assessed.get("state"))


@stage("approve", "approve the exact frozen plan and bind the Campaign to the profile revision")
def approve(context: Context) -> None:
    context.state.require("intake_id", "plan_hash", "profile_revision_id")
    approved = context.api.post(
        f"/intake/requests/{context.state.get('intake_id')}/approve",
        {
            "expected_plan_hash": context.state.get("plan_hash"),
            "rationale": "Reviewed the exact frozen Compose intake plan.",
            "idempotency_key": "e2e-intake-approve",
            "authorized_micros": 5000000,
        },
    )
    campaign_id = approved.get("campaignId")
    require(bool(campaign_id) and campaign_id != "None", "intake approval returned no Campaign")
    tasks = (approved.get("campaign") or {}).get("tasks") or []
    require(bool(tasks), "intake approval returned a Campaign without tasks")
    attempt_id = tasks[0].get("attempt_id")
    require(bool(attempt_id), "intake approval returned no Attempt identity")
    profile_revision = (approved.get("profile") or {}).get("revisionId")
    assert_equal(
        profile_revision,
        context.state.get("profile_revision_id"),
        "intake Campaign profile revision",
    )
    context.admit_scope(campaign_id)
    context.state.set("campaign_id", campaign_id)
    context.state.set("attempt_id", attempt_id)


@stage("gate", "wait for the candidate, independent receipts and deterministic gate")
def gate(context: Context) -> None:
    context.state.require("attempt_id", "campaign_id", "intake_id", "plan_hash")
    attempt_id = context.state.get("attempt_id")
    candidate_id = ""
    gate_payload: dict[str, Any] = {}
    deadline = time.monotonic() + context.config.timeout
    while time.monotonic() < deadline:
        try:
            candidates = context.api.get("/candidates")
        except ScenarioError:
            time.sleep(1)
            continue
        match = next(
            (candidate for candidate in candidates if candidate.get("attempt_id") == attempt_id),
            None,
        )
        if match:
            candidate_id = match["id"]
            try:
                gate_payload = context.api.get(f"/quality/candidates/{candidate_id}/gate")
            except ScenarioError:
                gate_payload = {}
            if gate_payload.get("outcome") == "ACCEPTED":
                break
        time.sleep(1)
    require(bool(candidate_id), "no candidate was produced for the intake Attempt")
    assert_equal(gate_payload.get("outcome"), "ACCEPTED", "intake Quality Gate outcome")

    receipts = context.api.get(f"/quality/candidates/{candidate_id}/receipts")
    verifier = next((receipt for receipt in receipts if receipt.get("role") == "verifier"), None)
    reviewer = next((receipt for receipt in receipts if receipt.get("role") == "reviewer"), None)
    require(verifier is not None, "intake verifier receipt is missing")
    require(reviewer is not None, "intake reviewer receipt is missing")
    assert_equal(reviewer.get("mechanism"), "MODEL", "intake reviewer provenance")

    candidate_commit = next(
        (candidate["commit_sha"] for candidate in candidates if candidate["id"] == candidate_id),
        None,
    )
    require(bool(candidate_commit), "candidate commit is missing")
    diff = context.api.get(
        f"/campaigns/{context.state.get('campaign_id')}/candidates/{candidate_id}/diff"
    )
    assert_equal(diff.get("changed_paths"), ["src/provider.txt"], "candidate changed paths")

    campaign = context.api.get(f"/campaigns/{context.state.get('campaign_id')}")
    tasks = campaign.get("tasks") or []
    require(bool(tasks), "Campaign projection has no tasks")
    task = tasks[0]
    assert_equal(task.get("status"), "SUCCEEDED", "Campaign task status")
    assert_equal(task.get("candidate_id"), candidate_id, "Campaign task candidate identity")
    assert_equal(task.get("candidate_commit"), candidate_commit, "Campaign task candidate commit")

    projection = context.api.get(f"/intake/requests/{context.state.get('intake_id')}")
    assert_equal(projection.get("state"), "COMPLETED", "intake request state")
    assert_equal(projection.get("gateOutcome"), "ACCEPTED", "intake request gate outcome")
    assert_equal(
        projection.get("campaignId"),
        context.state.get("campaign_id"),
        "intake request Campaign identity",
    )
    plan = projection.get("plan") or {}
    assert_equal(plan.get("planHash"), context.state.get("plan_hash"), "intake request plan identity")

    context.state.set("candidate_id", candidate_id)
    context.state.set("candidate_commit", candidate_commit)
    context.state.set("gate_outcome", gate_payload["outcome"])
    context.state.set("verifier_receipt_id", verifier.get("receipt_id"))
    context.state.set("reviewer_receipt_id", reviewer.get("receipt_id"))
    context.state.set("candidate_diff", diff)


@stage("restart", "restart Core without volumes and verify durable identity/persistence")
def restart(context: Context) -> None:
    config = context.config
    context.state.require(
        "project_id",
        "campaign_id",
        "candidate_id",
        "candidate_commit",
        "intake_id",
        "gate_outcome",
        "verifier_receipt_id",
        "reviewer_receipt_id",
    )
    context.compose.down(volumes=False)
    context.compose.up(build=False)
    core = context.compose.core_container()
    require(bool(core), "Core container was not recreated")
    wait_url(f"{config.base}/health", 300)
    wait_core(context.compose, "http://runtime:8770/health", 900)
    wait_core(context.compose, "http://supervisor:8766/health", 180)

    def worker_ready() -> bool:
        workers = context.api.get("/workers")
        return any("compose-worker" in json.dumps(worker) for worker in workers)

    wait_until(worker_ready, 60, "the Compose Worker re-registration")
    projects = context.api.get("/projects")
    require(
        any(project.get("id") == context.state.get("project_id") for project in projects),
        "project persistence: the exact project id is missing",
    )
    gate_payload = context.api.get(
        f"/quality/candidates/{context.state.get('candidate_id')}/gate"
    )
    assert_equal(gate_payload.get("outcome"), "ACCEPTED", "gate persistence")
    projection = context.api.get(f"/intake/requests/{context.state.get('intake_id')}")
    assert_equal(projection.get("state"), "COMPLETED", "intake projection persistence")
    assert_equal(projection.get("gateOutcome"), "ACCEPTED", "intake gate persistence")
    assert_equal(
        projection.get("campaignId"),
        context.state.get("campaign_id"),
        "intake Campaign persistence",
    )
    campaign = context.api.get(f"/campaigns/{context.state.get('campaign_id')}")
    tasks = campaign.get("tasks") or []
    require(bool(tasks), "Campaign persistence has no tasks")
    assert_equal(tasks[0].get("candidate_id"), context.state.get("candidate_id"), "Campaign candidate persistence")
    assert_equal(tasks[0].get("candidate_commit"), context.state.get("candidate_commit"), "Campaign commit persistence")
    receipts = context.api.get(
        f"/quality/candidates/{context.state.get('candidate_id')}/receipts"
    )
    verifier = next(
        (receipt for receipt in receipts if receipt.get("receipt_id") == context.state.get("verifier_receipt_id")),
        None,
    )
    reviewer = next(
        (receipt for receipt in receipts if receipt.get("receipt_id") == context.state.get("reviewer_receipt_id")),
        None,
    )
    require(verifier is not None and verifier.get("role") == "verifier", "verifier receipt persistence")
    require(reviewer is not None and reviewer.get("role") == "reviewer", "reviewer receipt persistence")
    context.state.set("restart_persistence", True)
    context.state.set("restarted_at", now_utc())


@stage("summary", "write the candidate-bound scenario summary")
def summary(context: Context) -> None:
    context.state.require(
        "project_id",
        "campaign_id",
        "candidate_id",
        "candidate_commit",
        "intake_id",
        "plan_hash",
        "profile_revision_id",
        "verifier_receipt_id",
        "reviewer_receipt_id",
        "gate_outcome",
    )
    clean = context.state.get("dirty") is not True
    result = {
        "status": "PASS" if context.state.get("restart_persistence") is True and clean else "PARTIAL",
        "diagnosticOnly": not clean,
        "project_id": context.state.get("project_id"),
        "action_id": context.state.get("action_id"),
        "intake_request_id": context.state.get("intake_id"),
        "intake_campaign_id": context.state.get("campaign_id"),
        "intake_attempt_id": context.state.get("attempt_id"),
        "intake_candidate_id": context.state.get("candidate_id"),
        "intake_candidate_commit": context.state.get("candidate_commit"),
        "intake_plan_hash": context.state.get("plan_hash"),
        "intake_profile_revision_id": context.state.get("profile_revision_id"),
        "intake_verifier_receipt_id": context.state.get("verifier_receipt_id"),
        "intake_reviewer_receipt_id": context.state.get("reviewer_receipt_id"),
        "gate": context.state.get("gate_outcome"),
        "model_mechanism": "DETERMINISTIC_FIXTURE",
        "restart_persistence": context.state.get("restart_persistence", False),
        "browser": context.state.get("browser_office", "NOT_RUN"),
        "paid_inference": False,
    }
    write_json(context.config.artifacts_dir / "summary.json", result)
    context.state.set("summary", result)


def _configure_fixture_providers(context: Context) -> None:
    """Configure the deterministic model fixture routes used by every stage."""

    # Core's provider settings schema accepts exactly these fields; sending
    # extra per-turn limits is rejected with HTTP 422 and is not part of the
    # scenario contract.
    context.api.put(
        "/settings/providers/opencode",
        {
            "model": "deepseek-v4.1-flash",
            "baseUrl": "https://opencode.ai/inference/go/openai/v1",
            "credential": "compose-fixture-credential-0123456789",
            "enabled": True,
            "isDefault": False,
        },
    )
    context.api.put(
        "/settings/providers/opencode-go",
        {
            "model": "deepseek-v4.1-flash",
            "baseUrl": "http://model-fixture:8080/v1",
            "credential": "compose-fixture-credential-0123456789",
            "enabled": True,
            "isDefault": True,
        },
    )
    context.api.put(
        "/settings/runtime",
        {"semanticReviewEnabled": True},
    )


def _fixture_counters(context: Context) -> dict[str, int] | None:
    """Read test-support per-phase counters from the fixture container.

    The fixture is not published on the host and this is not product telemetry;
    the harness reads it over the private Compose network through `exec`.
    """

    script = (
        "import urllib.request;"
        "print(urllib.request.urlopen('http://127.0.0.1:8080/counters', timeout=5)"
        ".read().decode())"
    )
    result = context.compose.run(
        ["exec", "model-fixture", "python3", "-c", script], check=False
    )
    if result.returncode != 0:
        return None
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        counters = payload.get("counters")
        if isinstance(counters, dict):
            return {str(key): int(value) for key, value in counters.items()}
    return None


@stage(
    "autonomous_setup",
    "configure the deterministic providers, autonomous R0 profile and subscription-confirmed policy",
)
def autonomous_setup(context: Context) -> None:
    context.state.require("project_id", "project_config_hash")
    _configure_fixture_providers(context)
    # Structured model output needs 16384 tokens and a bounded retry fits in
    # 32768. The profile must not depend on provider-default quotas that are
    # lower than the protocol requirement.
    structured_tokens = 16384
    generation_tokens = 32768
    require(
        generation_tokens >= structured_tokens * 2,
        "the generation budget does not cover one structured call plus one retry",
    )
    role = {
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "reasoning": "medium",
    }
    profile = context.api.post(
        "/execution-profiles",
        {
            "name": "Compose Autonomous R0",
            "description": "Deterministic autonomous R0 Compose profile",
            "category": "BALANCED",
            "roles": {
                "lead": role,
                "implementer": role,
                "semanticReviewer": role,
            },
            "operations": {
                "modelRequestTimeoutSecs": 300,
                "toolCallTimeoutSecs": 600,
                "maxRequestBytes": 2097152,
                "maxResponseBytes": 2097152,
                "maxGenerationTokens": 32768,
                "maxInFlightOperations": 2,
            },
            "progress": {
                "diagnoseAfterStagnantCheckpoints": 2,
                "escalateAfterUnsuccessfulReplans": 1,
                "observationTimeoutSecs": 300,
            },
            "auto_start": {"enabled": True, "risk_classes": ["R0"]},
        },
    )
    assert_equal(profile.get("executable"), True, "autonomous execution profile is not executable")
    context.state.set(
        "autonomous_generation_budget",
        {
            "structuredOutputTokens": structured_tokens,
            "maxGenerationTokens": generation_tokens,
        },
    )
    context.api.post(f"/execution-profiles/{profile['id']}/default", {"scope": "global"})
    granted = context.api.post(
        f"/projects/{context.state.get('project_id')}/execution-policy",
        {
            "execution_profile_id": profile["id"],
            "expected_revision": profile["revision"],
            "expected_revision_hash": profile["contentHash"],
            "project_config_hash": context.state.get("project_config_hash"),
            "acknowledge": True,
            "autonomous": True,
            "subscription_confirmed": True,
            "idempotency_key": "et1-autonomous-policy",
        },
    )
    delegation = granted.get("delegation") or {}
    context.state.set("profile_id", profile["id"])
    context.state.set("profile_revision", profile["revision"])
    context.state.set("profile_revision_id", profile["revisionId"])
    context.state.set("profile_hash", profile["contentHash"])
    context.state.set("delegation_id", delegation.get("id"))


@stage(
    "autonomous_request",
    "submit one clean AUTONOMOUS intake request without manual planning calls",
)
def autonomous_request(context: Context) -> None:
    context.state.require(
        "project_id",
        "profile_id",
        "project_base_commit",
        "project_config_hash",
        "model_dir",
    )
    objective = "Add the deterministic Compose fixture endpoint and cover it with the frozen checks."
    created = context.api.post(
        "/intake/requests",
        {
            "project_id": context.state.get("project_id"),
            "objective": objective,
            "execution_profile_id": context.state.get("profile_id"),
            "mode": "AUTONOMOUS",
            "idempotency_key": f"et1-autonomous-{context.config.run_id}",
            "authorized_micros": 5000000,
        },
    )
    intake_id = created["id"]
    action_id = created["actionId"]
    lead_request_id = created["leadRequestId"]
    assert_equal(created.get("mode"), "AUTONOMOUS", "intake request mode")
    # Test-support subscription admissions only; the request itself is the sole
    # mutating product call and no planning endpoint is invoked by this harness.
    context.admit_scope(intake_id)
    context.admit_scope(lead_request_id)
    context.admit_scope("*")
    context.state.set("manual_planning_calls", 0)
    context.state.set("intake_id", intake_id)
    context.state.set("intake_action_id", action_id)
    context.state.set("intake_lead_request_id", lead_request_id)
    context.state.set("intake_objective", objective)
    model_context = {
        "schema": "factory-compose-model-fixture-v1",
        "projectId": context.state.get("project_id"),
        "intakeRequestId": intake_id,
        "requestId": lead_request_id,
        "actionId": action_id,
        "objective": objective,
        "baseRevision": context.state.get("project_base_commit"),
        "projectConfigHash": context.state.get("project_config_hash"),
        "qualityProfile": "generic",
        "allowedPaths": ["src"],
        "capabilities": ["repository-edit", "generic-toolchain"],
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "checks": [
            {
                "id": "build",
                "kind": "build",
                "description": "deterministic build",
                "command": ["true"],
                "expectExit": 0,
                "timeoutSecs": 30,
            },
            {
                "id": "test",
                "kind": "test",
                "description": "deterministic test",
                "command": ["true"],
                "expectExit": 0,
                "timeoutSecs": 30,
            },
            {
                "id": "functional",
                "kind": "functional",
                "description": "provider candidate content exists",
                "command": ["grep", "-q", "provider candidate", "src/provider.txt"],
                "expectExit": 0,
                "timeoutSecs": 30,
            },
            {
                "id": "negative",
                "kind": "negative",
                "description": "absent content is rejected",
                "command": ["grep", "-q", "absent", "src/provider.txt"],
                "expectExit": 1,
                "timeoutSecs": 30,
            },
        ],
    }
    context_path = Path(context.state.get("model_dir")) / "context.json"
    write_json(context_path, model_context)
    os.chmod(context_path, 0o644)
    context.state.set("model_context", str(context_path))


@stage(
    "autonomous_campaign",
    "observe autopilot preparation: plan, graph, participants, environment and Campaign",
)
def autonomous_campaign(context: Context) -> None:
    context.state.require("intake_id")
    intake_id = context.state.get("intake_id")
    deadline = time.monotonic() + context.config.timeout
    projection: dict[str, Any] = {}
    while time.monotonic() < deadline:
        projection = context.api.get(f"/intake/requests/{intake_id}")
        if projection.get("campaignId"):
            break
        if projection.get("state") in {"BLOCKED", "FAILED", "REJECTED", "CANCELLED", "AWAITING_APPROVAL"}:
            break
        time.sleep(2)
    state_name = projection.get("state")
    campaign_id = projection.get("campaignId")
    planning = projection.get("planning") or {}
    environment = projection.get("environment") or {}
    plan = projection.get("plan") or {}
    if not campaign_id:
        raise ScenarioError(
            "autonomous intake produced no Campaign: "
            f"state={state_name!r} detail={projection.get('stateDetail')!r} "
            f"authorization={projection.get('authorization')!r} "
            f"missingArtifacts={planning.get('missingArtifacts')!r} "
            f"reviewStatus={planning.get('reviewStatus')!r} "
            f"materialsReady={planning.get('materialsReady')!r} "
            f"ready={planning.get('ready')!r} environment={environment.get('status')!r}"
        )
    # F14/F15: authorization availability and planning.ready are not start-ready.
    assert_equal(planning.get("ready"), True, "planning readiness")
    assert_equal(planning.get("missingArtifacts"), [], "planning missing artifacts")
    participant_plan = planning.get("participantPlan") or {}
    assert_equal(participant_plan.get("ready"), True, "participant plan readiness")
    assert_in(
        str(planning.get("reviewStatus")),
        ("AUTO_APPROVED", "OWNER_APPROVED"),
        "plan review status",
    )
    assert_equal(planning.get("reviewApproved"), True, "plan review approval")
    # Core publishes these only after the stage-1 fix. They are optional here so
    # the baseline can still be observed failing without inventing fields.
    if "reviewCurrent" in planning:
        assert_equal(planning.get("reviewCurrent"), True, "plan review freshness")
    if "startReady" in planning:
        assert_equal(planning.get("startReady"), True, "Core start-ready flag")
    assert_equal(environment.get("status"), "FROZEN", "environment freeze")
    require(bool(environment.get("specSha256")), "environment spec digest is missing")
    plan_hash = plan.get("planHash") or plan.get("plan_hash")
    require(bool(plan_hash), "autopilot produced no plan hash")
    assert_equal(plan_hash, context.state.get("plan_hash") and context.state.get("plan_hash") or plan_hash, "plan hash stability")

    graph = context.api.get(f"/agentic/requests/{intake_id}/plan")
    units = graph.get("units") or (graph.get("graph") or {}).get("units") or []
    require(bool(units), "autopilot Plan projection has no work units")
    graph_sha = graph.get("graphSha256") or (graph.get("graph") or {}).get("graphSha256")
    require(bool(graph_sha), "autopilot Plan projection has no graph digest")
    assert_equal(graph.get("provider"), "opencode-go", "WorkGraph provider provenance")
    assert_equal(graph.get("model"), "deepseek-v4.1-flash", "WorkGraph model provenance")

    participants = context.api.get(f"/agentic/requests/{intake_id}/participants")
    compiled = participants.get("compiled") or participants
    assert_equal(compiled.get("ready"), True, "participant plan compiled readiness")
    assert_equal(
        compiled.get("graphSha256"),
        graph_sha,
        "participant plan graph binding",
    )

    campaign = context.api.get(f"/campaigns/{campaign_id}")
    tasks = campaign.get("tasks") or []
    require(bool(tasks), "autonomous Campaign has no tasks")
    attempt_id = tasks[0].get("attempt_id") or tasks[0].get("attemptId")
    require(bool(attempt_id), "autonomous Campaign has no Attempt identity")
    counters = _fixture_counters(context) or {}
    require(counters.get("planner", 0) >= 1, "fixture never served a Planner call")
    require(counters.get("assessment", 0) >= 1, "fixture never served an assessment call")
    require(
        counters.get("environment", 0) >= 1,
        "fixture never served an environment review call",
    )

    context.state.set("plan_hash", plan_hash)
    context.state.set("graph_sha256", graph_sha)
    context.state.set("environment_spec_sha256", environment.get("specSha256"))
    context.state.set("campaign_id", campaign_id)
    context.state.set("attempt_id", attempt_id)
    context.state.set("autonomous_counters", counters)
    context.state.set("autonomous_state", state_name)
    context.state.set("autonomous_projection", {
        "planning": planning,
        "environment": environment,
        "authorization": projection.get("authorization"),
    })


@stage(
    "autonomous_negative",
    "a profile without an autonomous policy must fail closed with no Campaign",
)
def autonomous_negative(context: Context) -> None:
    context.state.require("project_id")
    role = {
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "reasoning": "medium",
    }
    profile = context.api.post(
        "/execution-profiles",
        {
            "name": "Compose Non-Autonomous Negative",
            "description": "Deterministic negative profile without autonomous start",
            "category": "BALANCED",
            "roles": {"lead": role, "implementer": role, "semanticReviewer": role},
            "operations": {
                "modelRequestTimeoutSecs": 300,
                "toolCallTimeoutSecs": 600,
                "maxRequestBytes": 2097152,
                "maxResponseBytes": 2097152,
                "maxGenerationTokens": 32768,
                "maxInFlightOperations": 2,
            },
            "progress": {
                "diagnoseAfterStagnantCheckpoints": 2,
                "escalateAfterUnsuccessfulReplans": 1,
                "observationTimeoutSecs": 300,
            },
            "auto_start": {"enabled": False, "risk_classes": []},
        },
    )
    try:
        created = context.api.post(
            "/intake/requests",
            {
                "project_id": context.state.get("project_id"),
                "objective": "Negative autonomous intake without an autonomous policy.",
                "execution_profile_id": profile["id"],
                "mode": "AUTONOMOUS",
                "idempotency_key": f"et1-autonomous-negative-{context.config.run_id}",
                "authorized_micros": 5000000,
            },
        )
    except ScenarioError as error:
        message = str(error)
        require(
            "AUTONOMOUS_POLICY_REQUIRED" in message
            or "EXECUTION_POLICY_REQUIRED" in message,
            f"negative autonomous intake returned an unexpected error: {message}",
        )
        context.state.set(
            "autonomous_negative",
            {"outcome": "REJECTED", "code": message.split("HTTP", 1)[-1][:200]},
        )
        return
    raise ScenarioError(
        "autonomous intake without an autonomous policy unexpectedly created a request: "
        f"{created.get('id')!r}"
    )


@stage(
    "autonomous_restart",
    "restart Core without volumes and verify the autonomous plan/Campaign identity persists",
)
def autonomous_restart(context: Context) -> None:
    config = context.config
    context.state.require(
        "intake_id",
        "campaign_id",
        "plan_hash",
        "graph_sha256",
        "environment_spec_sha256",
    )
    before = context.api.get(f"/intake/requests/{context.state.get('intake_id')}")
    context.compose.down(volumes=False)
    context.compose.up(build=False)
    wait_url(f"{config.base}/health", 300)
    wait_core(context.compose, "http://runtime:8770/health", 900)
    wait_core(context.compose, "http://supervisor:8766/health", 180)
    wait_core(context.compose, "http://model-fixture:8080/health", 120)

    def worker_ready() -> bool:
        workers = context.api.get("/workers")
        return any("compose-worker" in json.dumps(worker) for worker in workers)

    wait_until(worker_ready, 60, "the Compose Worker re-registration")
    after = context.api.get(f"/intake/requests/{context.state.get('intake_id')}")
    before_plan = (before.get("plan") or {}).get("planHash")
    after_plan = (after.get("plan") or {}).get("planHash")
    assert_equal(after_plan, before_plan, "plan hash persistence")
    assert_equal(after.get("campaignId"), context.state.get("campaign_id"), "Campaign persistence")
    after_environment = after.get("environment") or {}
    assert_equal(
        after_environment.get("specSha256"),
        context.state.get("environment_spec_sha256"),
        "environment spec persistence",
    )
    campaigns = context.api.get("/campaigns")
    action_id = context.state.get("intake_action_id")
    matching = [
        campaign
        for campaign in campaigns
        if (campaign.get("action_id") or campaign.get("actionId")) == action_id
    ]
    assert_equal(len(matching), 1, "exactly one Campaign per autonomous action")
    campaign = context.api.get(f"/campaigns/{context.state.get('campaign_id')}")
    tasks = campaign.get("tasks") or []
    require(bool(tasks), "restarted Campaign has no tasks")
    assert_equal(
        tasks[0].get("attempt_id") or tasks[0].get("attemptId"),
        context.state.get("attempt_id"),
        "Attempt persistence",
    )
    context.state.set("restart_persistence", True)


@stage("autonomous_summary", "write the mock-bound autonomous scenario summary")
def autonomous_summary(context: Context) -> None:
    context.state.require(
        "intake_id",
        "campaign_id",
        "plan_hash",
        "graph_sha256",
        "environment_spec_sha256",
    )
    clean = context.state.get("dirty") is not True
    counters = _fixture_counters(context) or context.state.get("autonomous_counters") or {}
    passed = (
        clean
        and context.state.get("restart_persistence") is True
        and counters.get("planner", 0) >= 1
        and context.state.get("manual_planning_calls", 0) == 0
    )
    result = {
        "status": "PASS" if passed else "PARTIAL",
        "diagnosticOnly": not clean,
        "mode": "AUTONOMOUS",
        "intake_request_id": context.state.get("intake_id"),
        "campaign_id": context.state.get("campaign_id"),
        "attempt_id": context.state.get("attempt_id"),
        "plan_hash": context.state.get("plan_hash"),
        "graph_sha256": context.state.get("graph_sha256"),
        "environment_spec_sha256": context.state.get("environment_spec_sha256"),
        "manual_planning_calls": context.state.get("manual_planning_calls", 0),
        "fixture_counters": counters,
        "model_evidence": "SIMULATED",
        "provider_provenance": "opencode-go/deepseek-v4.1-flash (fixture route)",
        "paid_inference": False,
        "live_inference": "NOT_TESTED",
        "restart_persistence": context.state.get("restart_persistence", False),
        "negative": context.state.get("autonomous_negative"),
        "browser": context.state.get("browser_office", "NOT_RUN"),
    }
    write_json(context.config.artifacts_dir / "summary.json", result)
    context.state.set("summary", result)


def stage_order() -> list[str]:
    return [name for name, _, _ in STAGES]


def stage_description(name: str) -> str:
    for stage_name, _, description in STAGES:
        if stage_name == name:
            return description
    raise ScenarioError(f"unknown stage: {name}")


def stage_function(name: str) -> Callable[[Context], None]:
    for stage_name, function, _ in STAGES:
        if stage_name == name:
            return function
    raise ScenarioError(f"unknown stage: {name}")
